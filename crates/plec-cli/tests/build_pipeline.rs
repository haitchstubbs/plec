//! End-to-end tests for the shared `com::build` application pipeline,
//! exercised through the `plec` CLI build subcommand.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::{fs, process::Output};

fn fixture_repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("mini-repo")
}

fn fixture_app(name: &str) -> PathBuf {
    fixture_repo().join("apps").join(name)
}

fn output_dir(tag: &str) -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("build-pipeline")
        .join(tag)
}

fn run_build(app: &Path, out_dir: &Path, extra_args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_plec"))
        .arg("build")
        .arg(app.join("src/router.tsx"))
        .arg("--out-dir")
        .arg(out_dir)
        .args(extra_args)
        .output()
        .expect("plec binary should be invocable")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "build should succeed\n--- stdout ---\n{}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn read(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path).expect("artifact should exist")
}

#[test]
fn builds_expected_output_structure() {
    let out_dir = output_dir("structure");

    let output = run_build(&fixture_app("mini-app"), &out_dir, &["--title", "Mini App"]);
    assert_success(&output);

    // Minimum required artifact structure.
    for artifact in [
        "server.mjs",
        "client.meta.json",
        "public/index.html",
        "public/assets/client.js",
        "public/assets/client.js.br",
        "public/route-manifest.json",
        "public/route-artifact.json",
        "public/runtime/runtime.js",
        "public/runtime/runtime_bg.wasm",
    ] {
        assert!(
            out_dir.join(artifact).is_file(),
            "missing artifact {}",
            artifact
        );
    }

    // Plec compiler route graphs.
    let graphs_dir = out_dir.join("public/graphs");
    let graphs = fs::read_dir(&graphs_dir)
        .expect("graphs directory should exist")
        .collect::<Result<Vec<_>, _>>()
        .expect("graphs directory should be readable");
    assert!(
        graphs.iter().any(|entry| entry
            .path()
            .extension()
            .is_some_and(|extension| extension == "json")),
        "route graphs should be emitted"
    );

    // Brotli sidecar must decompress back to the client artifact.
    let client = fs::read(out_dir.join("public/assets/client.js")).expect("client.js");
    let compressed = fs::read(out_dir.join("public/assets/client.js.br")).expect("client.js.br");

    let mut decompressed = Vec::new();
    let mut reader = brotli::Decompressor::new(compressed.as_slice(), 4096);
    std::io::Read::read_to_end(&mut reader, &mut decompressed)
        .expect("brotli sidecar should decompress");
    assert_eq!(
        decompressed, client,
        "brotli sidecar should round-trip the client artifact"
    );

    // Document shell: title option plus revisioned client script.
    let index = read(out_dir.join("public/index.html"));
    assert!(index.contains("<title>Mini App</title>"));
    assert!(index.contains(r#"<div id="app" aria-live="polite"></div>"#));

    let revision = client_revision(&client);
    assert!(index.contains(&format!("/assets/client.js?v={revision}")));
    assert!(index.contains(&format!("/assets/styles.css?v={revision}")));

    // The revision is also reported on stdout.
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(&format!("revision {revision}")));
}

#[test]
fn revision_derives_from_emitted_client_artifact() {
    let first_dir = output_dir("revision-first");
    let second_dir = output_dir("revision-second");

    let output = run_build(&fixture_app("mini-app"), &first_dir, &[]);
    assert_success(&output);
    let first = emitted_revision(&first_dir);

    // Same inputs, deterministic revision.
    let output = run_build(
        &fixture_app("mini-app"),
        &output_dir("revision-repeat"),
        &[],
    );
    assert_success(&output);
    let repeated = emitted_revision(&output_dir("revision-repeat"));
    assert_eq!(first, repeated, "revision should be deterministic");

    // A different client entry emits different bytes and a different revision.
    let output = run_build(
        &fixture_app("mini-app"),
        &second_dir,
        &["--client-entry", "src/client-alt.tsx"],
    );
    assert_success(&output);
    let second = emitted_revision(&second_dir);

    assert_ne!(
        first, second,
        "changing the bundled client must change the revision"
    );

    let index = read(second_dir.join("public/index.html"));
    assert!(index.contains(&format!("/assets/client.js?v={second}")));
    assert!(!index.contains(&format!("/assets/client.js?v={first}")));
}

#[test]
fn stale_artifacts_are_removed_between_builds() {
    let out_dir = output_dir("clean");

    let output = run_build(&fixture_app("mini-app"), &out_dir, &[]);
    assert_success(&output);

    let stale = out_dir.join("public/stale-artifact.txt");
    fs::write(&stale, "stale").expect("stale artifact should be writable");
    assert!(stale.is_file());

    let output = run_build(&fixture_app("mini-app"), &out_dir, &[]);
    assert_success(&output);

    assert!(
        !stale.exists(),
        "stale Plec artifacts must not survive a rebuild"
    );
    assert!(
        out_dir.join("public/index.html").is_file(),
        "fresh build must be intact"
    );
}

#[test]
fn forbidden_browser_dependency_fails_the_build() {
    let out_dir = output_dir("zod");

    let output = run_build(&fixture_app("zod-app"), &out_dir, &[]);

    assert!(
        !output.status.success(),
        "build must fail when a forbidden dependency reaches the browser bundle"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("forbidden dependency leaked into browser bundle"),
        "failure must name the dependency-boundary stage: {stderr}"
    );
    assert!(
        stderr.contains("zod"),
        "failure must identify the offending package: {stderr}"
    );
    assert!(
        stderr.contains("node_modules/zod"),
        "failure must include import path information: {stderr}"
    );
}

fn emitted_revision(out_dir: &Path) -> String {
    let index = read(out_dir.join("public/index.html"));
    let marker = "/assets/client.js?v=";

    let start = index
        .find(marker)
        .expect("index.html should reference the client revision")
        + marker.len();

    index[start..start + 12].to_string()
}

fn client_revision(client: &[u8]) -> String {
    use sha2::{Digest, Sha256};

    let digest = Sha256::digest(client);
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    hex[..12].to_string()
}
