//! Vertical equalizer faders on the iced canvas: eight draggable bands with
//! dB values above, frequency labels below, and a shared 0 dB reference
//! line. Mouse drag maps directly to gain; the widget publishes
//! [`Message::EqGainChanged`] and never touches the engine itself.

use crate::Message;
use crate::theme::Palette;

use iced::widget::canvas::{self, Canvas, Stroke};
use iced::{Color, Element, Point, Rectangle};

pub const GAIN_RANGE: f32 = 12.0;
const TOP_PAD: f32 = 22.0;
const BOTTOM_PAD: f32 = 26.0;
const BANDS: usize = 8;

pub struct EqFaders {
    gains: [f32; 8],
    palette: &'static Palette,
    enabled: bool,
}

impl EqFaders {
    pub fn new(gains: [f32; 8], palette: &'static Palette, enabled: bool) -> Self {
        EqFaders {
            gains,
            palette,
            enabled,
        }
    }

    pub fn view(self) -> Element<'static, Message> {
        Canvas::new(self)
            .width(iced::Fill)
            .height(iced::Fill)
            .into()
    }
}

/// Map a gain to a y coordinate inside the track area.
fn y_for_gain(bounds: &Rectangle, gain: f32) -> f32 {
    let top = bounds.y + TOP_PAD;
    let bottom = bounds.y + bounds.height - BOTTOM_PAD;
    let g = gain.clamp(-GAIN_RANGE, GAIN_RANGE);
    let t = (GAIN_RANGE - g) / (2.0 * GAIN_RANGE); // +12 -> 0.0 (top)
    top + t * (bottom - top)
}

/// Map a y coordinate back to a gain.
fn gain_for_y(bounds: &Rectangle, y: f32) -> f32 {
    let top = bounds.y + TOP_PAD;
    let bottom = bounds.y + bounds.height - BOTTOM_PAD;
    let t = ((y - top) / (bottom - top).max(1.0)).clamp(0.0, 1.0);
    GAIN_RANGE - t * 2.0 * GAIN_RANGE
}

fn band_center_x(bounds: &Rectangle, i: usize) -> f32 {
    bounds.x + (i as f32 + 0.5) * bounds.width / BANDS as f32
}

fn band_under(bounds: &Rectangle, x: f32) -> Option<usize> {
    if x < bounds.x || x >= bounds.x + bounds.width {
        return None;
    }
    let i = ((x - bounds.x) / bounds.width * BANDS as f32) as usize;
    Some(i.min(BANDS - 1))
}

/// Public because the canvas `Program` trait's associated `State` is a
/// public interface; opaque to everyone else.
pub struct DragState {
    band: usize,
}

impl canvas::Program<Message> for EqFaders {
    type State = Option<DragState>;

    fn update(
        &self,
        state: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let iced::event::Event::Mouse(mouse) = event else {
            return None;
        };
        match mouse {
            iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left) => {
                let pos = cursor.position_in(bounds)?;
                let band = band_under(&bounds, pos.x)?;
                *state = Some(DragState { band });
                Some(canvas::Action::publish(Message::EqGainChanged(
                    band,
                    gain_for_y(&bounds, pos.y),
                )))
            }
            iced::mouse::Event::CursorMoved { .. } => {
                let drag = state.as_ref()?;
                let pos = cursor.position_in(bounds)?;
                Some(canvas::Action::publish(Message::EqGainChanged(
                    drag.band,
                    gain_for_y(&bounds, pos.y),
                )))
            }
            iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left) => {
                if state.is_some() {
                    *state = None;
                    Some(canvas::Action::capture())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let p = self.palette;
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        // Canvas frames are translated to bounds.position already.
        let local = Rectangle::new(Point::ORIGIN, bounds.size());
        let alpha = if self.enabled { 1.0 } else { 0.35 };

        let track_top = local.y + TOP_PAD;
        let track_bottom = local.y + local.height - BOTTOM_PAD;
        let zero_y = y_for_gain(&local, 0.0);

        // Shared 0 dB reference line across all faders.
        let zero_line = canvas::Path::new(|b| {
            b.move_to(Point::new(local.x + 4.0, zero_y));
            b.line_to(Point::new(local.x + local.width - 4.0, zero_y));
        });
        frame.stroke(
            &zero_line,
            Stroke {
                style: Color::from_rgba(p.text_dim.r, p.text_dim.g, p.text_dim.b, 0.4).into(),
                width: 1.0,
                ..Stroke::default()
            },
        );

        for i in 0..BANDS {
            let x = band_center_x(&local, i);
            let gain = self.gains[i].clamp(-GAIN_RANGE, GAIN_RANGE);
            let y = y_for_gain(&local, gain);

            // Track.
            let track = canvas::Path::new(|b| {
                b.move_to(Point::new(x, track_top));
                b.line_to(Point::new(x, track_bottom));
            });
            frame.stroke(
                &track,
                Stroke {
                    style: Color::from_rgba(
                        p.bg_card_active.r,
                        p.bg_card_active.g,
                        p.bg_card_active.b,
                        1.0,
                    )
                    .into(),
                    width: 6.0,
                    ..Stroke::default()
                },
            );

            // Filled portion from 0 dB toward the handle.
            let fill = canvas::Path::new(|b| {
                b.move_to(Point::new(x, zero_y));
                b.line_to(Point::new(x, y));
            });
            frame.stroke(
                &fill,
                Stroke {
                    style: Color::from_rgba(p.accent.r, p.accent.g, p.accent.b, alpha).into(),
                    width: 6.0,
                    ..Stroke::default()
                },
            );

            // Handle.
            let handle = canvas::Path::circle(Point::new(x, y), 8.0);
            frame.fill(
                &handle,
                Color::from_rgba(p.accent.r, p.accent.g, p.accent.b, alpha),
            );

            // dB value above.
            frame.fill_text(canvas::Text {
                content: format!("{gain:+.0}"),
                position: Point::new(x, track_top - 12.0),
                color: Color::from_rgba(p.text.r, p.text.g, p.text.b, alpha),
                size: 11.0.into(),
                align_x: iced::widget::text::Alignment::Center,
                align_y: iced::alignment::Vertical::Bottom,
                ..Default::default()
            });

            // Frequency label below.
            frame.fill_text(canvas::Text {
                content: rt_config::band_label(i),
                position: Point::new(x, track_bottom + 8.0),
                color: Color::from_rgba(p.text_dim.r, p.text_dim.g, p.text_dim.b, alpha),
                size: 10.0.into(),
                align_x: iced::widget::text::Alignment::Center,
                align_y: iced::alignment::Vertical::Top,
                ..Default::default()
            });
        }

        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds() -> Rectangle {
        Rectangle::new(Point::new(0.0, 0.0), iced::Size::new(400.0, 200.0))
    }

    #[test]
    fn gain_round_trips_through_pixel_coordinates() {
        let b = bounds();
        for g in [-12.0f32, -6.0, 0.0, 3.5, 12.0] {
            let y = y_for_gain(&b, g);
            assert!((gain_for_y(&b, y) - g).abs() < 0.05, "{g}");
        }
    }

    #[test]
    fn positive_gain_is_above_zero_line_and_clamps() {
        let b = bounds();
        assert!(y_for_gain(&b, 6.0) < y_for_gain(&b, 0.0));
        assert!((y_for_gain(&b, 99.0) - y_for_gain(&b, 12.0)).abs() < f32::EPSILON);
        assert!((y_for_gain(&b, -99.0) - y_for_gain(&b, -12.0)).abs() < f32::EPSILON);
    }

    #[test]
    fn band_lookup_covers_all_columns_and_rejects_outside() {
        let b = bounds();
        for i in 0..BANDS {
            let x = band_center_x(&b, i);
            assert_eq!(band_under(&b, x), Some(i));
        }
        assert_eq!(band_under(&b, -1.0), None);
        assert_eq!(band_under(&b, b.width + 1.0), None);
    }
}
