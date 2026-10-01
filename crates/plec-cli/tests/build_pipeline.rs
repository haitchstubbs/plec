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

/// Copy an app into a disposable project rooted under Cargo's test output.
/// Its nearest `node_modules/@plec/core` is synthesized by the test, while other
/// build dependencies continue to resolve from the workspace ancestor.
fn fixture_project(tag: &str, name: &str) -> PathBuf {
    let project = output_dir(&format!("source-{tag}"));
    let _ = fs::remove_dir_all(&project);
    let app = project.join("apps").join(name);
    copy_dir_all(&fixture_app(name), &app);
    install_plec_package(&project, "fixture-package-runtime");
    app
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

fn files_below(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).expect("directory should be readable") {
        let path = entry.expect("entry should be readable").path();
        if path.is_dir() {
            files_below(&path, files);
        } else {
            files.push(path);
        }
    }
}

#[test]
fn server_action_implementation_is_absent_from_every_public_output() {
    const SECRET: &str = "PLEC_SERVER_ACTION_SECRET_7D3F";
    let app = fixture_project("server-action-secret", "server-action-app");
    let out_dir = output_dir("server-action-secret");
    let output = run_build(&app, &out_dir, &[]);
    assert_success(&output);

    let server_bundle = read(out_dir.join("server/app.mjs"));
    assert!(server_bundle.contains(SECRET));
    assert!(server_bundle.contains("invokeAction"));
    let route_artifact: serde_json::Value =
        serde_json::from_slice(&fs::read(out_dir.join("public/route-artifact.json")).unwrap())
            .expect("route artifact JSON");
    fn action_ids(value: &serde_json::Value, output: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(object) => {
                if let Some(actions) = object
                    .get("serverActions")
                    .and_then(|value| value.as_array())
                {
                    output.extend(actions.iter().filter_map(|action| {
                        action
                            .get("id")
                            .and_then(|id| id.as_str())
                            .map(str::to_owned)
                    }));
                }
                for value in object.values() {
                    action_ids(value, output);
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    action_ids(value, output);
                }
            }
            _ => {}
        }
    }
    let mut ids = Vec::new();
    action_ids(&route_artifact, &mut ids);
    assert!(
        !ids.is_empty(),
        "compiled public IR should contain the action reference"
    );
    assert!(
        ids.iter().all(|id| server_bundle.contains(id)),
        "generated registry IDs must match public IR: {ids:?}"
    );

    let public_dir = out_dir.join("public");
    let mut public_files = Vec::new();
    files_below(&public_dir, &mut public_files);
    assert!(!public_files.is_empty());
    for path in public_files {
        let bytes = fs::read(&path).expect("public artifact should be readable");
        assert!(
            !bytes
                .windows(SECRET.len())
                .any(|window| window == SECRET.as_bytes()),
            "server implementation leaked into {}",
            path.display(),
        );
    }
}

#[test]
fn builds_expected_output_structure() {
    let out_dir = output_dir("structure");
    let app = fixture_project("structure", "mini-app");

    let output = run_build(&app, &out_dir, &["--title", "Mini App"]);
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
    let app = fixture_project("revision", "mini-app");

    let output = run_build(&app, &first_dir, &[]);
    assert_success(&output);
    let first = emitted_revision(&first_dir);

    // Same inputs, deterministic revision.
    let output = run_build(&app, &output_dir("revision-repeat"), &[]);
    assert_success(&output);
    let repeated = emitted_revision(&output_dir("revision-repeat"));
    assert_eq!(first, repeated, "revision should be deterministic");

    // A different client entry emits different bytes and a different revision.
    let output = run_build(&app, &second_dir, &["--client-entry", "src/client-alt.tsx"]);
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
    let app = fixture_project("clean", "mini-app");

    let output = run_build(&app, &out_dir, &[]);
    assert_success(&output);

    let stale = out_dir.join("public/stale-artifact.txt");
    fs::write(&stale, "stale").expect("stale artifact should be writable");
    assert!(stale.is_file());

    let output = run_build(&app, &out_dir, &[]);
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
fn copies_public_assets_nested_and_removes_deleted_assets() {
    let out_dir = output_dir("public-assets");
    let app = fixture_project("public-assets", "mini-app");
    fs::create_dir_all(app.join("public/images")).unwrap();
    fs::write(app.join("public/site.css"), "body { color: red }").unwrap();
    fs::write(app.join("public/images/logo.svg"), "<svg/>").unwrap();

    assert_success(&run_build(&app, &out_dir, &[]));
    assert_eq!(read(out_dir.join("public/site.css")), "body { color: red }");
    assert_eq!(read(out_dir.join("public/images/logo.svg")), "<svg/>");

    fs::remove_file(app.join("public/images/logo.svg")).unwrap();
    assert_success(&run_build(&app, &out_dir, &[]));
    assert!(!out_dir.join("public/images/logo.svg").exists());
}

#[test]
fn public_assets_cannot_shadow_framework_output() {
    let out_dir = output_dir("public-collision");
    let app = fixture_project("public-collision", "mini-app");
    fs::create_dir_all(app.join("public/assets")).unwrap();
    fs::write(app.join("public/assets/client.js"), "application").unwrap();

    let output = run_build(&app, &out_dir, &[]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("conflicts with Plec-owned output"));

    fs::remove_file(app.join("public/assets/client.js")).unwrap();
    fs::create_dir_all(app.join("public/runtime")).unwrap();
    fs::write(app.join("public/runtime/runtime.js"), "application").unwrap();
    let output = run_build(&app, &output_dir("public-runtime-collision"), &[]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("conflicts with Plec-owned output"));

    fs::remove_dir_all(app.join("public/runtime")).unwrap();
    fs::write(app.join("public/host-providers.json"), "application").unwrap();
    let output = run_build(&app, &output_dir("public-provider-manifest-collision"), &[]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("conflicts with Plec-owned output"));

    fs::remove_file(app.join("public/host-providers.json")).unwrap();
    fs::create_dir_all(app.join("public/assets/compiled")).unwrap();
    fs::write(app.join("public/assets/compiled/foo.svg"), "application").unwrap();
    let output = run_build(&app, &output_dir("public-compiled-asset-collision"), &[]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("conflicts with Plec-owned output"));

    fs::remove_dir_all(app.join("public/assets/compiled")).unwrap();
    fs::write(app.join("public/assets/compiled"), "application").unwrap();
    let output = run_build(
        &app,
        &output_dir("public-compiled-assets-path-collision"),
        &[],
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("conflicts with Plec-owned output"));
}

#[test]
fn compiled_asset_imports_are_fingerprinted_deduplicated_and_cleaned() {
    let out_dir = output_dir("compiled-assets");
    let app = fixture_project("compiled-assets", "mini-app");
    let asset_bytes = b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>";
    fs::write(app.join("src/logo.svg"), asset_bytes).unwrap();
    fs::write(app.join("src/duplicate.svg"), asset_bytes).unwrap();
    fs::write(
        app.join("src/other.tsx"),
        "import sharedLogo from './logo.svg'; export function Other() { return <img src={sharedLogo}/>; }",
    )
    .unwrap();
    fs::write(
        app.join("src/home.tsx"),
        "import logo from './logo.svg';\nimport secondLogo from './logo.svg';\nimport thirdLogo from './duplicate.svg';\nimport { Other } from './other';\nexport function Home() { return <div><img src={logo}/><img src={secondLogo}/><img src={thirdLogo}/><Other/></div>; }\n",
    )
    .unwrap();
    fs::create_dir_all(app.join("public")).unwrap();
    fs::write(app.join("public/favicon.svg"), "public-owned").unwrap();

    assert_success(&run_build(&app, &out_dir, &[]));
    let manifest: serde_json::Value =
        serde_json::from_str(&read(out_dir.join("plec-assets.json"))).unwrap();
    assert!(!out_dir.join("public/plec-assets.json").exists());
    let entries = manifest.as_array().unwrap();
    assert_eq!(
        entries.len(),
        2,
        "both source paths should remain build dependencies"
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry["source"] == "src/logo.svg")
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry["source"] == "src/duplicate.svg")
    );
    let url = entries[0]["url"].as_str().unwrap();
    assert!(url.starts_with("/assets/compiled/"));
    let emitted = out_dir.join("public").join(url.trim_start_matches('/'));
    assert_eq!(fs::read(emitted).unwrap(), asset_bytes);
    assert_eq!(
        fs::read_dir(out_dir.join("public/assets/compiled"))
            .unwrap()
            .count(),
        1,
        "byte-identical sources should share emitted output"
    );
    assert!(out_dir.join("public/favicon.svg").is_file());
    let route_artifact = read(out_dir.join("public/route-artifact.json"));
    assert!(
        route_artifact.contains(url),
        "compiled route artifact must contain the imported URL string"
    );

    let repeat_dir = output_dir("compiled-assets-repeat");
    assert_success(&run_build(&app, &repeat_dir, &[]));
    let repeated: serde_json::Value =
        serde_json::from_str(&read(repeat_dir.join("plec-assets.json"))).unwrap();
    assert_eq!(
        repeated[0]["url"], url,
        "identical bytes must have a stable URL"
    );
    assert_eq!(
        manifest, repeated,
        "dependency metadata ordering must be deterministic"
    );

    fs::write(app.join("src/logo.svg"), b"<svg changed/>").unwrap();
    fs::write(app.join("src/duplicate.svg"), b"<svg changed/>").unwrap();
    assert_success(&run_build(&app, &out_dir, &[]));
    let changed: serde_json::Value =
        serde_json::from_str(&read(out_dir.join("plec-assets.json"))).unwrap();
    let changed_url = changed[0]["url"].as_str().unwrap();
    assert_ne!(changed_url, url);
    assert!(
        !out_dir
            .join("public")
            .join(url.trim_start_matches('/'))
            .exists()
    );
    assert_eq!(
        fs::read(
            out_dir
                .join("public")
                .join(changed_url.trim_start_matches('/'))
        )
        .unwrap(),
        b"<svg changed/>"
    );

    fs::write(
        app.join("src/home.tsx"),
        "export function Home() { return <div>No compiled asset reference</div>; }",
    )
    .unwrap();
    assert_success(&run_build(&app, &out_dir, &[]));
    let no_assets: serde_json::Value =
        serde_json::from_str(&read(out_dir.join("plec-assets.json"))).unwrap();
    assert!(no_assets.as_array().unwrap().is_empty());
    assert!(
        !out_dir
            .join("public")
            .join(changed_url.trim_start_matches('/'))
            .exists()
    );
}

#[test]
fn compiled_assets_reject_public_collisions_and_symlink_escapes() {
    use sha2::{Digest, Sha256};
    let app = fixture_project("compiled-assets-security", "mini-app");
    let bytes = b"<svg collision/>";
    let digest = Sha256::digest(bytes);
    let fingerprint = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let url_path = format!("assets/compiled/{}.svg", &fingerprint[..24]);
    fs::write(app.join("src/logo.svg"), bytes).unwrap();
    fs::write(
        app.join("src/home.tsx"),
        "import logo from './logo.svg'; export function Home() { return <img src={logo}/>; }",
    )
    .unwrap();
    fs::create_dir_all(
        app.join("public")
            .join(Path::new(&url_path).parent().unwrap()),
    )
    .unwrap();
    fs::write(app.join("public").join(&url_path), "public-owned").unwrap();
    let collision = run_build(&app, &output_dir("compiled-asset-collision"), &[]);
    assert!(!collision.status.success());
    assert!(
        String::from_utf8_lossy(&collision.stderr).contains("conflicts with Plec-owned output")
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let outside = output_dir("compiled-asset-outside.svg");
        fs::write(&outside, bytes).unwrap();
        fs::remove_file(app.join("public").join(&url_path)).unwrap();
        fs::remove_dir_all(app.join("public/assets/compiled")).unwrap();
        symlink(&outside, app.join("src/escape.svg")).unwrap();
        fs::write(
            app.join("src/home.tsx"),
            "import logo from './escape.svg'; export function Home() { return <img src={logo}/>; }",
        )
        .unwrap();
        let escaped = run_build(&app, &output_dir("compiled-asset-symlink"), &[]);
        assert!(!escaped.status.success());
        assert!(
            String::from_utf8_lossy(&escaped.stderr).contains("outside the approved source root")
        );
    }
}

#[test]
fn compiled_asset_imports_fail_for_missing_and_unsupported_files() {
    let app = fixture_project("compiled-assets-errors", "mini-app");
    fs::write(
        app.join("src/home.tsx"),
        "import logo from './missing.svg'; export function Home() { return <img src={logo}/>; }",
    )
    .unwrap();
    let missing = run_build(&app, &output_dir("compiled-asset-missing"), &[]);
    assert!(!missing.status.success());
    let stderr = String::from_utf8_lossy(&missing.stderr);
    assert!(
        stderr.contains("missing.svg") && stderr.contains("home.tsx"),
        "diagnostic should identify import and module: {stderr}"
    );

    fs::write(app.join("src/thing.exe"), b"not an asset").unwrap();
    fs::write(
        app.join("src/home.tsx"),
        "import thing from './thing.exe'; export function Home() { return <div>{thing}</div>; }",
    )
    .unwrap();
    let unsupported = run_build(&app, &output_dir("compiled-asset-unsupported"), &[]);
    assert!(!unsupported.status.success());
    assert!(String::from_utf8_lossy(&unsupported.stderr).contains("not supported"));

    fs::write(
        app.join("src/home.tsx"),
        "import { logo } from './thing.svg'; export function Home() { return <div>{logo}</div>; }",
    )
    .unwrap();
    let unsupported_shape = run_build(&app, &output_dir("compiled-asset-shape"), &[]);
    assert!(!unsupported_shape.status.success());
    assert!(
        String::from_utf8_lossy(&unsupported_shape.stderr).contains("use a single default import")
    );

    fs::write(
        app.join("src/home.tsx"),
        "export async function Home() { await import('./thing.svg'); return <div/>; }",
    )
    .unwrap();
    let dynamic = run_build(&app, &output_dir("compiled-asset-dynamic"), &[]);
    assert!(!dynamic.status.success());
    assert!(String::from_utf8_lossy(&dynamic.stderr).contains("Unsupported dynamic asset import"));
}

#[test]
fn forbidden_browser_dependency_fails_the_build() {
    let out_dir = output_dir("zod");
    let app = fixture_project("zod", "zod-app");

    let output = run_build(&app, &out_dir, &[]);

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
// and the runtime assets stage from `node_modules/@plec/core/dist/runtime`.
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
/// carrying a marker so the test can prove which source staged them. The
/// staged binaries carry a provenance record the build verifies before
/// copying them.
fn install_plec_package(project: &Path, runtime_marker: &str) {
    use sha2::{Digest, Sha256};

    let sha256_hex = |bytes: &[u8]| {
        let digest = Sha256::digest(bytes);
        digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };

    let plec = project.join("node_modules/@plec/core");
    fs::create_dir_all(plec.join("dist/runtime")).expect("plec package dirs should be creatable");
    fs::write(
        plec.join("package.json"),
        r#"{"name":"@plec/core","type":"module","exports":{".":"./dist/browser.js"}}"#,
    )
    .expect("plec package.json should be writable");
    fs::write(
        plec.join("dist/browser.js"),
        "export function createRouter() { return {}; }\n\
         export function createRootRoute() { return {}; }\n",
    )
    .expect("plec browser stub should be writable");
    let wasm = b"\0asm\x01\x00\x00\x00";
    fs::write(plec.join("dist/runtime/runtime.js"), runtime_marker)
        .expect("runtime.js stub should be writable");
    // Minimal valid WASM binary header; the Rust tests never execute it.
    fs::write(plec.join("dist/runtime/runtime_bg.wasm"), wasm)
        .expect("runtime_bg.wasm stub should be writable");
    let provenance = format!(
        r#"{{"jsSha256":"{}","wasmSha256":"{}"}}"#,
        sha256_hex(runtime_marker.as_bytes()),
        sha256_hex(wasm),
    );
    fs::write(plec.join("dist/runtime/provenance.json"), provenance)
        .expect("provenance.json stub should be writable");
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
        "runtime assets must stage from node_modules/@plec/core/dist/runtime"
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
fn staging_failure_names_installed_package_resolution_path() {
    let project = out_of_repo_project("missing-runtime");
    let app = project.join("mini-app");
    copy_dir_all(&fixture_app("mini-app"), &app);
    // No node_modules/@plec/core installed: runtime staging has nowhere to resolve.

    // Runtime staging runs before any bundling, so esbuild is not needed.
    let out_dir = project.join("dist");
    let output = run_build(&app, &out_dir, &[]);

    assert!(
        !output.status.success(),
        "build must fail without any runtime artifact source"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("node_modules/@plec/core/dist/runtime"),
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
