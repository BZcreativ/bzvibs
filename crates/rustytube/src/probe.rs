//! Live probes for the plan's verification boxes. Development surface, not
//! product UI.

use rt_api::{ApiClient, Track};
use rt_stream::{StreamResolver, VideoId, YtDlpResolver};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub fn run(args: &[String]) {
    match (
        args.first().map(String::as_str),
        args.get(1).map(String::as_str),
    ) {
        (Some("search"), Some(query)) => search(query),
        (Some("stream"), Some(video_id)) => stream(video_id),
        (Some("play"), Some(video_id)) => play(video_id),
        (Some("lyrics"), Some(video_id)) => lyrics(video_id),
        (Some("auth"), _) => auth(),
        (Some("cookie"), Some(path)) => cookie(path),
        (Some("login"), maybe_timeout) => login(
            maybe_timeout
                .and_then(|t| t.parse::<u64>().ok())
                .unwrap_or(300),
        ),
        (Some("fetchmt"), Some(video_id)) => fetch_mt(video_id),
        _ => {
            eprintln!(
                "usage: bzvibs probe search <query> | probe stream <video_id> | probe play <video_id> | probe cookie <file> | probe auth"
            );
            std::process::exit(2);
        }
    }
}

fn search(query: &str) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    rt.block_on(async {
        let started = Instant::now();
        let client = match ApiClient::new() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("client bootstrap failed: {e}");
                std::process::exit(1);
            }
        };
        let tracks: Vec<Track> = match client.search_songs(query).await {
            Ok(t) => t,
            Err(e) => {
                eprintln!("search failed: {e}");
                std::process::exit(1);
            }
        };
        let elapsed_ms = started.elapsed().as_millis();
        for (i, t) in tracks.iter().take(10).enumerate() {
            println!(
                "{}. {} - {} [{}] ({})",
                i + 1,
                t.title,
                t.artist,
                t.video_id,
                t.duration
            );
        }
        println!("results: {}", tracks.len());
        println!("elapsed_ms: {elapsed_ms}");
    });
}

fn stream(video_id: &str) {
    let id = match VideoId::parse(video_id) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    rt.block_on(async {
        let resolver = YtDlpResolver;
        if !resolver.is_available().await {
            eprintln!("yt-dlp not found in PATH");
            std::process::exit(1);
        }
        let started = Instant::now();
        let resolved = match resolver.resolve(&id).await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("resolve failed: {e}");
                std::process::exit(1);
            }
        };
        println!("host: {}", resolved.url.host_str().unwrap_or("?"));
        println!(
            "url_roundtrip: {}",
            resolved.url.as_str().chars().take(160).collect::<String>()
        );
        println!("resolve_ms: {}", started.elapsed().as_millis());

        let bytes = match rt_stream::fetch_full(&resolved).await {
            Ok(b) => b,
            Err(e) => {
                eprintln!("fetch failed: {e}");
                std::process::exit(1);
            }
        };
        let ftyp = bytes.len() >= 8 && &bytes[4..8] == b"ftyp";
        println!("fetched_bytes: {}", bytes.len());
        println!("container_is_m4a_ftyp: {ftyp}");
    });
}

/// Headless end-to-end: engine thread, loader, feeder, real output device.
fn play(video_id: &str) {
    let id = match VideoId::parse(video_id) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let track = Track {
        video_id: id.as_str().to_string(),
        title: "probe track".into(),
        artist: "probe".into(),
        album: None,
        duration: "0:00".into(),
    };
    let handle = match rt_player::PlayerHandle::spawn() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("player spawn failed: {e}");
            std::process::exit(1);
        }
    };
    let started = Instant::now();
    handle.send(rt_player::Cmd::PlayQueue(vec![track], 0));

    // Wait for audio to actually start (Loading -> Playing transition).
    let mut playing_at: Option<Duration> = None;
    let mut last_report = Instant::now();
    while started.elapsed() < Duration::from_secs(30) {
        let s = handle.status();
        if last_report.elapsed() >= Duration::from_secs(2) {
            eprintln!(
                "t={:.0}s transport={:?} error={:?}",
                started.elapsed().as_secs(),
                s.transport,
                s.error
            );
            last_report = Instant::now();
        }
        if s.transport == rt_player::TransportState::Playing && playing_at.is_none() {
            playing_at = Some(started.elapsed());
            println!("click_to_audio_ms: {}", started.elapsed().as_millis());
            break;
        }
        if s.transport == rt_player::TransportState::Stopped
            && let Some(err) = s.error
        {
            eprintln!("engine error: {err}");
            std::process::exit(1);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if playing_at.is_none() {
        eprintln!("never reached Playing within 30s");
        std::process::exit(1);
    }

    // Let it play, then check position advanced and no underruns.
    std::thread::sleep(Duration::from_secs(8));
    let s = handle.status();
    println!(
        "position_secs: {:.1} / {:.1}  underruns: {}",
        s.position_secs, s.duration_secs, s.underruns
    );

    // EQ live box: flip gains mid-playback; the engine must swap
    // coefficients without a blip (underruns stay 0, position keeps
    // advancing), then flip back through bypass.
    let eq = handle.eq().cloned();
    let pos_before = s.position_secs;
    if let Some(eq) = eq {
        let mut gains = [0.0f32; 8];
        gains[3] = 6.0; // 1 kHz, the PRD's mid-body band
        eq.set_gains_db(gains);
        std::thread::sleep(Duration::from_secs(3));
        eq.set_gains_db([0.0; 8]); // bypass-equivalent flat
        std::thread::sleep(Duration::from_secs(2));
    }
    let s2 = handle.status();
    println!(
        "after_eq position_secs: {:.1}  underruns: {}",
        s2.position_secs, s2.underruns
    );

    // Visualizer live box: the tap must carry post-EQ audio while playing.
    // Drain continuously like the UI does — the tap ring only holds ~93 ms.
    let mut analyzer = rt_audio::SpectrumAnalyzer::new();
    let mut buf = vec![0.0f32; rt_audio::ANALYSIS_LEN];
    for _ in 0..15 {
        std::thread::sleep(Duration::from_millis(100));
        handle.drain_tap(&mut buf);
    }
    let n = handle.drain_tap(&mut buf);
    let peak = buf[..n].iter().fold(0.0f32, |m, v| m.max(v.abs()));
    println!("tap_peak_amplitude: {peak:.4}");
    let mut bands = [0.0f32; rt_audio::BAND_COUNT];
    analyzer.analyze(&buf[..n], handle.device_rate(), &mut bands);
    let energy: f32 = bands.iter().sum();
    let top = bands
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .unwrap()
        .0;
    println!(
        "tap_samples: {n}  spectrum_energy: {energy:.1}  top_band_hz: {:.0}",
        rt_audio::band_center_hz(top, rt_audio::BAND_COUNT)
    );
    handle.send(rt_player::Cmd::Shutdown);
    let ok = s.position_secs > 5.0
        && s.underruns == 0
        && s2.position_secs > pos_before
        && s2.underruns == 0
        && n > 0
        && energy > 1.0;
    println!("{}", if ok { "PASS" } else { "FAIL" });
    if !ok {
        std::process::exit(1);
    }
}

/// Isolation: resolve + fetch on a multi-thread runtime, engine-free.
fn fetch_mt(video_id: &str) {
    let id = match VideoId::parse(video_id) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");
    rt.block_on(async {
        let resolver = YtDlpResolver;
        let started = Instant::now();
        let resolved = match resolver.resolve(&id).await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("resolve failed: {e}");
                std::process::exit(1);
            }
        };
        eprintln!("resolved in {} ms", started.elapsed().as_millis());
        let fetch_started = Instant::now();
        match rt_stream::fetch_full(&resolved).await {
            Ok(b) => println!(
                "fetched {} bytes in {} ms",
                b.len(),
                fetch_started.elapsed().as_millis()
            ),
            Err(e) => {
                eprintln!(
                    "fetch failed after {} ms: {e}",
                    fetch_started.elapsed().as_millis()
                );
                std::process::exit(1);
            }
        }
    });
}

/// Live box for the lyrics panel: fetch and print source, line count, and
/// the first lines for a track.
fn lyrics(video_id: &str) {
    let id = match VideoId::parse(video_id) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    rt.block_on(async {
        let client = match ApiClient::new() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("client bootstrap failed: {e}");
                std::process::exit(1);
            }
        };
        match client.lyrics_for(id.as_str()).await {
            Ok(l) => {
                let lines: Vec<&str> = l.lyrics.lines().collect();
                println!("source: {}", l.source);
                println!("line_count: {}", lines.len());
                for line in lines.iter().take(4) {
                    println!("  {line}");
                }
            }
            Err(e) => {
                println!("no lyrics: {e}");
                std::process::exit(1);
            }
        }
    });
}

/// Live OAuth device flow. Reads RT_OAUTH_ID / RT_OAUTH_SECRET, prints the
/// verification URL + code, waits for authorization, stores the token in
/// the OS credential store, then lists the account's playlists.
fn auth() {
    let (client_id, client_secret) = match rt_api::credentials_from_env() {
        Ok(pair) => pair,
        Err(e) => {
            println!("cannot start sign-in: {e}");
            std::process::exit(2);
        }
    };
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    rt.block_on(async {
        let session = match rt_api::begin_sign_in(client_id, client_secret).await {
            Ok(s) => s,
            Err(e) => {
                println!("device code request failed: {e}");
                std::process::exit(1);
            }
        };
        println!("1. open: {}", session.verification_url);
        println!("2. enter code: {}", session.user_code);
        println!("waiting for you to finish (this can take a couple of minutes)…");
        let token = match session.wait_for_authorization().await {
            Ok(t) => t,
            Err(e) => {
                println!("authorization failed: {e}");
                std::process::exit(1);
            }
        };
        if let Err(e) = rt_api::store_token(&token) {
            println!("token store failed: {e}");
            std::process::exit(1);
        }
        let client = match rt_api::AuthedClient::new(token) {
            Ok(c) => Arc::new(c),
            Err(e) => {
                println!("authenticated client failed: {e}");
                std::process::exit(1);
            }
        };
        match client.playlists().await {
            Ok(briefs) => {
                println!("signed in. playlists: {}", briefs.len());
                for b in briefs.iter().take(10) {
                    println!("  {} ({})", b.title, b.track_count);
                }
                println!("PASS");
            }
            Err(e) => {
                println!("playlist fetch failed: {e}");
                std::process::exit(1);
            }
        }
    });
}

/// `bzvibs probe login [timeout_secs]` — one-click sign-in check: opens
/// the login browser, waits for the Google login, then connects and
/// lists playlists with the harvested session.
fn login(timeout_secs: u64) {
    println!("opening the sign-in browser…");
    let browser = match rt_api::browser_login::open_login_browser() {
        Ok(b) => Arc::new(b),
        Err(e) => {
            println!("launch failed: {e}");
            std::process::exit(1);
        }
    };
    println!("sign-in window open; waiting for you to finish the Google login…");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let cookie = rt.block_on(async {
        browser
            .wait_for_cookie(Duration::from_secs(timeout_secs))
            .await
    });
    let browser = Arc::try_unwrap(browser).ok();
    drop(browser);
    let cookie = match cookie {
        Ok(c) => c,
        Err(e) => {
            println!("no session: {e}");
            std::process::exit(1);
        }
    };
    println!("session harvested; connecting…");
    rt.block_on(async {
        let client = match rt_api::AuthedClient::from_browser_cookie(&cookie).await {
            Ok(c) => c,
            Err(e) => {
                println!("session build failed: {e}");
                std::process::exit(1);
            }
        };
        match client.playlists().await {
            Ok(briefs) => {
                println!("signed in. playlists: {}", briefs.len());
                for b in briefs.iter().take(10) {
                    println!("  {} ({})", b.title, b.track_count);
                }
                println!("PASS");
            }
            Err(e) => {
                println!("playlist fetch failed: {e}");
                std::process::exit(1);
            }
        }
    });
}

/// `bzvibs probe cookie <file>` — end-to-end browser-cookie sign-in check
/// against the live API. The cookie is read from a file so it never
/// appears in argv or shell history. Exercises the exact production path
/// the UI uses: extract → validate → build → list playlists.
fn cookie(path: &str) {
    let raw = match std::fs::read_to_string(path) {
        Ok(r) => r,
        Err(e) => {
            println!("cannot read {path}: {e}");
            std::process::exit(2);
        }
    };
    let Some(cookie) = rt_api::extract_cookie(&raw) else {
        println!("nothing pasteable in that file");
        std::process::exit(2);
    };
    if let Err(e) = rt_api::validate_cookie(&cookie) {
        println!("cookie rejected: {e}");
        std::process::exit(2);
    }
    println!("cookie looks right (SAPISID present); building session…");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    rt.block_on(async {
        let client = match rt_api::AuthedClient::from_browser_cookie(&cookie).await {
            Ok(c) => c,
            Err(e) => {
                println!("session build failed: {e}");
                std::process::exit(1);
            }
        };
        // Diagnostics: which authenticated calls distinguish a real
        // session from a logged-out one. A bogus cookie silently yields
        // an empty library, so "0 playlists" alone proves nothing.
        for (name, result) in client.auth_probe().await {
            match result {
                Ok(n) => println!("  {name}: ok ({n})"),
                Err(e) => println!("  {name}: ERR {e}"),
            }
        }
        match client.playlists().await {
            Ok(briefs) => {
                println!("signed in. playlists: {}", briefs.len());
                for b in briefs.iter().take(10) {
                    println!("  {} ({})", b.title, b.track_count);
                }
                println!("PASS");
            }
            Err(e) => {
                println!("playlist fetch failed: {e}");
                std::process::exit(1);
            }
        }
    });
}
