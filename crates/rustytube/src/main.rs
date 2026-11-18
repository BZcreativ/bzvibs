mod probe;

fn main() {
    // A console-launched, manifest-less process is told the display is 96
    // DPI on this 168-DPI multi-monitor machine, so the window surface and
    // the layout disagree by the real scale factor and the UI clips. Claim
    // DPI awareness before the window system initializes.
    #[cfg(windows)]
    unsafe {
        #[link(name = "user32")]
        unsafe extern "system" {
            fn SetProcessDPIAware() -> i32;
        }
        SetProcessDPIAware();
    }

    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("probe") => probe::run(&args[1..]),
        _ => {
            if let Err(e) = rt_ui::run() {
                eprintln!("ui error: {e}");
                std::process::exit(1);
            }
        }
    }
}
