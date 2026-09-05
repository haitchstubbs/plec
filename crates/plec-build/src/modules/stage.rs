//! Stage the prebuilt Plec runtime assets into the application output.
//!
//! Resolution order:
//! 1. the workspace runtime build
//!    (`<repo_root>/packages/plec-runtime/dist/runtime`) — the canonical,
//!    freshness-audited in-workspace source;
//! 2. the installed `plec` package's staged assets
//!    (`<nearest node_modules>/plec/dist/runtime`), resolved npm-style by
//!    walking up from the application directory, for applications built
//!    outside the monorepo where the runtime arrives as a release-artifact
//!    dependency.

use std::fs::{copy, create_dir_all};
use std::path::{Path, PathBuf};

pub fn stage(
    app_dir: &Path,
    repo_root: &Path,
    out_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let workspace_dir = repo_root
        .join("packages")
        .join("plec-runtime")
        .join("dist")
        .join("runtime");

    let source_dir = if has_runtime_binaries(&workspace_dir) {
        workspace_dir
    } else if let Some(packaged) = packaged_runtime_dir(app_dir) {
        packaged
    } else {
        return Err(format!(
            "Plec runtime artifact not found — looked in:\n  1. {} (workspace runtime \
             build; compile it with `yarn workspace plec-runtime build`)\n  2. \
             node_modules/plec/dist/runtime in or above {} (installed plec package)\n\
             Provide one of these before building.",
            workspace_dir.display(),
            app_dir.display(),
        )
        .into());
    };

    let destination_dir = out_dir.join("runtime");

    create_dir_all(&destination_dir)?;

    for file in ["runtime.js", "runtime_bg.wasm"] {
        let source = source_dir.join(file);

        if !source.exists() {
            return Err(format!(
                "Plec runtime artifact not found: {} — compile it with `plec workspace compile` \
                 (dev frontend) or `yarn workspace plec-runtime build`",
                source.display()
            )
            .into());
        }

        copy(&source, destination_dir.join(file))?;
    }

    for file in ["runtime.js.br", "runtime_bg.wasm.br"] {
        let source = source_dir.join(file);

        if source.exists() {
            copy(&source, destination_dir.join(file))?;
        }
    }

    Ok(())
}

fn has_runtime_binaries(dir: &Path) -> bool {
    dir.join("runtime.js").is_file() && dir.join("runtime_bg.wasm").is_file()
}

/// Walks up from the application directory to the nearest installed `plec`
/// package that carries staged runtime assets.
fn packaged_runtime_dir(app_dir: &Path) -> Option<PathBuf> {
    app_dir
        .ancestors()
        .map(|ancestor| {
            ancestor
                .join("node_modules")
                .join("plec")
                .join("dist")
                .join("runtime")
        })
        .find(|candidate| has_runtime_binaries(candidate))
}
