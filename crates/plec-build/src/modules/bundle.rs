use std::path::Path;

use super::build::{BuildError, Stage};
use super::esbuild;

/// Bundle the browser client with Plec-owned configuration.
///
/// Applications never configure platform, target, JSX handling or output
/// conventions; those are Plec build semantics.
pub fn bundle(
    entry: &Path,
    app_dir: &Path,
    outfile: &Path,
    metafile: &Path,
    optimize: bool,
) -> Result<(), BuildError> {
    let stage = Stage::BrowserBundle;
    let esbuild_bin = esbuild::resolve(app_dir).map_err(|error| BuildError::new(stage, error))?;

    let mut args = vec![
        esbuild_bin.to_string_lossy().into_owned(),
        "--bundle".into(),
        "--format=esm".into(),
        "--tree-shaking=true".into(),
        "--legal-comments=none".into(),
        "--platform=browser".into(),
        "--target=es2022".into(),
        "--jsx=automatic".into(),
        "--jsx-import-source=plec".into(),
        format!("--outfile={}", outfile.display()),
        format!("--metafile={}", metafile.display()),
    ];

    if optimize {
        args.push("--minify".into());
    }

    args.push(entry.to_string_lossy().into_owned());

    esbuild::run(&args).map_err(|error| BuildError::new(stage, error))
}
