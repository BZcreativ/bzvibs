//! The bzvibs shell per the concept boards: one skeleton, three theme
//! palettes. Left sidebar (brand, nav, playlists), centered search pill,
//! Song/Video mode pills above the visualizer stage, right panel with
//! up-next chips and queue rows, bottom transport bar with a round play
//! button and thin accent sliders.
//!
//! Style rule: closures capture only `p: &'static Palette` (and Copy
//! booleans), never the app borrow, so widgets stay 'static.

use crate::theme::{self, Palette};
use crate::{AuthState, BzvibsApp, Message, RightTab, Section};

use iced::widget::slider::{Handle, HandleShape, Rail};
use iced::widget::{
    button, column, container, pick_list, row, scrollable, slider, text, text_input,
};
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

// ---------- shared button design system ----------
// Four kinds, one geometry: primary (accent gradient, strong actions),
// secondary (card fill, visible actions), ghost (transparent, tertiary),
// destructive-adjacent actions use secondary with dim text. All get hover
// brightening and consistent padding; disabled is dim text on transparent.

const BTN_PAD: [f32; 2] = [6.0, 14.0];

fn text_button(
    p: &'static Palette,
    label: String,
    kind: ButtonKind,
    enabled: bool,
    msg: Message,
) -> button::Button<'static, Message> {
    button(text(label).size(12))
        .on_press_maybe(enabled.then_some(msg))
        .padding(BTN_PAD)
        .style(move |_t, status| {
            let hovered = status == button::Status::Hovered;
            let disabled = !enabled;
            let (bg, fg) = match kind {
                ButtonKind::Primary => (
                    if disabled {
                        p.bg_card
                    } else if hovered {
                        p.accent_soft
                    } else {
                        p.accent
                    },
                    on_accent(p),
                ),
                ButtonKind::Secondary => {
                    if disabled {
                        (p.bg_card, p.text_dim)
                    } else if hovered {
                        (p.bg_card_active, p.text)
                    } else {
                        (p.bg_card, p.text)
                    }
                }
                ButtonKind::Ghost => {
                    if disabled {
                        (Color::TRANSPARENT, p.text_dim)
                    } else if hovered {
                        (p.bg_card, p.text)
                    } else {
                        (Color::TRANSPARENT, p.text_dim)
                    }
                }
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: match kind {
                    ButtonKind::Primary => Border {
                        radius: 999.0.into(),
                        ..Border::default()
                    },
                    ButtonKind::Secondary | ButtonKind::Ghost => pill_border(if disabled {
                        p.bg_card
                    } else {
                        p.bg_card_active
                    }),
                },
                text_color: fg,
                ..button::Style::default()
            }
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ButtonKind {
    Primary,
    Secondary,
    Ghost,
}

// ---------- sidebar ----------

fn sidebar(app: &BzvibsApp) -> Element<'static, Message> {
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

    let nav = ["Home", "Explore", "Library", "Equalizer", "Settings"];
    let sections = [
        Section::Home,
        Section::Explore,
        Section::Library,
        Section::Equalizer,
        Section::Settings,
    ];
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

    let new_playlist = text_button(
        p,
        "+ New playlist".into(),
        ButtonKind::Secondary,
        true,
        Message::NewPlaylistModal(true),
    )
    .width(Fill);

    // Google account section per the concept brief: prominent sign-in pill
    // when signed out; account state, your playlists, and disconnect when
    // signed in.
    let mut auth_col = column![].spacing(4).padding([4, 0]);
    match &app.auth {
        AuthState::SignedOut => {
            let sign_in = button(text("Sign in with Google").size(13))
                .on_press(Message::AuthStartPressed)
                .padding([8, 12])
                .width(Fill)
                .style(move |_t, _s| button::Style {
                    background: Some(accent_gradient(p)),
                    border: Border {
                        radius: 12.0.into(),
                        ..Border::default()
                    },
                    text_color: on_accent(p),
                    ..button::Style::default()
                });
            auth_col = auth_col.push(sign_in);
        }
        AuthState::AwaitingUser {
            user_code,
            verification_url,
        } => {
            auth_col = auth_col.push(
                column![
                    text("Open in your browser:").size(10).color(p.text_dim),
                    text(verification_url.clone()).size(10).color(p.text),
                    text("and enter this code:").size(10).color(p.text_dim),
                    text(user_code.clone()).size(16).color(p.accent),
                    text("waiting for you to finish…")
                        .size(10)
                        .color(p.text_dim),
                ]
                .spacing(2),
            );
        }
        AuthState::SignedIn { playlists } => {
            auth_col = auth_col.push(text("● Signed in").size(11).color(p.accent));
            if playlists.is_empty() {
                auth_col = auth_col.push(text("  loading playlists…").size(11).color(p.text_dim));
            }
            let mut n_shown = 0usize;
            for pl in playlists.iter().take(8) {
                let id = pl.id.clone();
                let title = pl.title.clone();
                let pl_btn = button(
                    column![
                        text(title).size(11).color(p.text),
                        text(pl.track_count.clone()).size(9).color(p.text_dim),
                    ]
                    .spacing(0),
                )
                .on_press(Message::PlaylistClicked(id))
                .width(Fill)
                .padding([3, 8])
                .style(move |_t, status| button::Style {
                    background: Some(if status == button::Status::Hovered {
                        Background::Color(palette_of(p).bg_card)
                    } else {
                        Background::Color(Color::TRANSPARENT)
                    }),
                    border: Border {
                        radius: 8.0.into(),
                        ..Border::default()
                    },
                    text_color: p.text,
                    ..button::Style::default()
                });
                auth_col = auth_col.push(pl_btn);
                n_shown += 1;
            }
            if playlists.len() > n_shown {
                auth_col = auth_col.push(
                    text(format!("  …and {} more", playlists.len() - n_shown))
                        .size(10)
                        .color(p.text_dim),
                );
            }
            let disconnect = button(text("Disconnect".to_string()).size(11))
                .on_press(Message::AuthDisconnectPressed)
                .padding([4, 10])
                .style(move |_t, _s| button::Style {
                    border: pill_border(p.text_dim),
                    text_color: p.text_dim,
                    ..button::Style::default()
                });
            auth_col = auth_col.push(disconnect);
        }
    }

    let mut playlists = column![text("Playlists").size(11).color(p.text_dim)].spacing(2);
    if !matches!(app.auth, AuthState::SignedIn { .. }) {
        // Before sign-in these are real starter mixes: each runs its search
        // and loads the queue, so the sidebar is never inert.
        for (label, query) in crate::STARTER_MIXES {
            let q = query.to_string();
            let b = button(text(format!("  {label}")).size(12))
                .on_press(Message::MoodSearch(q))
                .width(Fill)
                .padding([2, 6])
                .style(move |_t, status| button::Style {
                    background: Some(if status == button::Status::Hovered {
                        Background::Color(palette_of(p).bg_card)
                    } else {
                        Background::Color(Color::TRANSPARENT)
                    }),
                    border: Border {
                        radius: 6.0.into(),
                        ..Border::default()
                    },
                    text_color: p.text_dim,
                    ..button::Style::default()
                });
            playlists = playlists.push(b);
        }
    }

    column![brand, nav_col, auth_col, new_playlist, playlists]
        .spacing(10)
        .padding(16)
        .width(Fill)
        .into()
}

/// Resolve a palette back to its &'static slot for move closures.
fn palette_of(p: &Palette) -> &'static Palette {
    theme::at(
        theme::THEMES
            .iter()
            .position(|x| x.name == p.name)
            .unwrap_or(0),
    )
}

// ---------- top bar ----------

fn topbar(app: &BzvibsApp) -> Element<'static, Message> {
    let p = theme::at(app.theme_index);
    let query = app.search_query.clone();
    let search = text_input("Search songs, albums, artists, podcasts…", &query)
        .id(crate::SEARCH_INPUT_ID.to_string())
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

fn mode_pills(app: &BzvibsApp) -> Element<'static, Message> {
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
        .padding([5, 16])
        .style(move |_t, _s| button::Style {
            border: pill_border(p.bg_card_active),
            text_color: p.text_dim,
            ..button::Style::default()
        });
    // Honest disabled affordance: no press handler, caption explains why.
    row![
        song,
        video,
        text("video mode is a later milestone")
            .size(10)
            .color(p.text_dim),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .into()
}

fn visualizer_card(app: &BzvibsApp) -> Element<'static, Message> {
    let p = theme::at(app.theme_index);
    let transport = app.status.transport.clone();
    let loading = matches!(transport, TransportState::Loading(_));
    let active = transport == TransportState::Playing;

    // Caption line under the ring.
    let caption = match &transport {
        TransportState::Loading(what) => format!("loading {what}…"),
        TransportState::Playing => String::new(),
        TransportState::Paused => "paused".to_string(),
        TransportState::Stopped => "search to fill this stage".to_string(),
    };

    let ring = crate::visualizer::SpectrumRing::new(app.spectrum.clone(), p, active).view();

    let mut inner = column![].spacing(8).align_x(Alignment::Center);
    if loading {
        inner = inner.push(
            text("bzvibs")
                .size(20)
                .color(p.text_dim)
                .font(iced::Font::MONOSPACE),
        );
    }
    inner = inner.push(container(ring).width(300).height(300));
    if !caption.is_empty() {
        inner = inner.push(text(caption).size(11).color(p.text_dim));
    }

    container(inner)
        .width(340)
        .height(360)
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

// ---------- settings stage ----------

fn settings_stage(app: &BzvibsApp) -> Element<'static, Message> {
    let p = theme::at(app.theme_index);

    let heading = text("Settings").size(24).color(p.text);

    // Appearance: theme cycler.
    let theme_label = format!("◑ {}", p.name);
    let appearance = column![
        text("Appearance").size(15).color(p.text),
        button(text(theme_label).size(12))
            .on_press(Message::ThemeCycled)
            .padding([6, 14])
            .style(move |_t, _s| button::Style {
                background: Some(Background::Color(p.bg_card)),
                border: pill_border(p.bg_card_active),
                text_color: p.text,
                ..button::Style::default()
            }),
    ]
    .spacing(6);

    // Typography: font family and interface text size pick lists.
    let font_row = row![
        text("Font")
            .size(13)
            .color(p.text_dim)
            .width(Length::Fixed(140.0)),
        pick_list(
            rt_config::ui_prefs::FontChoice::all(),
            Some(app.prefs.font),
            Message::PrefsFontChanged,
        )
        .width(Length::Fixed(180.0))
        .text_size(12),
        text("applies after restart").size(10).color(p.text_dim),
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    let size_row = row![
        text("Interface size")
            .size(13)
            .color(p.text_dim)
            .width(Length::Fixed(140.0)),
        pick_list(
            rt_config::ui_prefs::TextSize::all(),
            Some(app.prefs.text_size),
            Message::PrefsTextSizeChanged,
        )
        .width(Length::Fixed(180.0))
        .text_size(12),
        text(format!("scale ×{:.2}", app.prefs.text_size.scale()))
            .size(10)
            .color(p.text_dim),
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    let typography = column![
        text("Typography").size(15).color(p.text),
        font_row,
        size_row,
        text("The size setting scales the whole interface immediately; the font switch needs a restart.")
            .size(10)
            .color(p.text_dim),
    ]
    .spacing(8);

    column![heading, appearance, typography]
        .spacing(18)
        .padding(24)
        .width(Fill)
        .into()
}

// ---------- explore and library stages ----------

fn tile(p: &'static Palette, label: String, msg: Message) -> button::Button<'static, Message> {
    button(text(label).size(14))
        .on_press(msg)
        .width(Length::Fixed(170.0))
        .height(Length::Fixed(80.0))
        .style(move |_t, status| button::Style {
            background: Some(if status == button::Status::Hovered {
                Background::Color(p.bg_card_active)
            } else {
                Background::Color(p.bg_card)
            }),
            border: Border {
                radius: 14.0.into(),
                color: p.bg_card_active,
                width: 1.0,
            },
            text_color: p.text,
            ..button::Style::default()
        })
}

fn explore_stage(app: &BzvibsApp) -> Element<'static, Message> {
    let p = theme::at(app.theme_index);
    let mut grid = column![].spacing(10);
    for chunk in crate::MOODS.chunks(3) {
        let mut r = row![].spacing(10);
        for (label, query) in chunk {
            r = r.push(tile(
                p,
                label.to_string(),
                Message::MoodSearch(query.to_string()),
            ));
        }
        grid = grid.push(r);
    }
    column![
        text("Explore").size(24).color(p.text),
        text("Pick a mood: each tile runs a real search and fills the queue.")
            .size(11)
            .color(p.text_dim),
        grid,
    ]
    .spacing(14)
    .padding(24)
    .into()
}

fn library_stage(app: &BzvibsApp) -> Element<'static, Message> {
    let p = theme::at(app.theme_index);
    match &app.auth {
        AuthState::SignedIn { playlists } => {
            let mut list = column![].spacing(6);
            if playlists.is_empty() {
                list = list.push(text("loading playlists…").size(12).color(p.text_dim));
            }
            for pl in playlists {
                list = list.push(
                    button(
                        row![
                            text(pl.title.clone()).size(13).color(p.text),
                            text(pl.track_count.clone()).size(10).color(p.text_dim),
                        ]
                        .spacing(10),
                    )
                    .on_press(Message::PlaylistClicked(pl.id.clone()))
                    .width(Fill)
                    .padding([8, 12])
                    .style(move |_t, status| button::Style {
                        background: Some(if status == button::Status::Hovered {
                            Background::Color(palette_of(p).bg_card_active)
                        } else {
                            Background::Color(palette_of(p).bg_card)
                        }),
                        border: Border {
                            radius: 10.0.into(),
                            ..Border::default()
                        },
                        text_color: p.text,
                        ..button::Style::default()
                    }),
                );
            }
            column![
                text("Library").size(24).color(p.text),
                text("Your saved playlists").size(11).color(p.text_dim),
                scrollable(list).height(Fill),
            ]
            .spacing(14)
            .padding(24)
            .into()
        }
        _ => {
            let mut col = column![
                text("Library").size(24).color(p.text),
                text("Sign in to see your saved playlists and liked music.")
                    .size(12)
                    .color(p.text_dim),
                text_button(
                    p,
                    if app.auth_loading {
                        "Starting sign-in…".to_string()
                    } else {
                        "Sign in with Google".to_string()
                    },
                    ButtonKind::Primary,
                    !app.auth_loading,
                    Message::AuthStartPressed,
                ),
            ]
            .spacing(14)
            .padding(24);
            if app.browser_cookie.is_some() {
                col = col.push(browser_cookie_card(app));
            }
            if app.auth_setup.is_some() {
                col = col.push(oauth_setup_card(app));
            }
            col.into()
        }
    }
}

/// Primary sign-in: paste the browser session's cookie header. This is the
/// method that needs no Google Cloud project, so it is the default path.
fn browser_cookie_card(app: &BzvibsApp) -> Element<'static, Message> {
    let p = theme::at(app.theme_index);
    let value = app.browser_cookie.clone().unwrap_or_default();

    let input = text_input("cookie: YSC=…; SAPISID=…; __Secure-3PAPISID=…", &value)
        .on_input(Message::BrowserCookieChanged)
        .on_submit(Message::BrowserSignInPressed)
        .size(11)
        .padding([6, 10])
        .style(move |_t, _s| text_input::Style {
            background: Color::from_rgba(0.0, 0.0, 0.0, 0.4).into(),
            border: pill_border(p.bg_card_active),
            icon: p.text_dim,
            placeholder: p.text_dim,
            value: p.text,
            selection: p.accent,
        });

    let step = |n: &'static str, body: &'static str| {
        text(format!("{n} {body}")).size(10).color(p.text_dim)
    };

    let card = column![
        text("Sign in with your browser session").size(16).color(p.text),
        step("1.", "Open music.youtube.com in your browser and sign in."),
        step("2.", "Press F12 → Network → click any request."),
        step("3.", "Find the 'cookie' request header and copy its value."),
        step("4.", "Paste it here and press Sign in."),
        text("No Google Cloud project is needed. The cookie is stored in the Windows credential store and can be removed with Disconnect.")
            .size(10)
            .color(p.accent),
        input,
        row![
            text_button(
                p,
                if app.auth_loading {
                    "Signing in…".to_string()
                } else {
                    "Sign in".to_string()
                },
                ButtonKind::Primary,
                !app.auth_loading,
                Message::BrowserSignInPressed,
            ),
            text_button(
                p,
                "Use OAuth instead".into(),
                ButtonKind::Ghost,
                true,
                Message::OauthInsteadPressed,
            ),
            text_button(
                p,
                "Cancel".into(),
                ButtonKind::Ghost,
                true,
                Message::AuthSetupCancel,
            ),
        ]
        .spacing(8),
    ]
    .spacing(10)
    .padding(18);

    container(card)
        .width(Length::Fixed(520.0))
        .style(move |_t| container::Style {
            background: Some(Background::Color(p.bg_card)),
            border: Border {
                radius: 14.0.into(),
                color: p.accent,
                width: 1.0,
            },
            ..container::Style::default()
        })
        .into()
}

/// In-app OAuth client setup: the credentials a user must create in Google
/// Cloud, pasted once and stored in the OS keychain. Removes the need for
/// environment variables when the app is launched by double-click.
fn oauth_setup_card(app: &BzvibsApp) -> Element<'static, Message> {
    let p = theme::at(app.theme_index);
    let (id, secret) = app.auth_setup.clone().unwrap_or_default();

    let field = |label: &'static str,
                 value: String,
                 placeholder: &'static str,
                 on_input: fn(String) -> Message| {
        column![
            text(label).size(10).color(p.text_dim),
            text_input(placeholder, &value)
                .on_input(on_input)
                .size(12)
                .padding([6, 10])
                .style(move |_t, _s| text_input::Style {
                    background: Color::from_rgba(0.0, 0.0, 0.0, 0.4).into(),
                    border: pill_border(p.bg_card_active),
                    icon: p.text_dim,
                    placeholder: p.text_dim,
                    value: p.text,
                    selection: p.accent,
                }),
        ]
        .spacing(3)
    };

    let card = column![
        text("Connect a Google OAuth client").size(16).color(p.text),
        text("1. console.cloud.google.com → create a project")
            .size(10)
            .color(p.text_dim),
        text("2. OAuth consent screen → External → add your Gmail as a test user")
            .size(10)
            .color(p.text_dim),
        text("3. Credentials → Create credentials → OAuth client ID")
            .size(10)
            .color(p.text_dim),
        text("4. Application type: TVs and Limited Input Devices")
            .size(10)
            .color(p.accent),
        text("Paste the id and secret below. They are stored in the Windows credential store, not in source.")
            .size(10)
            .color(p.text_dim),
        field(
            "Client ID",
            id,
            "xxxxxxxx.apps.googleusercontent.com",
            Message::OauthIdChanged,
        ),
        field("Client secret", secret, "GOCSPX-…", Message::OauthSecretChanged),
        row![
            text_button(
                p,
                "Save and sign in".into(),
                ButtonKind::Primary,
                !app.auth_loading,
                Message::OauthSetupSave,
            ),
            text_button(
                p,
                "Cancel".into(),
                ButtonKind::Ghost,
                true,
                Message::OauthSetupCancel,
            ),
        ]
        .spacing(8),
    ]
    .spacing(10)
    .padding(18);

    container(card)
        .width(Length::Fixed(430.0))
        .style(move |_t| container::Style {
            background: Some(Background::Color(p.bg_card)),
            border: Border {
                radius: 14.0.into(),
                color: p.accent,
                width: 1.0,
            },
            ..container::Style::default()
        })
        .into()
}

fn new_playlist_dialog(app: &BzvibsApp) -> Element<'static, Message> {
    let p = theme::at(app.theme_index);
    let Some(name) = &app.new_playlist else {
        return row![].into();
    };
    let name = name.clone();
    let signed_in = app.authed.is_some();
    let field = text_input("playlist name", &name)
        .on_input(Message::NewPlaylistNameChanged)
        .on_submit(Message::NewPlaylistSubmitted)
        .size(13)
        .padding([8, 12])
        .style(move |_t, _s| text_input::Style {
            background: Color::from_rgba(0.0, 0.0, 0.0, 0.4).into(),
            border: pill_border(p.bg_card_active),
            icon: p.text_dim,
            placeholder: p.text_dim,
            value: p.text,
            selection: p.accent,
        });
    let card = column![
        text("New playlist").size(18).color(p.text),
        field,
        if signed_in {
            text("Created as Private on your account.")
                .size(10)
                .color(p.text_dim)
        } else {
            text("Sign in with Google to create playlists.")
                .size(10)
                .color(p.accent)
        },
        row![
            text_button(
                p,
                "Create".into(),
                ButtonKind::Primary,
                signed_in,
                Message::NewPlaylistSubmitted,
            ),
            text_button(
                p,
                "Cancel".into(),
                ButtonKind::Ghost,
                true,
                Message::NewPlaylistModal(false),
            ),
        ]
        .spacing(8),
    ]
    .spacing(12)
    .padding(20);
    container(card)
        .width(Length::Fixed(360.0))
        .style(move |_t| container::Style {
            background: Some(Background::Color(p.bg_panel)),
            border: Border {
                radius: 16.0.into(),
                color: p.accent,
                width: 1.0,
            },
            ..container::Style::default()
        })
        .into()
}

fn stage_inner(app: &BzvibsApp) -> Element<'static, Message> {
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

/// Section dispatch plus the New playlist dialog overlay.
fn stage(app: &BzvibsApp) -> Element<'static, Message> {
    let base = match app.section {
        Section::Equalizer => eq_stage(app),
        Section::Settings => settings_stage(app),
        Section::Explore => explore_stage(app),
        Section::Library => library_stage(app),
        Section::Home => stage_inner(app),
    };
    if app.help_open {
        return column![base, help_overlay(app)].spacing(8).into();
    }
    if app.new_playlist.is_some() {
        return column![base, new_playlist_dialog(app)].spacing(8).into();
    }
    base
}

fn help_overlay(app: &BzvibsApp) -> Element<'static, Message> {
    let p = theme::at(app.theme_index);
    let mut rows = column![].spacing(4);
    for (label, keys) in crate::shortcuts::TABLE {
        rows = rows.push(
            row![
                text(label)
                    .size(12)
                    .color(p.text)
                    .width(Length::Fixed(190.0)),
                text(keys).size(12).color(p.accent),
            ]
            .spacing(12),
        );
    }
    let card = column![
        text("Keyboard shortcuts").size(18).color(p.text),
        rows,
        text("Press ? again to close").size(10).color(p.text_dim),
    ]
    .spacing(12)
    .padding(20);
    container(card)
        .width(Length::Fixed(380.0))
        .style(move |_t| container::Style {
            background: Some(Background::Color(p.bg_panel)),
            border: Border {
                radius: 16.0.into(),
                color: p.accent,
                width: 1.0,
            },
            ..container::Style::default()
        })
        .into()
}

// ---------- equalizer stage ----------

fn eq_preset_chip(
    p: &'static Palette,
    label: String,
    active: bool,
    msg: Message,
) -> button::Button<'static, Message> {
    button(text(label).size(11))
        .on_press(msg)
        .padding([4, 12])
        .style(move |_t, _s| button::Style {
            background: Some(if active {
                Background::Color(p.accent)
            } else {
                Background::Color(p.bg_card)
            }),
            border: pill_border(Color::TRANSPARENT),
            text_color: if active { on_accent(p) } else { p.text_dim },
            ..button::Style::default()
        })
}

fn eq_stage(app: &BzvibsApp) -> Element<'static, Message> {
    let p = theme::at(app.theme_index);

    let eq_enabled = app.eq.enabled;
    let header = row![
        text("Equalizer").size(22).color(p.text),
        text(app.eq.preset_name.clone()).size(12).color(p.accent),
        button(text(if eq_enabled { "EQ on" } else { "EQ bypassed" }).size(11))
            .on_press(Message::EqBypassToggled)
            .padding([4, 12])
            .style(move |_t, _s| button::Style {
                background: Some(if eq_enabled {
                    Background::Color(p.bg_card_active)
                } else {
                    Background::Color(p.bg_card)
                }),
                border: pill_border(p.accent),
                text_color: p.text,
                ..button::Style::default()
            }),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    let mut presets = row![].spacing(6);
    for (i, preset) in rt_config::builtin_presets().iter().enumerate() {
        let active = app.eq.preset_name == preset.name;
        presets = presets.push(eq_preset_chip(
            p,
            preset.name.clone(),
            active,
            Message::EqBuiltinSelected(i),
        ));
    }
    for (i, preset) in app.eq.user_presets.iter().enumerate() {
        let active = app.eq.preset_name == preset.name;
        presets = presets.push(eq_preset_chip(
            p,
            preset.name.clone(),
            active,
            Message::EqUserSelected(i),
        ));
    }

    let save_name = app.eq.save_name.clone();
    let is_user_preset = app
        .eq
        .user_presets
        .iter()
        .any(|p| p.name == app.eq.preset_name);
    // Unified design system: primary = Save, secondary = Rename/Delete
    // (disabled unless a user preset is selected), ghost = Reset.
    let action_row = row![
        text_input("preset name", &save_name)
            .on_input(Message::EqNameChanged)
            .size(12)
            .padding([6, 12])
            .style(move |_t, _s| text_input::Style {
                background: Color::from_rgba(0.0, 0.0, 0.0, 0.35).into(),
                border: pill_border(p.bg_card_active),
                icon: p.text_dim,
                placeholder: p.text_dim,
                value: p.text,
                selection: p.accent,
            })
            .width(Length::Fixed(170.0)),
        text_button(
            p,
            "Save".into(),
            ButtonKind::Primary,
            true,
            Message::EqSavePressed
        ),
        text_button(
            p,
            "Rename".into(),
            ButtonKind::Secondary,
            is_user_preset,
            Message::EqRenamePressed,
        ),
        text_button(
            p,
            "Delete".into(),
            ButtonKind::Secondary,
            is_user_preset,
            Message::EqDeletePressed,
        ),
        text_button(
            p,
            "Reset".into(),
            ButtonKind::Ghost,
            true,
            Message::EqResetPressed
        ),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    // Vertical faders: drag on the canvas maps y to gain directly.
    let faders = crate::eq_faders::EqFaders::new(app.eq.gains_db, p, app.eq.enabled).view();
    let fader_box = container(faders)
        .width(Fill)
        .height(Length::Fixed(260.0))
        .padding([8, 16])
        .style(move |_t| container::Style {
            background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.25))),
            border: Border {
                radius: 16.0.into(),
                color: p.bg_card_active,
                width: 1.0,
            },
            ..container::Style::default()
        });

    column![header, presets, action_row, fader_box]
        .spacing(14)
        .padding(24)
        .width(Fill)
        .into()
}

// ---------- right panel ----------

fn chips_with(p: &'static Palette, selected: usize) -> Element<'static, Message> {
    let mut chips = row![].spacing(6);
    for (i, label) in crate::CHIPS.iter().enumerate() {
        let active = i == selected;
        let label = label.to_string();
        // Chips are live: selecting one re-runs the queue search for that
        // mood (InnerTube has no anonymous continuation-token filter, so a
        // mood search is the honest equivalent).
        let chip = button(text(label).size(11))
            .on_press(Message::ChipSelected(i))
            .padding([4, 12])
            .style(move |_t, status| button::Style {
                background: Some(if active {
                    Background::Color(palette_of(p).accent)
                } else if status == button::Status::Hovered {
                    Background::Color(palette_of(p).bg_card_active)
                } else {
                    Background::Color(palette_of(p).bg_card)
                }),
                border: Border {
                    radius: 999.0.into(),
                    ..Border::default()
                },
                text_color: if active {
                    Color::from_rgb(0.05, 0.03, 0.02)
                } else {
                    palette_of(p).text_dim
                },
                ..button::Style::default()
            });
        chips = chips.push(chip);
    }
    chips.into()
}

fn queue_row(app: &BzvibsApp, i: usize) -> Element<'static, Message> {
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

fn right_panel(app: &BzvibsApp) -> Element<'static, Message> {
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
            body = body.push(chips_with(p, app.active_chip));
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
        RightTab::Lyrics => match &app.lyrics {
            Some(l) if !l.lines.is_empty() => {
                let mut col = column![].spacing(4);
                for line in &l.lines {
                    col = col.push(text(line.clone()).size(12).color(p.text));
                }
                body = body.push(scrollable(col).height(Fill));
                if !l.source.is_empty() {
                    body = body.push(
                        text(format!("from {}", l.source))
                            .size(10)
                            .color(p.text_dim),
                    );
                }
            }
            Some(_) => {
                body = body.push(text("No lyrics for this track").size(12).color(p.text_dim));
            }
            None => {
                body = body.push(
                    text(if app.status.current.is_some() {
                        "Loading lyrics…"
                    } else {
                        "Play something to see lyrics"
                    })
                    .size(12)
                    .color(p.text_dim),
                );
            }
        },
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

fn accent_slider<F>(
    p: &'static Palette,
    range: std::ops::RangeInclusive<f32>,
    value: f32,
    msg: F,
) -> slider::Slider<'static, f32, Message>
where
    F: Fn(f32) -> Message + 'static,
{
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

fn playback_bar(app: &BzvibsApp) -> Element<'static, Message> {
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
    app: &BzvibsApp,
    content: impl Into<Element<'a, Message>>,
) -> container::Container<'a, Message> {
    let bg = theme::at(app.theme_index).bg_panel;
    container(content).style(move |_t| container::Style {
        background: Some(bg.into()),
        ..container::Style::default()
    })
}

pub fn view(app: &BzvibsApp) -> Element<'static, Message> {
    let bg = theme::at(app.theme_index).bg_base;
    let p = theme::at(app.theme_index);
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

    // Global error banner: errors used to render only inside the Home
    // stage, so a failed sign-in on the Library page looked like nothing
    // happened at all.
    let banner: Element<'static, Message> = match &app.status.error {
        Some(err) => container(
            row![
                text(format!("⚠ {err}")).size(11).color(p.bg_base),
                button(text("dismiss".to_string()).size(10))
                    .on_press(Message::ErrorDismissed)
                    .padding([2, 8])
                    .style(move |_t, _s| button::Style {
                        background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.25,))),
                        border: Border {
                            radius: 999.0.into(),
                            ..Border::default()
                        },
                        text_color: p.bg_base,
                        ..button::Style::default()
                    }),
            ]
            .spacing(12)
            .padding([6, 14]),
        )
        .width(Fill)
        .style(move |_t| container::Style {
            background: Some(Background::Color(p.accent)),
            ..container::Style::default()
        })
        .into(),
        None => row![].height(0).into(),
    };

    let page = column![banner, body, playback_bar(app)]
        .width(Fill)
        .height(Fill);

    container(page)
        .width(Fill)
        .height(Fill)
        .style(move |_t| container::Style {
            background: Some(bg.into()),
            ..container::Style::default()
        })
        .into()
}
