//! InnerTube client for YouTube Music. Wraps `ytmapi-rs` with the bootstrap
//! workaround, an owned domain model, and the shared URL guard.

pub mod browser_login;
pub mod guard;
pub mod model;
pub mod oauth;

pub use guard::{GuardError, guard};
pub use model::Track;
pub use oauth::{
    AuthedClient, PlaylistBrief, SignInSession, begin_sign_in, clear_browser_cookie, clear_token,
    credentials_from_env, extract_cookie, load_browser_cookie, load_token, needs_client_setup,
    store_browser_cookie, store_client_credentials, store_token, stored_client_credentials,
    validate_cookie,
};
pub use ytmapi_rs::common::{LyricsID, PlaylistID, VideoID, YoutubeID};
pub use ytmapi_rs::parse::{Lyrics, PlaylistItem, WatchPlaylistTrack};

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error(transparent)]
    Ytm(#[from] ytmapi_rs::Error),
    #[error(transparent)]
    Guard(#[from] GuardError),
    #[error("bootstrap token deserialization: {0}")]
    Bootstrap(#[from] serde_json::Error),
}

/// A fixed bootstrap token. `YtMusic::new_unauthenticated` fetches
/// music.youtube.com and parses `ytcfg.set(` from the page shell; that shell
/// stopped carrying ytcfg, so the fetch fails. Injecting a deserialized
/// `NoAuthToken` skips the page fetch entirely (verified against live search,
/// 2026-11-18). The token only carries a visitor id hint; the API issues real
/// visitor data per response.
const BOOTSTRAP_TOKEN_JSON: &str =
    r#"{"create_time":"2026-11-18T12:00:00Z","visitor_id":"rustytube-bootstrap"}"#;

pub struct ApiClient {
    ytm: ytmapi_rs::YtMusic<ytmapi_rs::auth::noauth::NoAuthToken>,
}

impl ApiClient {
    pub fn new() -> Result<Self, ApiError> {
        let token = serde_json::from_str(BOOTSTRAP_TOKEN_JSON)?;
        // UFCS picks the sync `build` that consumes the provided token; the
        // `.build()` sugar resolves ambiguously against the async NoToken
        // bootstrap build on this builder type.
        let builder = ytmapi_rs::YtMusicBuilder::new().with_auth_token(token);
        let ytm =
            ytmapi_rs::YtMusicBuilder::<ytmapi_rs::auth::noauth::NoAuthToken>::build(builder)?;
        Ok(Self { ytm })
    }

    pub async fn search_songs(&self, query: &str) -> Result<Vec<Track>, ApiError> {
        let songs = self.ytm.search_songs(query).await?;
        Ok(songs.into_iter().map(Track::from).collect())
    }

    pub async fn watch_playlist(
        &self,
        video_id: &str,
    ) -> Result<Vec<WatchPlaylistTrack>, ApiError> {
        let id = VideoID::from_raw(video_id.to_string());
        Ok(self.ytm.get_watch_playlist_from_video_id(id).await?)
    }

    pub async fn lyrics_id(&self, video_id: &str) -> Result<LyricsID<'static>, ApiError> {
        let id = VideoID::from_raw(video_id.to_string());
        Ok(self.ytm.get_lyrics_id(id).await?)
    }

    pub async fn lyrics(&self, lyrics_id: LyricsID<'_>) -> Result<Lyrics, ApiError> {
        Ok(self.ytm.get_lyrics(lyrics_id).await?)
    }

    /// One-shot: video id to lyric text and source. Fails when the track
    /// has no lyrics (the API surfaces that as an error).
    pub async fn lyrics_for(&self, video_id: &str) -> Result<Lyrics, ApiError> {
        let id = self.lyrics_id(video_id).await?;
        self.lyrics(id).await
    }

    pub async fn playlist_tracks(&self, playlist_id: &str) -> Result<Vec<PlaylistItem>, ApiError> {
        let id = PlaylistID::from_raw(playlist_id.to_string());
        Ok(self.ytm.get_playlist_tracks(id).await?)
    }
}
