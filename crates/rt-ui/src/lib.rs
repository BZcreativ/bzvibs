//! The application shell: navigation, panels, playback bar. The full layout
//! is PR-6; async wiring (search, playback) lands in PR-7.

pub mod state;
pub mod view;

pub use state::{RightTab, Section, Transport, TransportEvent, transport_step};

use iced::Element;

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
}

#[derive(Debug, Clone)]
pub enum Message {
    NavSelected(Section),
    SearchQueryChanged(String),
    SearchSubmitted,
    TabSelected(RightTab),
    PlayPressed,
    NextPressed,
    PrevPressed,
    ShuffleToggled,
    RepeatCycled,
    ThumbsUp,
    ThumbsDown,
    VolumeChanged(f32),
}

pub struct RustyTube {
    pub section: Section,
    pub tab: RightTab,
    pub transport: Transport,
    pub search_query: String,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub volume: f32,
}

impl Default for RustyTube {
    fn default() -> Self {
        RustyTube {
            section: Section::Home,
            tab: RightTab::UpNext,
            transport: Transport::Stopped,
            search_query: String::new(),
            shuffle: false,
            repeat: Repeat::Off,
            volume: 0.75,
        }
    }
}

impl RustyTube {
    pub fn update(&mut self, message: Message) {
        match message {
            Message::NavSelected(s) => self.section = s,
            Message::SearchQueryChanged(q) => self.search_query = q,
            Message::SearchSubmitted => {
                // Results wiring lands with PR-7; the query is captured.
            }
            Message::TabSelected(t) => self.tab = t,
            Message::PlayPressed => {
                self.transport = transport_step(self.transport, TransportEvent::Toggle)
            }
            Message::NextPressed | Message::PrevPressed => {
                self.transport = transport_step(self.transport, TransportEvent::Stop)
            }
            Message::ShuffleToggled => self.shuffle = !self.shuffle,
            Message::RepeatCycled => self.repeat = self.repeat.next(),
            Message::ThumbsUp | Message::ThumbsDown => {}
            Message::VolumeChanged(v) => self.volume = v.clamp(0.0, 1.0),
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        view::view(self)
    }

    pub fn new() -> Self {
        RustyTube::default()
    }
}

/// Open the RustyTube window. Blocks until the window closes.
pub fn run() -> Result {
    iced::application(RustyTube::new, RustyTube::update, RustyTube::view)
        .title("RustyTube")
        .window_size(iced::Size::new(1280.0, 800.0))
        .run()
}
