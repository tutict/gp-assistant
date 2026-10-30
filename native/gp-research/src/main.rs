fn main() {
    std::process::exit(gp_research::cli::run(std::env::args().skip(1).collect()));
}
