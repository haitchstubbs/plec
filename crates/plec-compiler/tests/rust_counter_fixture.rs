use std::fs;

use plec_compiler::{
    discover_root_component, lower_root_component, lower_component_to_executable,
};
use plec_parser::parse_module;
use plec_sema::build_semantic_graph;

const SOURCE: &str = r#"
    export function Counter() {
        const [count, setCount] = useState(0);
        return <button onClick={() => setCount(count + 1)}>{count}</button>;
    }
"#;

const COLLECTION_SOURCE: &str = r#"
    export function Todos() {
        const todos = useCollection("items");
        const [selected, setSelected] = useState("");
        return <ul>{todos.map(todo => <li key={todo.id}>{todo.title}{todo.done && <button onClick={() => setSelected(todo.title)}>Done</button>}</li>)}</ul>;
    }
"#;

const CONDITIONAL_SOURCE: &str = r#"
    export function Conditional() {
        const [enabled, setEnabled] = useState(false);
        return <div>{enabled ? <button onClick={() => setEnabled(false)}>On</button> : <button onClick={() => setEnabled(true)}>Off</button>}</div>;
    }
"#;

#[test]
fn rust_counter_artifact_matches_runtime_fixture() {
    assert_fixture("rust-counter.tsx", "Counter", SOURCE, "rust-counter-0.9.json");
}

#[test]
fn rust_collection_rows_artifact_matches_runtime_fixture() {
    assert_fixture("rust-collection-rows.tsx", "Todos", COLLECTION_SOURCE, "rust-collection-rows-0.9.json");
}

#[test]
fn rust_static_conditional_artifact_matches_runtime_fixture() {
    assert_fixture("rust-static-conditional.tsx", "Conditional", CONDITIONAL_SOURCE, "rust-static-conditional-0.9.json");
}

fn assert_fixture(path: &str, component: &str, source: &str, fixture_name: &str) {
    let module = parse_module(path, source).expect("source should parse");
    let modules = vec![module];
    let semantic_graph = build_semantic_graph(&modules, &Default::default())
        .expect("semantic graph should build");
    let root = discover_root_component(&modules, &semantic_graph, path, Some(component))
        .expect("Counter should be discovered");
    let hir = lower_root_component(&root, &semantic_graph).expect("Counter should lower to HIR");
    let application = lower_component_to_executable(&hir).expect("Counter should lower to IR");
    let actual = serde_json::to_string_pretty(&application).expect("IR should serialize");
    let fixture = fs::read_to_string(format!(
        "{}/../../packages/plec-runtime/crates/runtime/tests/fixtures/{fixture_name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("runtime fixture should exist");

    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
        serde_json::from_str::<serde_json::Value>(&fixture).unwrap()
    );
}
