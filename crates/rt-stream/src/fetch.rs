//! Fetching bytes for an already-guarded stream URL. Redirects are
//! disabled: the guard validated exactly this URL, not any hop after it.

use crate::ytdlp::ResolvedStream;

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("unexpected status {0}")]
    Status(u16),
    #[error("stream ended after {0} bytes, expected {1}")]
    Truncated(u64, u64),
}

fn client() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
}

const UA: &str = "Mozilla/5.0";
const CHUNK: u64 = 512 * 1024;

/// Build a request carrying the resolver's headers (they are what the CDN
/// expects; a URL resolved for a non-browser client answers 403 to a plain
/// browser UA) plus this module's own Range header.
fn request(
    client: &reqwest::Client,
    resolved: &ResolvedStream,
    range: Option<String>,
) -> reqwest::RequestBuilder {
    let mut rb = client.get(resolved.url.clone());
    if resolved.headers.is_empty() {
        rb = rb.header("User-Agent", UA);
    } else {
        for (k, v) in &resolved.headers {
            // Range is ours to set; skip anything the resolver sent for it.
            if !k.eq_ignore_ascii_case("range") {
                rb = rb.header(k, v);
            }
        }
    }
    if let Some(r) = range {
        rb = rb.header("Range", r);
    }
    rb
}

/// Fetch the whole stream into memory. itag-140 tracks are ~2 MB per
/// minute, so whole-track fetch matches the decode-everything pipeline.
///
/// Fetched as ranged chunks: the CDN honors ranged reads reliably while
/// unbounded full-body reads can stall (measured 2026-11-18), and chunked
/// downloads are also what the youtui player uses.
pub async fn fetch_full(resolved: &ResolvedStream) -> Result<Vec<u8>, FetchError> {
    let client = client()?;

    // Probe the total size with a one-byte range; Content-Range carries
    // "bytes 0-0/<total>".
    let probe = request(&client, resolved, Some("bytes=0-0".into()))
        .send()
        .await?
        .error_for_status()?;
    let total: u64 = probe
        .headers()
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit('/').next())
        .and_then(|s| s.trim().parse().ok())
        .ok_or(FetchError::Truncated(0, 0))?;

    let mut out = Vec::with_capacity(total as usize);
    let mut pos = 0u64;
    while pos < total {
        let end = (pos + CHUNK - 1).min(total - 1);
        let range = format!("bytes={pos}-{end}");
        let mut chunk: Option<Vec<u8>> = None;
        let mut last_err: Option<reqwest::Error> = None;
        for attempt in 0..3u32 {
            match request(&client, resolved, Some(range.clone())).send().await {
                Ok(resp) if resp.status().is_success() => match resp.bytes().await {
                    Ok(b) => {
                        chunk = Some(b.to_vec());
                        break;
                    }
                    Err(e) => last_err = Some(e),
                },
                Ok(resp) => {
                    // A 3xx lands here too: error_for_status() treats
                    // redirects as Ok, so it cannot be the error source.
                    let status = resp.status();
                    match resp.error_for_status() {
                        Err(e) => last_err = Some(e),
                        Ok(_) => return Err(FetchError::Status(status.as_u16())),
                    }
                }
                Err(e) => last_err = Some(e),
            }
            tokio::time::sleep(std::time::Duration::from_millis(
                300 * u64::from(attempt + 1),
            ))
            .await;
        }
        let bytes = chunk.ok_or_else(|| {
            last_err
                .map(FetchError::Http)
                .unwrap_or(FetchError::Truncated(pos, total))
        })?;
        let want = (end - pos + 1) as usize;
        if bytes.len() != want {
            return Err(FetchError::Truncated(pos + bytes.len() as u64, total));
        }
        out.extend_from_slice(&bytes);
        pos = end + 1;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fetch_rejects_redirects() {
        // A resolved URL that answers 3xx must surface as an HTTP error,
        // never be followed.
        let url = url::Url::parse("https://rr3.googlevideo.com/videoplayback?itest=1").unwrap();
        let resolved = ResolvedStream {
            url,
            headers: Vec::new(),
        };
        // The host is allowlisted syntactically but does not resolve in
        // tests without network; a DNS failure is fine, a redirect is not.
        match fetch_full(&resolved).await {
            Err(FetchError::Http(_)) => {}
            other => panic!("expected http error, got {other:?}"),
        }
    }
}
