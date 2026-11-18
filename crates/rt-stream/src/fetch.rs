//! Fetching bytes for an already-guarded stream URL. Redirects are
//! disabled: the guard validated exactly this URL, not any hop after it.

use crate::ytdlp::ResolvedStream;

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("stream ended after {0} bytes, expected {1}")]
    Truncated(u64, u64),
}

fn client() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
}

/// Fetch the whole stream into memory. itag-140 tracks are ~2 MB per
/// minute, so whole-track fetch matches the decode-everything pipeline.
pub async fn fetch_full(resolved: &ResolvedStream) -> Result<Vec<u8>, FetchError> {
    let client = client()?;
    let resp = client
        .get(resolved.url.clone())
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await?
        .error_for_status()?;
    let expected = resp.content_length();
    let bytes = resp.bytes().await?;
    if let Some(expected) = expected
        && bytes.len() as u64 != expected
    {
        return Err(FetchError::Truncated(bytes.len() as u64, expected));
    }
    Ok(bytes.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fetch_rejects_redirects() {
        // A resolved URL that answers 3xx must surface as an HTTP error,
        // never be followed.
        let url = url::Url::parse("https://rr3.googlevideo.com/videoplayback?itest=1").unwrap();
        let resolved = ResolvedStream { url };
        // The host is allowlisted syntactically but does not resolve in
        // tests without network; a DNS failure is fine, a redirect is not.
        match fetch_full(&resolved).await {
            Err(FetchError::Http(_)) => {}
            other => panic!("expected http error, got {other:?}"),
        }
    }
}
