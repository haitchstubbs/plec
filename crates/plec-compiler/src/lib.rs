mod component_discovery;
mod hir_builder;
mod lowering;
mod read_source_graph;
mod routes;

pub use component_discovery::{
    discover_root_component, ComponentDeclaration, ComponentDiscoveryError,
    ReturnedComponentExpression, RootComponent,
};
pub use hir_builder::{lower_application, lower_application_with_options, lower_root_component};
pub use lowering::{
    lower_application_to_executable, lower_component_to_executable,
    lower_route_loader_to_executable, LoweringError,
};
pub use read_source_graph::{read_source_graph, read_source_graph_with_options, SourceGraph};
pub use routes::{
    lower_route_application_to_executable, lower_route_artifacts,
    lower_route_artifacts_with_options, lower_route_manifest, lower_routes, RouteArtifact,
    RouteArtifactBundle, RouteError,
};

use plec_ir::ComponentApplication;
use std::{collections::BTreeMap, path::Path};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompilerOptions {
    /// Exact source import specifier to runtime host-provider identity.
    pub host_imports: BTreeMap<String, String>,
    /// Trusted custom element tags (`plec.toml` `[compiler] custom-elements`).
    /// The compiler diagnostics accept these intrinsic JSX tags; the runtime
    /// enforces the same list as an explicit host capability.
    pub custom_elements: std::collections::BTreeSet<String>,
}

/// Parse the source graph reachable from `source` and build its semantic graph.
pub fn load(
    source: &Path,
) -> Result<(Vec<plec_parser::ParsedModule>, plec_model::SemanticGraph), Box<dyn std::error::Error>>
{
    load_with_options(source, &CompilerOptions::default())
}

pub fn load_with_options(
    source: &Path,
    options: &CompilerOptions,
) -> Result<(Vec<plec_parser::ParsedModule>, plec_model::SemanticGraph), Box<dyn std::error::Error>>
{
    let root_dir = std::env::current_dir()?;

    let source_graph = read_source_graph_with_options(source, &root_dir, &root_dir, options)?;

    let semantic_graph =
        plec_model::build_semantic_graph(&source_graph.modules, &source_graph.resolved_imports)?;

    Ok((source_graph.modules, semantic_graph))
}

pub fn compile(source: &Path) -> Result<ComponentApplication, Box<dyn std::error::Error>> {
    compile_with_options(source, &CompilerOptions::default())
}

pub fn compile_with_options(
    source: &Path,
    options: &CompilerOptions,
) -> Result<ComponentApplication, Box<dyn std::error::Error>> {
    let (modules, semantic_graph) = load_with_options(source, options)?;

    // read_source_graph guarantees modules[0] is the entry module.
    let entry_module_id = modules[0].id.clone();

    let root = discover_root_component(&modules, &semantic_graph, &entry_module_id, None)?;

    let hir =
        lower_application_with_options(&modules, &root, &semantic_graph, &options.custom_elements)?;

    Ok(lower_application_to_executable(&hir)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use plec_ir::{ComponentProp, Node};
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn lowers_host_callback_props_into_runtime_callable_handles() {
        let temp = tempdir().expect("temporary app");
        let entry = temp.path().join("App.tsx");
        fs::write(
            &entry,
            r#"
                import { Widget } from "host-widget";

                export function App() {
                  const [count, setCount] = useState(0);
                  return <Widget className="control" onActivate={() => setCount(count + 1)} />;
                }
            "#,
        )
        .expect("source writes");
        let options = CompilerOptions {
            host_imports: BTreeMap::from([("host-widget".into(), "test".into())]),
            custom_elements: Default::default(),
        };

        let source_graph = read_source_graph_with_options(
            &entry,
            temp.path(),
            temp.path(),
            &options,
        )
        .expect("host source graph resolves");
        let semantic_graph = plec_model::build_semantic_graph(
            &source_graph.modules,
            &source_graph.resolved_imports,
        )
        .expect("semantic graph builds");
        let root = discover_root_component(
            &source_graph.modules,
            &semantic_graph,
            &source_graph.modules[0].id,
            None,
        )
        .expect("root component resolves");
        let hir = lower_application_with_options(
            &source_graph.modules,
            &root,
            &semantic_graph,
            &options.custom_elements,
        )
        .expect("host callback HIR lowers");
        let application = lower_application_to_executable(&hir).expect("host callback compiles");
        let component = &application.components[application.root_component];
        let Node::HostComponent {
            provider,
            component: host_component,
            props,
            ..
        } = component
            .nodes
            .iter()
            .find(|node| matches!(node, Node::HostComponent { .. }))
            .expect("host component lowers")
        else {
            unreachable!("matched host component")
        };
        assert_eq!(provider, "test");
        assert_eq!(host_component, "Widget");
        assert!(props.iter().any(|prop| {
            matches!(prop, ComponentProp::Value { name, .. } if component.strings[*name] == "__plec_props")
        }));
        assert!(props.iter().any(|prop| {
            matches!(prop, ComponentProp::Callable { name, action } if component.strings[*name] == "onActivate" && *action < component.actions.len())
        }));
    }
}
