//! Live probes for the plan's verification boxes. Development surface, not
//! product UI.

use rt_api::{ApiClient, Track};
use std::time::Instant;

pub fn run(args: &[String]) {
    match (
        args.first().map(String::as_str),
        args.get(1).map(String::as_str),
    ) {
        (Some("search"), Some(query)) => search(query),
        _ => {
            eprintln!("usage: rustytube probe search <query>");
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
