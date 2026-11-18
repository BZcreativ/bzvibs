//! The bzvibs shell per the concept boards: one skeleton, three theme
//! palettes. Left sidebar (brand, nav, playlists), centered search pill,
//! Song/Video mode pills above the visualizer stage, right panel with
//! up-next chips and queue rows, bottom transport bar with a round play
//! button and thin accent sliders.
//!
//! Style rule: closures capture only `p: &'static Palette` (and Copy
//! booleans), never the app borrow, so widgets stay 'static.

use crate::theme::{self, Palette};
use crate::{Message, RightTab, RustyTube, Section};

use iced::widget::slider::{Handle, HandleShape, Rail};
use iced::widget::{button, column, container, row, scrollable, slider, text, text_input};
use iced::{Alignment, Background, Border, Color, Element, Fill, Length};
use rt_player::TransportState;

fn luminance(c: Color) -> f32 {
    0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b
}

/// Text color that stays readable on the accent (volt's lime needs dark).
fn on_accent(p: &Palette) -> Color {
    if luminance(p.accent) > 0.62 {
        p.bg_base
    } else {
        Color::WHITE
    }
}

fn accent_gradient(p: &Palette) -> Background {
    Background::Gradient(
        iced::gradient::Linear::new(std::f32::consts::FRAC_PI_4)
            .add_stop(0.0, p.accent)
            .add_stop(1.0, p.accent_soft)
            .into(),
    )
}

fn pill_border(color: Color) -> Border {
    Border {
        radius: 999.0.into(),
        color,
        width: 1.0,
    }
}

// ---------- sidebar ----------

fn sidebar(app: &RustyTube) -> Element<'static, Message> {
    let p = theme::at(app.theme_index);
    let brand = row![
        text("▮▮▮")
            .size(14)
            .color(p.accent)
            .font(iced::Font::MONOSPACE),
        text("bzvibs").size(20).color(p.text),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let nav = ["Home", "Explore", "Library"];
    let sections = [Section::Home, Section::Explore, Section::Library];
    let mut nav_col = column![].spacing(4);
    for (label, section) in nav.iter().zip(sections) {
        let active = app.section == section;
        let msg = Message::NavSelected(section);
        let pill = button(text(label.to_string()).size(14))
            .on_press(msg)
            .width(Fill)
            .padding([6, 12])
            .style(move |_t, _s| button::Style {
                background: Some(if active {
                    accent_gradient(p)
                } else {
                    Background::Color(Color::TRANSPARENT)
                }),
                border: Border {
                    radius: 12.0.into(),
                    ..Border::default()
                },
                text_color: if active { on_accent(p) } else { p.text_dim },
                ..button::Style::default()
            });
        nav_col = nav_col.push(pill);
    }

    let new_playlist = button(text("+ New playlist").size(12))
        .on_press(Message::Noop)
        .padding([6, 12])
        .width(Fill)
        .style(move |_t, _s| button::Style {
            border: pill_border(p.text_dim),
            text_color: p.text_dim,
            ..button::Style::default()
        });

    let mut playlists = column![text("Playlists").size(11).color(p.text_dim)].spacing(2);
    for name in [
        "Liked music",
        "Focus Flow",
        "Late Night",
        "Chill Vibes",
        "Ambient",
    ] {
        playlists = playlists.push(text(format!("  {name}")).size(12).color(p.text_dim));
    }

    column![brand, nav_col, new_playlist, playlists]
        .spacing(10)
        .padding(16)
        .width(Fill)
        .into()
}

// ---------- top bar ----------

fn topbar(app: &RustyTube) -> Element<'static, Message> {
    let p = theme::at(app.theme_index);
    let query = app.search_query.clone();
    let search = text_input("Search songs, albums, artists, podcasts…", &query)
        .on_input(Message::SearchQueryChanged)
        .on_submit(Message::SearchSubmitted)
        .size(13)
        .padding([8, 16])
        .style(move |_t, _s| text_input::Style {
            background: Color::from_rgba(0.0, 0.0, 0.0, 0.35).into(),
            border: pill_border(p.bg_card_active),
            icon: p.text_dim,
            placeholder: p.text_dim,
            value: p.text,
            selection: p.accent,
        });

    let theme_label = format!("◑ {}", p.name);
    let theme_button = button(text(theme_label).size(12))
        .on_press(Message::ThemeCycled)
        .padding([6, 12])
        .style(move |_t, _s| button::Style {
            border: pill_border(p.bg_card_active),
            text_color: p.text_dim,
            ..button::Style::default()
        });

    row![search, theme_button]
        .spacing(8)
        .padding([8, 16])
        .width(Fill)
        .align_y(Alignment::Center)
        .into()
}

// ---------- center stage ----------

fn mode_pills(app: &RustyTube) -> Element<'static, Message> {
    let p = theme::at(app.theme_index);
    let song = button(text("Song").size(12))
        .padding([5, 16])
        .style(move |_t, _s| button::Style {
            background: Some(accent_gradient(p)),
            border: Border {
                radius: 999.0.into(),
                ..Border::default()
            },
            text_color: on_accent(p),
            ..button::Style::default()
        });
    let video = button(text("Video").size(12))
        .on_press(Message::Noop)
        .padding([5, 16])
        .style(move |_t, _s| button::Style {
            border: pill_border(p.bg_card_active),
            text_color: p.text_dim,
            ..button::Style::default()
        });
    row![song, video].spacing(6).into()
}

fn visualizer_card(app: &RustyTube) -> Element<'static, Message> {
    let p = theme::at(app.theme_index);
    let transport = app.status.transport.clone();
    let inner = match &transport {
        TransportState::Loading(what) => column![
            text("bzvibs")
                .size(28)
                .color(p.text_dim)
                .font(iced::Font::MONOSPACE),
            text(format!("loading {what}…")).size(12).color(p.accent),
        ],
        TransportState::Playing | TransportState::Paused => column![
            text("bzvibs")
                .size(28)
                .color(p.accent)
                .font(iced::Font::MONOSPACE),
            text(if transport == TransportState::Playing {
                "visualizer arrives with PR-12"
            } else {
                "paused"
            })
            .size(11)
            .color(p.text_dim),
        ],
        TransportState::Stopped => column![
            text("bzvibs")
                .size(28)
                .color(p.accent_soft)
                .font(iced::Font::MONOSPACE),
            text("search to fill this stage").size(11).color(p.text_dim),
        ],
    }
    .spacing(8)
    .align_x(Alignment::Center);

    container(inner)
        .width(340)
        .height(340)
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .style(move |_t| container::Style {
            background: Some(Background::Gradient(
                iced::gradient::Linear::new(std::f32::consts::FRAC_PI_2)
                    .add_stop(0.0, p.bg_card)
                    .add_stop(1.0, p.bg_base)
                    .into(),
            )),
            border: Border {
                radius: 24.0.into(),
                color: p.accent,
                width: 1.0,
            },
            ..container::Style::default()
        })
        .into()
}

fn stage(app: &RustyTube) -> Element<'static, Message> {
    let p = theme::at(app.theme_index);
    let mut col = column![mode_pills(app), visualizer_card(app)]
        .spacing(14)
        .padding(16)
        .align_x(Alignment::Center)
        .width(Fill);

    if let Some(track) = &app.status.current {
        col = col.push(
            column![
                text(track.title.clone()).size(15).color(p.text),
                text(track.artist.clone()).size(12).color(p.text_dim),
            ]
            .spacing(2)
            .align_x(Alignment::Center),
        );
    }
    if let Some(err) = &app.status.error {
        col = col.push(text(format!("error: {err}")).size(11).color(p.accent));
    }
    col.into()
}

// ---------- right panel ----------

fn chips_with(p: &'static Palette) -> Element<'static, Message> {
    let mut chips = row![].spacing(6);
    for (i, label) in ["All", "Chill", "Focus", "Sleep", "Familiar"]
        .iter()
        .enumerate()
    {
        let active = i == 0;
        let label = label.to_string();
        let chip = container(text(label).size(11))
            .padding([4, 12])
            .style(move |_t| container::Style {
                background: Some(if active {
                    Background::Color(p.accent)
                } else {
                    Background::Color(p.bg_card)
                }),
                border: Border {
                    radius: 999.0.into(),
                    ..Border::default()
                },
                ..container::Style::default()
            });
        chips = chips.push(chip);
    }
    chips.into()
}

fn queue_row(app: &RustyTube, i: usize) -> Element<'static, Message> {
    let p = theme::at(app.theme_index);
    let t = &app.results[i];
    let active = app
        .status
        .current
        .as_ref()
        .map(|c| c.video_id == t.video_id)
        .unwrap_or(false);
    let row_content = row![
        text(format!("{}.", i + 1)).size(11).color(p.text_dim),
        column![
            text(t.title.clone()).size(12).color(p.text),
            text(t.artist.clone()).size(10).color(p.text_dim),
        ]
        .spacing(1),
        text(t.duration.clone()).size(11).color(p.text_dim),
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    button(row_content)
        .on_press(Message::TrackClicked(i))
        .width(Fill)
        .padding([8, 10])
        .style(move |_t, _s| button::Style {
            background: Some(if active {
                Background::Color(p.bg_card_active)
            } else {
                Background::Color(Color::TRANSPARENT)
            }),
            border: Border {
                radius: 10.0.into(),
                color: if active { p.accent } else { Color::TRANSPARENT },
                width: if active { 1.0 } else { 0.0 },
            },
            text_color: p.text,
            ..button::Style::default()
        })
        .into()
}

fn right_panel(app: &RustyTube) -> Element<'static, Message> {
    let p = theme::at(app.theme_index);
    let tabs = ["UP NEXT", "LYRICS", "RELATED"];
    let tab_vals = [RightTab::UpNext, RightTab::Lyrics, RightTab::Related];
    let mut tabs_row = row![].spacing(4);
    for (label, tab) in tabs.iter().zip(tab_vals) {
        let active = app.tab == tab;
        let label = label.to_string();
        let msg = Message::TabSelected(tab);
        let tab_btn = button(text(label).size(10))
            .on_press(msg)
            .padding([4, 8])
            .style(move |_t, _s| button::Style {
                background: Some(if active {
                    Background::Color(p.bg_card_active)
                } else {
                    Background::Color(Color::TRANSPARENT)
                }),
                border: Border {
                    radius: 8.0.into(),
                    ..Border::default()
                },
                text_color: if active { p.accent } else { p.text_dim },
                ..button::Style::default()
            });
        tabs_row = tabs_row.push(tab_btn);
    }

    let mut body = column![].spacing(8);
    match app.tab {
        RightTab::UpNext => {
            body = body.push(chips_with(p));
            if app.results.is_empty() {
                body = body.push(text("Queue is empty").size(12).color(p.text_dim));
            } else {
                let rows = app
                    .results
                    .iter()
                    .enumerate()
                    .map(|(i, _)| queue_row(app, i));
                body = body.push(scrollable(column(rows).spacing(2)).height(Fill));
            }
        }
        RightTab::Lyrics => {
            body = body.push(text("No lyrics loaded").size(12).color(p.text_dim));
        }
        RightTab::Related => {
            body = body.push(text("Nothing related yet").size(12).color(p.text_dim));
        }
    }

    column![tabs_row, body]
        .spacing(10)
        .padding(12)
        .width(Fill)
        .into()
}

// ---------- bottom bar ----------

fn mmss(secs: f64) -> String {
    let total = secs.max(0.0) as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

fn round_button(
    p: &'static Palette,
    label: String,
    primary: bool,
    msg: Message,
) -> button::Button<'static, Message> {
    button(text(label).size(if primary { 14 } else { 12 }))
        .on_press(msg)
        .width(if primary { 44 } else { 34 })
        .height(if primary { 44 } else { 34 })
        .style(move |_t, _s| button::Style {
            background: Some(if primary {
                accent_gradient(p)
            } else {
                Background::Color(p.bg_card)
            }),
            border: Border {
                radius: 999.0.into(),
                ..Border::default()
            },
            text_color: if primary { on_accent(p) } else { p.text },
            ..button::Style::default()
        })
}

fn accent_slider(
    p: &'static Palette,
    range: std::ops::RangeInclusive<f32>,
    value: f32,
    msg: fn(f32) -> Message,
) -> slider::Slider<'static, f32, Message> {
    slider(range, value, msg).style(move |_t, _s| slider::Style {
        rail: Rail {
            backgrounds: (
                Background::Color(p.accent),
                Background::Color(p.bg_card_active),
            ),
            width: 3.0,
            border: Border::default(),
        },
        handle: Handle {
            shape: HandleShape::Circle { radius: 6.0 },
            background: Background::Color(p.accent),
            border_width: 0.0,
            border_color: Color::TRANSPARENT,
        },
    })
}

fn playback_bar(app: &RustyTube) -> Element<'static, Message> {
    let p = theme::at(app.theme_index);
    let playing = app.status.transport == TransportState::Playing;
    let paused = app.status.transport == TransportState::Paused;
    let play_label = if playing {
        "❚❚".to_string()
    } else {
        "▶".to_string()
    };

    let duration = app.status.duration_secs;
    let position = app.status.position_secs.min(duration.max(0.001));
    let frac = if duration > 0.0 {
        (position / duration) as f32
    } else {
        0.0
    };

    row![
        round_button(p, "⏮".to_string(), false, Message::PrevPressed),
        round_button(p, play_label, true, Message::PlayPressed),
        round_button(p, "⏭".to_string(), false, Message::NextPressed),
        text(format!("{} / {}", mmss(position), mmss(duration)))
            .size(11)
            .color(p.text_dim),
        accent_slider(p, 0.0..=1.0, frac, Message::SeekChanged),
        text(if paused { "paused" } else { "" })
            .size(10)
            .color(p.text_dim),
        round_button(p, "⤨".to_string(), false, Message::ShuffleToggled),
        round_button(p, "↻".to_string(), false, Message::RepeatCycled),
        text(format!("{:?}", app.repeat)).size(9).color(p.text_dim),
        round_button(p, "♡".to_string(), false, Message::ThumbsUp),
        accent_slider(p, 0.0..=1.0, app.volume, Message::VolumeChanged),
    ]
    .spacing(8)
    .padding([10, 16])
    .align_y(Alignment::Center)
    .width(Fill)
    .into()
}

// ---------- root ----------

fn panel<'a>(
    app: &RustyTube,
    content: impl Into<Element<'a, Message>>,
) -> container::Container<'a, Message> {
    let bg = theme::at(app.theme_index).bg_panel;
    container(content).style(move |_t| container::Style {
        background: Some(bg.into()),
        ..container::Style::default()
    })
}

pub fn view(app: &RustyTube) -> Element<'static, Message> {
    let bg = theme::at(app.theme_index).bg_base;
    let body = row![
        panel(app, sidebar(app))
            .width(Length::Fixed(220.0))
            .height(Fill),
        column![topbar(app), stage(app)].width(Fill),
        panel(app, right_panel(app))
            .width(Length::Fixed(300.0))
            .height(Fill),
    ]
    .width(Fill)
    .height(Fill);

    let page = column![body, playback_bar(app)].width(Fill).height(Fill);

    container(page)
        .width(Fill)
        .height(Fill)
        .style(move |_t| container::Style {
            background: Some(bg.into()),
            ..container::Style::default()
        })
        .into()
}
