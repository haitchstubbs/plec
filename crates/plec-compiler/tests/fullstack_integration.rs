//! Real-source regression for source-graph identity and imported component calls.

use std::path::{Path, PathBuf};

use plec_compiler::{
    discover_root_component, lower_root_component, lower_route_manifest, lower_routes,
    read_source_graph,
};
use plec_hir::{ComponentId, HirNode};
use plec_model::build_semantic_graph;

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .find(|candidate| {
            candidate.join("Cargo.toml").is_file() && candidate.join("apps/fullstack").is_dir()
        })
        .expect("plec workspace root should contain apps/fullstack")
        .to_path_buf()
}

#[test]
fn resolves_fullstack_imported_component_calls_to_canonical_targets() {
    let repo_root = repository_root();
    let app_root = repo_root.join("apps/fullstack");
    let entry = app_root.join("src/routes/todos.tsx");
    let source_graph = read_source_graph(&entry, &app_root, &repo_root)
        .expect("apps/fullstack source graph should load");
    let semantic_graph =
        build_semantic_graph(&source_graph.modules, &source_graph.resolved_imports)
            .expect("semantic graph should use source-graph resolutions");
    let entry_module = source_graph
        .modules
        .iter()
        .find(|module| module.id == "src/routes/todos.tsx")
        .expect("source graph should contain todos.tsx");
    let root = discover_root_component(
        &source_graph.modules,
        &semantic_graph,
        &entry_module.id,
        Some("TodosPending"),
    )
    .expect("TodosPending should be discovered");
    let hir = lower_root_component(&root, &semantic_graph)
        .expect("TodosPending should lower to structural HIR");

    assert_eq!(
        hir.id,
        ComponentId::new("src/routes/todos.tsx", "TodosPending")
    );
    let page_frame = hir
        .nodes
        .iter()
        .find_map(|node| match node {
            HirNode::Component(call)
                if call.target
                    == plec_hir::HirComponentTarget::Static(ComponentId::new(
                        "src/components/page-primitives.tsx",
                        "PageFrame",
                    )) =>
            {
                Some(call)
            }
            _ => None,
        })
        .expect("PageFrame should resolve to its defining module and local symbol");
    assert_eq!(page_frame.span.module_id, "src/routes/todos.tsx");
}

#[test]
fn lowers_the_fullstack_todos_page_through_the_rust_pipeline() {
    let repo_root = repository_root();
    let app_root = repo_root.join("apps/fullstack");
    let entry = app_root.join("src/routes/todos.tsx");
    let source_graph = read_source_graph(&entry, &app_root, &repo_root).unwrap();
    let semantic_graph =
        build_semantic_graph(&source_graph.modules, &source_graph.resolved_imports).unwrap();
    let root = discover_root_component(
        &source_graph.modules,
        &semantic_graph,
        "src/routes/todos.tsx",
        Some("TodosPage"),
    )
    .unwrap();
    let hir = lower_root_component(&root, &semantic_graph)
        .expect("TodosPage should lower without a JavaScript fallback");
    assert!(hir.inputs.iter().any(|input| input.kind == "loaderData"));
    assert!(hir
        .expressions
        .iter()
        .any(|expression| matches!(expression.expression, plec_hir::HirExpr::Filter { .. })));
}

#[test]
fn lowers_fullstack_route_tree_to_a_rust_manifest() {
    let repo_root = repository_root();
    let app_root = repo_root.join("apps/fullstack");
    let source_graph = read_source_graph(app_root.join("src/router.tsx"), &app_root, &repo_root)
        .expect("apps/fullstack router source graph should load");
    let semantic_graph =
        build_semantic_graph(&source_graph.modules, &source_graph.resolved_imports)
            .expect("router source graph should resolve imports");
    let manifest = lower_route_manifest(
        &lower_routes(&source_graph.modules, &semantic_graph)
            .expect("static fullstack route declarations should lower"),
    );
    assert_eq!(manifest.version, 3);
    assert_eq!(manifest.routes.len(), 7);
    assert!(manifest
        .routes
        .iter()
        .all(|route| route.graph_id != manifest.root_graph_id));
    // The parameterized fixture route keeps its `$param` path segment.
    let project = manifest
        .routes
        .iter()
        .find(|route| route.path == "projects/$id")
        .unwrap();
    assert_eq!(project.outlet_id, "main");
    let todos = manifest
        .routes
        .iter()
        .find(|route| route.path == "todos")
        .unwrap();
    assert_eq!(todos.loader_action, Some(0));
    assert!(todos.pending_graph_id.is_some());
    assert!(todos.error_graph_id.is_some());
}
