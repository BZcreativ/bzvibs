//! Play a decoded file through the real output device.
//!
//! Usage: `cargo run -p rt-audio --release --example play_file -- <track.m4a> [volume_0_1000] [seconds]`
//! Decodes, resamples to the device rate, plays, then exercises
//! pause/resume. Prints the underrun count.

use std::time::{Duration, Instant};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .unwrap_or_else(|| "../../prototypes/pb-decode/track.m4a".to_string());
    let volume = args
        .next()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(1000);
    let seconds = args
        .next()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(10);

    let track = match rt_audio::decode(Box::new(std::fs::File::open(&path).unwrap())) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("decode failed: {e}");
            std::process::exit(1);
        }
    };
    println!(
        "decoded: {} frames @ {} Hz, {} ch, {:.1}s",
        track.frames(),
        track.rate,
        track.channels,
        track.duration_secs()
    );

    let mut player = match rt_audio::Player::open() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("output open failed: {e}");
            std::process::exit(1);
        }
    };
    player.set_volume_milli(volume);
    let spec = player.spec();
    println!("device: {} Hz, {} ch", spec.rate, spec.channels);
    if spec.rate != track.rate {
        println!("resampling {} -> {} Hz", track.rate, spec.rate);
    }
    let samples = rt_audio::linear_resample(
        &track.samples,
        track.channels as usize,
        track.rate,
        spec.rate,
    );

    // Pre-fill the ring before starting the device, so the first callback
    // never starves.
    let mut fed = 0usize;
    while fed < samples.len() {
        let n = player.queue(&samples[fed..]);
        if n == 0 {
            break;
        }
        fed += n;
    }
    println!("pre-filled {fed} samples");
    player.start().expect("start stream");

    // Feed the ring at a sustainable rate and play for `seconds` wall-clock
    // seconds, pausing 1 s in the middle to exercise resume.
    let started = Instant::now();
    let half = seconds / 2;
    while started.elapsed().as_secs() < seconds {
        if fed < samples.len() {
            let chunk = &samples[fed..(fed + 4800 * spec.channels as usize).min(samples.len())];
            fed += player.queue(chunk);
        }
        if started.elapsed().as_secs() == half && !player.paused() {
            player.pause();
            println!(
                "paused at {}s (buffered {} samples)",
                started.elapsed().as_secs(),
                player.buffered_samples()
            );
            std::thread::sleep(Duration::from_millis(1000));
            player.resume();
            println!("resumed");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    println!(
        "played {:.1}s wall, fed {:.1}s of audio, underruns: {}",
        started.elapsed().as_secs_f32(),
        fed as f32 / spec.rate as f32 / spec.channels as f32,
        player.underruns()
    );
}
