use std::path::Path;

use super::build::{BuildError, Stage};
use super::esbuild;

/// Bundle the server entry with Plec-owned configuration.
///
/// Dependencies stay external so the server runs against the application's
/// installed packages at runtime.
pub fn bundle(
    entry: &Path,
    app_dir: &Path,
    outfile: &Path,
    optimize: bool,
) -> Result<(), BuildError> {
    let stage = Stage::ServerBundle;
    let esbuild_bin = esbuild::resolve(app_dir).map_err(|error| BuildError::new(stage, error))?;

    let mut args = vec![
        esbuild_bin.to_string_lossy().into_owned(),
        "--bundle".into(),
        "--format=esm".into(),
        "--tree-shaking=true".into(),
        "--legal-comments=none".into(),
        "--platform=node".into(),
        "--target=node20".into(),
        "--packages=external".into(),
        format!("--outfile={}", outfile.display()),
    ];

    if optimize {
        args.push("--minify".into());
    }

    args.push(entry.to_string_lossy().into_owned());

    esbuild::run(&args).map_err(|error| BuildError::new(stage, error))
}
