//! End-to-end tests for the shared application build pipeline
//! (`plec_build::modules::build`), exercised through the `plec` CLI build
//! subcommand that delegates to it.

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
        "server/app.mjs",
        "plec-server.json",
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
    assert!(
        !out_dir.join("server.mjs").exists(),
        "legacy root server bundle must not be emitted"
    );

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

// ---------------------------------------------------------------------------
// Out-of-repo builds: the `plec` package arrives as an installed dependency
// and the runtime assets stage from `node_modules/plec/dist/runtime`.
// ---------------------------------------------------------------------------

/// The workspace root, for reaching the real node_modules install.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("plec-cli sits three levels below the workspace root")
        .to_path_buf()
}

/// A project directory outside the workspace. Must not live under
/// `CARGO_TARGET_TMPDIR`: the target tree is inside the workspace, and the
/// repo-root heuristic would then resolve the monorepo runtime instead of
/// exercising the installed-package path.
fn out_of_repo_project(tag: &str) -> PathBuf {
    let project = std::env::temp_dir()
        .join("plec-build-out-of-repo")
        .join(tag);
    let _ = fs::remove_dir_all(&project);
    fs::create_dir_all(project.join("node_modules")).expect("project dir should be creatable");
    project
}

fn copy_dir_all(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).expect("destination dir should be creatable");
    for entry in fs::read_dir(src).expect("source dir should be readable") {
        let entry = entry.expect("source entry should be readable");
        let file_type = entry.file_type().expect("entry type should be readable");
        let target = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_all(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("file copy should succeed");
        }
    }
}

/// Copy the real esbuild install so the browser/server bundles can run from
/// the isolated project directory.
fn install_esbuild(project: &Path) {
    let node_modules = workspace_root().join("node_modules");
    copy_dir_all(
        &node_modules.join("esbuild"),
        &project.join("node_modules/esbuild"),
    );
    let platform_packages = node_modules.join("@esbuild");
    if platform_packages.is_dir() {
        copy_dir_all(&platform_packages, &project.join("node_modules/@esbuild"));
    }
}

/// Install a minimal stand-in for the release `plec` package: the exports the
/// fixture apps import plus the staged runtime assets, with `runtime.js`
/// carrying a marker so the test can prove which source staged them.
fn install_plec_package(project: &Path, runtime_marker: &str) {
    let plec = project.join("node_modules/plec");
    fs::create_dir_all(plec.join("dist/runtime")).expect("plec package dirs should be creatable");
    fs::write(
        plec.join("package.json"),
        r#"{"name":"plec","type":"module","exports":{".":"./dist/browser.js"}}"#,
    )
    .expect("plec package.json should be writable");
    fs::write(
        plec.join("dist/browser.js"),
        "export function createRouter() { return {}; }\n\
         export function createRootRoute() { return {}; }\n",
    )
    .expect("plec browser stub should be writable");
    fs::write(plec.join("dist/runtime/runtime.js"), runtime_marker)
        .expect("runtime.js stub should be writable");
    // Minimal valid WASM binary header; the Rust tests never execute it.
    fs::write(
        plec.join("dist/runtime/runtime_bg.wasm"),
        b"\0asm\x01\x00\x00\x00",
    )
    .expect("runtime_bg.wasm stub should be writable");
}

#[test]
fn builds_out_of_repo_app_from_installed_plec_package() {
    let project = out_of_repo_project("installed-package");
    let app = project.join("mini-app");
    copy_dir_all(&fixture_app("mini-app"), &app);
    install_esbuild(&project);
    install_plec_package(&project, "installed-package-runtime");

    let out_dir = project.join("dist");
    let output = run_build(&app, &out_dir, &[]);
    assert_success(&output);

    // The staged runtime must be the installed package's, not some
    // workspace-ancestor copy: no workspace exists above this directory.
    let staged = read(out_dir.join("public/runtime/runtime.js"));
    assert_eq!(
        staged, "installed-package-runtime",
        "runtime assets must stage from node_modules/plec/dist/runtime"
    );

    // Same minimum artifact structure as the in-workspace build.
    for artifact in [
        "server/app.mjs",
        "plec-server.json",
        "client.meta.json",
        "public/index.html",
        "public/route-manifest.json",
        "public/runtime/runtime_bg.wasm",
    ] {
        assert!(
            out_dir.join(artifact).is_file(),
            "out-of-repo build should emit {artifact}"
        );
    }
    assert!(
        !out_dir.join("server.mjs").exists(),
        "out-of-repo builds must not emit the legacy root server bundle"
    );
}

#[test]
fn staging_failure_names_both_resolution_paths() {
    let project = out_of_repo_project("missing-runtime");
    let app = project.join("mini-app");
    copy_dir_all(&fixture_app("mini-app"), &app);
    // No node_modules/plec installed: runtime staging has nowhere to resolve.

    // Runtime staging runs before any bundling, so esbuild is not needed.
    let out_dir = project.join("dist");
    let output = run_build(&app, &out_dir, &[]);

    assert!(
        !output.status.success(),
        "build must fail without any runtime artifact source"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("packages/plec-runtime/dist/runtime"),
        "failure must name the workspace runtime path: {stderr}"
    );
    assert!(
        stderr.contains("node_modules/plec/dist/runtime"),
        "failure must name the installed-package runtime path: {stderr}"
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
