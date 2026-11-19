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
pub struct AuthedClient {
    inner: YtMusic<OAuthToken>,
}

impl std::fmt::Debug for AuthedClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthedClient").finish()
    }
}

impl AuthedClient {
    pub fn new(token: OAuthToken) -> Result<Self, ApiError> {
        let builder = YtMusicBuilder::new().with_auth_token(token);
        // UFCS: pick the sync build over the async bootstrap build.
        let inner = YtMusicBuilder::<OAuthToken>::build(builder)?;
        Ok(AuthedClient { inner })
    }

    /// Sidebar briefs for the signed-in account's playlists.
    pub async fn playlists(&self) -> Result<Vec<PlaylistBrief>, ApiError> {
        let lists = self.inner.get_library_playlists().await?;
        Ok(lists.into_iter().map(PlaylistBrief::from).collect())
    }

    /// Tracks of one playlist, mapped to the owned Track model. Non-song
    /// entries (videos) map with their channel as the artist.
    pub async fn playlist_tracks(&self, playlist_id: &str) -> Result<Vec<crate::Track>, String> {
        let id = crate::PlaylistID::from_raw(playlist_id.to_string());
        let items = self
            .inner
            .get_playlist_tracks(id)
            .await
            .map_err(|e| e.to_string())?;
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
        let id = self
            .inner
            .create_playlist(query)
            .await
            .map_err(|e| e.to_string())?;
        Ok(id.get_raw().to_string())
    }
}

// ---------- credential storage (Windows Credential Manager via keyring) ----------

const KEYRING_SERVICE: &str = "bzvibs";
const KEYRING_USER: &str = "youtube-oauth-token";
const KEYRING_CLIENT_ID: &str = "oauth-client-id";
const KEYRING_CLIENT_SECRET: &str = "oauth-client-secret";

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
}
