fn main() -> iced::Result {
    iced::application(boot, update, view).title("pc-iced smoke").run()
}

fn boot() {}

fn update(_state: &mut (), _message: ()) {}

fn view(_state: &()) -> iced::Element<'static, ()> {
    iced::widget::text("pc-iced smoke").into()
}
