slint::slint! {
    export component App inherits Window {
        Text { text: "pd-slint smoke"; }
    }
}

fn main() {
    App::new().unwrap().run().unwrap();
}
