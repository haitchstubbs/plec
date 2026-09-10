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
