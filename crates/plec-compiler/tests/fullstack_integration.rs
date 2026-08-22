//! Integration test for the real source → discovery → HIR pipeline.
//!
//! This intentionally uses `apps/fullstack` as an external fixture rather than
//! constructing a synthetic SWC tree. The goal is to see what the Rust compiler
//! currently understands about a real Plec component.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use plec_compiler::{build_component_prop_lookup, discover_root_component, lower_root_component, read_source_graph};
use plec_hir::{HirCallable, HirExpr, HirNode, HirProp};
use plec_sema::build_semantic_graph;

fn repository_root() -> PathBuf {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));

    manifest_dir
        .ancestors()
        .find(|candidate| {
            candidate.join("Cargo.toml").is_file() && candidate.join("apps/fullstack").is_dir()
        })
        .expect("plec workspace root should contain apps/fullstack")
        .to_path_buf()
}

#[test]
#[ignore = "integration fixture: reads apps/fullstack"]
fn lowers_fullstack_home_page_to_hir() {
    let repo_root = repository_root();
    let app_root = repo_root.join("apps/fullstack");
    let entry = app_root.join("src/routes/todos.tsx");

    assert!(
        entry.is_file(),
        "fullstack fixture does not exist: {}",
        entry.display()
    );

    // 1. Load the actual source graph.
    let source_graph = read_source_graph(&entry, &app_root, &repo_root)
        .expect("apps/fullstack source graph should load");

    println!("\n=== SOURCE GRAPH ===");
    println!("modules: {}", source_graph.modules.len());

    for module in &source_graph.modules {
        println!("  {}", module.id);
    }

    // 2. Build sema using the import/module resolutions already determined
    // by source graph loading.
    let semantic_graph = build_semantic_graph(&source_graph.modules, &HashMap::new())
        .expect("apps/fullstack semantic graph should build");

    // 3. Find the actual entry module.
    let entry_module = source_graph
        .modules
        .iter()
        .find(|module| {
            module.id.ends_with("src/routes/todos.tsx")
                || module.id.ends_with(r"src\routes\todos.tsx")
        })
        .expect("source graph should contain todos.tsx");

    println!("\n=== ENTRY MODULE ===");
    println!("{}", entry_module.id);

    // 4. Run actual component discovery.
    let root = discover_root_component(
        &source_graph.modules,
        &semantic_graph,
        &entry_module.id,
        Some("TodosPage"),
    )
    .expect("TodosPage should be discovered");

    println!("\n=== DISCOVERED ROOT ===");
    println!("{root:#?}");

    assert_eq!(root.symbol.local_name, "TodosPage");
    assert_eq!(root.symbol.module_id, entry_module.id);

    // 5. Lower the discovered root into HIR.
    let component_props = build_component_prop_lookup(&semantic_graph, &root.symbol.module_id);
    let hir = lower_root_component(&root, &component_props).expect("TodosPage should lower to HIR");

    println!("\n=== HIR ===");
    println!("{hir:#?}");

    assert_eq!(hir.name, "TodosPage");
    assert_eq!(hir.module_id, entry_module.id);
    assert!(!hir.root_nodes.is_empty());
    assert!(!hir.nodes.is_empty());

    // Verify semantic structure: should have PageFrame component
    let page_frame = hir.nodes.iter().find(|n| {
        matches!(n, HirNode::Component(comp) if comp.name == "PageFrame")
    });
    assert!(page_frame.is_some(), "TodosPage should contain PageFrame component");

    // Verify form element exists
    let form = hir.nodes.iter().find(|n| {
        matches!(n, HirNode::Element(el) if el.tag == "form")
    });
    assert!(form.is_some(), "TodosPage should contain form element");

    // Verify ForEach node exists for the todo list
    let foreach_node = hir.nodes.iter().find(|n| matches!(n, HirNode::ForEach(_)));
    assert!(foreach_node.is_some(), "TodosPage should contain ForEach node for todo list");

    if let HirNode::ForEach(foreach) = foreach_node.unwrap() {
        assert_eq!(foreach.item_param, "todo", "ForEach should bind 'todo' parameter");
        assert!(!foreach.body.is_empty(), "ForEach body should not be empty");
        let identity = foreach.identity.expect("ForEach should preserve the todo key");
        assert!(matches!(
            hir.expressions.iter().find(|expression| expression.id == identity).map(|expression| &expression.expression),
            Some(HirExpr::Member { property, .. }) if property == "id"
        ));
    }

    // Count Empty nodes - should be minimal (only for legitimate absences like null/false)
    let empty_count = hir.nodes.iter().filter(|n| matches!(n, HirNode::Empty)).count();
    assert!(empty_count <= 1, "Should have at most 1 Empty node (for component root), found {empty_count}");

    // Verify conditional nodes exist (for the conditional branches in the UI)
    let conditional_count = hir.nodes.iter().filter(|n| matches!(n, HirNode::Conditional(_))).count();
    assert!(conditional_count >= 2, "Should have at least 2 conditional nodes (openCount, visibleTodos.length)");

    // Verify event bindings on form element
    if let HirNode::Element(el) = form.unwrap() {
        assert!(!el.events.is_empty(), "Form should have event bindings");
        let submit_event = el.events.iter().find(|e| e.event == "submit");
        assert!(submit_event.is_some(), "Form should have submit event binding");
    }

    // Verify TodoRow component props are NOT events
    let todo_row = hir.nodes.iter().find(|n| {
        matches!(n, HirNode::Component(comp) if comp.name == "TodoRow")
    });
    assert!(todo_row.is_some(), "Should have TodoRow component");

    if let HirNode::Component(comp) = todo_row.unwrap() {
        // Component props, NOT events
        assert!(!comp.props.is_empty(), "TodoRow should have props");

        assert!(
            !comp.props.iter().any(|prop| matches!(prop, HirProp::Static { name, .. } | HirProp::Expression { name, .. } | HirProp::Callable { name, .. } if name == "key")),
            "TodoRow key should be structural identity, not a component prop"
        );

        // Find callable and value props.
        let on_save = comp.props.iter().find(|p| {
            matches!(p, HirProp::Callable { name, .. } | HirProp::Expression { name, .. } if name == "onSave")
        });
        assert!(on_save.is_some(), "TodoRow should have onSave prop");

        let on_edit_title = comp.props.iter().find(|p| matches!(p, HirProp::Callable { name, .. } if name == "onEditTitle"));
        assert!(on_edit_title.is_some(), "TodoRow should have onEditTitle prop");

        // Verify onSave is Callable (inline arrow) not Expression
        if let Some(HirProp::Callable { .. }) = on_save {
            // Good - inline arrow is recognized as callable
        } else {
            panic!("onSave should be a Callable prop (inline arrow), not Expression");
        }
        assert!(matches!(on_edit_title, Some(HirProp::Callable { callable: HirCallable::Reference { .. }, .. })));
        for name in ["todo", "editing", "editingTitle", "pending"] {
            assert!(
                comp.props.iter().any(|prop| matches!(prop, HirProp::Expression { name: prop_name, .. } if prop_name == name)),
                "TodoRow {name} should be a value expression"
            );
        }
    }

    // Verify input elements have DOM event bindings
    let input_count = hir.nodes.iter().filter(|n| {
        matches!(n, HirNode::Element(el) if el.tag == "input")
    }).count();
    assert!(input_count > 0, "Should have input elements");

    // At least one input should have an event binding
    let input_with_events = hir.nodes.iter().find(|n| {
        matches!(n, HirNode::Element(el) if el.tag == "input" && !el.events.is_empty())
    });
    assert!(input_with_events.is_some(), "At least one input should have event bindings");
}
