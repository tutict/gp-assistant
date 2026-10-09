fn main() {
    if let Err(error) = stock_optimizer_desktop_lib::eval_replay::cli(std::env::args()) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
