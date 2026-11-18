//! The three-column shell: left nav, center stage with the search bar and
//! results, right tabbed panel, playback bar across the bottom. Dark theme
//! per the PRD (#000000 to #121212, high-contrast text).

use crate::{Message, RightTab, RustyTube, Section};

use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Alignment, Color, Element, Fill, Length};
use rt_player::TransportState;

pub const BG_ROOT: Color = Color::from_rgb(0.0, 0.0, 0.0);
pub const BG_PANEL: Color = Color::from_rgb(0.07, 0.07, 0.07); // #121212
pub const BG_HOVER: Color = Color::from_rgb(0.14, 0.14, 0.14);
pub const FG: Color = Color::from_rgb(0.93, 0.93, 0.93);
pub const FG_DIM: Color = Color::from_rgb(0.55, 0.55, 0.55);
pub const ACCENT: Color = Color::from_rgb(1.0, 0.3, 0.35);

fn panel<'a>(content: impl Into<Element<'a, Message>>) -> container::Container<'a, Message> {
    container(content).style(|_t| container::Style {
        background: Some(BG_PANEL.into()),
        ..container::Style::default()
    })
}

fn nav_button<'a>(label: &'a str, active: bool, msg: Message) -> button::Button<'a, Message> {
    button(text(label).color(if active { FG } else { FG_DIM }))
        .on_press(msg)
        .style(move |_t, status| button::Style {
            background: Some(
                if status == button::Status::Hovered || active {
                    BG_HOVER
                } else {
                    Color::TRANSPARENT
                }
                .into(),
            ),
            text_color: if active { FG } else { FG_DIM },
            ..button::Style::default()
        })
}

fn sidebar(app: &RustyTube) -> Element<'static, Message> {
    let nav = column![
        nav_button(
            "Home",
            app.section == Section::Home,
            Message::NavSelected(Section::Home)
        ),
        nav_button(
            "Explore",
            app.section == Section::Explore,
            Message::NavSelected(Section::Explore)
        ),
        nav_button(
            "Library",
            app.section == Section::Library,
            Message::NavSelected(Section::Library)
        ),
        text("Playlists").color(FG_DIM),
        text("  Liked music").color(FG_DIM),
    ]
    .spacing(6)
    .padding(12)
    .width(Fill);

    scrollable(nav).into()
}

fn topbar(app: &RustyTube) -> Element<'static, Message> {
    let input = text_input("Search songs, artists, albums…", &app.search_query)
        .on_input(Message::SearchQueryChanged)
        .on_submit(Message::SearchSubmitted)
        .style(move |_t, _s| text_input::Style {
            background: Color::from_rgb(0.1, 0.1, 0.1).into(),
            border: Default::default(),
            icon: FG_DIM,
            placeholder: FG_DIM,
            value: FG,
            selection: ACCENT,
        });
    row![input]
        .spacing(8)
        .padding(8)
        .width(Fill)
        .align_y(Alignment::Center)
        .into()
}

fn result_row(
    i: usize,
    title: &str,
    artist: &str,
    duration: &str,
) -> button::Button<'static, Message> {
    button(
        row![
            text(format!("{}. {}", i + 1, title)).color(FG),
            text(artist.to_string()).color(FG_DIM),
            text(duration.to_string()).color(FG_DIM),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .on_press(Message::TrackClicked(i))
    .width(Fill)
    .style(move |_t, status| button::Style {
        background: Some(
            if status == button::Status::Hovered {
                BG_HOVER
            } else {
                Color::TRANSPARENT
            }
            .into(),
        ),
        text_color: FG,
        ..button::Style::default()
    })
}

fn stage(app: &RustyTube) -> Element<'static, Message> {
    let headline = match app.section {
        Section::Home => "Home",
        Section::Explore => "Explore",
        Section::Library => "Library",
    };

    let mut body = column![].spacing(4).padding(16);
    body = body.push(text(headline).size(24).color(FG));

    match &app.status.transport {
        TransportState::Loading(what) => {
            body = body.push(text(format!("Loading: {what}…")).color(ACCENT));
        }
        TransportState::Playing | TransportState::Paused => {
            if let Some(track) = &app.status.current {
                body = body.push(
                    text(format!("{} — {}", track.title, track.artist))
                        .size(16)
                        .color(FG),
                );
            }
        }
        TransportState::Stopped => {
            if app.results.is_empty() {
                body = body.push(text("Search to fill this stage").color(FG_DIM));
            }
        }
    }
    if let Some(err) = &app.status.error {
        body = body.push(text(format!("error: {err}")).color(ACCENT));
    }

    if !app.results.is_empty() {
        let rows = app
            .results
            .iter()
            .enumerate()
            .map(|(i, t)| result_row(i, &t.title, &t.artist, &t.duration).into());
        body = body.push(scrollable(column(rows).spacing(2)).height(Fill));
    }

    body.width(Fill).height(Fill).into()
}

fn tab_button<'a>(label: &'a str, active: bool, msg: Message) -> button::Button<'a, Message> {
    button(text(label).size(12))
        .on_press(msg)
        .style(move |_t, _s| button::Style {
            background: Some(if active { BG_HOVER } else { Color::TRANSPARENT }.into()),
            text_color: if active { FG } else { FG_DIM },
            ..button::Style::default()
        })
}

fn right_panel(app: &RustyTube) -> Element<'static, Message> {
    let tabs = row![
        tab_button(
            "UP NEXT",
            app.tab == RightTab::UpNext,
            Message::TabSelected(RightTab::UpNext)
        ),
        tab_button(
            "LYRICS",
            app.tab == RightTab::Lyrics,
            Message::TabSelected(RightTab::Lyrics)
        ),
        tab_button(
            "RELATED",
            app.tab == RightTab::Related,
            Message::TabSelected(RightTab::Related)
        ),
    ]
    .spacing(4)
    .padding(8);

    let body = match app.tab {
        RightTab::UpNext => text(format!(
            "Queue: {} tracks (index {})",
            app.status.queue_len, app.status.queue_index
        ))
        .color(FG_DIM),
        RightTab::Lyrics => text("No lyrics loaded").color(FG_DIM),
        RightTab::Related => text("Nothing related yet").color(FG_DIM),
    };

    column![tabs, body].spacing(8).width(Fill).into()
}

fn mmss(secs: f64) -> String {
    let total = secs.max(0.0) as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

fn playback_bar(app: &RustyTube) -> Element<'static, Message> {
    let playing = app.status.transport == TransportState::Playing;
    let paused = app.status.transport == TransportState::Paused;
    let play_label = if playing {
        "Pause"
    } else if paused {
        "Resume"
    } else {
        "Play"
    };

    let controls = row![
        button(text("Prev").color(FG)).on_press(Message::PrevPressed),
        button(text(play_label).color(FG)).on_press(Message::PlayPressed),
        button(text("Next").color(FG)).on_press(Message::NextPressed),
        button(
            text(if app.shuffle {
                "Shuffle on"
            } else {
                "Shuffle off"
            })
            .color(FG)
        )
        .on_press(Message::ShuffleToggled),
        button(
            text(format!("Repeat: {:?}", app.repeat))
                .size(12)
                .color(FG_DIM)
        )
        .on_press(Message::RepeatCycled),
        button(text("+1").color(FG)).on_press(Message::ThumbsUp),
        button(text("-1").color(FG)).on_press(Message::ThumbsDown),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let duration = app.status.duration_secs;
    let position = app.status.position_secs.min(duration.max(0.001));
    let frac = if duration > 0.0 {
        (position / duration) as f32
    } else {
        0.0
    };
    let seek = iced::widget::slider(0.0..=1.0, frac, Message::SeekChanged);
    let volume = iced::widget::slider(0.0..=1.0, app.volume, Message::VolumeChanged);

    row![
        controls,
        text(format!("{} / {}", mmss(position), mmss(duration))).color(FG_DIM),
        seek,
        volume,
    ]
    .spacing(12)
    .padding(10)
    .align_y(Alignment::Center)
    .width(Fill)
    .into()
}

pub fn view(app: &RustyTube) -> Element<'static, Message> {
    let body = row![
        panel(sidebar(app)).width(Length::Fixed(220.0)).height(Fill),
        column![topbar(app), stage(app)].width(Fill),
        panel(right_panel(app))
            .width(Length::Fixed(280.0))
            .height(Fill),
    ]
    .width(Fill)
    .height(Fill);

    column![body, playback_bar(app)]
        .width(Fill)
        .height(Fill)
        .into()
}
