//! The application shell: navigation, panels, playback bar, and the wiring
//! to search and playback (PR-7).

pub mod state;
pub mod theme;
pub mod view;

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
    EqUserPresetsLoaded(Vec<rt_config::Preset>),
    LyricsLoaded(String, std::result::Result<(String, String), String>),
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
    pub eq: EqUiState,
    pub lyrics: Option<LyricsState>,
    lyrics_loading_for: Option<String>,
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
            eq: EqUiState::default(),
            lyrics: None,
            lyrics_loading_for: None,
            player: PlayerHandle::spawn().ok(),
            api: ApiClient::new().ok().map(Arc::new),
        }
    }
}

/// What the stage loads on first open, so the app never greets an empty
/// queue. Chips in the right panel map onto moods later; the default is
/// deliberately broad.
pub const DEFAULT_QUERY: &str = "today's hits";

impl RustyTube {
    pub fn new() -> Self {
        RustyTube::default()
    }

    /// Boot with content already loading: the default query fills the stage
    /// and the queue without a keystroke.
    pub fn boot() -> (RustyTube, Task<Message>) {
        let mut app = RustyTube::new();
        app.search_query = DEFAULT_QUERY.to_string();
        let search = app.update(Message::SearchSubmitted);
        let presets = Task::perform(
            async { rt_config::list_user() },
            Message::EqUserPresetsLoaded,
        );
        (app, Task::batch([search, presets]))
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::NavSelected(s) => self.section = s,
            Message::TabSelected(t) => self.tab = t,
            Message::SearchQueryChanged(q) => self.search_query = q,
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
        iced::time::every(Duration::from_millis(250)).map(|_| Message::Tick)
    }
}

/// Open the RustyTube window. Blocks until the window closes.
pub fn run() -> Result {
    iced::application(RustyTube::boot, RustyTube::update, RustyTube::view)
        .title("RustyTube")
        .window(iced::window::Settings {
            size: iced::Size::new(1280.0, 800.0),
            // Start maximized: the shell is Fill-based and adapts to whatever
            // surface the OS hands out on this multi-DPI display set.
            maximized: true,
            resizable: true,
            ..Default::default()
        })
        .subscription(RustyTube::subscription)
        .run()
}
