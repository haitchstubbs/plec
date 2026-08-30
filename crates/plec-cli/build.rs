use std::env;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let env_file = manifest_dir
        .ancestors()
        .nth(2)
        .expect("workspace root above crates/plec-cli")
        .join(".env.plec");

    // The CLI variant is baked in at compile time, so the build must rerun
    // (and re-bake) whenever `.env.plec` changes.
    println!("cargo:rerun-if-changed={}", env_file.display());
    println!("cargo:rerun-if-env-changed=PLEC_CLI_VERSION");

    // An explicitly set build-environment variable wins over `.env.plec`.
    if env::var_os("PLEC_CLI_VERSION").is_some() {
        return;
    }

    let Ok(iter) = dotenvy::from_path_iter(&env_file) else {
        // Missing `.env.plec` is fine: the default is the dev CLI.
        return;
    };

    for (key, value) in iter.flatten() {
        if key == "PLEC_CLI_VERSION" {
            println!("cargo:rustc-env=PLEC_CLI_VERSION={}", value);
            return;
        }
    }
}
