use std::{fs, path::Path};

use super::build::{BuildError, Stage};

/// Deterministically start the build from a clean Plec output directory.
///
/// Refuses filesystem roots and empty paths so removal can never escape the
/// configured build directory.
pub fn prepare(out_dir: &Path, assets_dir: &Path) -> Result<(), BuildError> {
    if out_dir.as_os_str().is_empty() {
        return Err(BuildError::new(
            Stage::Clean,
            "output directory must not be empty",
        ));
    }

    if out_dir.parent().is_none() {
        return Err(BuildError::new(
            Stage::Clean,
            format!(
                "refusing to use filesystem root {} as output directory",
                out_dir.display()
            ),
        ));
    }

    if out_dir.exists() {
        fs::remove_dir_all(out_dir).map_err(|error| {
            BuildError::with_source(
                Stage::Clean,
                format!("failed to remove {}", out_dir.display()),
                error,
            )
        })?;
    }

    fs::create_dir_all(assets_dir).map_err(|error| {
        BuildError::with_source(
            Stage::Clean,
            format!("failed to create {}", assets_dir.display()),
            error,
        )
    })
}
