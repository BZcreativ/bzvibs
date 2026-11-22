//! DPI ground-truth probe: prints what the window system reports to a
//! process launched from this same shell as the app. Run:
//! `cargo run -p bzvibs --example dpi_probe --release`

fn main() {
    #[cfg(windows)]
    unsafe {
        #[link(name = "user32")]
        unsafe extern "system" {
            fn SetProcessDPIAware() -> i32;
            fn GetDpiForSystem() -> u32;
        }
        let set = SetProcessDPIAware();
        println!("SetProcessDPIAware returned: {set}");
        println!("GetDpiForSystem (post-set): {}", GetDpiForSystem());
    }

    let event_loop = winit::event_loop::EventLoop::new().unwrap();
    let attrs = winit::window::Window::default_attributes()
        .with_title("dpi probe")
        .with_inner_size(winit::dpi::LogicalSize::new(1280.0f64, 800.0f64));
    #[allow(deprecated)]
    let window = event_loop.create_window(attrs).unwrap();

    println!("window.scale_factor(): {}", window.scale_factor());
    println!("inner_size (physical): {:?}", window.inner_size());
    println!("outer_size (physical): {:?}", window.outer_size());
    if let Some(m) = window.current_monitor() {
        println!(
            "monitor: {} scale {}",
            m.name().unwrap_or_default(),
            m.scale_factor()
        );
        println!("monitor size (physical): {:?}", m.size());
    }
    std::process::exit(0);
}
