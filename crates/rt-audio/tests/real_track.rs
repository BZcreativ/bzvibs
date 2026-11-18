//! Live box for PR-4: decode the real itag-140 m4a the prototype fetched
//! from YouTube Music. Skips when the fixture is absent (clean checkouts, CI).

use std::path::Path;
use std::time::Instant;

#[test]
fn decodes_real_youtube_track_at_five_times_realtime() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../prototypes/pb-decode/track.m4a");
    if !path.exists() {
        eprintln!("skipping: {} not present", path.display());
        return;
    }
    let file = std::fs::File::open(&path).expect("open fixture");
    let started = Instant::now();
    let track = rt_audio::decode(Box::new(file)).expect("decode");
    let elapsed = started.elapsed().as_secs_f64();
    let duration = track.duration_secs();
    let throughput = duration / elapsed.max(1e-9);
    eprintln!(
        "frames={} rate={} duration_s={duration:.1} elapsed_s={elapsed:.2} realtime_x={throughput:.1}",
        track.frames(),
        track.rate
    );
    assert!(
        (duration - 213.0).abs() <= 1.0,
        "duration was {duration:.1} s"
    );
    assert!(track.frames() >= 9_400_000, "frames was {}", track.frames());
    assert!(
        throughput >= 5.0,
        "throughput was {throughput:.1}x realtime"
    );
}
