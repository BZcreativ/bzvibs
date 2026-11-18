// Prototype A (throwaway): can ytmapi-rs search YouTube Music unauthenticated,
// and can rusty_ytdl hand us a decodable audio stream for the top result?
use rusty_ytdl::{Video, VideoOptions, VideoQuality, VideoSearchOptions};
use std::error::Error;
use ytmapi_rs::auth::noauth::NoAuthToken;
use ytmapi_rs::common::YoutubeID;
use ytmapi_rs::{YtMusic, YtMusicBuilder};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // new_unauthenticated() fetches music.youtube.com and parses ytcfg.set({ from
    // the page shell. That shell no longer carries ytcfg, so bootstrap fails with
    // "Unable to parse ytcfg". Workaround under test: inject a NoAuthToken via
    // serde (its fields are private but Deserialize-derived) and skip the page fetch.
    let token: NoAuthToken =
        serde_json::from_str(r#"{"create_time":"2026-11-18T12:00:00Z","visitor_id":"rustytube-probe"}"#)?;
    let ytm: YtMusic<NoAuthToken> = YtMusicBuilder::new().with_auth_token(token).build()?;
    println!("[1] client created (injected NoAuthToken, no page bootstrap)");

    let results = ytm.search_songs("never gonna give you up").await?;
    println!("[1] search_songs returned {} songs", results.len());
    for s in results.iter().take(3) {
        println!("    {} - {} [{}] ({})", s.title, s.artist, s.video_id.get_raw(), s.duration);
    }
    if results.is_empty() {
        return Err("search returned no songs".into());
    }

    let top = &results[0];
    println!("[2] resolving audio stream for '{}'", top.title);

    // List every format rusty_ytdl can see (proves extraction + deciphering work),
    // then pick the m4a audio format by hand below.
    let list_options = VideoOptions {
        quality: VideoQuality::HighestAudio,
        filter: VideoSearchOptions::Audio,
        ..Default::default()
    };
    let video = Video::new_with_options(top.video_id.get_raw().to_string(), list_options)?;
    let info = video.get_info().await?;
    println!("[2] get_info ok: '{}' by {:?}", info.video_details.title, info.video_details.author);
    for f in &info.formats {
        println!(
            "    fmt itag={} audio={} video={} container={} bitrate={}",
            f.itag, f.has_audio, f.has_video, f.mime_type.container, f.bitrate
        );
    }

    // rusty_ytdl 0.7.4 Custom filter + empty get_info URLs both point at the
    // poToken experiment. rusty_ytdl has its own IOS-client fallback — try the
    // built-in Audio filter so that fallback runs, and log what we get.
    match video.stream().await {
        Ok(stream) => {
            println!("[2] stream() ok, content_length = {:?}", stream.content_length());
            let mut head: Vec<u8> = Vec::new();
            while head.len() < 256 * 1024 {
                match stream.chunk().await {
                    Ok(Some(bytes)) => head.extend_from_slice(&bytes),
                    Ok(None) => break,
                    Err(e) => {
                        println!("[3] chunk error after {} bytes: {e}", head.len());
                        break;
                    }
                }
            }
            let magic = &head[..16.min(head.len())];
            println!("[3] first 16 bytes: {:02X?}", magic);
            println!(
                "[3] container: {}",
                if head.len() >= 12 && &head[4..8] == b"ftyp" { "m4a (ftyp)" } else if head.starts_with(&[0x1A,0x45,0xDF,0xA3]) { "webm/mkv (EBML)" } else { "unknown" }
            );
            std::fs::write("stream_head.bin", &head)?;
            println!("[3] wrote {} bytes to stream_head.bin", head.len());
        }
        Err(e) => println!("[2] stream() with built-in Audio filter FAILED: {e}"),
    }
    Ok(())
}
