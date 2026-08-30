mod app;
mod com;
mod dev;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    // The CLI variant is selected at compile time: build.rs bakes
    // PLEC_CLI_VERSION in from `.env.plec` (or the build environment).
    // `release` selects the app CLI; anything else keeps the dev CLI.
    match option_env!("PLEC_CLI_VERSION") {
        Some("release") => app::cli::run(),
        _ => dev::cli::run(),
    }
}
