//! The three board themes: ember, nebula, volt. One layout skeleton, three
//! palettes, per the bzvibs concept boards (2026-11-18). Swapping a theme is
//! a data change; the view reads everything from [`Palette`].

use iced::Color;

#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub name: &'static str,
    /// Window background.
    pub bg_base: Color,
    /// Sidebar and right panel.
    pub bg_panel: Color,
    /// Cards and rows.
    pub bg_card: Color,
    /// Active/hovered row.
    pub bg_card_active: Color,
    /// Primary accent: logo, active pills, progress, play button.
    pub accent: Color,
    /// Gradient partner for the accent.
    pub accent_soft: Color,
    pub text: Color,
    pub text_dim: Color,
}

const fn rgb(hex: u32) -> Color {
    Color {
        r: ((hex >> 16) & 0xFF) as f32 / 255.0,
        g: ((hex >> 8) & 0xFF) as f32 / 255.0,
        b: (hex & 0xFF) as f32 / 255.0,
        a: 1.0,
    }
}

pub const EMBER: Palette = Palette {
    name: "ember",
    bg_base: rgb(0x1A0E0A),
    bg_panel: rgb(0x241411),
    bg_card: rgb(0x2A1A15),
    bg_card_active: rgb(0x3A221A),
    accent: rgb(0xF4744E),
    accent_soft: rgb(0xF4A261),
    text: rgb(0xF2E8E2),
    text_dim: rgb(0xB09A90),
};

pub const NEBULA: Palette = Palette {
    name: "nebula",
    bg_base: rgb(0x120A1E),
    bg_panel: rgb(0x1C1230),
    bg_card: rgb(0x251838),
    bg_card_active: rgb(0x2B2447),
    accent: rgb(0xC86BFF),
    accent_soft: rgb(0x4ED8E8),
    text: rgb(0xEDE6F5),
    text_dim: rgb(0x9C8FB5),
};

pub const VOLT: Palette = Palette {
    name: "volt",
    bg_base: rgb(0x141410),
    bg_panel: rgb(0x1E1E16),
    bg_card: rgb(0x26261C),
    bg_card_active: rgb(0x2A2A20),
    accent: rgb(0xD4F542),
    accent_soft: rgb(0xC8E830),
    text: rgb(0xEFEFE6),
    text_dim: rgb(0xA8A890),
};

pub const THEMES: [Palette; 3] = [EMBER, NEBULA, VOLT];

pub fn at(index: usize) -> &'static Palette {
    &THEMES[index % THEMES.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palettes_carry_distinct_accents_and_readable_text() {
        for p in THEMES {
            // Accent must differ clearly from the base background and the
            // text must be lighter than the panel it sits on.
            let lum = |c: Color| 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
            assert!(lum(p.accent) > lum(p.bg_base) + 0.25, "{} accent", p.name);
            assert!(lum(p.text) > lum(p.bg_panel), "{} text", p.name);
            assert!(lum(p.text_dim) < lum(p.text), "{} dim text", p.name);
        }
    }

    #[test]
    fn theme_cycling_wraps() {
        assert_eq!(at(0).name, "ember");
        assert_eq!(at(1).name, "nebula");
        assert_eq!(at(2).name, "volt");
        assert_eq!(at(3).name, "ember");
    }
}
