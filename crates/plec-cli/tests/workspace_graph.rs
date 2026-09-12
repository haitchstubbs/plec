use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixture_source() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("mini-repo")
        .join("apps")
        .join("mini-app")
        .join("src")
        .join("router.tsx")
}

fn graph_id() -> &'static str {
    "src/home.tsx#Home"
}

fn run_graph(command: &str, graph_id: &str, json: bool) -> Output {
    let mut process = Command::new(env!("CARGO_BIN_EXE_plec"));
    process
        .args(["workspace", "graph", command, graph_id, "--source"])
        .arg(fixture_source());
    if json {
        process.arg("--json");
    }
    process.output().expect("plec binary should be invocable")
}

#[test]
fn resolve_reports_direct_registry_path_as_json() {
    let output = run_graph("resolve", graph_id(), true);

    assert!(
        output.status.success(),
        "resolve should succeed\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["graphId"], graph_id());
    assert_eq!(report["resolution"]["kind"], "direct");
    assert_eq!(report["resolution"]["registryKey"], graph_id());
}

#[test]
fn tree_prints_compact_component_structure() {
    let output = run_graph("tree", graph_id(), false);

    assert!(
        output.status.success(),
        "tree should succeed\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("direct registry key"));
    assert!(stdout.contains("node[0] element <div>"));
    assert!(stdout.contains("node[1] text \"Hello Plec\""));
}

#[test]
fn missing_graph_reports_fail_closed_and_exits_nonzero() {
    let output = run_graph("resolve", "missing", false);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("runtime would fail closed"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("not registered"));
}
