use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixture_app() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("mini-repo")
        .join("apps")
        .join("markers-app")
        .join("src")
        .join("router.tsx")
}

fn fixture(file: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("markers")
        .join(file)
}

fn root_graph() -> &'static str {
    "src/layout.tsx#Layout"
}

fn run_validate(args: &[&str]) -> Output {
    let mut process = Command::new(env!("CARGO_BIN_EXE_plec"));
    process.args([
        "workspace",
        "markers",
        "validate",
        "--graph",
        root_graph(),
        "--source",
    ]);
    process.arg(fixture_app());
    for arg in args {
        if let Some(path) = arg.strip_prefix("fixture:") {
            process.arg(fixture(path));
        } else {
            process.arg(arg);
        }
    }
    process.output().expect("plec binary should be invocable")
}

fn run_explain(address: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_plec"))
        .args(["workspace", "markers", "explain", address, "--json"])
        .output()
        .expect("plec binary should be invocable")
}

#[test]
fn explain_decomposes_an_address_as_json() {
    let output = run_explain("root/outlet:main/component:2/loop:3/key:one/node:1");
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let kinds: Vec<&str> = report["segments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|segment| segment["kind"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        vec!["root", "outlet", "component", "loop", "key", "node"]
    );
}

#[test]
fn structural_validation_accepts_the_list_route_html() {
    let output = run_validate(&["--html", "fixture:list.html", "--route", "/list"]);
    assert!(
        output.status.success(),
        "structural validation should succeed\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("markers valid"), "{stdout}");
    assert!(stdout.contains("structural"));
}

#[test]
fn snapshot_validation_is_strict_and_exact_for_the_list_route() {
    let output = run_validate(&[
        "--html",
        "fixture:list.html",
        "--route",
        "/list",
        "--snapshot",
        "fixture:list-snapshot.json",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("strict"), "{stdout}");
    assert!(stdout.contains("expected markers  21"), "{stdout}");
    assert!(stdout.contains("actual markers    21"), "{stdout}");
}

#[test]
fn snapshot_validation_selects_the_alternate_branch_on_the_about_route() {
    let output = run_validate(&[
        "--html",
        "fixture:about.html",
        "--route",
        "/about",
        "--snapshot",
        "fixture:about-snapshot.json",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn missing_component_end_fails_with_the_adoption_code() {
    let output = run_validate(&[
        "--html",
        "fixture:list-missing-component-end.html",
        "--route",
        "/list",
        "--snapshot",
        "fixture:list-snapshot.json",
    ]);
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("missing:ssr-component: component-end:root:2"),
        "{stdout}"
    );
}

#[test]
fn stray_marker_is_reported_as_unexpected() {
    let output = run_validate(&[
        "--html",
        "fixture:list-stray-marker.html",
        "--route",
        "/list",
    ]);
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("unexpected:ssr-marker:component-end:root:9"),
        "{stdout}"
    );
}

#[test]
fn route_filter_selects_the_outlet_child() {
    // Forcing the about child against the list-route HTML must fail on the
    // about graph's outlet markers.
    let output = run_validate(&["--html", "fixture:list.html", "--route", "/about"]);
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("missing:ssr-node: node:root/outlet:main/node:1"),
        "{stdout}"
    );
}
