use super::build::{BuildError, Stage};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Derive the asset revision from every browser bootstrap artifact.
///
/// Hashing emitted output (rather than entry sources) keeps the revision
/// honest when transitive dependencies change. Provider chunks participate so
/// a cached manifest can never select code from a different client build.
pub fn revision(paths: &[PathBuf]) -> Result<String, BuildError> {
    let stage = Stage::Hash;
    let mut digest = Sha256::new();
    for path in paths {
        let bytes = fs::read(path).map_err(|error| {
            BuildError::with_source(
                stage,
                format!("failed to read browser artifact {}", path.display()),
                error,
            )
        })?;
        digest.update(bytes);
    }

    let hex = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();

    Ok(hex[..12].to_string())
}

/// Emit a maximum-quality Brotli sidecar (`<artifact>.br`) for a Plec-owned
/// browser asset.
pub fn brotli(client_path: &Path) -> Result<(), BuildError> {
    let stage = Stage::Brotli;

    let client = fs::read(client_path).map_err(|error| {
        BuildError::with_source(
            stage,
            format!("failed to read browser artifact {}", client_path.display()),
            error,
        )
    })?;

    let mut params = brotli::enc::BrotliEncoderParams::default();
    params.quality = 11;

    let mut compressed = Vec::new();
    let mut input = client.as_slice();

    brotli::BrotliCompress(&mut input, &mut compressed, &params).map_err(|error| {
        BuildError::with_source(stage, "failed to Brotli-compress browser artifact", error)
    })?;

    let sidecar_path = client_path.with_file_name(format!(
        "{}.br",
        client_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("client.js")
    ));

    fs::write(&sidecar_path, compressed).map_err(|error| {
        BuildError::with_source(
            stage,
            format!("failed to write {}", sidecar_path.display()),
            error,
        )
    })
}

/// Copy application-owned `public/` files into the build public directory.
/// Plec owns `/assets/` for generated output, so application files may not
/// claim that namespace.
pub fn copy_public(app_dir: &Path, public_dir: &Path) -> Result<(), BuildError> {
    let source = app_dir.join("public");
    if !source.exists() {
        return Ok(());
    }
    copy_public_tree(&source, &source, public_dir)
}

fn copy_public_tree(root: &Path, directory: &Path, destination: &Path) -> Result<(), BuildError> {
    let stage = Stage::PublicAssets;
    let entries = fs::read_dir(directory).map_err(|error| {
        BuildError::with_source(
            stage,
            format!("failed to read {}", directory.display()),
            error,
        )
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            BuildError::with_source(stage, "failed to read public asset entry", error)
        })?;
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .expect("entry is beneath public root");
        let reserved = [
            "assets/client.js",
            "assets/client.js.br",
            "index.html",
            "route-manifest.json",
            "route-artifact.json",
            "provider-manifest.json",
        ];
        let relative_text = relative.to_string_lossy().replace('\\', "/");
        let provider_output =
            relative_text.starts_with("assets/providers/") && relative_text.ends_with(".js");
        let runtime_output = relative_text == "runtime" || relative_text.starts_with("runtime/");
        if reserved.contains(&relative_text.as_str())
            || relative_text.starts_with("graphs/")
            || provider_output
            || runtime_output
        {
            return Err(BuildError::new(
                stage,
                format!(
                    "application public asset {} conflicts with Plec-owned output",
                    relative.display()
                ),
            ));
        }
        let target = destination.join(relative);
        let file_type = entry.file_type().map_err(|error| {
            BuildError::with_source(
                stage,
                format!("failed to inspect {}", path.display()),
                error,
            )
        })?;
        if file_type.is_dir() {
            fs::create_dir_all(&target).map_err(|error| {
                BuildError::with_source(
                    stage,
                    format!("failed to create {}", target.display()),
                    error,
                )
            })?;
            copy_public_tree(root, &path, destination)?;
        } else if file_type.is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|error| {
                    BuildError::with_source(
                        stage,
                        format!("failed to create {}", parent.display()),
                        error,
                    )
                })?;
            }
            fs::copy(&path, &target).map_err(|error| {
                BuildError::with_source(stage, format!("failed to copy {}", path.display()), error)
            })?;
        } else {
            return Err(BuildError::new(
                stage,
                format!("unsupported public asset type: {}", path.display()),
            ));
        }
    }
    Ok(())
}
