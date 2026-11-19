//! The application shell: navigation, panels, playback bar, and the wiring
//! to search and playback (PR-7).

pub mod eq_faders;
pub mod fonts;
pub mod shortcuts;
pub mod state;
pub mod theme;
pub mod view;
pub mod visualizer;

pub use state::{RightTab, Section, Transport, TransportEvent, transport_step};

use iced::{Element, Task};
use rt_api::{ApiClient, Track};
use rt_player::{Cmd, PlayerHandle, RepeatMode, Status};
use std::sync::Arc;
use std::time::Duration;

/// Re-exported so the binary crate needs no direct iced dependency.
pub use iced::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Repeat {
    Off,
    All,
    One,
}

impl Repeat {
    pub fn next(self) -> Self {
        match self {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        }
    }

    fn to_player(self) -> RepeatMode {
        match self {
            Repeat::Off => RepeatMode::Off,
            Repeat::All => RepeatMode::All,
            Repeat::One => RepeatMode::One,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    NavSelected(Section),
    TabSelected(RightTab),
    SearchQueryChanged(String),
    SearchSubmitted,
    SearchDone(std::result::Result<Vec<Track>, String>),
    TrackClicked(usize),
    PlaylistClicked(String),
    AuthStartPressed,
    AuthSessionArrived(std::result::Result<Arc<rt_api::SignInSession>, String>),
    AuthCompleted(
        std::result::Result<(Arc<rt_api::AuthedClient>, Vec<rt_api::PlaylistBrief>), String>,
    ),
    AuthDisconnectPressed,
    /// Run the starter-mix search for a sidebar playlist / mood tile.
    MoodSearch(String),
    /// Select one of the right-panel filter chips.
    ChipSelected(usize),
    NewPlaylistModal(bool),
    NewPlaylistNameChanged(String),
    NewPlaylistSubmitted,
    NewPlaylistCreated(std::result::Result<String, String>),
    KeyPressed(iced::keyboard::Key, iced::keyboard::Modifiers),
    HelpToggled,
    Tick,
    PlayPressed,
    NextPressed,
    PrevPressed,
    ShuffleToggled,
    RepeatCycled,
    ThumbsUp,
    ThumbsDown,
    VolumeChanged(f32),
    SeekChanged(f32),
    ThemeCycled,
    EqGainChanged(usize, f32),
    EqBuiltinSelected(usize),
    EqUserSelected(usize),
    EqBypassToggled,
    EqNameChanged(String),
    EqSavePressed,
    EqResetPressed,
    EqRenamePressed,
    EqDeletePressed,
    EqUserPresetsLoaded(Vec<rt_config::Preset>),
    LyricsLoaded(String, std::result::Result<(String, String), String>),
    PrefsFontChanged(rt_config::ui_prefs::FontChoice),
    PrefsTextSizeChanged(rt_config::ui_prefs::TextSize),
    Noop,
}

/// UI-side EQ state. The engine holds the live truth; this mirrors what the
/// sliders show and remembers gains while bypassed.
#[derive(Debug, Clone)]
pub struct EqUiState {
    pub enabled: bool,
    pub gains_db: [f32; 8],
    pub preset_name: String,
    pub user_presets: Vec<rt_config::Preset>,
    pub save_name: String,
}

impl Default for EqUiState {
    fn default() -> Self {
        EqUiState {
            enabled: true,
            gains_db: [0.0; 8],
            preset_name: "Flat".into(),
            user_presets: Vec::new(),
            save_name: String::new(),
        }
    }
}

/// Lyrics shown in the right panel: parsed at the boundary into owned
/// strings, keyed by the video they belong to.
#[derive(Debug, Clone)]
pub struct LyricsState {
    pub video_id: String,
    pub lines: Vec<String>,
    pub source: String,
}

/// Google account sign-in state for the sidebar.
#[derive(Debug, Clone, PartialEq)]
pub enum AuthState {
    SignedOut,
    /// Google answered with a code; the user must visit the URL.
    AwaitingUser {
        user_code: String,
        verification_url: String,
    },
    SignedIn {
        playlists: Vec<rt_api::PlaylistBrief>,
    },
}

pub struct RustyTube {
    pub section: Section,
    pub tab: RightTab,
    pub search_query: String,
    pub results: Vec<Track>,
    pub status: Status,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub volume: f32,
    pub theme_index: usize,
    pub prefs: rt_config::ui_prefs::UiPrefs,
    pub eq: EqUiState,
    pub lyrics: Option<LyricsState>,
    lyrics_loading_for: Option<String>,
    /// Smoothed visualizer bands (peak-hold decay), driven by the tap.
    pub spectrum: Vec<f32>,
    spectrum_analyzer: rt_audio::SpectrumAnalyzer,
    pub auth: AuthState,
    /// Right-panel filter chip index; 0 is All.
    pub active_chip: usize,
    /// `Some(name)` while the New playlist dialog is open.
    pub new_playlist: Option<String>,
    /// Set while the user types in the search field; gates bare-key
    /// shortcuts so typing is never intercepted.
    pub typing: bool,
    /// True while the `?` shortcut overlay is open.
    pub help_open: bool,
    /// Volume before mute, restored on unmute.
    mute_level: f32,
    auth_session: Option<Arc<rt_api::SignInSession>>,
    pub authed: Option<Arc<rt_api::AuthedClient>>,
    pub player: Option<PlayerHandle>,
    pub api: Option<Arc<ApiClient>>,
}

impl Default for RustyTube {
    fn default() -> Self {
        RustyTube {
            section: match std::env::var("RT_SECTION").as_deref() {
                Ok("equalizer") => Section::Equalizer,
                Ok("explore") => Section::Explore,
                Ok("library") => Section::Library,
                _ => Section::Home,
            },
            tab: RightTab::UpNext,
            search_query: String::new(),
            results: Vec::new(),
            status: Status::default(),
            shuffle: false,
            repeat: Repeat::Off,
            volume: 1.0,
            theme_index: std::env::var("RT_THEME")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .map(|i| i % theme::THEMES.len())
                .unwrap_or(0),
            prefs: rt_config::ui_prefs::load(),
            eq: EqUiState::default(),
            lyrics: None,
            lyrics_loading_for: None,
            spectrum: vec![0.0; rt_audio::BAND_COUNT],
            spectrum_analyzer: rt_audio::SpectrumAnalyzer::new(),
            auth: AuthState::SignedOut,
            active_chip: 0,
            new_playlist: None,
            typing: false,
            help_open: false,
            mute_level: 1.0,
            auth_session: None,
            authed: None,
            player: PlayerHandle::spawn().ok(),
            api: ApiClient::new().ok().map(Arc::new),
        }
    }
}

/// What the stage loads on first open, so the app never greets an empty
/// queue. Chips in the right panel map onto moods later; the default is
/// deliberately broad.
pub const DEFAULT_QUERY: &str = "today's hits";

/// Widget id for the search field, so Ctrl+K can focus it.
pub const SEARCH_INPUT_ID: &str = "rt-search";

/// Right-panel filter chips, index 0 is the unfiltered queue.
pub const CHIPS: [&str; 5] = ["All", "Chill", "Focus", "Sleep", "Familiar"];

/// Centre-stage mood tiles for Explore: (label, search query).
pub const MOODS: [(&str, &str); 6] = [
    ("Chill", "chill music"),
    ("Focus", "focus music"),
    ("Sleep", "sleep music"),
    ("Workout", "workout music"),
    ("Party", "party music"),
    ("Relax", "relaxing music"),
];

/// Sidebar starter mixes shown before sign-in: (label, search query).
pub const STARTER_MIXES: [(&str, &str); 4] = [
    ("Focus Flow", "focus music"),
    ("Late Night", "late night music"),
    ("Chill Vibes", "chill music"),
    ("Ambient", "ambient music"),
];

impl RustyTube {
    pub fn new() -> Self {
        RustyTube::default()
    }

    /// Boot with content already loading: the default query fills the stage
    /// and the queue without a keystroke, and a stored Google token (if any)
    /// restores the signed-in session.
    pub fn boot() -> (RustyTube, Task<Message>) {
        let mut app = RustyTube::new();
        app.search_query = DEFAULT_QUERY.to_string();
        let search = app.update(Message::SearchSubmitted);
        let presets = Task::perform(
            async { rt_config::list_user() },
            Message::EqUserPresetsLoaded,
        );
        // Restore a previous session from the OS credential store.
        let auth = match rt_api::load_token() {
            Ok(Some(token)) => match rt_api::AuthedClient::new(token) {
                Ok(client) => {
                    let client = Arc::new(client);
                    app.authed = Some(client.clone());
                    app.auth = AuthState::SignedIn {
                        playlists: Vec::new(),
                    };
                    Task::perform(
                        async move {
                            client
                                .playlists()
                                .await
                                .map(|b| (client, b))
                                .map_err(|e| e.to_string())
                        },
                        |r| match r {
                            Ok((client, playlists)) => {
                                Message::AuthCompleted(Ok((client, playlists)))
                            }
                            Err(e) => Message::AuthCompleted(Err(e)),
                        },
                    )
                }
                Err(e) => {
                    app.status.error = Some(format!("stored sign-in: {e}"));
                    Task::none()
                }
            },
            Ok(None) => Task::none(),
            Err(e) => {
                app.status.error = Some(format!("credential store: {e}"));
                Task::none()
            }
        };
        (app, Task::batch([search, presets, auth]))
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::NavSelected(s) => {
                // Clicking navigation means the search field is not in use.
                self.typing = false;
                self.section = s;
            }
            Message::TabSelected(t) => self.tab = t,
            Message::SearchQueryChanged(q) => {
                // Typing in the field: bare-key shortcuts must not fire.
                self.typing = true;
                self.search_query = q;
            }
            Message::SearchSubmitted => {
                let query = self.search_query.clone();
                if query.trim().is_empty() {
                    return Task::none();
                }
                let Some(api) = self.api.clone() else {
                    return Task::none();
                };
                return Task::perform(
                    async move { api.search_songs(&query).await.map_err(|e| e.to_string()) },
                    Message::SearchDone,
                );
            }
            Message::SearchDone(Ok(tracks)) => {
                // Results land in the queue panel; the section stays where
                // the user is (and the boot search must not stomp an
                // RT_SECTION override).
                self.results = tracks;
            }
            Message::PlaylistClicked(id) => {
                let Some(client) = self.authed.clone() else {
                    return Task::none();
                };
                return Task::perform(
                    async move { client.playlist_tracks(&id).await },
                    Message::SearchDone,
                );
            }
            Message::AuthStartPressed => {
                if !matches!(self.auth, AuthState::SignedOut) {
                    return Task::none();
                }
                return Task::perform(
                    async {
                        let (id, secret) = rt_api::credentials_from_env()?;
                        rt_api::begin_sign_in(id, secret).await.map(Arc::new)
                    },
                    Message::AuthSessionArrived,
                );
            }
            Message::AuthSessionArrived(Ok(session)) => {
                self.auth = AuthState::AwaitingUser {
                    user_code: session.user_code.clone(),
                    verification_url: session.verification_url.clone(),
                };
                self.auth_session = Some(session.clone());
                return Task::perform(
                    async move {
                        let token = session.wait_for_authorization().await?;
                        rt_api::store_token(&token).map_err(|e| e.to_string())?;
                        let client = rt_api::AuthedClient::new(token).map_err(|e| e.to_string())?;
                        let client = Arc::new(client);
                        let playlists = client.playlists().await.map_err(|e| e.to_string())?;
                        Ok((client, playlists))
                    },
                    Message::AuthCompleted,
                );
            }
            Message::AuthSessionArrived(Err(e)) => {
                self.status.error = Some(e);
            }
            Message::AuthCompleted(Ok((client, playlists))) => {
                self.authed = Some(client);
                self.auth = AuthState::SignedIn { playlists };
                self.auth_session = None;
            }
            Message::AuthCompleted(Err(e)) => {
                self.auth = AuthState::SignedOut;
                self.auth_session = None;
                self.status.error = Some(format!("sign-in: {e}"));
            }
            Message::AuthDisconnectPressed => {
                if let Err(e) = rt_api::clear_token() {
                    self.status.error = Some(format!("disconnect: {e}"));
                }
                self.auth = AuthState::SignedOut;
                self.authed = None;
                self.auth_session = None;
            }
            Message::MoodSearch(query) => {
                // Sidebar starter mixes and Explore mood tiles are real
                // searches, not placeholders.
                self.search_query = query;
                return self.update(Message::SearchSubmitted);
            }
            Message::ChipSelected(i) => {
                self.active_chip = i;
                let mood = CHIPS.get(i).copied().unwrap_or("All");
                let query = if mood == "All" {
                    DEFAULT_QUERY.to_string()
                } else {
                    format!("{mood} music")
                };
                self.search_query = query;
                return self.update(Message::SearchSubmitted);
            }
            Message::NewPlaylistModal(open) => {
                self.new_playlist = open.then(String::new);
            }
            Message::NewPlaylistNameChanged(name) => {
                self.new_playlist = Some(name);
            }
            Message::NewPlaylistSubmitted => {
                let name = self
                    .new_playlist
                    .take()
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if name.is_empty() {
                    self.status.error = Some("playlist name cannot be empty".to_string());
                    return Task::none();
                }
                let Some(client) = self.authed.clone() else {
                    self.status.error = Some("sign in with Google to create playlists".to_string());
                    return Task::none();
                };
                return Task::perform(
                    async move { client.create_playlist(&name).await },
                    Message::NewPlaylistCreated,
                );
            }
            Message::NewPlaylistCreated(Ok(_id)) => {
                // Refresh the sidebar from the account so the new list shows.
                if let Some(client) = self.authed.clone() {
                    return Task::perform(
                        async move {
                            client
                                .playlists()
                                .await
                                .map(|b| (client, b))
                                .map_err(|e| e.to_string())
                        },
                        |r| match r {
                            Ok((client, playlists)) => {
                                Message::AuthCompleted(Ok((client, playlists)))
                            }
                            Err(e) => Message::AuthCompleted(Err(e)),
                        },
                    );
                }
            }
            Message::NewPlaylistCreated(Err(e)) => {
                self.status.error = Some(format!("create playlist: {e}"));
            }
            Message::KeyPressed(key, modifiers) => {
                use shortcuts::Shortcut;
                let Some(action) = shortcuts::resolve(&key, modifiers, self.typing) else {
                    return Task::none();
                };
                match action {
                    Shortcut::PlayPause => {
                        if let Some(p) = &self.player {
                            p.send(Cmd::Toggle);
                        }
                    }
                    Shortcut::SeekBack | Shortcut::SeekForward => {
                        let dur = self.status.duration_secs.max(1.0);
                        let delta = if action == Shortcut::SeekForward {
                            5.0
                        } else {
                            -5.0
                        };
                        let target = (self.status.position_secs + delta).clamp(0.0, dur);
                        if let Some(p) = &self.player {
                            p.send(Cmd::Seek((target / dur) as f32));
                        }
                    }
                    Shortcut::VolumeUp | Shortcut::VolumeDown => {
                        let step = if action == Shortcut::VolumeUp {
                            0.05
                        } else {
                            -0.05
                        };
                        self.volume = (self.volume + step).clamp(0.0, 1.0);
                        if let Some(p) = &self.player {
                            p.send(Cmd::SetVolume((self.volume * 1000.0) as u32));
                        }
                    }
                    Shortcut::Mute => {
                        if self.volume > 0.0 {
                            self.mute_level = self.volume;
                            self.volume = 0.0;
                        } else {
                            self.volume = if self.mute_level > 0.0 {
                                self.mute_level
                            } else {
                                1.0
                            };
                        }
                        if let Some(p) = &self.player {
                            p.send(Cmd::SetVolume((self.volume * 1000.0) as u32));
                        }
                    }
                    Shortcut::FocusSearch => {
                        self.typing = true;
                        return iced::widget::operation::focus(SEARCH_INPUT_ID.to_string());
                    }
                    Shortcut::Home => self.section = Section::Home,
                    Shortcut::Explore => self.section = Section::Explore,
                    Shortcut::Library => self.section = Section::Library,
                    Shortcut::Equalizer => self.section = Section::Equalizer,
                    Shortcut::Like => {
                        // Same path as the thumbs-up button: the like API
                        // needs the signed-in client and is not wired yet.
                        let _ = Message::ThumbsUp;
                    }
                    Shortcut::Help => self.help_open = !self.help_open,
                }
            }
            Message::HelpToggled => self.help_open = !self.help_open,
            Message::SearchDone(Err(e)) => {
                self.status.error = Some(format!("search: {e}"));
            }
            Message::TrackClicked(i) => {
                if let Some(player) = &self.player
                    && i < self.results.len()
                {
                    player.send(Cmd::PlayQueue(self.results.clone(), i));
                }
            }
            Message::Tick => {
                if let Some(player) = &self.player {
                    self.status = player.status();
                }
                // Visualizer: drain the tap, analyze, smooth with decay so
                // bars fall gracefully instead of flickering.
                {
                    let mut buf = vec![0.0f32; rt_audio::ANALYSIS_LEN];
                    let n = self
                        .player
                        .as_ref()
                        .map(|p| p.drain_tap(&mut buf))
                        .unwrap_or(0);
                    let playing = self.status.transport == rt_player::TransportState::Playing;
                    if n > 0 && playing {
                        let rate = self
                            .player
                            .as_ref()
                            .map(|p| p.device_rate())
                            .unwrap_or(44_100);
                        let mut bands = [0.0f32; rt_audio::BAND_COUNT];
                        self.spectrum_analyzer.analyze(&buf[..n], rate, &mut bands);
                        for (s, b) in self.spectrum.iter_mut().zip(bands) {
                            *s = (*s * 0.72).max(b);
                        }
                    } else {
                        for s in self.spectrum.iter_mut() {
                            *s *= 0.72;
                        }
                    }
                }
                // Load lyrics for whatever is playing, once per track.
                let current = self.status.current.as_ref().map(|t| t.video_id.clone());
                if let Some(video_id) = current
                    && self.lyrics.as_ref().map(|l| &l.video_id) != Some(&video_id)
                    && self.lyrics_loading_for.as_ref() != Some(&video_id)
                    && let Some(api) = self.api.clone()
                {
                    let for_loading = video_id.clone();
                    self.lyrics_loading_for = Some(video_id.clone());
                    let for_message = video_id.clone();
                    return Task::perform(
                        async move {
                            api.lyrics_for(&for_loading)
                                .await
                                .map(|l| (l.lyrics, l.source))
                                .map_err(|e| e.to_string())
                        },
                        move |r| Message::LyricsLoaded(for_message, r),
                    );
                }
            }
            Message::PlayPressed => {
                if let Some(p) = &self.player {
                    p.send(Cmd::Toggle);
                }
            }
            Message::NextPressed => {
                if let Some(p) = &self.player {
                    p.send(Cmd::Next);
                }
            }
            Message::PrevPressed => {
                if let Some(p) = &self.player {
                    p.send(Cmd::Prev);
                }
            }
            Message::ShuffleToggled => {
                self.shuffle = !self.shuffle;
                if let Some(p) = &self.player {
                    p.send(Cmd::SetShuffle(self.shuffle));
                }
            }
            Message::RepeatCycled => {
                self.repeat = self.repeat.next();
                if let Some(p) = &self.player {
                    p.send(Cmd::SetRepeat(self.repeat.to_player()));
                }
            }
            Message::ThumbsUp | Message::ThumbsDown => {}
            Message::VolumeChanged(v) => {
                self.volume = v.clamp(0.0, 1.0);
                if let Some(p) = &self.player {
                    p.send(Cmd::SetVolume((self.volume * 1000.0) as u32));
                }
            }
            Message::SeekChanged(f) => {
                if let Some(p) = &self.player {
                    p.send(Cmd::Seek(f));
                }
            }
            Message::ThemeCycled => {
                self.theme_index = (self.theme_index + 1) % theme::THEMES.len();
            }
            Message::PrefsFontChanged(f) => {
                self.prefs.font = f;
                if let Err(e) = rt_config::ui_prefs::store(&self.prefs) {
                    self.status.error = Some(format!("prefs save: {e}"));
                }
            }
            Message::PrefsTextSizeChanged(s) => {
                self.prefs.text_size = s;
                if let Err(e) = rt_config::ui_prefs::store(&self.prefs) {
                    self.status.error = Some(format!("prefs save: {e}"));
                }
            }
            Message::EqGainChanged(i, v) => {
                if i < 8 {
                    self.eq.gains_db[i] = v.clamp(-12.0, 12.0);
                    self.eq.preset_name = "Custom".into();
                    self.push_eq();
                }
            }
            Message::EqBuiltinSelected(i) => {
                let presets = rt_config::builtin_presets();
                if let Some(preset) = presets.get(i) {
                    self.eq.gains_db = preset.gains;
                    self.eq.preset_name = preset.name.clone();
                    self.push_eq();
                }
            }
            Message::EqUserSelected(i) => {
                if let Some(preset) = self.eq.user_presets.get(i) {
                    self.eq.gains_db = preset.gains;
                    self.eq.preset_name = preset.name.clone();
                    self.push_eq();
                }
            }
            Message::EqBypassToggled => {
                self.eq.enabled = !self.eq.enabled;
                self.push_eq();
            }
            Message::EqNameChanged(s) => self.eq.save_name = s,
            Message::EqUserPresetsLoaded(ps) => self.eq.user_presets = ps,
            Message::LyricsLoaded(video_id, Ok((text, source))) => {
                self.lyrics_loading_for = None;
                self.lyrics = Some(LyricsState {
                    video_id,
                    lines: text.lines().map(str::to_string).collect(),
                    source,
                });
            }
            Message::LyricsLoaded(video_id, Err(_e)) => {
                // Most tracks simply have no lyrics; keep the panel honest.
                self.lyrics_loading_for = None;
                self.lyrics = Some(LyricsState {
                    video_id,
                    lines: Vec::new(),
                    source: String::new(),
                });
            }
            Message::EqSavePressed => {
                let name = if self.eq.save_name.trim().is_empty() {
                    "Custom".to_string()
                } else {
                    self.eq.save_name.trim().to_string()
                };
                let preset = rt_config::Preset {
                    name,
                    gains: self.eq.gains_db,
                };
                match rt_config::save(&preset) {
                    Ok(_) => {
                        self.eq.preset_name = preset.name.clone();
                        self.eq.user_presets = rt_config::list_user();
                    }
                    Err(e) => self.status.error = Some(format!("preset save: {e}")),
                }
            }
            Message::EqResetPressed => {
                self.eq.gains_db = [0.0; 8];
                self.eq.preset_name = "Flat".into();
                self.push_eq();
            }
            Message::EqRenamePressed => {
                let new = if self.eq.save_name.trim().is_empty() {
                    "Custom".to_string()
                } else {
                    self.eq.save_name.trim().to_string()
                };
                let old = self.eq.preset_name.clone();
                let is_user_preset = self.eq.user_presets.iter().any(|p| p.name == old);
                if is_user_preset {
                    match rt_config::rename_in(&rt_config::presets_dir(), &old, &new) {
                        Ok(p) => {
                            self.eq.preset_name = p.name.clone();
                            self.eq.user_presets = rt_config::list_user();
                        }
                        Err(e) => self.status.error = Some(format!("preset rename: {e}")),
                    }
                } else {
                    self.status.error = Some("only your own presets can be renamed".to_string());
                }
            }
            Message::EqDeletePressed => {
                let old = self.eq.preset_name.clone();
                let is_user_preset = self.eq.user_presets.iter().any(|p| p.name == old);
                if is_user_preset {
                    if let Err(e) = rt_config::delete_from(&rt_config::presets_dir(), &old) {
                        self.status.error = Some(format!("preset delete: {e}"));
                    }
                    self.eq.user_presets = rt_config::list_user();
                    if self.eq.preset_name == old {
                        self.eq.preset_name = "Flat".into();
                    }
                } else {
                    self.status.error = Some("only your own presets can be deleted".to_string());
                }
            }
            Message::Noop => {}
        };
        Task::none()
    }

    /// Mirror the UI EQ state into the engine: gains when enabled, flat
    /// when bypassed. Lock-free on the engine side.
    fn push_eq(&self) {
        if let Some(handle) = self.player.as_ref().and_then(|p| p.eq()) {
            handle.set_gains_db(if self.eq.enabled {
                self.eq.gains_db
            } else {
                [0.0; 8]
            });
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        view::view(self)
    }

    pub fn subscription(&self) -> iced::Subscription<Message> {
        // 20 fps: smooth visualizer decay without hammering the UI.
        let tick = iced::time::every(Duration::from_millis(50)).map(|_| Message::Tick);
        let keys = iced::keyboard::listen().filter_map(|event| match event {
            iced::keyboard::Event::KeyPressed {
                modified_key,
                modifiers,
                repeat,
                ..
            } => {
                // Key repeat would retrigger seeks/volume dozens of
                // times per second; take the first press only.
                (!repeat).then_some(Message::KeyPressed(modified_key, modifiers))
            }
            _ => None,
        });
        iced::Subscription::batch([tick, keys])
    }
}

/// Open the RustyTube window. Blocks until the window closes.
pub fn run() -> Result {
    let prefs = rt_config::ui_prefs::load();
    let font = fonts::resolved_font(prefs.font);
    iced::application(RustyTube::boot, RustyTube::update, RustyTube::view)
        .title("RustyTube")
        // Inter is bundled; register it so Font::with_name("Inter") resolves.
        .font(fonts::INTER_TTF)
        // The stored font choice applies to every text in the app.
        .default_font(font)
        .window(iced::window::Settings {
            size: iced::Size::new(1280.0, 800.0),
            // Start maximized: the shell is Fill-based and adapts to whatever
            // surface the OS hands out on this multi-DPI display set.
            maximized: true,
            resizable: true,
            ..Default::default()
        })
        // Global interface scaling from the text-size setting: text,
        // padding, and controls scale together. Reads live state, so
        // changing the size applies immediately.
        .scale_factor(|state: &RustyTube| state.prefs.text_size.scale())
        .subscription(RustyTube::subscription)
        .run()
}
