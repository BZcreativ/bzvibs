//! Live probes for the plan's verification boxes. Development surface, not
//! product UI.

use rt_api::{ApiClient, Track};
use rt_stream::{StreamResolver, VideoId, YtDlpResolver};
use std::time::Instant;

pub fn run(args: &[String]) {
    match (
        args.first().map(String::as_str),
        args.get(1).map(String::as_str),
    ) {
        (Some("search"), Some(query)) => search(query),
        (Some("stream"), Some(video_id)) => stream(video_id),
        _ => {
            eprintln!("usage: rustytube probe search <query> | rustytube probe stream <video_id>");
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
