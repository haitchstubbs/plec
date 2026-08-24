use std::fs;

use plec_compiler::{
    discover_root_component, lower_application, lower_application_to_executable,
    lower_component_to_executable, lower_root_component, lower_route_loader_to_executable,
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
        return <main><p>{selected}</p><ul>{todos.map(todo => <Child key={todo.id} title={todo.title} onPick={(title) => setSelected(title)} />)}</ul></main>;
    }
    function Child({ title, onPick }: { title: string, onPick: (title: string) => void }) {
        return <button onClick={() => onPick(title)}>Pick</button>;
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

const LOCAL_ACTION_COMPONENT_SOURCE: &str = r#"
    export function Counter() {
        const [count, setCount] = useState(0);
        const incrementBy = (step) => setCount(count + step);
        return <button onClick={() => incrementBy(1)}>{count}</button>;
    }
"#;

const KEYED_LOCAL_ACTION_COMPONENT_SOURCE: &str = r#"
    export function Todos() {
        const todos = useCollection("items");
        const [selected, setSelected] = useState("");
        const select = (title) => setSelected(title);
        return <main><p>{selected}</p><ul>{todos.map(todo => <li key={todo.id}><button onClick={() => select(todo.title)}>{todo.title}</button></li>)}</ul></main>;
    }
"#;

const COLLECTION_MUTATION_COMPONENT_SOURCE: &str = r#"
    export function Todos() {
        const todos = useCollection("items");
        return <main><button onClick={() => todos.append("one", { id: "one", title: "One", done: false })}>Add</button><ul>{todos.map(todo => <li key={todo.id}><span>{todo.title}</span><button onClick={() => todos.keyedReplace(todo.id, { id: todo.id, title: "Two", done: true })}>Replace</button><button onClick={() => todos.keyedRemove(todo.id)}>Remove</button></li>)}</ul></main>;
    }
"#;

const ROUTE_ASYNC_SOURCE: &str = r#"
    export function RoutePage() {
        const [result, setResult] = useState("");
        const loader = async () => { return await fetch("/route-data"); };
        return <main>{result}</main>;
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

#[test]
fn rust_local_action_artifact_matches_runtime_fixture() {
    assert_component_fixture(
        "rust-local-action.tsx",
        "Counter",
        LOCAL_ACTION_COMPONENT_SOURCE,
        "rust-local-action-0.10.json",
    );
}

#[test]
fn rust_keyed_local_action_artifact_matches_runtime_fixture() {
    assert_component_fixture(
        "rust-keyed-local-action.tsx",
        "Todos",
        KEYED_LOCAL_ACTION_COMPONENT_SOURCE,
        "rust-keyed-local-action-0.10.json",
    );
}

#[test]
fn rust_collection_mutation_artifact_matches_runtime_fixture() {
    assert_component_fixture(
        "rust-collection-mutation.tsx",
        "Todos",
        COLLECTION_MUTATION_COMPONENT_SOURCE,
        "rust-collection-mutation-0.10.json",
    );
}

#[test]
fn rust_route_async_artifact_matches_runtime_fixture() {
    let module = parse_module("rust-route-async.tsx", ROUTE_ASYNC_SOURCE)
        .expect("route source should parse");
    let modules = vec![module];
    let graph = build_semantic_graph(&modules, &Default::default())
        .expect("route source should build a semantic graph");
    let root = discover_root_component(&modules, &graph, "rust-route-async.tsx", Some("RoutePage"))
        .expect("route page should be discovered");
    let hir = lower_root_component(&root, &graph).expect("route page should lower to HIR");
    let executable = lower_route_loader_to_executable(&hir, "loader", "result", "main")
        .expect("route loader should lower to executable IR");
    let loader = executable.actions.first().expect("loader action should be first");
    assert!(loader.route_loader);
    assert_eq!(loader.loader_result_state, Some(0));
    assert_eq!(loader.frame_slots, 2);
    assert!(matches!(loader.instructions.first(), Some(plec_ir::ActionInstruction::CapabilityRequest {
        request: plec_ir::CapabilityRequest::Fetch { method: "GET", decode: "text", require_ok: true, .. },
        success_pc: 1, failure_pc: 1, result_slot: 0, error_slot: 1,
    })));
    assert_eq!(executable.route_outlets[0].id, "main");
    assert_eq!(executable.route_outlets[0].node, executable.root_node);
    let fixture = fs::read_to_string(format!(
        "{}/../../packages/plec-runtime/crates/runtime/tests/fixtures/rust-route-async-0.9.json",
        env!("CARGO_MANIFEST_DIR")
    )).expect("runtime fixture should exist");
    assert_eq!(
        serde_json::to_value(executable).unwrap(),
        serde_json::from_str::<serde_json::Value>(&fixture).unwrap()
    );
}

fn assert_component_fixture(path: &str, component: &str, source: &str, fixture_name: &str) {
    let module = parse_module(path, source).expect("source should parse");
    let modules = vec![module];
    let graph =
        build_semantic_graph(&modules, &Default::default()).expect("semantic graph should build");
    let root = discover_root_component(&modules, &graph, path, Some(component))
        .expect("root should be discovered");
    let application = lower_application(&modules, &root, &graph).expect("root should lower to HIR");
    let executable =
        lower_application_to_executable(&application).expect("HIR should lower to component IR");
    let fixture = fs::read_to_string(format!(
        "{}/../../packages/plec-runtime/crates/runtime/tests/fixtures/{fixture_name}",
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
