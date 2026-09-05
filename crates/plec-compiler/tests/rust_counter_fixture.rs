use std::{collections::HashMap, fs};

use plec_compiler::{
    discover_root_component, lower_application, lower_application_to_executable,
    lower_component_to_executable, lower_root_component, lower_route_loader_to_executable,
};
use plec_model::build_semantic_graph;
use plec_parser::parse_module;

const SOURCE: &str = r#"
    export function Counter() {
        const [count, setCount] = useState(0);
        return <button onClick={() => setCount(count + 1)}>{count}</button>;
    }
"#;

const REF_SOURCE: &str = r#"
    export function RefCounter() {
        const count = useRef(1);
        const panel = useHostRef();
        return <div ref={panel}><button onClick={() => count.current = count.current + 1}>{count.current}</button></div>;
    }
"#;

const REACTION_SOURCE: &str = r#"
    export function Reactions() {
        const [open, setOpen] = useState(false);
        const panel = useHostRef();
        const previous = useRef(null);
        useReaction(() => { if (open) { previous.current = document.activeElement; panel.current.focus(); } else { previous.current.focus(); } }, [open]);
        return <button ref={panel} onClick={() => setOpen(!open)}>Toggle</button>;
    }
"#;

const SVG_LIBRARY_SOURCE: &str = r#"
    export const Mark = (props: Record<string, unknown>) => (
      <svg viewBox="0 0 24 24" stroke-width="2" {...props}>
        <path d="M2 2h20" />
      </svg>
    );
"#;

const DYNAMIC_SVG_COMPONENT_SOURCE: &str = r#"
    import { Mark } from "@scope/icons/mark";
    export function App() { return <Frame Icon={Mark} />; }
    function Frame({ Icon }) { return <Icon className="size-4 shrink-0" />; }
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

const CONDITIONAL_ACTION_SOURCE: &str = r#"
    export function ConditionalAction() {
        const [enabled, setEnabled] = useState(false);
        return <button onClick={enabled ? () => setEnabled(false) : () => setEnabled(true)}>Toggle</button>;
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

const GENERAL_ASYNC_ACTION_SOURCE: &str = r#"
    export function Todos() {
        const todos = useCollection("items");
        const [error, setError] = useState("");
        const [done, setDone] = useState(false);
        async function refresh() {
            try {
                const todo = await fetch("/todo");
                todos.keyedReplace("one", todo);
            } catch (reason) {
                setError(reason.message);
            } finally {
                setDone(true);
            }
        }
        return <main><button onClick={refresh}>Refresh</button><p>{error}</p><p>{done}</p><ul>{todos.map(todo => <li key={todo.id}>{todo.title}</li>)}</ul></main>;
    }
"#;

const ASYNC_CALLABLE_PARAMETER_SOURCE: &str = r#"
    export function App() {
        async function request(operation: string, action: () => Promise<Response>) {
            try {
                return await action();
            } catch (reason) {
                throw reason;
            }
        }
        return <button>Save</button>;
    }
"#;

const COOKIE_ACTION_SOURCE: &str = r#"
    export function CookieActions() {
        const [value, setValue] = useState("");
        async function save() {
            const current = await cookie.get('sidebar', { path: '/', sameSite: 'lax', secure: true });
            await cookie.set('sidebar', value, { path: '/', sameSite: 'lax', secure: true, maxAge: 60 });
            void cookie.delete('old_sidebar', { path: '/' });
            setValue(current);
        }
        return <button onClick={save}>{value}</button>;
    }
"#;

#[test]
fn rust_counter_artifact_matches_runtime_fixture() {
    assert_fixture(
        "rust-counter.tsx",
        "Counter",
        SOURCE,
        "rust-counter-0.10.json",
    );
}

#[test]
fn rust_collection_rows_artifact_matches_runtime_fixture() {
    assert_fixture(
        "rust-collection-rows.tsx",
        "Todos",
        COLLECTION_SOURCE,
        "rust-collection-rows-0.10.json",
    );
}

#[test]
fn rust_static_conditional_artifact_matches_runtime_fixture() {
    assert_fixture(
        "rust-static-conditional.tsx",
        "Conditional",
        CONDITIONAL_SOURCE,
        "rust-static-conditional-0.10.json",
    );
}

#[test]
fn rust_conditional_action_jumps_to_a_valid_return_instruction() {
    let module = parse_module("rust-conditional-action.tsx", CONDITIONAL_ACTION_SOURCE).unwrap();
    let modules = vec![module];
    let graph = build_semantic_graph(&modules, &Default::default()).unwrap();
    let root = discover_root_component(
        &modules,
        &graph,
        "rust-conditional-action.tsx",
        Some("ConditionalAction"),
    )
    .unwrap();
    let hir = lower_root_component(&root, &graph).unwrap();
    let app = lower_component_to_executable(&hir).unwrap();
    let conditional = app
        .actions
        .iter()
        .find(|action| {
            action.instructions.iter().any(|instruction| {
                matches!(instruction, plec_ir::ActionInstruction::JumpIfFalse { .. })
            })
        })
        .expect("conditional handler should lower to an action");
    assert!(conditional
        .instructions
        .iter()
        .all(|instruction| match instruction {
            plec_ir::ActionInstruction::Jump { target }
            | plec_ir::ActionInstruction::JumpIfFalse { target } => {
                *target < conditional.instructions.len()
            }
            _ => true,
        }));
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
        "{}/../plec-runtime/tests/fixtures/rust-component-0.10.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("runtime fixture should exist");

    assert_eq!(
        serde_json::to_value(executable).unwrap(),
        serde_json::from_str::<serde_json::Value>(&fixture).expect("fixture JSON should parse")
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
        "{}/../plec-runtime/tests/fixtures/rust-keyed-component-0.10.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("runtime fixture should exist");

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
        serde_json::from_str::<serde_json::Value>(&fixture).expect("fixture JSON should parse")
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
        "{}/../plec-runtime/tests/fixtures/rust-nested-component-0.10.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("runtime fixture should exist");

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
        serde_json::from_str::<serde_json::Value>(&fixture).expect("fixture JSON should parse")
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
        "{}/../plec-runtime/tests/fixtures/rust-keyed-callback-component-0.10.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("runtime fixture should exist");
    assert_eq!(
        serde_json::to_value(executable).unwrap(),
        serde_json::from_str::<serde_json::Value>(&fixture).expect("fixture JSON should parse")
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
        "{}/../plec-runtime/tests/fixtures/rust-keyed-slot-component-0.10.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("runtime fixture should exist");
    assert_eq!(
        serde_json::to_value(executable).unwrap(),
        serde_json::from_str::<serde_json::Value>(&fixture).expect("fixture JSON should parse")
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
    let loader = executable
        .actions
        .first()
        .expect("loader action should be first");
    assert!(loader.route_loader);
    assert_eq!(loader.loader_result_state, Some(0));
    assert_eq!(loader.frame_slots, 2);
    assert!(matches!(
        loader.instructions.first(),
        Some(plec_ir::ActionInstruction::CapabilityRequest {
            request: plec_ir::CapabilityRequest::Fetch {
                method: "GET",
                decode: "text",
                require_ok: true,
                ..
            },
            success_pc: 1,
            failure_pc: 2,
            result_slot: 0,
            error_slot: 1,
            ..
        })
    ));
    assert_eq!(executable.route_outlets[0].id, "main");
    assert_eq!(executable.route_outlets[0].node, executable.root_node);
    let fixture = fs::read_to_string(format!(
        "{}/../plec-runtime/tests/fixtures/rust-route-async-0.10.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("runtime fixture should exist");
    assert_eq!(
        serde_json::to_value(executable).unwrap(),
        serde_json::from_str::<serde_json::Value>(&fixture).expect("fixture JSON should parse")
    );
}

#[test]
fn rust_general_async_actions_artifact_matches_runtime_fixture() {
    assert_fixture(
        "rust-general-async-actions.tsx",
        "Todos",
        GENERAL_ASYNC_ACTION_SOURCE,
        "rust-general-async-actions-0.10.json",
    );
}

#[test]
fn rust_async_callable_parameter_uses_frame_slot_and_continuations() {
    let module = parse_module(
        "rust-async-callable-parameter.tsx",
        ASYNC_CALLABLE_PARAMETER_SOURCE,
    )
    .unwrap();
    let modules = vec![module];
    let graph = build_semantic_graph(&modules, &Default::default()).unwrap();
    let root = discover_root_component(
        &modules,
        &graph,
        "rust-async-callable-parameter.tsx",
        Some("App"),
    )
    .unwrap();
    let hir = lower_root_component(&root, &graph).unwrap();
    let app = lower_component_to_executable(&hir).unwrap();
    let request = app
        .actions
        .iter()
        .find(|action| action.parameter_slots.len() == 2)
        .expect("request action should have operation and callable parameters");
    let call_frame = request
        .instructions
        .iter()
        .find_map(|instruction| match instruction {
            plec_ir::ActionInstruction::CallFrame {
                parameter,
                success_pc,
                failure_pc,
                result_slot,
                error_slot,
                ..
            } => Some((parameter, success_pc, failure_pc, result_slot, error_slot)),
            _ => None,
        })
        .expect("awaited callable parameter should lower to callFrame");
    assert_eq!(*call_frame.0, 1);
    assert!(call_frame.1.is_some());
    assert!(call_frame.2.is_some());
    assert!(call_frame.3.is_some());
    assert!(call_frame.4.is_some());
}

#[test]
fn rust_cookie_actions_lower_to_declared_capability_requests() {
    let module = parse_module("rust-cookie-actions.tsx", COOKIE_ACTION_SOURCE).unwrap();
    let modules = vec![module];
    let graph = build_semantic_graph(&modules, &Default::default()).unwrap();
    let root = discover_root_component(
        &modules,
        &graph,
        "rust-cookie-actions.tsx",
        Some("CookieActions"),
    )
    .unwrap();
    let hir = lower_root_component(&root, &graph).unwrap();
    let executable = lower_component_to_executable(&hir).unwrap();
    assert_eq!(executable.capabilities.len(), 2);
    let sidebar = executable
        .capabilities
        .iter()
        .find(|capability| capability.name == "sidebar")
        .unwrap();
    assert_eq!(sidebar.operations, vec!["get", "set"]);
    assert_eq!(sidebar.path, "/");
    assert_eq!(sidebar.same_site.as_deref(), Some("lax"));
    assert_eq!(sidebar.secure, Some(true));
    assert_eq!(sidebar.expiry_modes, vec!["session", "maxAge"]);
    assert!(executable
        .actions
        .iter()
        .flat_map(|action| &action.instructions)
        .any(|instruction| matches!(
            instruction,
            plec_ir::ActionInstruction::CapabilityRequest {
                request: plec_ir::CapabilityRequest::Cookie {
                    operation: "set",
                    max_age: Some(60),
                    ..
                },
                ..
            }
        )));
    let serialized = serde_json::to_value(executable).unwrap();
    let cookie_request = serialized["actions"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|action| action["instructions"].as_array().unwrap())
        .find(|instruction| {
            instruction["capability"] == "cookie" && instruction["request"]["operation"] == "set"
        })
        .unwrap();
    assert_eq!(cookie_request["request"]["maxAge"], 60);
    assert!(cookie_request["request"].get("max_age").is_none());
}

#[test]
fn rust_refs_lower_to_non_reactive_slots_and_explicit_host_attachment() {
    let modules = vec![parse_module("refs.tsx", REF_SOURCE).unwrap()];
    let semantic = build_semantic_graph(&modules, &Default::default()).unwrap();
    let root =
        discover_root_component(&modules, &semantic, "refs.tsx", Some("RefCounter")).unwrap();
    let hir = lower_root_component(&root, &semantic).unwrap();
    let app = lower_component_to_executable(&hir).unwrap();
    assert_eq!(app.ref_slots.len(), 1);
    assert_eq!(app.host_refs.len(), 1);
    assert!(matches!(
        app.nodes[0],
        plec_ir::Node::Element {
            host_ref: Some(0),
            ..
        }
    ));
    assert!(app.dependency_edges.is_empty());
    assert!(app
        .actions
        .iter()
        .flat_map(|action| &action.instructions)
        .any(|instruction| matches!(
            instruction,
            plec_ir::ActionInstruction::StoreRef { reference: 0 }
        )));
}

#[test]
fn rust_reactions_lower_to_edges_and_opaque_focus_operations() {
    let modules = vec![parse_module("reactions.tsx", REACTION_SOURCE).unwrap()];
    let semantic = build_semantic_graph(&modules, &Default::default()).unwrap();
    let root =
        discover_root_component(&modules, &semantic, "reactions.tsx", Some("Reactions")).unwrap();
    let hir = lower_root_component(&root, &semantic).unwrap();
    let app = lower_component_to_executable(&hir).unwrap();
    assert_eq!(app.reactions.len(), 1);
    assert!(app
        .dependency_edges
        .iter()
        .any(|edge| edge.target.kind == "reaction"));
    assert!(app
        .actions
        .iter()
        .flat_map(|action| &action.instructions)
        .any(|instruction| matches!(
            instruction,
            plec_ir::ActionInstruction::CaptureActiveElement { .. }
                | plec_ir::ActionInstruction::FocusHostRef { .. }
                | plec_ir::ActionInstruction::FocusRef { .. }
        )));
}

#[test]
fn rust_svg_library_component_uses_namespace_and_explicit_props_spread() {
    let modules = vec![
        parse_module("src/app.tsx", r#"import { Mark } from "@scope/icons/mark"; export function App() { return <Mark className="size-4" />; }"#).unwrap(),
        parse_module("packages/icons/src/mark.tsx", SVG_LIBRARY_SOURCE).unwrap(),
    ];
    let imports = HashMap::from([(
        ("src/app.tsx".into(), "@scope/icons/mark".into()),
        "packages/icons/src/mark.tsx".into(),
    )]);
    let semantic = build_semantic_graph(&modules, &imports).unwrap();
    let root = discover_root_component(&modules, &semantic, "src/app.tsx", Some("App")).unwrap();
    let hir = lower_application(&modules, &root, &semantic).unwrap();
    let app = lower_application_to_executable(&hir).unwrap();
    let mark = app
        .components
        .iter()
        .find(|component| component.id.ends_with("#Mark"))
        .unwrap();
    assert!(matches!(
        mark.nodes[0],
        plec_ir::Node::Element {
            namespace: "svg",
            ..
        }
    ));
    assert!(mark
        .prop_programs
        .iter()
        .flat_map(|program| &program.writes)
        .any(|write| write.spread));
    assert!(mark.strings.iter().any(|value| value == "stroke-width"));
}

#[test]
fn rust_dynamic_component_preserves_value_props_for_direct_props_targets() {
    let modules = vec![
        parse_module("src/app.tsx", DYNAMIC_SVG_COMPONENT_SOURCE).unwrap(),
        parse_module("packages/icons/src/mark.tsx", SVG_LIBRARY_SOURCE).unwrap(),
    ];
    let imports = HashMap::from([(
        ("src/app.tsx".into(), "@scope/icons/mark".into()),
        "packages/icons/src/mark.tsx".into(),
    )]);
    let semantic = build_semantic_graph(&modules, &imports).unwrap();
    let root = discover_root_component(&modules, &semantic, "src/app.tsx", Some("App")).unwrap();
    let hir = lower_application(&modules, &root, &semantic).unwrap();
    let app = lower_application_to_executable(&hir).unwrap();
    let frame = app
        .components
        .iter()
        .find(|component| component.id.ends_with("#Frame"))
        .unwrap();
    let dynamic_props = frame
        .nodes
        .iter()
        .find_map(|node| match node {
            plec_ir::Node::DynamicComponent { props, .. } => Some(props),
            _ => None,
        })
        .expect("Frame should render its component prop dynamically");
    assert!(dynamic_props.iter().any(|prop| matches!(prop,
        plec_ir::ComponentProp::Value { name, .. } if frame.strings[*name] == "className"
    )));
    let mark = app
        .components
        .iter()
        .find(|component| component.id.ends_with("#Mark"))
        .unwrap();
    assert!(mark
        .parameters
        .iter()
        .any(|parameter| mark.strings[parameter.name] == "__plec_props"));
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
        "{}/../plec-runtime/tests/fixtures/{fixture_name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("runtime fixture should exist");
    assert_eq!(
        serde_json::to_value(executable).unwrap(),
        serde_json::from_str::<serde_json::Value>(&fixture).expect("fixture JSON should parse")
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
        "{}/../plec-runtime/tests/fixtures/{fixture_name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("runtime fixture should exist");

    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
        serde_json::from_str::<serde_json::Value>(&fixture).expect("fixture JSON should parse")
    );
}
