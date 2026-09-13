//! Stage the prebuilt Plec runtime assets into the application output.
//!
//! Runtime assets resolve from the workspace in Auto mode, then from the
//! installed `plec` package (`<nearest node_modules>/plec/dist/runtime`) by
//! walking up from the application directory. Package mode always selects the
//! installed release surface.
//!
//! The selected directory's runtime asset tree is a supply-chain boundary:
//! every staged file is canonicalized and must resolve inside the selected
//! runtime directory, and the runtime JS/WASM bytes are verified against the
//! SHA-256 digests recorded in the source tree's `provenance.json` before
//! anything is copied. A symlink escape, missing provenance record, or hash
//! mismatch fails the build instead of staging unverified executable bytes.

use sha2::{Digest, Sha256};
use std::fs::{self};
use std::path::{Path, PathBuf};

const RUNTIME_BINARIES: [&str; 2] = ["runtime.js", "runtime_bg.wasm"];

/// Provenance record written next to the runtime binaries by the runtime
/// build (`scripts/build-wasm.mjs`). Field names mirror its camelCase keys.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Provenance {
    js_sha256: String,
    wasm_sha256: String,
}

use super::build::RuntimeSource;

pub fn stage(
    app_dir: &Path,
    repo_root: &Path,
    out_dir: &Path,
    runtime_source: RuntimeSource,
) -> Result<(), Box<dyn std::error::Error>> {
    let source_dir = if runtime_source == RuntimeSource::Auto {
        workspace_runtime_dir(repo_root).or_else(|| packaged_runtime_dir(app_dir))
    } else {
        packaged_runtime_dir(app_dir)
    };
    let source_dir = if let Some(source_dir) = source_dir {
        source_dir
    } else {
        return Err(format!(
            "Plec runtime artifact not found — looked in:\n  \
             node_modules/plec/dist/runtime in or above {} (installed plec package)\n\
             Build the release artifact with `yarn workspace plec build` before building.",
            app_dir.display(),
        )
        .into());
    };

    // The boundary is the canonical runtime directory: all staged files must
    // resolve inside it, so symlinks that point elsewhere are rejected.
    let boundary = fs::canonicalize(&source_dir).map_err(|error| {
        format!(
            "cannot resolve runtime asset directory {}: {error}",
            source_dir.display()
        )
    })?;

    let destination_dir = out_dir.join("runtime");

    // Verify every executable binary before writing anything, so a rejected
    // source cannot leave a partially staged runtime behind.
    let verified: Vec<(&'static str, Vec<u8>)> = RUNTIME_BINARIES
        .iter()
        .map(|file| {
            contained_bytes(&source_dir.join(file), &boundary, file).map(|bytes| (*file, bytes))
        })
        .collect::<Result<_, _>>()?;

    verify_provenance(&source_dir, &boundary, &verified)?;

    fs::create_dir_all(&destination_dir)?;

    for (file, bytes) in &verified {
        fs::write(destination_dir.join(file), bytes)
            .map_err(|error| format!("cannot write staged runtime asset {}: {error}", file))?;
    }

    // Brotli sidecars are optional, but a staged sidecar must round-trip to
    // the verified binary: clients that accept compressed responses would
    // otherwise execute bytes the hash verification never saw.
    for (file, bytes) in &verified {
        let sidecar = format!("{file}.br");
        let sidecar_path = source_dir.join(&sidecar);

        if !sidecar_path.exists() {
            continue;
        }

        let compressed = contained_bytes(&sidecar_path, &boundary, &sidecar)?;
        let mut decompressed = Vec::new();

        brotli::BrotliDecompress(&mut compressed.as_slice(), &mut decompressed).map_err(
            |error| {
                format!("runtime asset {sidecar} is not a valid Brotli encoding of {file}: {error}")
            },
        )?;

        if decompressed != *bytes {
            return Err(format!(
                "runtime asset {sidecar} does not match the verified {file} bytes — \
                 refusing to stage it"
            )
            .into());
        }

        fs::write(destination_dir.join(&sidecar), compressed)
            .map_err(|error| format!("cannot write staged runtime asset {sidecar}: {error}"))?;
    }

    Ok(())
}

fn has_runtime_binaries(dir: &Path) -> bool {
    RUNTIME_BINARIES.iter().all(|file| dir.join(file).is_file())
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

fn workspace_runtime_dir(repo_root: &Path) -> Option<PathBuf> {
    let candidate = repo_root.join("packages/plec-runtime/dist/runtime");
    has_runtime_binaries(&candidate).then_some(candidate)
}

/// Read a runtime asset only after confirming that its fully resolved path
/// stays inside the selected runtime directory.
fn contained_bytes(
    path: &Path,
    boundary: &Path,
    name: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let resolved = fs::canonicalize(path)
        .map_err(|error| format!("cannot resolve runtime asset {name}: {error}"))?;

    if !resolved.starts_with(boundary) {
        return Err(format!(
            "runtime asset {name} resolves to {} outside the selected runtime directory \
             {} — refusing to stage it",
            resolved.display(),
            boundary.display()
        )
        .into());
    }

    fs::read(&resolved).map_err(|error| format!("cannot read runtime asset {name}: {error}").into())
}

fn verify_provenance(
    source_dir: &Path,
    boundary: &Path,
    verified: &[(&'static str, Vec<u8>)],
) -> Result<(), Box<dyn std::error::Error>> {
    let provenance_path = source_dir.join("provenance.json");
    let raw = contained_bytes(&provenance_path, boundary, "provenance.json").map_err(|_| {
        format!(
            "runtime provenance record not found at {} — rebuild the runtime with \
              `plec workspace compile` (dev frontend) or `yarn workspace plec build:wasm`",
            provenance_path.display()
        )
    })?;

    let provenance: Provenance = serde_json::from_slice(&raw).map_err(|error| {
        format!(
            "invalid runtime provenance record {}: {error}",
            provenance_path.display()
        )
    })?;

    let expected = [
        ("runtime_bg.wasm", provenance.wasm_sha256.as_str()),
        ("runtime.js", provenance.js_sha256.as_str()),
    ];

    for (file, recorded) in expected {
        let actual = verified
            .iter()
            .find(|(name, _)| *name == file)
            .map(|(_, bytes)| sha256_hex(bytes))
            .expect("verified binaries cover both provenance entries");

        if !recorded.eq_ignore_ascii_case(&actual) {
            return Err(format!(
                "runtime provenance hash mismatch for {file}: provenance records \
                 {recorded} but the staged source hashes to {actual} — refusing to \
                 stage unverified runtime bytes"
            )
            .into());
        }
    }

    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write a runtime asset tree at the installed package layout whose
    /// binaries carry real provenance digests.
    fn write_runtime_source(app: &Path, js: &str, wasm: &[u8]) {
        let runtime = app
            .join("node_modules")
            .join("plec")
            .join("dist")
            .join("runtime");
        fs::create_dir_all(&runtime).expect("runtime dir");
        fs::write(runtime.join("runtime.js"), js).expect("runtime.js");
        fs::write(runtime.join("runtime_bg.wasm"), wasm).expect("runtime_bg.wasm");

        let provenance = format!(
            r#"{{"jsSha256":"{}","wasmSha256":"{}"}}"#,
            sha256_hex(js.as_bytes()),
            sha256_hex(wasm),
        );
        fs::write(runtime.join("provenance.json"), provenance).expect("provenance.json");
    }

    #[test]
    fn stages_verified_runtime_assets_and_sidecars() {
        let app = tempfile::tempdir().expect("app dir");
        write_runtime_source(app.path(), "runtime glue", b"\0asm\x01\x00\x00\x00");
        // A valid sidecar: Brotli bytes of the verified runtime.js.
        let mut compressed = Vec::new();
        brotli::BrotliCompress(
            &mut "runtime glue".as_bytes(),
            &mut compressed,
            &Default::default(),
        )
        .expect("compress");
        fs::write(
            app.path()
                .join("node_modules/plec/dist/runtime/runtime.js.br"),
            compressed,
        )
        .expect("sidecar");

        let out = tempfile::tempdir().expect("out dir");

        stage(app.path(), app.path(), out.path(), RuntimeSource::Package)
            .expect("staging succeeds");

        let staged = out.path().join("runtime");
        assert_eq!(
            fs::read_to_string(staged.join("runtime.js")).expect("staged js"),
            "runtime glue"
        );
        assert_eq!(
            fs::read(staged.join("runtime_bg.wasm")).expect("staged wasm"),
            b"\0asm\x01\x00\x00\x00"
        );
        assert!(staged.join("runtime.js.br").is_file());
        assert!(!staged.join("runtime_bg.wasm.br").is_file());
    }

    #[test]
    fn auto_prefers_workspace_runtime_over_installed_package() {
        let root = tempfile::tempdir().expect("repo dir");
        let app = root.path().join("apps/fullstack");
        fs::create_dir_all(&app).expect("app dir");
        write_runtime_source(&app, "package runtime", b"package wasm");
        let workspace = root.path().join("packages/plec-runtime/dist/runtime");
        fs::create_dir_all(&workspace).expect("workspace runtime dir");
        fs::write(workspace.join("runtime.js"), "workspace runtime").expect("workspace js");
        fs::write(workspace.join("runtime_bg.wasm"), b"workspace wasm").expect("workspace wasm");
        let provenance = format!(
            r#"{{"jsSha256":"{}","wasmSha256":"{}"}}"#,
            sha256_hex(b"workspace runtime"),
            sha256_hex(b"workspace wasm"),
        );
        fs::write(workspace.join("provenance.json"), provenance).expect("provenance");

        let out = tempfile::tempdir().expect("out dir");
        stage(&app, root.path(), out.path(), RuntimeSource::Auto).expect("staging succeeds");
        assert_eq!(
            fs::read_to_string(out.path().join("runtime/runtime.js")).expect("staged js"),
            "workspace runtime"
        );
    }

    #[test]
    #[cfg(unix)]
    fn rejects_symlinks_that_escape_the_runtime_directory() {
        let app = tempfile::tempdir().expect("app dir");
        write_runtime_source(app.path(), "runtime glue", b"\0asm\x01\x00\x00\x00");

        let outside = tempfile::tempdir().expect("outside dir");
        fs::write(outside.path().join("escape.wasm"), b"hostile").expect("outside wasm");

        // Replace the real binary with a symlink that leaves the runtime
        // directory.
        fs::remove_file(
            app.path()
                .join("node_modules/plec/dist/runtime/runtime_bg.wasm"),
        )
        .expect("remove real binary");

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(
                outside.path().join("escape.wasm"),
                app.path()
                    .join("node_modules/plec/dist/runtime/runtime_bg.wasm"),
            )
            .expect("symlink");
        }

        let out = tempfile::tempdir().expect("out dir");

        let error = stage(app.path(), app.path(), out.path(), RuntimeSource::Package)
            .expect_err("must fail");
        let message = error.to_string();
        assert!(
            message.contains("outside the selected runtime directory"),
            "failure must name the containment boundary: {message}"
        );
        assert!(
            !out.path().join("runtime/runtime.js").exists(),
            "nothing may be staged when containment fails"
        );
    }

    #[test]
    fn rejects_a_missing_provenance_record() {
        let app = tempfile::tempdir().expect("app dir");
        let runtime = app
            .path()
            .join("node_modules")
            .join("plec")
            .join("dist")
            .join("runtime");
        fs::create_dir_all(&runtime).expect("runtime dir");
        fs::write(runtime.join("runtime.js"), "runtime glue").expect("runtime.js");
        fs::write(runtime.join("runtime_bg.wasm"), b"\0asm\x01\x00\x00\x00")
            .expect("runtime_bg.wasm");
        // No provenance.json.

        let out = tempfile::tempdir().expect("out dir");

        let error = stage(app.path(), app.path(), out.path(), RuntimeSource::Package)
            .expect_err("must fail");
        let message = error.to_string();
        assert!(
            message.contains("provenance record not found"),
            "failure must name the missing provenance record: {message}"
        );
    }

    #[test]
    fn rejects_a_provenance_hash_mismatch() {
        let app = tempfile::tempdir().expect("app dir");
        write_runtime_source(app.path(), "runtime glue", b"\0asm\x01\x00\x00\x00");

        // Tamper with a verified binary after writing provenance.
        fs::write(
            app.path()
                .join("node_modules/plec/dist/runtime/runtime_bg.wasm"),
            b"\0asm\x01\x00\x00\x00tampered",
        )
        .expect("tampered wasm");

        let out = tempfile::tempdir().expect("out dir");

        let error = stage(app.path(), app.path(), out.path(), RuntimeSource::Package)
            .expect_err("must fail");
        let message = error.to_string();
        assert!(
            message.contains("provenance hash mismatch for runtime_bg.wasm"),
            "failure must name the mismatched binary: {message}"
        );
    }

    #[test]
    fn rejects_a_sidecar_that_does_not_round_trip_the_verified_binary() {
        let app = tempfile::tempdir().expect("app dir");
        write_runtime_source(app.path(), "runtime glue", b"\0asm\x01\x00\x00\x00");

        // Valid Brotli of different bytes than runtime.js carries.
        let mut compressed = Vec::new();
        brotli::BrotliCompress(
            &mut "different bytes".as_bytes(),
            &mut compressed,
            &Default::default(),
        )
        .expect("compress");
        fs::write(
            app.path()
                .join("node_modules/plec/dist/runtime/runtime.js.br"),
            compressed,
        )
        .expect("sidecar");

        let out = tempfile::tempdir().expect("out dir");

        let error = stage(app.path(), app.path(), out.path(), RuntimeSource::Package)
            .expect_err("must fail");
        let message = error.to_string();
        assert!(
            message.contains("does not match the verified runtime.js bytes"),
            "failure must name the sidecar mismatch: {message}"
        );
        assert!(
            !out.path().join("runtime/runtime.js.br").exists(),
            "a rejected sidecar must not be staged"
        );
    }
}
