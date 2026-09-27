use super::doctor::{
    compile_application, resolution_status, resolve_graph, resolved_component, GraphResolution,
};
use super::repo::Repo;
use plec_ir::{ComponentApplication, ExecutableComponent, Node};
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveReport {
    pub graph_id: String,
    pub resolution: GraphResolution,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeReport {
    pub graph_id: String,
    pub resolution: GraphResolution,
    pub lines: Vec<String>,
}

impl ResolveReport {
    pub fn found(&self) -> bool {
        !matches!(self.resolution, GraphResolution::Missing)
    }
}

impl TreeReport {
    pub fn found(&self) -> bool {
        !matches!(self.resolution, GraphResolution::Missing)
    }
}

pub fn resolve(repo: &Repo, source: &Path, graph_id: &str) -> Result<ResolveReport, String> {
    let compiled = compile_application(repo, source)?;
    Ok(ResolveReport {
        graph_id: graph_id.into(),
        resolution: resolve_graph(&compiled, graph_id),
    })
}

pub fn tree(repo: &Repo, source: &Path, graph_id: &str) -> Result<TreeReport, String> {
    let compiled = compile_application(repo, source)?;
    let resolution = resolve_graph(&compiled, graph_id);
    let lines = resolved_component(&compiled, &resolution)
        .and_then(|component| {
            let registry_key = match &resolution {
                GraphResolution::Direct { registry_key, .. }
                | GraphResolution::RegisteredComponent { registry_key, .. } => registry_key,
                GraphResolution::Missing => return None,
            };
            compiled
                .registry
                .get(registry_key)
                .map(|application| render_tree(application, component))
        })
        .unwrap_or_default();
    Ok(TreeReport {
        graph_id: graph_id.into(),
        resolution,
        lines,
    })
}

pub fn print_resolve(report: &ResolveReport) {
    println!("graph {}", report.graph_id);
    println!("  {}", resolution_status(&report.resolution));
    match &report.resolution {
        GraphResolution::Direct {
            registry_key,
            component_index,
            component_id,
        } => println!(
            "  registry[{registry_key}] -> root component[{component_index}] ({component_id})"
        ),
        GraphResolution::RegisteredComponent {
            registry_key,
            component_index,
            component_id,
        } => println!(
            "  registry.values() -> application[{registry_key}] -> component[{component_index}] ({component_id})"
        ),
        GraphResolution::Missing => println!("  runtime would fail closed"),
    }
}

pub fn print_tree(report: &TreeReport) {
    print_resolve(&ResolveReport {
        graph_id: report.graph_id.clone(),
        resolution: report.resolution.clone(),
    });
    for line in &report.lines {
        println!("  {line}");
    }
}

fn render_tree(application: &ComponentApplication, component: &ExecutableComponent) -> Vec<String> {
    let mut lines = Vec::new();
    let mut ancestors = BTreeSet::new();
    render_node(
        application,
        component,
        component.root_node,
        0,
        &mut ancestors,
        &mut lines,
    );
    lines
}

fn render_node(
    application: &ComponentApplication,
    component: &ExecutableComponent,
    handle: usize,
    depth: usize,
    ancestors: &mut BTreeSet<usize>,
    lines: &mut Vec<String>,
) {
    let indent = "  ".repeat(depth);
    if !ancestors.insert(handle) {
        lines.push(format!("{indent}node[{handle}] cycle"));
        return;
    }

    let Some(node) = component.nodes.get(handle) else {
        lines.push(format!("{indent}node[{handle}] invalid handle"));
        ancestors.remove(&handle);
        return;
    };

    match node {
        Node::Element { tag, children, .. } => {
            let tag = component
                .strings
                .get(*tag)
                .map(String::as_str)
                .unwrap_or("<invalid tag>");
            lines.push(format!("{indent}node[{handle}] element <{tag}>"));
            render_children(
                application,
                component,
                children,
                depth + 1,
                ancestors,
                lines,
            );
        }
        Node::Text { text, .. } => {
            let label = match component.texts.get(*text) {
                Some(text) => text
                    .value
                    .as_deref()
                    .map(|value| format!("text {value:?}"))
                    .unwrap_or_else(|| "text dynamic".into()),
                None => format!("text[{text}] invalid handle"),
            };
            lines.push(format!("{indent}node[{handle}] {label}"));
        }
        Node::Conditional {
            consequent,
            alternate,
            ..
        } => {
            lines.push(format!("{indent}node[{handle}] conditional"));
            lines.push(format!("{indent}  consequent"));
            render_node(
                application,
                component,
                *consequent,
                depth + 2,
                ancestors,
                lines,
            );
            if let Some(alternate) = alternate {
                lines.push(format!("{indent}  alternate"));
                render_node(
                    application,
                    component,
                    *alternate,
                    depth + 2,
                    ancestors,
                    lines,
                );
            }
        }
        Node::Loop { r#loop, .. } => {
            let loop_handle = *r#loop;
            let Some(loop_info) = component.loops.get(loop_handle) else {
                lines.push(format!(
                    "{indent}node[{handle}] loop[{loop_handle}] invalid handle"
                ));
                ancestors.remove(&handle);
                return;
            };
            lines.push(format!("{indent}node[{handle}] keyed loop[{loop_handle}]"));
            render_node(
                application,
                component,
                loop_info.row_template,
                depth + 1,
                ancestors,
                lines,
            );
        }
        Node::Component {
            component: target,
            children,
            ..
        } => {
            let target = application
                .components
                .get(*target)
                .map(|component| format!("component[{target}] ({})", component.id))
                .unwrap_or_else(|| format!("component[{target}] invalid handle"));
            lines.push(format!("{indent}node[{handle}] call {target}"));
            if !children.is_empty() {
                lines.push(format!("{indent}  call-site children"));
                render_children(
                    application,
                    component,
                    children,
                    depth + 2,
                    ancestors,
                    lines,
                );
            }
        }
        Node::DynamicComponent { children, .. } => {
            lines.push(format!("{indent}node[{handle}] dynamic component"));
            if !children.is_empty() {
                lines.push(format!("{indent}  call-site children"));
                render_children(
                    application,
                    component,
                    children,
                    depth + 2,
                    ancestors,
                    lines,
                );
            }
        }
        Node::HostComponent {
            provider,
            component,
            ..
        } => lines.push(format!(
            "{indent}node[{handle}] host {provider}/{component}"
        )),
        Node::Slot { .. } => lines.push(format!("{indent}node[{handle}] slot")),
    }

    ancestors.remove(&handle);
}

fn render_children(
    application: &ComponentApplication,
    component: &ExecutableComponent,
    children: &[usize],
    depth: usize,
    ancestors: &mut BTreeSet<usize>,
    lines: &mut Vec<String>,
) {
    for child in children {
        render_node(application, component, *child, depth, ancestors, lines);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plec_ir::{ComponentApplication, ExecutableComponent, Loop, Node, RouteManifest, Text};
    use std::collections::BTreeMap;

    use crate::dev::doctor::CompiledApp;

    fn component(id: &str, nodes: Vec<Node>, loops: Vec<Loop>) -> ExecutableComponent {
        ExecutableComponent {
            id: id.into(),
            root_node: 0,
            strings: vec!["main".into()],
            constants: vec![],
            nodes,
            texts: vec![Text {
                value: Some("row".into()),
                binding: None,
            }],
            bindings: vec![],
            prop_programs: vec![],
            events: vec![],
            inputs: vec![],
            host_slots: vec![],
            capabilities: vec![],
            state_slots: vec![],
            ref_slots: vec![],
            host_refs: vec![],
            reactions: vec![],
            listeners: vec![],
            parameters: vec![],
            expressions: vec![],
            actions: vec![],
            loops,
            dependency_edges: vec![],
            route_outlets: vec![],
        }
    }

    fn test_app() -> CompiledApp {
        let app = component(
            "App",
            vec![
                Node::Element {
                    tag: 0,
                    namespace: "html",
                    parent: None,
                    children: vec![1, 2, 3],
                    host_ref: None,
                },
                Node::Conditional {
                    test: 0,
                    parent: Some(0),
                    consequent: 4,
                    alternate: Some(5),
                },
                Node::Loop {
                    r#loop: 0,
                    parent: Some(0),
                },
                Node::Component {
                    component: 1,
                    parent: Some(0),
                    props: vec![],
                    children: vec![6],
                },
                Node::Text {
                    text: 0,
                    parent: Some(1),
                },
                Node::Slot { parent: Some(1) },
                Node::HostComponent {
                    provider: "test".into(),
                    component: "card".into(),
                    parent: Some(3),
                    props: vec![],
                },
                Node::Text {
                    text: 0,
                    parent: None,
                },
            ],
            vec![Loop {
                source_expression: 0,
                key_expression: 0,
                item_slot: 0,
                row_template: 7,
                dependency_slots: vec![],
                input: None,
            }],
        );
        let panel = component("Panel", vec![Node::Slot { parent: None }], vec![]);
        CompiledApp {
            manifest: RouteManifest {
                version: 3,
                revision: String::new(),
                root_graph_id: "page".into(),
                routes: vec![],
            },
            registry: BTreeMap::from([(
                "page".into(),
                ComponentApplication {
                    version: "0.10",
                    root_component: 0,
                    components: vec![app, panel],
                },
            )]),
        }
    }

    #[test]
    fn renders_nested_node_structure() {
        let compiled = test_app();
        let resolution = resolve_graph(&compiled, "page");
        let component = resolved_component(&compiled, &resolution).unwrap();
        let lines = render_tree(compiled.registry.get("page").unwrap(), component);

        assert!(lines.iter().any(|line| line.contains("element <main>")));
        assert!(lines.iter().any(|line| line.contains("consequent")));
        assert!(lines.iter().any(|line| line.contains("alternate")));
        assert!(lines.iter().any(|line| line.contains("keyed loop[0]")));
        assert!(lines
            .iter()
            .any(|line| line.contains("call component[1] (Panel)")));
        assert!(lines.iter().any(|line| line.contains("call-site children")));
    }

    #[test]
    fn resolves_direct_component_fallback_and_missing_graphs() {
        let compiled = test_app();

        assert!(matches!(
            resolve_graph(&compiled, "page"),
            GraphResolution::Direct {
                registry_key,
                component_index: 0,
                ..
            } if registry_key == "page"
        ));
        assert!(matches!(
            resolve_graph(&compiled, "Panel"),
            GraphResolution::RegisteredComponent {
                registry_key,
                component_index: 1,
                ..
            } if registry_key == "page"
        ));
        assert!(matches!(
            resolve_graph(&compiled, "missing"),
            GraphResolution::Missing
        ));
    }

    #[test]
    fn reports_invalid_handles_without_panicking() {
        let component = component(
            "Broken",
            vec![
                Node::Element {
                    tag: 3,
                    namespace: "html",
                    parent: None,
                    children: vec![1, 9],
                    host_ref: None,
                },
                Node::Text {
                    text: 9,
                    parent: Some(0),
                },
            ],
            vec![],
        );
        let application = ComponentApplication {
            version: "0.10",
            root_component: 0,
            components: vec![component.clone()],
        };
        let lines = render_tree(&application, &component);

        assert!(lines.iter().any(|line| line.contains("<invalid tag>")));
        assert!(lines
            .iter()
            .any(|line| line.contains("node[9] invalid handle")));
        assert!(lines
            .iter()
            .any(|line| line.contains("text[9] invalid handle")));
    }
}
