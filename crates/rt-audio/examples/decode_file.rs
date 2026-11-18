//! Decode a file and report the numbers the plan's live and perf boxes need:
//! frames, duration, wall time, realtime multiple.
//!
//! Usage: `cargo run -p rt-audio --release --example decode_file -- <track.m4a>`

use std::time::Instant;

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "../../prototypes/pb-decode/track.m4a".to_string());
    let file = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("cannot open {path}: {e}");
            std::process::exit(1);
        }
    };
    let started = Instant::now();
    let track = match rt_audio::decode(Box::new(file)) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("decode failed: {e}");
            std::process::exit(1);
        }
    };
    let elapsed = started.elapsed();
    let duration = track.duration_secs();
    println!(
        "frames: {} rate: {} channels: {} duration_s: {:.1} elapsed_ms: {} realtime_x: {:.1}",
        track.frames(),
        track.rate,
        track.channels,
        duration,
        elapsed.as_millis(),
        duration / elapsed.as_secs_f64().max(1e-9),
    );
}
