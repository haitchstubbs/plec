use super::id::sanitize;
use super::json_out::json_out;
use super::stage::stage;

use serde_json::Value;
use std::{fs, path::Path, process::Command as ProcessCommand};

/// Compile routed application artifacts using the compiler-owned
/// `plec-route-manifest --artifacts` path and emit them into `out_dir`.
pub fn build(source: &Path, out_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let repo_root = std::env::current_dir()?;

    let app_dir = source
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| {
            format!(
                "could not determine app directory from {}",
                source.display()
            )
        })?
        .to_path_buf();

    let output = ProcessCommand::new("cargo")
        .args([
            "run",
            "-q",
            "-p",
            "plec-compiler",
            "--bin",
            "plec-route-manifest",
            "--",
        ])
        .arg(source)
        .arg(&app_dir)
        .arg(&repo_root)
        .arg("--artifacts")
        .current_dir(&repo_root)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);

        return Err(format!("plec-route-manifest failed:\n{}", stderr).into());
    }

    let artifacts: Value = serde_json::from_slice(&output.stdout)?;

    let manifest = artifacts
        .get("manifest")
        .ok_or("compiler artifact is missing `manifest`")?;

    let graphs = artifacts
        .get("graphs")
        .and_then(Value::as_array)
        .ok_or("compiler artifact is missing `graphs`")?;

    let graphs_dir = out_dir.join("graphs");

    fs::create_dir_all(&graphs_dir)?;

    //
    // Individual route graphs
    //

    for graph_entry in graphs {
        let graph_id = graph_entry
            .get("graphId")
            .and_then(Value::as_str)
            .ok_or("route graph is missing `graphId`")?;

        let graph = graph_entry
            .get("graph")
            .ok_or("route graph is missing `graph`")?;

        let filename = format!("{}.json", sanitize(graph_id),);

        json_out(&graphs_dir.join(filename), graph, true)?;
    }

    json_out(&out_dir.join("route-manifest.json"), manifest, true)?;
    json_out(&out_dir.join("route-artifact.json"), &artifacts, false)?;
    stage(&repo_root, out_dir)?;

    println!("Compiled {} -> {}", source.display(), out_dir.display(),);

    Ok(())
}
