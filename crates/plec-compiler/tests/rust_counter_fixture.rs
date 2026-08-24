use std::fs;

use plec_compiler::{
    discover_root_component, lower_application, lower_application_to_executable,
    lower_component_to_executable, lower_root_component,
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

const COMPONENT_SOURCE: &str = r#"
    export function App() {
        const [name, setName] = useState("one");
        return <div><Child title={name} /><button onClick={() => setName("two")} /></div>;
    }
    function Child({ title }) {
        return <span>{title}</span>;
    }
"#;

const KEYED_COMPONENT_SOURCE: &str = r#"
    export function Todos() {
        const todos = useCollection("items");
        return <ul>{todos.map(todo => <Child key={todo.id} title={todo.title} />)}</ul>;
    }
    function Child({ title }) {
        const [clicks, setClicks] = useState(0);
        return <li><span>{title}</span><button onClick={() => setClicks(clicks + 1)}>{clicks}</button></li>;
    }
"#;

const NESTED_COMPONENT_SOURCE: &str = r#"
    export function App() {
        const [title, setTitle] = useState("one");
        return <main><Child title={title} /><button onClick={() => setTitle("two")} /></main>;
    }
    function Child({ title }) {
        return <section><Grandchild title={title} /></section>;
    }
    function Grandchild({ title }) {
        const [clicks, setClicks] = useState(0);
        return <div><span>{title}</span><button onClick={() => setClicks(clicks + 1)}>{clicks}</button></div>;
    }
"#;

const KEYED_CALLBACK_COMPONENT_SOURCE: &str = r#"
    export function Todos() {
        const todos = useCollection("items");
        const [selected, setSelected] = useState("");
        return <main><p>{selected}</p><ul>{todos.map(todo => <Child key={todo.id} onPick={() => setSelected(todo.title)} />)}</ul></main>;
    }
    function Child({ onPick }: { onPick: () => void }) {
        return <button onClick={onPick}>Pick</button>;
    }
"#;

const KEYED_SLOT_COMPONENT_SOURCE: &str = r#"
    export function Todos() {
        const todos = useCollection("items");
        const [selected, setSelected] = useState("");
        return <main><p>{selected}</p><ul>{todos.map(todo => <Frame key={todo.id}><button onClick={() => setSelected(todo.title)}>{todo.title}</button>{todo.done ? <strong>Done</strong> : <em>Open</em>}</Frame>)}</ul></main>;
    }
    function Frame({ children }) {
        return <li>{children}</li>;
    }
"#;

#[test]
fn rust_counter_artifact_matches_runtime_fixture() {
    assert_fixture(
        "rust-counter.tsx",
        "Counter",
        SOURCE,
        "rust-counter-0.9.json",
    );
}

#[test]
fn rust_collection_rows_artifact_matches_runtime_fixture() {
    assert_fixture(
        "rust-collection-rows.tsx",
        "Todos",
        COLLECTION_SOURCE,
        "rust-collection-rows-0.9.json",
    );
}

#[test]
fn rust_static_conditional_artifact_matches_runtime_fixture() {
    assert_fixture(
        "rust-static-conditional.tsx",
        "Conditional",
        CONDITIONAL_SOURCE,
        "rust-static-conditional-0.9.json",
    );
}

#[test]
fn rust_component_artifact_matches_runtime_fixture() {
    let module = parse_module("rust-component.tsx", COMPONENT_SOURCE).expect("source should parse");
    let modules = vec![module];
    let semantic_graph =
        build_semantic_graph(&modules, &Default::default()).expect("semantic graph should build");
    let root =
        discover_root_component(&modules, &semantic_graph, "rust-component.tsx", Some("App"))
            .expect("App should be discovered");
    let application =
        lower_application(&modules, &root, &semantic_graph).expect("App should lower to HIR");
    let executable =
        lower_application_to_executable(&application).expect("App should lower to component IR");
    let fixture = fs::read_to_string(format!(
        "{}/../../packages/plec-runtime/crates/runtime/tests/fixtures/rust-component-0.10.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("runtime fixture should exist");

    assert_eq!(
        serde_json::to_value(executable).unwrap(),
        serde_json::from_str::<serde_json::Value>(&fixture).unwrap()
    );
}

#[test]
fn rust_keyed_component_artifact_matches_runtime_fixture() {
    let module = parse_module("rust-keyed-component.tsx", KEYED_COMPONENT_SOURCE)
        .expect("source should parse");
    let modules = vec![module];
    let semantic_graph =
        build_semantic_graph(&modules, &Default::default()).expect("semantic graph should build");
    let root = discover_root_component(
        &modules,
        &semantic_graph,
        "rust-keyed-component.tsx",
        Some("Todos"),
    )
    .expect("Todos should be discovered");
    let application =
        lower_application(&modules, &root, &semantic_graph).expect("Todos should lower to HIR");
    let executable =
        lower_application_to_executable(&application).expect("Todos should lower to component IR");
    let fixture = fs::read_to_string(format!(
        "{}/../../packages/plec-runtime/crates/runtime/tests/fixtures/rust-keyed-component-0.10.json",
        env!("CARGO_MANIFEST_DIR")
    )).expect("runtime fixture should exist");

    assert!(executable.components[0]
        .dependency_edges
        .iter()
        .any(|edge| edge.source.kind == "rowField" && edge.target.kind == "component"));
    assert!(executable.components[1]
        .dependency_edges
        .iter()
        .any(|edge| edge.source.kind == "prop" && edge.target.kind == "binding"));
    assert_eq!(
        serde_json::to_value(executable).unwrap(),
        serde_json::from_str::<serde_json::Value>(&fixture).unwrap()
    );
}

#[test]
fn rust_nested_component_artifact_matches_runtime_fixture() {
    let module = parse_module("rust-nested-component.tsx", NESTED_COMPONENT_SOURCE)
        .expect("source should parse");
    let modules = vec![module];
    let semantic_graph =
        build_semantic_graph(&modules, &Default::default()).expect("semantic graph should build");
    let root = discover_root_component(
        &modules,
        &semantic_graph,
        "rust-nested-component.tsx",
        Some("App"),
    )
    .expect("App should be discovered");
    let application =
        lower_application(&modules, &root, &semantic_graph).expect("App should lower to HIR");
    let executable =
        lower_application_to_executable(&application).expect("App should lower to component IR");
    let fixture = fs::read_to_string(format!(
        "{}/../../packages/plec-runtime/crates/runtime/tests/fixtures/rust-nested-component-0.10.json",
        env!("CARGO_MANIFEST_DIR")
    )).expect("runtime fixture should exist");

    assert_eq!(executable.components.len(), 3);
    assert!(executable.components[0]
        .dependency_edges
        .iter()
        .any(|edge| edge.source.kind == "state" && edge.target.kind == "component"));
    assert!(executable.components[1]
        .dependency_edges
        .iter()
        .any(|edge| edge.source.kind == "prop" && edge.target.kind == "component"));
    assert!(executable.components[2]
        .dependency_edges
        .iter()
        .any(|edge| edge.source.kind == "prop" && edge.target.kind == "binding"));
    assert_eq!(
        serde_json::to_value(executable).unwrap(),
        serde_json::from_str::<serde_json::Value>(&fixture).unwrap()
    );
}

#[test]
fn rust_keyed_callback_component_artifact_matches_runtime_fixture() {
    let module = parse_module(
        "rust-keyed-callback-component.tsx",
        KEYED_CALLBACK_COMPONENT_SOURCE,
    )
    .expect("source should parse");
    let modules = vec![module];
    let semantic_graph =
        build_semantic_graph(&modules, &Default::default()).expect("semantic graph should build");
    let root = discover_root_component(
        &modules,
        &semantic_graph,
        "rust-keyed-callback-component.tsx",
        Some("Todos"),
    )
    .expect("Todos should be discovered");
    let application =
        lower_application(&modules, &root, &semantic_graph).expect("Todos should lower to HIR");
    let executable =
        lower_application_to_executable(&application).expect("Todos should lower to component IR");
    let fixture = fs::read_to_string(format!(
        "{}/../../packages/plec-runtime/crates/runtime/tests/fixtures/rust-keyed-callback-component-0.10.json",
        env!("CARGO_MANIFEST_DIR")
    )).expect("runtime fixture should exist");
    assert_eq!(
        serde_json::to_value(executable).unwrap(),
        serde_json::from_str::<serde_json::Value>(&fixture).unwrap()
    );
}

#[test]
fn rust_keyed_slot_component_artifact_matches_runtime_fixture() {
    let module = parse_module("rust-keyed-slot-component.tsx", KEYED_SLOT_COMPONENT_SOURCE)
        .expect("source should parse");
    let modules = vec![module];
    let graph =
        build_semantic_graph(&modules, &Default::default()).expect("semantic graph should build");
    let root = discover_root_component(
        &modules,
        &graph,
        "rust-keyed-slot-component.tsx",
        Some("Todos"),
    )
    .expect("Todos should be discovered");
    let application =
        lower_application(&modules, &root, &graph).expect("Todos should lower to HIR");
    let executable =
        lower_application_to_executable(&application).expect("Todos should lower to component IR");
    assert!(executable.components[0]
        .dependency_edges
        .iter()
        .any(|edge| edge.source.kind == "rowField" && edge.target.kind == "binding"));
    assert!(executable.components[1]
        .nodes
        .iter()
        .any(|node| matches!(node, plec_ir::Node::Slot { .. })));
    let fixture = fs::read_to_string(format!(
        "{}/../../packages/plec-runtime/crates/runtime/tests/fixtures/rust-keyed-slot-component-0.10.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("runtime fixture should exist");
    assert_eq!(
        serde_json::to_value(executable).unwrap(),
        serde_json::from_str::<serde_json::Value>(&fixture).unwrap()
    );
}

fn assert_fixture(path: &str, component: &str, source: &str, fixture_name: &str) {
    let module = parse_module(path, source).expect("source should parse");
    let modules = vec![module];
    let semantic_graph =
        build_semantic_graph(&modules, &Default::default()).expect("semantic graph should build");
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
