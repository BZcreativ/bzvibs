//! The 8-band equalizer from the PRD: 60 Hz low shelf, 250/500/1k/2k/4k/8k
//! peaking, 16 kHz high shelf. Band frequencies and Q are fixed; gains are
//! live-adjustable.
//!
//! Threading shape: [`EqHandle`] stores new gains lock-free from any thread
//! (the UI). [`Eq8::process`] runs on the audio thread, snapshots the current
//! gains, and when they changed calls `update_coefficients` on each existing
//! filter so coefficient replacement never mutates shared state and filter
//! memory is preserved across slider moves.

use arc_swap::ArcSwap;
use biquad::{Biquad, Coefficients, DirectForm1, Type as BandType};
use std::sync::Arc;

pub const BAND_FREQS_HZ: [f32; 8] = [60.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];
pub const BAND_Q: f32 = 1.0;

#[derive(Clone, Debug, PartialEq)]
pub struct EqParams {
    pub gains_db: [f32; 8],
}

impl Default for EqParams {
    fn default() -> Self {
        EqParams { gains_db: [0.0; 8] }
    }
}

fn band_kind(index: usize, gain_db: f32) -> BandType<f32> {
    match index {
        0 => BandType::LowShelf(gain_db),
        7 => BandType::HighShelf(gain_db),
        _ => BandType::PeakingEQ(gain_db),
    }
}

fn band_coeffs(index: usize, gain_db: f32, rate: f32) -> Coefficients<f32> {
    // Infallible for our constant band table: every frequency is below
    // Nyquist at any supported sample rate and Q is positive.
    Coefficients::<f32>::from_params(
        band_kind(index, gain_db),
        biquad::frequency::Hertz::<f32>::from_hz(rate).unwrap(),
        biquad::frequency::Hertz::<f32>::from_hz(BAND_FREQS_HZ[index]).unwrap(),
        BAND_Q,
    )
    .unwrap()
}

/// Cloneable, thread-safe setter owned by the UI side.
#[derive(Clone)]
pub struct EqHandle(Arc<ArcSwap<EqParams>>);

impl EqHandle {
    pub fn set_gains_db(&self, gains_db: [f32; 8]) {
        self.0.store(Arc::new(EqParams { gains_db }));
    }
}

pub struct Eq8 {
    params: Arc<ArcSwap<EqParams>>,
    /// cascades[channel][band]
    cascades: Vec<Vec<DirectForm1<f32>>>,
    rate: f32,
    channels: usize,
    applied: [f32; 8],
}

impl Eq8 {
    pub fn new(rate: u32, channels: usize) -> Self {
        let params = Arc::new(ArcSwap::from_pointee(EqParams::default()));
        let cascades = (0..channels.max(1))
            .map(|_| {
                BAND_FREQS_HZ
                    .iter()
                    .enumerate()
                    .map(|(i, _)| DirectForm1::new(band_coeffs(i, 0.0, rate as f32)))
                    .collect()
            })
            .collect();
        Eq8 {
            params,
            cascades,
            rate: rate as f32,
            channels: channels.max(1),
            applied: [0.0; 8],
        }
    }

    pub fn handle(&self) -> EqHandle {
        EqHandle(self.params.clone())
    }

    /// Process an interleaved buffer in place. Audio thread only.
    pub fn process(&mut self, interleaved: &mut [f32]) {
        let current = self.params.load_full();
        if current.gains_db != self.applied {
            for cascade in &mut self.cascades {
                for (i, band) in cascade.iter_mut().enumerate() {
                    band.update_coefficients(band_coeffs(i, current.gains_db[i], self.rate));
                }
            }
            self.applied = current.gains_db;
        }
        for (i, s) in interleaved.iter_mut().enumerate() {
            let mut v = *s;
            for band in &mut self.cascades[i % self.channels] {
                v = band.run(v);
            }
            *s = v;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, rate: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / rate).sin())
            .collect()
    }

    fn rms_db(x: &[f32]) -> f64 {
        let s: f64 = x.iter().map(|v| (*v as f64) * (*v as f64)).sum();
        10.0 * (s / x.len() as f64).log10()
    }

    /// Run a mono signal through a fresh Eq8 with the given gains and return
    /// the dB change measured over the settled second half.
    fn measured_delta_db(gains: [f32; 8], freq: f32) -> f64 {
        const RATE: u32 = 44_100;
        let mut eq = Eq8::new(RATE, 1);
        eq.handle().set_gains_db(gains);
        let mut buf = sine(freq, RATE as f32, 1 << 15);
        let warmup = buf.len() / 2;
        let before = rms_db(&buf[warmup..]);
        eq.process(&mut buf);
        let after = rms_db(&buf[warmup..]);
        after - before
    }

    #[test]
    fn peaking_band_hits_its_gain_exactly() {
        let mut gains = [0.0f32; 8];
        gains[3] = 6.0; // BAND_FREQS_HZ[3] = 1 kHz
        let d = measured_delta_db(gains, 1000.0);
        assert!(
            (d - 6.0).abs() < 0.1,
            "1 kHz peaking +6 dB measured {d:.2} dB"
        );
    }

    #[test]
    fn shelves_hit_half_gain_at_their_corner() {
        let mut gains = [0.0f32; 8];
        gains[0] = 9.0; // 60 Hz low shelf
        let d = measured_delta_db(gains, 60.0);
        assert!(
            (d - 4.5).abs() < 0.3,
            "60 Hz shelf +9 dB @60 Hz measured {d:.2} dB"
        );

        let mut gains = [0.0f32; 8];
        gains[7] = 9.0; // 16 kHz high shelf
        let d = measured_delta_db(gains, 16000.0);
        assert!(
            (d - 4.5).abs() < 0.3,
            "16 kHz shelf +9 dB @16 kHz measured {d:.2} dB"
        );
    }

    #[test]
    fn flat_response_is_transparent() {
        // Deterministic noise: x_i+1 = (a*x + c) mod m, kept in [-1, 1).
        let mut state: u64 = 0x2545F4914F6CDD1D;
        let mut buf: Vec<f32> = (0..1 << 16)
            .map(|_| {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                ((state >> 33) as f32 / (u32::MAX >> 1) as f32) - 1.0
            })
            .collect();
        let before = rms_db(&buf);
        let mut eq = Eq8::new(44_100, 1);
        eq.process(&mut buf);
        let after = rms_db(&buf);
        assert!(
            (after - before).abs() < 0.01,
            "flat EQ changed RMS by {:.3} dB",
            after - before
        );
    }

    #[test]
    fn gain_changes_apply_mid_stream_via_handle() {
        const RATE: u32 = 44_100;
        let mut eq = Eq8::new(RATE, 1);
        let handle = eq.handle();
        let mut first = sine(1000.0, RATE as f32, 1 << 14);
        let mut second = sine(1000.0, RATE as f32, 1 << 14);
        eq.process(&mut first);

        let mut gains = [0.0f32; 8];
        gains[3] = 6.0; // 1 kHz
        handle.set_gains_db(gains);
        eq.process(&mut second);

        let warm = |b: &mut Vec<f32>| b.len() / 2;
        let w = warm(&mut first);
        let flat = rms_db(&first[w..]);
        let boosted = rms_db(&second[w..]);
        let delta = boosted - flat;
        assert!(
            (delta - 6.0).abs() < 0.1,
            "mid-stream swap measured {delta:.2} dB"
        );
    }

    #[test]
    fn stereo_interleaving_applies_the_same_gain_to_both_channels() {
        // One preset, all channels: both L and R measure the same boost, and
        // the interleaved walk must not cross-wire samples between channels.
        const RATE: u32 = 44_100;
        let mut eq = Eq8::new(RATE, 2);
        let mut gains = [0.0f32; 8];
        gains[3] = 6.0; // 1 kHz
        eq.handle().set_gains_db(gains);
        let n = 1 << 15;
        let mut buf: Vec<f32> = Vec::with_capacity(n * 2);
        for i in 0..n {
            let v = (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / RATE as f32).sin();
            buf.push(v);
            buf.push(v);
        }
        eq.process(&mut buf);
        let left: Vec<f32> = buf.chunks(2).map(|c| c[0]).collect();
        let right: Vec<f32> = buf.chunks(2).map(|c| c[1]).collect();
        let w = n / 2;
        let raw = sine(1000.0, RATE as f32, 1 << 15);
        let l = rms_db(&left[w..]) - rms_db(&raw[w..]);
        let r = rms_db(&right[w..]) - rms_db(&raw[w..]);
        assert!((l - 6.0).abs() < 0.1, "left measured {l:.2} dB");
        assert!((r - 6.0).abs() < 0.1, "right measured {r:.2} dB");
    }
}
