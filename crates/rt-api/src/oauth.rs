//! Google OAuth device flow (PR-8): sign in via google.com/device, token
//! persistence in the OS credential store, and the authenticated client.
//!
//! Credentials never live in source: they are read from the environment
//! (`RT_OAUTH_ID`, `RT_OAUTH_SECRET`) at call time, per the operator
//! security directive. The device flow means the user's Google password is
//! never entered in this app — Google shows a code, the user types it on
//! google.com/device in their browser.

use crate::ApiError;
use ytmapi_rs::auth::oauth::{OAuthToken, OAuthTokenGenerator};
use ytmapi_rs::common::YoutubeID as _;
use ytmapi_rs::parse::LibraryPlaylist;
use ytmapi_rs::{Client, YtMusic, YtMusicBuilder};

/// A signed-in account: sidebar-ready playlist briefs.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaylistBrief {
    pub id: String,
    pub title: String,
    pub track_count: String,
}

impl From<LibraryPlaylist> for PlaylistBrief {
    fn from(p: LibraryPlaylist) -> Self {
        PlaylistBrief {
            id: p.playlist_id.get_raw().to_string(),
            title: p.title,
            track_count: p.tracks,
        }
    }
}

/// Read the operator's OAuth client credentials: environment first, then
/// the OS credential store (so a user can paste them in the app's setup
/// card and never touch a shell). Missing credentials are an Err with setup
/// instructions, not a panic.
pub fn credentials_from_env() -> Result<(String, String), String> {
    match (
        std::env::var("RT_OAUTH_ID"),
        std::env::var("RT_OAUTH_SECRET"),
    ) {
        (Ok(id), Ok(secret)) if !id.trim().is_empty() && !secret.trim().is_empty() => {
            Ok((id.trim().to_string(), secret.trim().to_string()))
        }
        _ => stored_client_credentials().ok_or_else(|| {
            "Google sign-in needs an OAuth client. Create one at console.cloud.google.com \
             (APIs & Services > Credentials > OAuth client ID > 'TVs and Limited Input \
             Devices'), then paste its id and secret in this card."
                .to_string()
        }),
    }
}

/// True when no client credentials exist anywhere, so the UI can show the
/// setup card instead of a bare error.
pub fn needs_client_setup() -> bool {
    credentials_from_env().is_err()
}

/// One sign-in session: what the UI shows plus what polling needs.
/// Cloneable so an `Arc` of it can cross into the polling task.
#[derive(Clone)]
pub struct SignInSession {
    pub user_code: String,
    pub verification_url: String,
    generator: OAuthTokenGenerator,
    client_id: String,
    client_secret: String,
}

impl std::fmt::Debug for SignInSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Secrets are held inside; only the user-facing code prints.
        f.debug_struct("SignInSession")
            .field("user_code", &self.user_code)
            .field("verification_url", &self.verification_url)
            .finish()
    }
}

/// Google answers OAuth errors as JSON inside the transport error text.
/// Surface the human sentence ("The OAuth client was not found.") instead
/// of the raw payload, and add the usual cause.
fn humanize_oauth_error(raw: String) -> String {
    let pick = |key: &str| -> Option<String> {
        let start = raw.find(&format!("\"{key}\""))?;
        let after = &raw[start + key.len() + 2..];
        let colon = after.find(':')?;
        let rest = after[colon + 1..].trim_start();
        let rest = rest.strip_prefix('"')?;
        let end = rest.find('"')?;
        Some(rest[..end].to_string())
    };
    let desc = pick("error_description").or_else(|| pick("error"));
    match desc {
        Some(d) => {
            let mut msg = d;
            if raw.contains("invalid_client") {
                msg.push_str(
                    " Check that RT_OAUTH_ID and RT_OAUTH_SECRET belong to an OAuth client \
                     of type 'TVs and Limited Input Devices'.",
                );
            }
            msg
        }
        None => raw,
    }
}

/// Step 1: ask Google for a device code.
pub async fn begin_sign_in(
    client_id: String,
    client_secret: String,
) -> Result<SignInSession, String> {
    let client = Client::new().map_err(|e| e.to_string())?;
    let generator = OAuthTokenGenerator::new(&client, client_id.clone())
        .await
        .map_err(|e| humanize_oauth_error(e.to_string()))?;
    Ok(SignInSession {
        user_code: generator.user_code.clone(),
        verification_url: generator.verification_url.clone(),
        generator,
        client_id,
        client_secret,
    })
}

impl SignInSession {
    /// Step 2 loop: poll Google until the user authorizes (or the device
    /// code expires). `from_code` polls once per call; authorization-pending
    /// errors retry on the generator's advertised interval.
    pub async fn wait_for_authorization(&self) -> Result<OAuthToken, String> {
        let client = Client::new().map_err(|e| e.to_string())?;
        let deadline = std::time::Instant::now()
            + std::time::Duration::from_secs(self.generator.expires_in as u64);
        let interval = std::time::Duration::from_secs((self.generator.interval as u64).max(2));
        loop {
            match OAuthToken::from_code(
                &client,
                self.generator.device_code.clone(),
                self.client_id.clone(),
                self.client_secret.clone(),
            )
            .await
            {
                Ok(token) => return Ok(token),
                Err(e) => {
                    let text = e.to_string();
                    let pending = text.contains("authorization_pending")
                        || text.contains("slow_down")
                        || text.contains("pending");
                    if pending && std::time::Instant::now() < deadline {
                        tokio::time::sleep(interval).await;
                    } else {
                        return Err(text);
                    }
                }
            }
        }
    }
}

/// Step 3: the authenticated client. Opaque on purpose — the UI never
/// sees ytmapi-rs types.
///
/// Two credentials work: the cookies of a signed-in browser session (the
/// zero-setup path, no Google Cloud project), or an OAuth device token.
pub enum AuthedClient {
    Browser(YtMusic<ytmapi_rs::auth::browser::BrowserToken>),
    OAuth(YtMusic<OAuthToken>),
}

impl std::fmt::Debug for AuthedClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthedClient").finish()
    }
}

/// Pull the cookie header out of whatever the user pasted: the raw cookie
/// string, a pasted request-headers block, or a "Copy as cURL" command.
pub fn extract_cookie(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    for line in trimmed.lines() {
        let line = line.trim();
        let lower = line.to_ascii_lowercase();
        if let Some(pos) = lower.find("cookie:") {
            let rest = &line[pos + "cookie:".len()..];
            // A cURL paste wraps the header in quotes; a cookie value never
            // contains one, so the first quote ends the value.
            let end = rest.find(['\'', '"']).unwrap_or(rest.len());
            let value = rest[..end].trim().to_string();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    Some(trimmed.to_string())
}

/// Cookie names the music session actually needs. Windows Credential
/// Manager caps one secret at 2560 UTF-16 chars and a full YouTube jar
/// (analytics, prefs, visitor ids, widevine…) overflows that; this
/// curated set authenticates the InnerTube API and fits comfortably.
const SESSION_COOKIES: [&str; 15] = [
    "SAPISID",
    "APISID",
    "SID",
    "HSID",
    "SSID",
    "SIDCC",
    "__Secure-1PAPISID",
    "__Secure-3PAPISID",
    "__Secure-1PSID",
    "__Secure-3PSID",
    "__Secure-1PSIDTS",
    "__Secure-3PSIDTS",
    "__Secure-1PSIDCC",
    "__Secure-3PSIDCC",
    "LOGIN_INFO",
];

/// True when the cookie name participates in the music session.
fn is_session_cookie(name: &str) -> bool {
    SESSION_COOKIES.iter().any(|keep| *keep == name)
}

/// Trim a raw cookie header down to the session cookies. Used both for
/// the credential we send the API and for the one we persist, so the
/// Windows Credential Manager 2560-char secret limit cannot reject it.
pub fn trim_to_session_cookies(header: &str) -> String {
    header
        .split(';')
        .filter_map(|part| {
            let part = part.trim();
            let (name, value) = part.split_once('=')?;
            is_session_cookie(name.trim()).then(|| format!("{}={}", name.trim(), value.trim()))
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Cookies must carry SAPISID; without it signed requests are treated as
/// logged out.
pub fn validate_cookie(cookie: &str) -> Result<(), String> {
    if cookie.contains("SAPISID=") {
        Ok(())
    } else if cookie.contains("__Secure-3PAPISID=") {
        Err(
            "that cookie has __Secure-3PAPISID but not SAPISID. Copy the cookie \
             header from a request made while signed in at music.youtube.com."
                .to_string(),
        )
    } else {
        Err(
            "that does not look like a signed-in cookie: SAPISID is missing. Open \
             music.youtube.com signed in, press F12, open Network, click a request \
             and copy its 'cookie' request header."
                .to_string(),
        )
    }
}

/// ytmapi reports a failed browser-session bootstrap as "Error parsing
/// header." — true (the music.youtube.com scrape found no
/// INNERTUBE_CLIENT_VERSION) but useless to a human holding a pasted
/// cookie. Translate the common failures into the action that fixes them.
fn humanize_browser_error(raw: String) -> String {
    let lower = raw.to_ascii_lowercase();
    if raw.contains("Error parsing header") {
        "couldn't start a session with that cookie. Copy the full 'cookie' request \
         header from music.youtube.com while signed in (it must contain SAPISID) \
         and paste it again."
            .to_string()
    } else if lower.contains("error sending request")
        || lower.contains("dns")
        || lower.contains("connection")
    {
        "couldn't reach music.youtube.com — check the network and try again.".to_string()
    } else {
        raw
    }
}

/// Client version reported by the live music.youtube.com shell on
/// 2026-11-21. The API tolerates version drift; this only seeds the
/// request context when the best-effort live scrape below cannot run.
const FALLBACK_CLIENT_VERSION: &str = "1.20261117.06.00";

/// The SAPISID value out of a cookie header, mirroring the parse ytmapi
/// itself does. `SAPISID=` cannot match inside `__Secure-3PAPISID=`
/// (the char before APISID there is 'P', not 'S').
fn parse_sapisid(cookie: &str) -> Option<String> {
    let after = cookie.split_once("SAPISID=")?.1;
    let value = after.split_once(';').map_or(after, |(v, _)| v).trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// Best-effort live scrape of the client version. Any failure simply
/// falls back to the pinned constant: the sign-in must not hinge on the
/// homepage shell, which redirects and reshapes at will.
async fn live_client_version() -> Option<String> {
    const UA: &str =
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:132.0) Gecko/20100101 Firefox/132.0";
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .ok()?;
    let page = client
        .get("https://music.youtube.com/")
        .header("User-Agent", UA)
        .send()
        .await
        .ok()?
        .text()
        .await
        .ok()?;
    let after = page.split_once("INNERTUBE_CLIENT_VERSION\":\"")?.1;
    let version = after.split_once('"')?.0;
    (!version.is_empty()).then(|| version.to_string())
}

impl AuthedClient {
    /// Sign in with a browser session's cookie header. No Google Cloud
    /// project is involved, which is why this is the primary path.
    ///
    /// The token is assembled locally instead of via ytmapi's
    /// `BrowserToken::from_str`: that path fetches music.youtube.com and
    /// string-parses `INNERTUBE_CLIENT_VERSION` out of the page shell —
    /// the same page-shell brittleness already worked around for
    /// `NoAuthToken` (see `BOOTSTRAP_TOKEN_JSON`). A redirect, consent
    /// wall or A/B shell change there must not fail a sign-in that holds
    /// a perfectly good cookie; the credential proves itself on the first
    /// API call (listing playlists), which is where a genuinely bad
    /// cookie surfaces.
    pub async fn from_browser_cookie(cookie: &str) -> Result<Self, String> {
        validate_cookie(cookie)?;
        // Send (and, via store_browser_cookie, persist) only the
        // session cookies: a full jar overflows the Windows
        // Credential Manager secret limit and carries nothing the
        // InnerTube session needs.
        let cookie = trim_to_session_cookies(cookie);
        if !cookie.contains("SAPISID=") {
            return Err(
                "that cookie has SAPISID= but no value for it — copy the whole header again."
                    .to_string(),
            );
        }
        let sapisid = parse_sapisid(&cookie).ok_or_else(|| {
            "that cookie has SAPISID= but no value for it — copy the whole header again."
                .to_string()
        })?;
        let client_version =
            live_client_version().await.unwrap_or_else(|| FALLBACK_CLIENT_VERSION.to_string());
        // Field names are the private BrowserToken fields; the derived
        // Deserialize constructs it exactly as from_str would have.
        let token_json = serde_json::json!({
            "sapisid": sapisid,
            "client_version": client_version,
            "cookies": cookie,
        });
        let token: ytmapi_rs::auth::browser::BrowserToken =
            serde_json::from_value(token_json).map_err(|e| e.to_string())?;
        // UFCS: the generic AuthToken build consumes the token without
        // any network bootstrap.
        let inner = YtMusicBuilder::<ytmapi_rs::auth::browser::BrowserToken>::build(
            YtMusicBuilder::new().with_auth_token(token),
        )
        .map_err(|e| humanize_browser_error(e.to_string()))?;
        // A cookie YouTube ignores still builds and "works": every library
        // call silently returns empty, which the UI rendered as a signed-in
        // account with zero playlists and an eternal "loading playlists…"
        // line. The history response has a different shape when logged out
        // (its parse fails with a missing-key error, verified live
        // 2026-11-21), so that failure is the "not actually signed in"
        // signal. Rare casualty: a genuinely empty history may parse the
        // same way — the error message asks for a re-paste, not a bug
        // report.
        if let Err(e) = inner.get_history().await {
            let text = e.to_string();
            if text.contains("not found in Api response") {
                return Err(
                    "that cookie did not authenticate — YouTube treated it as signed out. \
                     Copy the whole 'cookie' request header from music.youtube.com while \
                     signed in and paste it again."
                        .to_string(),
                );
            }
            return Err(humanize_browser_error(text));
        }
        Ok(AuthedClient::Browser(inner))
    }

    pub fn new(token: OAuthToken) -> Result<Self, ApiError> {
        let builder = YtMusicBuilder::new().with_auth_token(token);
        // UFCS: pick the sync build over the async bootstrap build.
        let inner = YtMusicBuilder::<OAuthToken>::build(builder)?;
        Ok(AuthedClient::OAuth(inner))
    }

    /// Diagnostics for `bzvibs probe cookie`: run a few authenticated
    /// calls and report their outcomes. A logged-out session silently
    /// returns empty library lists, so these separate a real session
    /// from a cookie YouTube ignores.
    pub async fn auth_probe(&self) -> Vec<(&'static str, Result<usize, String>)> {
        async fn probe<T: ytmapi_rs::auth::LoggedIn>(
            c: &YtMusic<T>,
        ) -> Vec<(&'static str, Result<usize, String>)> {
            vec![
                (
                    "history",
                    c.get_history().await.map(|v| v.len()).map_err(|e| e.to_string()),
                ),
                (
                    "library_artists",
                    c.get_library_artists()
                        .await
                        .map(|v| v.len())
                        .map_err(|e| e.to_string()),
                ),
                (
                    "library_songs",
                    c.get_library_songs().await.map(|v| v.len()).map_err(|e| e.to_string()),
                ),
            ]
        }
        match self {
            AuthedClient::Browser(c) => probe(c).await,
            AuthedClient::OAuth(c) => probe(c).await,
        }
    }

    /// Sidebar briefs for the signed-in account's playlists.
    pub async fn playlists(&self) -> Result<Vec<PlaylistBrief>, ApiError> {
        let lists = match self {
            AuthedClient::Browser(c) => c.get_library_playlists().await?,
            AuthedClient::OAuth(c) => c.get_library_playlists().await?,
        };
        Ok(lists.into_iter().map(PlaylistBrief::from).collect())
    }

    /// Tracks of one playlist, mapped to the owned Track model. Non-song
    /// entries (videos) map with their channel as the artist.
    pub async fn playlist_tracks(&self, playlist_id: &str) -> Result<Vec<crate::Track>, String> {
        let id = crate::PlaylistID::from_raw(playlist_id.to_string());
        let items = match self {
            AuthedClient::Browser(c) => {
                c.get_playlist_tracks(id).await.map_err(|e| e.to_string())?
            }
            AuthedClient::OAuth(c) => c.get_playlist_tracks(id).await.map_err(|e| e.to_string())?,
        };
        Ok(items
            .into_iter()
            .filter_map(|item| match item {
                ytmapi_rs::parse::PlaylistItem::Song(s) => Some(crate::Track {
                    video_id: s.video_id.get_raw().to_string(),
                    title: s.title,
                    artist: s
                        .artists
                        .iter()
                        .map(|a| a.name.clone())
                        .collect::<Vec<_>>()
                        .join(", "),
                    album: Some(s.album.name),
                    duration: s.duration,
                }),
                ytmapi_rs::parse::PlaylistItem::Video(v) => Some(crate::Track {
                    video_id: v.video_id.get_raw().to_string(),
                    title: v.title,
                    artist: v.channel_name,
                    album: None,
                    duration: v.duration,
                }),
                _ => None,
            })
            .collect())
    }

    /// Create a private playlist on the signed-in account. Returns its id.
    pub async fn create_playlist(&self, title: &str) -> Result<String, String> {
        let query = ytmapi_rs::query::CreatePlaylistQuery::new(
            title,
            None,
            ytmapi_rs::query::playlist::PrivacyStatus::Private,
        );
        let id = match self {
            AuthedClient::Browser(c) => {
                c.create_playlist(query).await.map_err(|e| e.to_string())?
            }
            AuthedClient::OAuth(c) => c.create_playlist(query).await.map_err(|e| e.to_string())?,
        };
        Ok(id.get_raw().to_string())
    }
}

// ---------- credential storage (Windows Credential Manager via keyring) ----------

const KEYRING_SERVICE: &str = "bzvibs";
const KEYRING_USER: &str = "youtube-oauth-token";
const KEYRING_CLIENT_ID: &str = "oauth-client-id";
const KEYRING_CLIENT_SECRET: &str = "oauth-client-secret";
const KEYRING_COOKIE: &str = "browser-cookie";

/// Fallback store for the session cookie when the OS credential store
/// rejects it: `%LOCALAPPDATA%\bzvibs\session-cookie.txt`. The user
/// profile directory is per-account and ACL-protected by Windows, which
/// is a weaker guarantee than the credential manager but an honest one
/// for a session credential that must exceed the manager's 2560-char
/// secret limit after trimming.
fn cookie_file() -> Option<std::path::PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .map(|base| base.join("bzvibs").join("session-cookie.txt"))
}

/// Persist a browser cookie header so the session restores on next
/// launch. Only the session cookies are kept (see
/// [`trim_to_session_cookies`]); if the credential store still refuses
/// the value, it lands in the app-local fallback file instead — a live
/// session must not be lost to a storage size limit.
pub fn store_browser_cookie(cookie: &str) -> Result<(), StoreError> {
    let trimmed = trim_to_session_cookies(cookie);
    if trimmed.is_empty() {
        return Err(StoreError::Custom("no session cookies in header".to_string()));
    }
    match keyring::Entry::new(KEYRING_SERVICE, KEYRING_COOKIE)
        .and_then(|entry| entry.set_password(&trimmed))
    {
        Ok(()) => {
            let _ = std::fs::remove_file(cookie_file().unwrap_or_default());
            Ok(())
        }
        Err(e) => {
            let path = cookie_file()
                .ok_or_else(|| StoreError::Custom("no local app data directory".to_string()))?;
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, &trimmed)?;
            eprintln!("[auth] credential store refused the cookie ({e}); using {path_display}", path_display = path.display());
            Ok(())
        }
    }
}

pub fn load_browser_cookie() -> Option<String> {
    if let Some(cookie) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_COOKIE)
        .ok()
        .and_then(|entry| entry.get_password().ok())
        .filter(|c| !c.trim().is_empty())
    {
        return Some(cookie);
    }
    let cookie = std::fs::read_to_string(cookie_file()?).ok()?;
    (!cookie.trim().is_empty()).then(|| cookie.trim().to_string())
}

pub fn clear_browser_cookie() {
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_COOKIE) {
        let _ = entry.delete_credential();
    }
    if let Some(path) = cookie_file() {
        let _ = std::fs::remove_file(path);
    }
}

/// Persist the OAuth client id and secret in the OS credential store. The
/// app then signs in without any environment variables.
pub fn store_client_credentials(id: &str, secret: &str) -> Result<(), StoreError> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_CLIENT_ID)?.set_password(id.trim())?;
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_CLIENT_SECRET)?.set_password(secret.trim())?;
    Ok(())
}

pub fn stored_client_credentials() -> Option<(String, String)> {
    let id = keyring::Entry::new(KEYRING_SERVICE, KEYRING_CLIENT_ID)
        .ok()?
        .get_password()
        .ok()?;
    let secret = keyring::Entry::new(KEYRING_SERVICE, KEYRING_CLIENT_SECRET)
        .ok()?
        .get_password()
        .ok()?;
    (!id.trim().is_empty() && !secret.trim().is_empty()).then_some((id, secret))
}

pub fn clear_client_credentials() {
    for user in [KEYRING_CLIENT_ID, KEYRING_CLIENT_SECRET] {
        if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, user) {
            let _ = entry.delete_credential();
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("credential store: {0}")]
    Keyring(#[from] keyring::Error),
    #[error("stored token parse: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("session store: {0}")]
    Custom(String),
    #[error("fallback file: {0}")]
    File(#[from] std::io::Error),
}

/// Persist the token JSON in the OS credential store.
pub fn store_token(token: &OAuthToken) -> Result<(), StoreError> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)?;
    entry.set_password(&serde_json::to_string(token)?)?;
    Ok(())
}

/// Load a previously stored token, if any.
pub fn load_token() -> Result<Option<OAuthToken>, StoreError> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)?;
    match entry.get_password() {
        Ok(json) => Ok(Some(serde_json::from_str(&json)?)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Remove the stored token (disconnect). Missing is not an error.
pub fn clear_token() -> Result<(), StoreError> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_env_is_an_instructions_error_not_a_panic() {
        // The test process normally has neither var set; if the operator's
        // shell does, the function must still return Ok with their values.
        match (
            std::env::var("RT_OAUTH_ID"),
            std::env::var("RT_OAUTH_SECRET"),
        ) {
            (Ok(id), Ok(secret)) if !id.trim().is_empty() && !secret.trim().is_empty() => {
                assert!(credentials_from_env().is_ok());
                let _ = (id, secret);
            }
            _ => {
                let err = credentials_from_env().unwrap_err();
                assert!(err.contains("console.cloud.google.com"), "{err}");
            }
        }
    }

    #[test]
    fn token_survives_a_keyring_round_trip() {
        // A fabricated token (fixture values assembled at runtime, never a
        // real credential) through the same store the app uses.
        let json = format!(
            r#"{{"token_type":"Bearer","access_token":"{}","refresh_token":"{}","expires_in":3600,"request_time":{{"secs_since_epoch":1795000000,"nanos_since_epoch":0}},"client_id":"{}","client_secret":"{}"}}"#,
            "probe-access", "probe-refresh", "probe-id", "probe-secret"
        );
        let token: OAuthToken = serde_json::from_str(&json).expect("fabricated token parses");
        let entry = keyring::Entry::new(KEYRING_SERVICE, "test-roundtrip").unwrap();
        entry
            .set_password(&serde_json::to_string(&token).unwrap())
            .unwrap();
        let loaded = entry.get_password().unwrap();
        let back: OAuthToken = serde_json::from_str(&loaded).unwrap();
        // PartialEq on OAuthToken confirms the round trip preserved it.
        assert_eq!(back, token);
        let _ = entry.delete_credential();
    }

    #[test]
    fn trim_keeps_only_session_cookies() {
        // The Windows credential store rejects secrets over 2560
        // chars; a full YouTube jar overflows it, the curated session
        // set must not.
        let header = "YSC=abc; SAPISID=aa/bb; APISID=ap; SID=long1; HSID=h; SSID=s; \
                      SIDCC=cc; __Secure-3PSID=p3; __Secure-1PAPISID=pa1; \
                      PREF=f2b4; VISITOR_INFO1_LIVE=v; WID=w; LOGIN_INFO=li; NID=n";
        let trimmed = trim_to_session_cookies(header);
        assert!(trimmed.contains("SAPISID=aa/bb"), "{trimmed}");
        assert!(trimmed.contains("__Secure-3PSID=p3"), "{trimmed}");
        assert!(trimmed.contains("LOGIN_INFO=li"), "{trimmed}");
        for dropped in ["YSC=", "PREF=", "VISITOR_INFO1_LIVE=", "WID=", "NID="] {
            assert!(!trimmed.contains(dropped), "{trimmed}");
        }
        assert!(trimmed.len() < 2560);
        // A header with none of the session cookies trims to nothing.
        assert_eq!(trim_to_session_cookies("PREF=x; YSC=y"), "");
    }

    #[test]
    fn extract_cookie_handles_raw_header_and_curl_pastes() {
        let raw = "YSC=abc; SAPISID=xyz; __Secure-3PAPISID=def";
        assert_eq!(extract_cookie(raw).as_deref(), Some(raw));

        let headers = "accept: */*\ncookie: YSC=abc; SAPISID=xyz\naccept-language: en";
        assert_eq!(
            extract_cookie(headers).as_deref(),
            Some("YSC=abc; SAPISID=xyz")
        );

        let curl = "curl 'https://music.youtube.com/' -H 'cookie: YSC=1; SAPISID=2' -H 'x: y'";
        assert_eq!(extract_cookie(curl).as_deref(), Some("YSC=1; SAPISID=2"));

        assert_eq!(extract_cookie("   "), None);
    }

    #[test]
    fn validate_cookie_requires_sapisid_and_explains_failures() {
        assert!(validate_cookie("YSC=1; SAPISID=2; HSID=3").is_ok());
        // The modern cookie name alone is not enough.
        let e = validate_cookie("YSC=1; __Secure-3PAPISID=2").unwrap_err();
        assert!(e.contains("SAPISID"), "{e}");
        // Random text is rejected with instructions.
        let e = validate_cookie("hello world").unwrap_err();
        assert!(e.contains("F12"), "{e}");
    }

    #[test]
    fn browser_bootstrap_errors_become_instructions() {
        // ytmapi's "Error parsing header." verbatim helps nobody.
        let e = humanize_browser_error("Error parsing header.".to_string());
        assert!(e.contains("request header"), "{e}");
        let e = humanize_browser_error(
            "error sending request for url (https://music.youtube.com/)".to_string(),
        );
        assert!(e.contains("network"), "{e}");
        // Anything else passes through untouched.
        assert_eq!(humanize_browser_error("boom".to_string()), "boom");
    }

    #[test]
    fn parse_sapisid_reads_the_real_cookie_not_the_3papisid() {
        let c = "YSC=1; SAPISID=abc/xyz; __Secure-3PAPISID=other";
        assert_eq!(parse_sapisid(c).as_deref(), Some("abc/xyz"));
        // Value at the end of the header, without a trailing ';'.
        assert_eq!(parse_sapisid("SAPISID=tail").as_deref(), Some("tail"));
        // Empty value and the __Secure- namesake are not a SAPISID.
        assert_eq!(parse_sapisid("SAPISID=;").as_deref(), None);
        assert_eq!(parse_sapisid("__Secure-3PAPISID=only").as_deref(), None);
    }

    #[test]
    fn injected_browser_token_carries_the_cookie_into_its_headers() {
        // The serde-injection contract: these field names are
        // BrowserToken's private fields. If a future ytmapi renames them,
        // this breaks here — not inside a user's sign-in.
        let token: ytmapi_rs::auth::browser::BrowserToken =
            serde_json::from_value(serde_json::json!({
                "sapisid": "aa/bb",
                "client_version": "1.20261117.06.00",
                "cookies": "YSC=1; SAPISID=aa/bb",
            }))
            .expect("field names still match BrowserToken");
        use ytmapi_rs::auth::AuthToken as _;
        let headers: Vec<_> = token.headers().expect("headers build").into_iter().collect();
        let (_, cookie) = headers
            .iter()
            .find(|(k, _)| *k == "Cookie")
            .expect("Cookie header present");
        assert_eq!(cookie.as_ref(), "YSC=1; SAPISID=aa/bb");
        let (_, auth) = headers
            .iter()
            .find(|(k, _)| *k == "Authorization")
            .expect("Authorization header present");
        assert!(auth.starts_with("SAPISIDHASH "), "got {auth}");
    }
}
