//! Stream resolution. Native InnerTube extraction is attestation-gated
//! today (the poToken wall, measured 2026-11-18), so the primary resolver
//! runs yt-dlp as a subprocess, the design the maintained youtui player
//! also falls back on. The subprocess mechanics live in [`crate::proc`]:
//! fixed literal command line, URL over stdin. A native implementation can
//! slot in behind the trait when the ecosystem recovers.

use crate::id::VideoId;
use crate::proc;
use rt_api::guard;

/// A resolved, guarded, directly fetchable stream URL plus the request
/// headers yt-dlp itself would send. googlevideo binds some URLs (notably
/// the VISIONOS client) to a specific User-Agent; replaying the resolver's
/// headers is what keeps those fetches from returning 403.
#[derive(Debug, Clone)]
pub struct ResolvedStream {
    pub url: url::Url,
    pub headers: Vec<(String, String)>,
}

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("yt-dlp not runnable: {0}")]
    NotRunnable(String),
    #[error("yt-dlp failed: {0}")]
    YtDlp(String),
    #[error("yt-dlp produced no url")]
    EmptyOutput,
    #[error("resolved url rejected: {0}")]
    Guard(#[from] rt_api::GuardError),
}

// One implementation today, never used as a dyn object. Desugared with a
// Send bound so futures can cross into the UI runtime in PR-7.
pub trait StreamResolver {
    fn resolve(
        &self,
        id: &VideoId,
    ) -> impl std::future::Future<Output = Result<ResolvedStream, ResolveError>> + Send;
}

/// Resolves itag 140 (audio-only m4a, decodable by symphonia) via yt-dlp.
/// The binary is found through PATH (or the working directory on Windows).
#[derive(Debug, Clone, Copy, Default)]
pub struct YtDlpResolver;

impl YtDlpResolver {
    /// Cheap availability probe: an empty batch still runs the binary, so
    /// a spawn error means yt-dlp is missing while any output means present.
    pub async fn is_available(&self) -> bool {
        proc::run_with_stdin_line("").await.is_ok()
    }
}

/// Parse the resolver's output: a `--dump-json` line (preferred: carries
/// the format URL and its http_headers) or a bare URL line (fallback).
/// No guard here: the guard runs once, in `resolve`, so a fixture test of
/// parsing never touches the network.
fn parse_resolved(stdout: &str) -> Result<(url::Url, Vec<(String, String)>), ResolveError> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Err(ResolveError::EmptyOutput);
    }
    // yt-dlp -J prints one JSON object for one URL; accept a multi-line
    // pretty form too by parsing the whole capture first.
    let json_text = if trimmed.starts_with('{') {
        Some(trimmed)
    } else {
        trimmed.lines().find(|l| l.trim().starts_with('{'))
    };

    if let Some(text) = json_text
        && let Ok(json) = serde_json::from_str::<serde_json::Value>(text)
    {
        // Prefer the requested format by itag 140; fall back to `url`.
        let formats = json
            .get("formats")
            .and_then(|f| f.as_array())
            .cloned()
            .unwrap_or_default();
        let chosen = formats
            .iter()
            .find(|f| f.get("format_id").and_then(|v| v.as_str()) == Some("140"))
            .or_else(|| formats.iter().find(|f| f.get("url").is_some()))
            .cloned();
        let (url_str, headers) = match chosen {
            Some(f) => (
                f.get("url").and_then(|v| v.as_str()).map(str::to_string),
                f.get("http_headers")
                    .and_then(|h| h.as_object())
                    .map(|m| {
                        m.iter()
                            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default(),
            ),
            None => (
                json.get("url").and_then(|v| v.as_str()).map(str::to_string),
                Vec::new(),
            ),
        };
        let url_str = url_str.ok_or(ResolveError::EmptyOutput)?;
        let url = url::Url::parse(&url_str)
            .map_err(|_| ResolveError::YtDlp(format!("unparseable url: {url_str:?}")))?;
        return Ok((url, headers));
    }

    // Bare URL fallback: the first non-empty line.
    let line = trimmed
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or(trimmed)
        .trim();
    let url = url::Url::parse(line)
        .map_err(|_| ResolveError::YtDlp(format!("unparseable output: {line:?}")))?;
    Ok((url, Vec::new()))
}

impl StreamResolver for YtDlpResolver {
    async fn resolve(&self, id: &VideoId) -> Result<ResolvedStream, ResolveError> {
        let output = proc::run_with_stdin_line(&id.watch_url())
            .await
            .map_err(|e| ResolveError::NotRunnable(e.to_string()))?;
        if !output.status.success() {
            return Err(ResolveError::YtDlp(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        let (raw, headers) = parse_resolved(&String::from_utf8_lossy(&output.stdout))?;
        let guarded = guard(raw.as_str())?;
        Ok(ResolvedStream {
            url: guarded,
            headers,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_first_url_line_from_fixture_output() {
        let fixture = "\nhttps://rr3---sn-jvhh4pcgx-ajts.googlevideo.com/videoplayback?itag=140&clen=3459077\n";
        let (url, headers) = parse_resolved(fixture).unwrap();
        assert_eq!(
            url.host_str(),
            Some("rr3---sn-jvhh4pcgx-ajts.googlevideo.com")
        );
        assert!(headers.is_empty(), "bare URL carries no headers");
    }

    #[test]
    fn dump_json_yields_itag_140_url_and_its_headers() {
        // Trimmed shape of yt-dlp -J output: two formats, only 140 is m4a
        // audio. The VISIONOS UA is the header that matters for googlevideo.
        let fixture = r#"{"id":"PY2j2S_mybo","formats":[
            {"format_id":"251","url":"https://cdn.example/webm","http_headers":{"User-Agent":"audio-ua"}},
            {"format_id":"140","url":"https://rr3---sn-x.googlevideo.com/videoplayback?itag=140","http_headers":{"User-Agent":"com.google.ios.youtube/19.29.1","Accept":"*/*"}}
        ]}"#;
        let (url, headers) = parse_resolved(fixture).unwrap();
        assert_eq!(url.host_str(), Some("rr3---sn-x.googlevideo.com"));
        assert_eq!(
            url.query_pairs()
                .find(|(k, _)| k == "itag")
                .map(|(_, v)| v.to_string()),
            Some("140".into())
        );
        assert!(
            headers
                .iter()
                .any(|(k, v)| k == "User-Agent" && v.contains("google.ios")),
            "the format's UA must be replayed: {headers:?}"
        );
    }

    #[test]
    fn dump_json_without_formats_falls_back_to_top_level_url() {
        let fixture = r#"{"url":"https://rr1---sn-y.googlevideo.com/videoplayback?itag=140"}"#;
        let (url, headers) = parse_resolved(fixture).unwrap();
        assert_eq!(url.host_str(), Some("rr1---sn-y.googlevideo.com"));
        assert!(headers.is_empty());
    }

    #[test]
    fn empty_output_is_its_own_error() {
        assert!(matches!(
            parse_resolved("   \n"),
            Err(ResolveError::EmptyOutput)
        ));
    }
}
