//! The application shell: navigation, panels, playback bar. The full layout
//! lands in PR-6; this module owns the window entry point from day one.

/// Re-exported so the binary crate needs no direct iced dependency.
pub use iced::Result;

/// Open the RustyTube window. Blocks until the window closes.
pub fn run() -> Result {
    iced::application(boot, update, view)
        .title("RustyTube")
        .run()
}

fn boot() {}

fn update(_state: &mut (), _message: ()) {}

fn view<'a>(_state: &'a ()) -> iced::Element<'a, ()> {
    iced::widget::text("RustyTube").into()
}
