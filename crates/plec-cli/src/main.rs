fn main() {
    if let Err(error) = plec_cli::run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
