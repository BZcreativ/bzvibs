//! The application shell: navigation, panels, playback bar, and the wiring
//! to search and playback (PR-7).

pub mod state;
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
    pub player: Option<PlayerHandle>,
    pub api: Option<Arc<ApiClient>>,
}

impl Default for RustyTube {
    fn default() -> Self {
        RustyTube {
            section: Section::Home,
            tab: RightTab::UpNext,
            search_query: String::new(),
            results: Vec::new(),
            status: Status::default(),
            shuffle: false,
            repeat: Repeat::Off,
            volume: 1.0,
            player: PlayerHandle::spawn().ok(),
            api: ApiClient::new().ok().map(Arc::new),
        }
    }
}

impl RustyTube {
    pub fn new() -> Self {
        RustyTube::default()
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
                self.results = tracks;
                self.section = Section::Explore;
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
        };
        Task::none()
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
    iced::application(RustyTube::new, RustyTube::update, RustyTube::view)
        .title("RustyTube")
        .window_size(iced::Size::new(1280.0, 800.0))
        .subscription(RustyTube::subscription)
        .run()
}
