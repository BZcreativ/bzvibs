//! Bundled fonts and the resolved typography from user prefs.

/// Inter variable font (OFL license, see assets/fonts/LICENSE.txt),
/// embedded at compile time and registered with iced at startup.
pub const INTER_TTF: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");

/// Family name iced resolves for the embedded Inter font. Must match the
/// family declared inside the TTF.
pub const INTER_FAMILY: &str = "Inter";

/// Resolve a stored font choice to the iced font used for the whole app.
pub fn resolved_font(choice: rt_config::ui_prefs::FontChoice) -> iced::Font {
    match choice {
        rt_config::ui_prefs::FontChoice::System => iced::Font::default(),
        rt_config::ui_prefs::FontChoice::Inter => iced::Font::with_name(INTER_FAMILY),
    }
}
