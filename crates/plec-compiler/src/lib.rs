mod component_discovery;
mod hir_builder;
mod lowering;
mod read_source_graph;
mod routes;

pub use component_discovery::{
    discover_root_component, ComponentDeclaration, ComponentDiscoveryError,
    ReturnedComponentExpression, RootComponent,
};
pub use hir_builder::{lower_application, lower_root_component};
pub use lowering::{
    lower_application_to_executable, lower_component_to_executable,
    lower_route_loader_to_executable, LoweringError,
};
pub use read_source_graph::{read_source_graph, SourceGraph};
pub use routes::{
    lower_route_application_to_executable, lower_route_artifacts, lower_route_manifest,
    lower_routes, RouteArtifact, RouteArtifactBundle, RouteError,
};

use plec_ir::ComponentApplication;
use std::path::Path;

/// Parse the source graph reachable from `source` and build its semantic graph.
pub fn load(
    source: &Path,
) -> Result<(Vec<plec_parser::ParsedModule>, plec_model::SemanticGraph), Box<dyn std::error::Error>>
{
    let root_dir = std::env::current_dir()?;

    let source_graph = read_source_graph(source, &root_dir, &root_dir)?;

    let semantic_graph =
        plec_model::build_semantic_graph(&source_graph.modules, &source_graph.resolved_imports)?;

    Ok((source_graph.modules, semantic_graph))
}

pub fn compile(source: &Path) -> Result<ComponentApplication, Box<dyn std::error::Error>> {
    let (modules, semantic_graph) = load(source)?;

    // read_source_graph guarantees modules[0] is the entry module.
    let entry_module_id = modules[0].id.clone();

    let root = discover_root_component(&modules, &semantic_graph, &entry_module_id, None)?;

    let hir = lower_application(&modules, &root, &semantic_graph)?;

    Ok(lower_application_to_executable(&hir)?)
}
