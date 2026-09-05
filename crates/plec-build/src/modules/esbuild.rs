use std::path::{Path, PathBuf};
use std::process::Command;

/// Locate the esbuild JavaScript launcher by walking up from the application
/// directory to the nearest `node_modules` install. The launcher is run under
/// `node`, which keeps the invocation platform-independent.
pub fn resolve(from: &Path) -> Result<PathBuf, String> {
    for ancestor in from.ancestors() {
        let candidate = ancestor
            .join("node_modules")
            .join("esbuild")
            .join("bin")
            .join("esbuild");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    Err("esbuild not found: install workspace dependencies (expected node_modules/esbuild)".into())
}

/// Run esbuild with the given arguments, surfacing its diagnostics on failure.
pub fn run(args: &[String]) -> Result<(), String> {
    let output = Command::new("node")
        .args(args)
        .output()
        .map_err(|error| format!("failed to invoke esbuild via node: {error}"))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    Err(format!(
        "esbuild exited with {}: {}{}",
        output.status,
        stdout.trim(),
        stderr.trim()
    ))
}
