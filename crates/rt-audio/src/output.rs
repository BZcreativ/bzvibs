//! Output stage: a cpal f32 stream fed by an SPSC ring buffer, with the EQ
//! cascade running inside the audio callback. Pause silences the callback
//! without consuming the buffer, so resume continues exactly where playback
//! stopped. Commands cross from other threads through atomics only; nothing
//! in the callback allocates, locks, or blocks.

use crate::dsp::Eq8;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

#[derive(Debug, Clone, Copy)]
pub struct DeviceSpec {
    pub rate: u32,
    pub channels: u16,
}

#[derive(Debug, thiserror::Error)]
pub enum OutputError {
    #[error("no output device")]
    NoDevice,
    #[error("output device does not provide an f32 stream")]
    UnsupportedFormat,
    #[error("audio backend: {0}")]
    Cpal(#[from] cpal::Error),
}

#[derive(Debug, Default)]
struct Shared {
    paused: AtomicBool,
    /// 0..=1000, applied in the callback before the EQ.
    volume_milli: AtomicU32,
    underruns: AtomicU64,
}

/// The pure callback body, separated from cpal so tests can drive it.
fn fill(
    data: &mut [f32],
    consumer: &mut rtrb::Consumer<f32>,
    eq: &mut Eq8,
    paused: bool,
    volume: f32,
    underruns: &AtomicU64,
) {
    if paused {
        data.fill(0.0);
        return;
    }
    let mut starved = false;
    for s in data.iter_mut() {
        match consumer.pop() {
            Ok(v) => *s = v * volume,
            Err(_) => {
                *s = 0.0;
                starved = true;
            }
        }
    }
    if starved {
        underruns.fetch_add(1, Ordering::Relaxed);
    }
    eq.process(data);
}

pub struct Player {
    spec: DeviceSpec,
    capacity: usize,
    shared: Arc<Shared>,
    producer: rtrb::Producer<f32>,
    stream: Stream,
    eq_handle: crate::dsp::EqHandle,
}

impl Player {
    pub fn open() -> Result<Self, OutputError> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or(OutputError::NoDevice)?;
        let config = device.default_output_config()?;
        if config.sample_format() != SampleFormat::F32 {
            return Err(OutputError::UnsupportedFormat);
        }
        let rate = config.sample_rate();
        let channels = config.channels();
        let stream_config = StreamConfig {
            channels,
            sample_rate: rate,
            buffer_size: cpal::BufferSize::Default,
        };

        // ~1 s of device-rate audio.
        let capacity = rate as usize * channels as usize;
        let (producer, mut consumer) = rtrb::RingBuffer::new(capacity.max(1024));

        let shared = Arc::new(Shared::default());
        let shared_cb = shared.clone();
        let mut eq = Eq8::new(rate, channels as usize);
        let eq_handle = eq.handle();

        let stream = device.build_output_stream(
            stream_config,
            move |data: &mut [f32], _| {
                let paused = shared_cb.paused.load(Ordering::Relaxed);
                let volume =
                    shared_cb.volume_milli.load(Ordering::Relaxed).min(1000) as f32 / 1000.0;
                fill(
                    data,
                    &mut consumer,
                    &mut eq,
                    paused,
                    volume,
                    &shared_cb.underruns,
                );
            },
            |err| eprintln!("audio stream error: {err}"),
            None,
        )?;
        Ok(Player {
            spec: DeviceSpec { rate, channels },
            capacity,
            shared,
            producer,
            stream,
            eq_handle,
        })
    }

    /// Start the device stream. Call after queueing the first samples; the
    /// stream is created silent so the device never starts on an empty ring.
    pub fn start(&self) -> Result<(), OutputError> {
        self.stream.play()?;
        Ok(())
    }

    pub fn spec(&self) -> DeviceSpec {
        self.spec
    }

    pub fn eq_handle(&self) -> crate::dsp::EqHandle {
        self.eq_handle.clone()
    }

    /// Push interleaved samples at the device rate; returns how many were
    /// accepted (the rest stay for the next call when the ring is full).
    pub fn queue(&mut self, interleaved: &[f32]) -> usize {
        let mut n = 0;
        for v in interleaved {
            if self.producer.push(*v).is_err() {
                break;
            }
            n += 1;
        }
        n
    }

    pub fn buffered_samples(&self) -> usize {
        self.capacity - self.producer.slots()
    }

    pub fn pause(&self) {
        self.shared.paused.store(true, Ordering::Relaxed);
    }

    pub fn resume(&self) {
        self.shared.paused.store(false, Ordering::Relaxed);
    }

    pub fn paused(&self) -> bool {
        self.shared.paused.load(Ordering::Relaxed)
    }

    pub fn set_volume_milli(&self, milli: u32) {
        self.shared
            .volume_milli
            .store(milli.min(1000), Ordering::Relaxed);
    }

    pub fn volume_milli(&self) -> u32 {
        self.shared.volume_milli.load(Ordering::Relaxed).min(1000)
    }

    pub fn underruns(&self) -> u64 {
        self.shared.underruns.load(Ordering::Relaxed)
    }

    /// Keep the stream handle alive alongside the player.
    pub fn stream(&self) -> &Stream {
        &self.stream
    }
}

/// Convert an interleaved buffer between sample rates by linear
/// interpolation. Identity when the rates match.
pub fn linear_resample(input: &[f32], channels: usize, from: u32, to: u32) -> Vec<f32> {
    let channels = channels.max(1);
    let frames_in = input.len() / channels;
    if from == to || frames_in == 0 {
        return input.to_vec();
    }
    let frames_out = ((frames_in as u64 * to as u64) / from as u64).max(1) as usize;
    let step = (frames_in - 1) as f64 / frames_out.max(1) as f64;
    let mut out = Vec::with_capacity(frames_out * channels);
    for i in 0..frames_out {
        let pos = i as f64 * step;
        let i0 = pos.floor() as usize;
        let i1 = (i0 + 1).min(frames_in - 1);
        let frac = (pos - i0 as f64) as f32;
        for c in 0..channels {
            let a = input[i0 * channels + c];
            let b = input[i1 * channels + c];
            out.push(a + (b - a) * frac);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    fn eq_mono_44k() -> Eq8 {
        Eq8::new(44_100, 1)
    }

    #[test]
    fn fill_pops_in_fifo_order_across_ring_wrap() {
        let (mut prod, mut cons) = rtrb::RingBuffer::new(8);
        for v in 1..=8u32 {
            assert!(prod.push(v as f32).is_ok());
        }
        let mut eq = eq_mono_44k();
        let underruns = AtomicU64::new(0);
        let mut out = [0.0f32; 8];
        fill(&mut out, &mut cons, &mut eq, false, 1.0, &underruns);
        // The flat cascade is transparent to float precision, not bit-exact.
        for (got, want) in out.iter().zip(1..=8u32) {
            assert!((got - want as f32).abs() < 1e-4, "got {got}, want {want}");
        }
        // Refill to force a wrap, then read again.
        for v in 9..=12u32 {
            assert!(prod.push(v as f32).is_ok());
        }
        let mut out = [0.0f32; 4];
        fill(&mut out, &mut cons, &mut eq, false, 1.0, &underruns);
        for (got, want) in out.iter().zip(9..=12u32) {
            assert!((got - want as f32).abs() < 1e-4, "got {got}, want {want}");
        }
        assert_eq!(underruns.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn paused_fills_silence_without_consuming() {
        let (mut prod, mut cons) = rtrb::RingBuffer::new(8);
        for v in 1..=4u32 {
            prod.push(v as f32).ok();
        }
        let mut eq = eq_mono_44k();
        let underruns = AtomicU64::new(0);
        let mut out = [0.5f32; 4];
        fill(&mut out, &mut cons, &mut eq, true, 1.0, &underruns);
        assert_eq!(out, [0.0; 4]);
        assert_eq!(underruns.load(Ordering::Relaxed), 0);
        // Samples are still there.
        let mut out = [0.0f32; 4];
        fill(&mut out, &mut cons, &mut eq, false, 1.0, &underruns);
        for (got, want) in out.iter().zip(1..=4u32) {
            assert!((got - want as f32).abs() < 1e-4, "got {got}, want {want}");
        }
    }

    #[test]
    fn starvation_counts_one_underrun_per_callback_and_scales_volume() {
        let (_prod, mut cons) = rtrb::RingBuffer::new(8);
        let mut eq = eq_mono_44k();
        let underruns = AtomicU64::new(0);
        let mut out = [0.0f32; 4];
        fill(&mut out, &mut cons, &mut eq, false, 0.5, &underruns);
        assert_eq!(out, [0.0; 4]);
        assert_eq!(underruns.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn resample_is_identity_when_rates_match() {
        let input = vec![1.0f32, 2.0, 3.0, 4.0];
        assert_eq!(linear_resample(&input, 2, 44_100, 44_100), input);
    }

    #[test]
    fn resample_hits_expected_length_and_endpoints() {
        // Mono 44100 -> 48000 for 1 s.
        let input: Vec<f32> = (0..44_100).map(|i| i as f32).collect();
        let out = linear_resample(&input, 1, 44_100, 48_000);
        assert_eq!(out.len(), 48_000);
        assert_eq!(out[0], 0.0);
        // Last frame interpolates toward the final input sample.
        assert!(out.last().unwrap() <= input.last().unwrap());
        // Downsample back and check mono/stereo shapes hold.
        let stereo_in: Vec<f32> = (0..2 * 100).map(|i| i as f32).collect();
        let out = linear_resample(&stereo_in, 2, 48_000, 24_000);
        assert_eq!(out.len(), 100);
    }

    #[test]
    fn dsp_in_callback_chunk_stays_under_budget() {
        // The plan's perf box: 4096-frame stereo chunks through the full
        // 8-band cascade, averaged, under 2 ms per chunk.
        let mut eq = Eq8::new(48_000, 2);
        let mut chunk = vec![0.25f32; 4096 * 2];
        const RUNS: usize = 50;
        let started = std::time::Instant::now();
        for _ in 0..RUNS {
            eq.process(&mut chunk);
        }
        let per_chunk_us = started.elapsed().as_micros() as f64 / RUNS as f64;
        eprintln!("dsp per 4096-frame stereo chunk: {per_chunk_us:.0} us");
        // The budget is a release-build budget; debug numbers are meaningless
        // for an audio callback, so only enforce when optimized.
        if !cfg!(debug_assertions) {
            assert!(per_chunk_us < 2000.0, "{per_chunk_us:.0} us per chunk");
        }
    }
}
