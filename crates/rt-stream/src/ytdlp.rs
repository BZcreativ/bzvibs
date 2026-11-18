//! Stream resolution. Native InnerTube extraction is attestation-gated
//! today (the poToken wall, measured 2026-11-18), so the primary resolver
//! runs yt-dlp as a subprocess, the design the maintained youtui player
//! also falls back on. The subprocess mechanics live in [`crate::proc`]:
//! fixed literal command line, URL over stdin. A native implementation can
//! slot in behind the trait when the ecosystem recovers.

use crate::id::VideoId;
use crate::proc;
use rt_api::guard;

/// A resolved, guarded, directly fetchable stream URL.
#[derive(Debug, Clone)]
pub struct ResolvedStream {
    pub url: url::Url,
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

/// Extract the first URL line from resolver stdout. No guard here: the
/// guard runs once, in `resolve`, so a fixture test of parsing never
/// touches the network.
fn parse_resolve_output(stdout: &str) -> Result<url::Url, ResolveError> {
    let line = stdout
        .lines()
        .find(|l| !l.trim().is_empty())
        .ok_or(ResolveError::EmptyOutput)?;
    url::Url::parse(line.trim())
        .map_err(|_| ResolveError::YtDlp(format!("unparseable output: {line:?}")))
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
        let raw = parse_resolve_output(&String::from_utf8_lossy(&output.stdout))?;
        let guarded = guard(raw.as_str())?;
        Ok(ResolvedStream { url: guarded })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_first_url_line_from_fixture_output() {
        let fixture = "\nhttps://rr3---sn-jvhh4pcgx-ajts.googlevideo.com/videoplayback?itag=140&clen=3459077\n";
        let url = parse_resolve_output(fixture).unwrap();
        assert_eq!(
            url.host_str(),
            Some("rr3---sn-jvhh4pcgx-ajts.googlevideo.com")
        );
    }

    #[test]
    fn empty_output_is_its_own_error() {
        assert!(matches!(
            parse_resolve_output("   \n"),
            Err(ResolveError::EmptyOutput)
        ));
    }
}
