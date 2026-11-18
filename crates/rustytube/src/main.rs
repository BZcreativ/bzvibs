mod probe;

fn main() {
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
