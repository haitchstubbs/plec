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

/// Brotli-compress every Vite-emitted JavaScript and CSS asset below its
/// output assets directory. Other Vite assets (images and fonts) are served
/// uncompressed; their presence does not imply a `.br` sibling.
pub fn brotli_vite_assets(assets_dir: &Path) -> Result<(), BuildError> {
    fn collect(directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), BuildError> {
        for entry in fs::read_dir(directory).map_err(|error| {
            BuildError::with_source(
                Stage::Brotli,
                format!(
                    "failed to read Vite assets directory {}",
                    directory.display()
                ),
                error,
            )
        })? {
            let entry = entry.map_err(|error| {
                BuildError::with_source(Stage::Brotli, "failed to read Vite asset entry", error)
            })?;
            let path = entry.path();
            let file_type = entry.file_type().map_err(|error| {
                BuildError::with_source(
                    Stage::Brotli,
                    format!("failed to inspect Vite asset {}", path.display()),
                    error,
                )
            })?;
            if file_type.is_dir() {
                collect(&path, files)?;
            } else if file_type.is_file()
                && matches!(
                    path.extension().and_then(|extension| extension.to_str()),
                    Some("js" | "css")
                )
            {
                files.push(path);
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    collect(assets_dir, &mut files)?;
    files.sort();
    for file in files {
        brotli(&file)?;
    }
    Ok(())
}

/// Copy application-owned `public/` files into the build public directory.
/// Application files may not claim paths reserved for framework output.
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
        if is_framework_owned_public_path(relative) {
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

fn is_framework_owned_public_path(path: &Path) -> bool {
    let path = path.to_string_lossy().replace('\\', "/");
    let reserved_files = ["index.html"];
    reserved_files.contains(&path.as_str())
        || path == "assets/compiled"
        || path.starts_with("assets/compiled/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn brotli_compresses_vite_javascript_and_css_but_not_other_assets() {
        let dir = tempfile::tempdir().expect("temp dir");
        let assets = dir.path().join("assets");
        let chunks = assets.join("chunks");
        fs::create_dir_all(&chunks).expect("asset directory");
        let javascript = chunks.join("shared.js");
        let stylesheet = assets.join("client.css");
        let image = assets.join("logo.png");
        let font = assets.join("font.woff2");
        fs::write(&javascript, b"export const shared = true;").expect("JS asset");
        fs::write(&stylesheet, b"body { color: red; }").expect("CSS asset");
        fs::write(&image, b"png").expect("image asset");
        fs::write(&font, b"font").expect("font asset");

        brotli_vite_assets(&assets).expect("compress Vite text assets");

        for original in [&javascript, &stylesheet] {
            let sidecar = original.with_file_name(format!(
                "{}.br",
                original.file_name().unwrap().to_string_lossy()
            ));
            let compressed = fs::read(sidecar).expect("Brotli sidecar");
            let mut decoded = Vec::new();
            brotli::Decompressor::new(compressed.as_slice(), 4096)
                .read_to_end(&mut decoded)
                .expect("decompress sidecar");
            assert_eq!(decoded, fs::read(original).expect("original asset"));
        }
        assert!(!image.with_file_name("logo.png.br").exists());
        assert!(!font.with_file_name("font.woff2.br").exists());
    }
}
