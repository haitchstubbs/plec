use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use plec_compiler::{
    lower_route_artifacts_with_options, lower_routes, read_source_graph_with_options,
    CompilerOptions,
};
use plec_model::build_semantic_graph;

use super::id::sanitize;
use super::json::out;
use super::stage::stage;

use super::build::{BuildError, Stage};

pub struct ArtifactOutput {
    pub host_components: BTreeMap<String, BTreeSet<String>>,
}

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
    host_imports: &std::collections::BTreeMap<String, String>,
    custom_elements: &std::collections::BTreeSet<String>,
) -> Result<ArtifactOutput, BuildError> {
    let stage = Stage::Compile;

    let source_graph = read_source_graph_with_options(
        source,
        app_dir,
        repo_root,
        &CompilerOptions {
            host_imports: host_imports.clone(),
            custom_elements: custom_elements.clone(),
        },
    )
    .map_err(|error| BuildError::new(stage, error))?;

    let semantic_graph =
        build_semantic_graph(&source_graph.modules, &source_graph.resolved_imports)
            .map_err(|error| BuildError::new(stage, error.to_string()))?;

    let routes = lower_routes(&source_graph.modules, &semantic_graph)
        .map_err(|error| BuildError::new(stage, error.to_string()))?;

    let bundle = lower_route_artifacts_with_options(
        &source_graph.modules,
        &semantic_graph,
        &routes,
        custom_elements,
    )
    .map_err(|error| {
        BuildError::new(
            stage,
            format!("unsupported compiled route application: {error}"),
        )
    })?;

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

    let host_components = collect_host_components(&bundle);
    stage_runtime(app_dir, public_dir)?;
    Ok(ArtifactOutput { host_components })
}

fn collect_host_components(
    bundle: &plec_compiler::RouteArtifactBundle,
) -> BTreeMap<String, BTreeSet<String>> {
    use plec_ir::{ComponentProp, Node};

    let mut components = BTreeMap::<String, BTreeSet<String>>::new();
    let mut insert = |provider: &str, component: &str| {
        components
            .entry(provider.to_owned())
            .or_default()
            .insert(component.to_owned());
    };

    for graph in &bundle.graphs {
        for application in &graph.graph.components {
            for node in &application.nodes {
                let props = match node {
                    Node::HostComponent {
                        provider,
                        component,
                        props,
                        ..
                    } => {
                        insert(provider, component);
                        props
                    }
                    Node::Component { props, .. } | Node::DynamicComponent { props, .. } => props,
                    _ => continue,
                };
                for prop in props {
                    if let ComponentProp::Component {
                        host: Some(target), ..
                    } = prop
                    {
                        insert(&target.provider, &target.component);
                    }
                }
            }
        }
    }

    components
}

/// Stage the prebuilt WASM runtime next to the compiler artifacts.
fn stage_runtime(app_dir: &Path, public_dir: &Path) -> Result<(), BuildError> {
    stage(app_dir, public_dir).map_err(|error| {
        BuildError::new(
            Stage::Compile,
            format!("failed to stage Plec runtime: {error}"),
        )
    })
}
