//! The center-stage visualizer: a circular spectrum ring drawn on the iced
//! canvas, themed per palette. Bars come from the app's smoothed spectrum
//! snapshot; zero bars render the resting ring.

use crate::Message;
use crate::theme::Palette;

use iced::widget::canvas::{self, Canvas, Stroke};
use iced::{Color, Element, Point, Rectangle};

pub struct SpectrumRing {
    bars: Vec<f32>,
    palette: &'static Palette,
    active: bool,
}

impl SpectrumRing {
    pub fn new(bars: Vec<f32>, palette: &'static Palette, active: bool) -> Self {
        SpectrumRing {
            bars,
            palette,
            active,
        }
    }

    pub fn view(self) -> Element<'static, Message> {
        Canvas::new(self)
            .width(iced::Fill)
            .height(iced::Fill)
            .into()
    }
}

impl canvas::Program<Message> for SpectrumRing {
    type State = ();

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

        let size = bounds.size();
        let center = Point::new(size.width / 2.0, size.height / 2.0);
        let r0 = size.width.min(size.height) * 0.42;
        let n = self.bars.len().max(1);

        // Resting ring.
        let ring = canvas::Path::circle(center, r0);
        frame.stroke(
            &ring,
            Stroke {
                style: Color::from_rgba(p.accent.r, p.accent.g, p.accent.b, 0.35).into(),
                width: 1.5,
                ..Stroke::default()
            },
        );

        // Radial bars, mirrored inward and outward.
        let alpha = if self.active { 0.95 } else { 0.4 };
        for (i, bar) in self.bars.iter().enumerate() {
            let angle = i as f32 * std::f32::consts::TAU / n as f32 - std::f32::consts::FRAC_PI_2;
            let (sin, cos) = angle.sin_cos();
            let outer = r0 * (1.0 + 0.55 * bar);
            let inner = r0 * (1.0 - 0.28 * bar);
            let spike = canvas::Path::new(|b| {
                b.move_to(Point::new(center.x + cos * inner, center.y + sin * inner));
                b.line_to(Point::new(center.x + cos * outer, center.y + sin * outer));
            });
            frame.stroke(
                &spike,
                Stroke {
                    style: Color::from_rgba(p.accent.r, p.accent.g, p.accent.b, alpha).into(),
                    width: 2.0,
                    ..Stroke::default()
                },
            );
        }

        vec![frame.into_geometry()]
    }
}
