use std::path::Path;

use plec_compiler::{lower_route_artifacts, lower_routes, read_source_graph};
use plec_model::build_semantic_graph;

use super::id::sanitize;
use super::json::out;
use super::stage::stage;

use super::build::{BuildError, Stage};

/// Emit the Plec compiler artifacts for the routed application in-process.
///
/// This mirrors the `plec-route-manifest --artifacts` pipeline (source graph
/// -> semantic graph -> routes -> executable route artifacts) without
/// spawning another compiler process.
pub fn emit(
    source: &Path,
    app_dir: &Path,
    repo_root: &Path,
    public_dir: &Path,
) -> Result<(), BuildError> {
    let stage = Stage::Compile;

    let source_graph = read_source_graph(source, app_dir, repo_root)
        .map_err(|error| BuildError::new(stage, error))?;

    let semantic_graph =
        build_semantic_graph(&source_graph.modules, &source_graph.resolved_imports)
            .map_err(|error| BuildError::new(stage, error.to_string()))?;

    let routes = lower_routes(&source_graph.modules, &semantic_graph)
        .map_err(|error| BuildError::new(stage, error.to_string()))?;

    let bundle = lower_route_artifacts(&source_graph.modules, &semantic_graph, &routes).map_err(
        |error| {
            BuildError::new(
                stage,
                format!("unsupported compiled route application: {error}"),
            )
        },
    )?;

    let graphs_dir = public_dir.join("graphs");

    for artifact in &bundle.graphs {
        let filename = format!("{}.json", sanitize(&artifact.graph_id));

        out(&graphs_dir.join(filename), &artifact.graph, true).map_err(|error| {
            BuildError::with_source(stage, format!("failed to write route graph"), error)
        })?;
    }

    out(
        &public_dir.join("route-manifest.json"),
        &bundle.manifest,
        true,
    )
    .map_err(|error| BuildError::with_source(stage, "failed to write route manifest", error))?;

    out(&public_dir.join("route-artifact.json"), &bundle, false)
        .map_err(|error| BuildError::with_source(stage, "failed to write route artifact", error))?;

    stage_runtime(app_dir, repo_root, public_dir)
}

/// Stage the prebuilt WASM runtime next to the compiler artifacts.
fn stage_runtime(app_dir: &Path, repo_root: &Path, public_dir: &Path) -> Result<(), BuildError> {
    stage(app_dir, repo_root, public_dir).map_err(|error| {
        BuildError::new(
            Stage::Compile,
            format!("failed to stage Plec runtime: {error}"),
        )
    })
}
