#![cfg(target_arch = "wasm32")]

use plec_runtime::PlecRuntime;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;
use web_sys::{Element, Event};

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen::prelude::wasm_bindgen(inline_js = r#"
let originalFetch;
let fetchQueue = [];
let aborts = 0;
export function setPlecFetchQueue(specs) {
  originalFetch ??= window.fetch;
  fetchQueue = JSON.parse(specs);
  aborts = 0;
  window.fetch = (_request) => {
    const spec = fetchQueue.shift();
    if (spec.reject) return Promise.reject(Object.assign(new Error(spec.reject), { name: spec.name || 'TypeError' }));
    if (spec.pending) return new Promise((_resolve, reject) => _request.signal.addEventListener('abort', () => { aborts++; reject(Object.assign(new Error('aborted'), { name: 'AbortError' })); }));
    return Promise.resolve(new Response(spec.body ?? '', { status: spec.status ?? 200, statusText: spec.statusText ?? '', headers: spec.headers ?? {} }));
  };
}
export function restorePlecFetch() { if (originalFetch) window.fetch = originalFetch; fetchQueue = []; }
export function plecFetchAborts() { return aborts; }
"#)]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = setPlecFetchQueue)]
    fn set_plec_fetch_queue(specs: &str);
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = restorePlecFetch)]
    fn restore_plec_fetch();
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = plecFetchAborts)]
    fn plec_fetch_aborts() -> u32;
}

fn mount_root() -> Element {
    web_sys::window()
        .unwrap()
        .document()
        .unwrap()
        .create_element("div")
        .unwrap()
}

fn load_and_mount(runtime: &PlecRuntime, artifact: serde_json::Value, root: &Element) {
    runtime
        .load_application(serde_wasm_bindgen::to_value(&artifact).unwrap())
        .unwrap();
    runtime.mount(root.clone()).unwrap();
}

fn rust_counter_artifact() -> serde_json::Value {
    serde_json::from_str(include_str!("fixtures/rust-counter-0.9.json"))
        .expect("Rust counter fixture should be valid JSON")
}

fn rust_collection_rows_artifact() -> serde_json::Value {
    serde_json::from_str(include_str!("fixtures/rust-collection-rows-0.9.json"))
        .expect("Rust collection fixture should be valid JSON")
}

fn rust_static_conditional_artifact() -> serde_json::Value {
    serde_json::from_str(include_str!("fixtures/rust-static-conditional-0.9.json"))
        .expect("Rust conditional fixture should be valid JSON")
}

fn rust_component_artifact() -> serde_json::Value {
    serde_json::from_str(include_str!("fixtures/rust-component-0.10.json"))
        .expect("Rust component fixture should be valid JSON")
}

fn rust_keyed_component_artifact() -> serde_json::Value {
    serde_json::from_str(include_str!("fixtures/rust-keyed-component-0.10.json"))
        .expect("Rust keyed component fixture should be valid JSON")
}

fn rust_nested_component_artifact() -> serde_json::Value {
    serde_json::from_str(include_str!("fixtures/rust-nested-component-0.10.json"))
        .expect("Rust nested component fixture should be valid JSON")
}

fn rust_keyed_callback_component_artifact() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "fixtures/rust-keyed-callback-component-0.10.json"
    ))
    .expect("Rust keyed callback fixture should be valid JSON")
}

fn rust_keyed_slot_component_artifact() -> serde_json::Value {
    serde_json::from_str(include_str!("fixtures/rust-keyed-slot-component-0.10.json"))
        .expect("Rust keyed slot fixture should be valid JSON")
}

fn rust_local_action_artifact() -> serde_json::Value {
    serde_json::from_str(include_str!("fixtures/rust-local-action-0.10.json"))
        .expect("Rust local-action fixture should be valid JSON")
}

fn rust_keyed_local_action_artifact() -> serde_json::Value {
    serde_json::from_str(include_str!("fixtures/rust-keyed-local-action-0.10.json"))
        .expect("Rust keyed local-action fixture should be valid JSON")
}

fn rust_collection_mutation_artifact() -> serde_json::Value {
    serde_json::from_str(include_str!("fixtures/rust-collection-mutation-0.10.json"))
        .expect("Rust collection mutation fixture should be valid JSON")
}

fn rust_route_async_artifact() -> serde_json::Value {
    serde_json::from_str(include_str!("fixtures/rust-route-async-0.9.json"))
        .expect("Rust route async fixture should be valid JSON")
}

fn component_slot_artifact() -> serde_json::Value {
    serde_json::json!({
        "version":"0.10", "rootComponent":0,
        "components":[
            {"id":"App","rootNode":0,"strings":["main","p"],"constants":[],
             "nodes":[
                {"op":"element","tag":0,"parent":null,"children":[3]},
                {"op":"element","tag":1,"parent":null,"children":[2]},
                {"op":"text","text":0,"parent":1},
                {"op":"component","component":1,"parent":0,"props":[],"children":[1]}
             ],"texts":[{"value":"Inside"}],"bindings":[],"propPrograms":[],"events":[],"inputs":[],"stateSlots":[],"parameters":[],"expressions":[],"actions":[],"loops":[],"dependencyEdges":[]},
            {"id":"Frame","rootNode":0,"strings":["section"],"constants":[],
             "nodes":[{"op":"element","tag":0,"parent":null,"children":[1]},{"op":"slot","parent":0}],
             "texts":[],"bindings":[],"propPrograms":[],"events":[],"inputs":[],"stateSlots":[],"parameters":[],"expressions":[],"actions":[],"loops":[],"dependencyEdges":[]}
        ]
    })
}

/// A minimal external keyed loop whose row button writes the row title to the
/// static output text. It exercises row-owned listener frames without relying
/// on any legacy runtime behaviour.
fn keyed_row_artifact() -> serde_json::Value {
    serde_json::json!({
        "version": "0.9", "rootNode": 0,
        "strings": ["div", "ul", "li", "button", "click", "items", "id", "title"],
        "constants": [[], null],
        "nodes": [
            {"op":"element", "tag":0, "children":[1, 3]},
            {"op":"element", "tag":1, "parent":0, "children":[2]},
            {"op":"loop", "loop":0, "parent":1},
            {"op":"text", "text":0, "parent":0},
            {"op":"element", "tag":2, "children":[5]},
            {"op":"element", "tag":3, "parent":4, "children":[6]},
            {"op":"text", "text":1, "parent":5}
        ],
        "texts": [{"binding":0}, {"binding":1}],
        "bindings": [
            {"target":3, "sink":"text", "expression":1},
            {"target":6, "sink":"text", "expression":4}
        ],
        "inputs": [{"name":5, "kind":"collection"}],
        "events": [{"target":5, "type":4, "action":0, "loop":0, "fields":[]}],
        "stateSlots": [{"initialExpression":0, "frameSlot":0}],
        "expressions": [
            {"instructions":[{"op":"constant","constant":1},{"op":"return"}]},
            {"instructions":[{"op":"loadState","state":0},{"op":"return"}]},
            {"instructions":[{"op":"constant","constant":0},{"op":"return"}]},
            {"instructions":[{"op":"loadRowField","field":6},{"op":"return"}]},
            {"instructions":[{"op":"loadRowField","field":7},{"op":"return"}]}
        ],
        "actions": [{"frameSlots":0, "instructions":[
            {"op":"evaluate","expression":4}, {"op":"storeState","state":0}, {"op":"return"}
        ]}],
        "loops": [{"sourceExpression":2,"keyExpression":3,"itemSlot":0,"rowTemplate":4,"input":0}],
        "dependencyEdges": [{"source":{"kind":"state","handle":0},"target":{"kind":"binding","handle":0}}]
    })
}

fn static_conditional_artifact(alternate: bool) -> serde_json::Value {
    let alternate = alternate.then_some(3);
    serde_json::json!({
        "version":"0.9", "rootNode":0,
        "strings":["div", "button", "click"], "constants":[false, true],
        "nodes":[
            {"op":"element", "tag":0, "children":[1]},
            {"op":"conditional", "test":2, "parent":0, "consequent":2, "alternate":alternate},
            {"op":"element", "tag":1, "children":[]},
            {"op":"element", "tag":1, "children":[]}
        ],
        "stateSlots":[{"initialExpression":0,"frameSlot":0}],
        "expressions":[
            {"instructions":[{"op":"constant","constant":0},{"op":"return"}]},
            {"instructions":[{"op":"constant","constant":1},{"op":"return"}]},
            {"instructions":[{"op":"loadState","state":0},{"op":"return"}]}
        ],
        "actions":[
            {"frameSlots":0,"instructions":[{"op":"evaluate","expression":1},{"op":"storeState","state":0},{"op":"return"}]},
            {"frameSlots":0,"instructions":[{"op":"evaluate","expression":0},{"op":"storeState","state":0},{"op":"return"}]}
        ],
        "events":[
            {"target":2,"type":2,"action":1,"fields":[]},
            {"target":3,"type":2,"action":0,"fields":[]}
        ],
        "dependencyEdges":[{"source":{"kind":"state","handle":0},"target":{"kind":"conditional","handle":1}}]
    })
}

fn row_conditional_artifact() -> serde_json::Value {
    let mut app = keyed_row_artifact();
    app["strings"] = serde_json::json!([
        "div", "ul", "li", "button", "click", "items", "id", "title", "enabled"
    ]);
    app["nodes"] = serde_json::json!([
        {"op":"element", "tag":0, "children":[1, 3]},
        {"op":"element", "tag":1, "parent":0, "children":[2]},
        {"op":"loop", "loop":0, "parent":1},
        {"op":"text", "text":0, "parent":0},
        {"op":"element", "tag":2, "children":[5]},
        {"op":"conditional", "test":5, "parent":4, "consequent":6, "alternate":null},
        {"op":"element", "tag":3, "parent":5, "children":[7]},
        {"op":"text", "text":1, "parent":6}
    ]);
    app["events"] = serde_json::json!([{"target":6,"type":4,"action":0,"loop":0,"fields":[]}]);
    app["expressions"] = serde_json::json!([
        {"instructions":[{"op":"constant","constant":1},{"op":"return"}]},
        {"instructions":[{"op":"loadState","state":0},{"op":"return"}]},
        {"instructions":[{"op":"constant","constant":0},{"op":"return"}]},
        {"instructions":[{"op":"loadRowField","field":6},{"op":"return"}]},
        {"instructions":[{"op":"loadRowField","field":7},{"op":"return"}]},
        {"instructions":[{"op":"loadRowField","field":8},{"op":"return"}]}
    ]);
    app
}

fn nested_row_conditional_artifact() -> serde_json::Value {
    serde_json::json!({
        "version":"0.9", "rootNode":0,
        "strings":["div", "ul", "li", "button", "click", "items", "id", "title", "enabled", "active", "span"],
        "constants":[[], null],
        "nodes":[
            {"op":"element", "tag":0, "children":[1,3]},
            {"op":"element", "tag":1, "parent":0, "children":[2]},
            {"op":"loop", "loop":0, "parent":1},
            {"op":"text", "text":0, "parent":0},
            {"op":"element", "tag":2, "children":[5]},
            {"op":"conditional", "test":5, "parent":4, "consequent":6, "alternate":null},
            {"op":"element", "tag":0, "parent":5, "children":[7]},
            {"op":"conditional", "test":6, "parent":6, "consequent":8, "alternate":10},
            {"op":"element", "tag":3, "parent":7, "children":[9]},
            {"op":"text", "text":1, "parent":8},
            {"op":"element", "tag":10, "parent":7, "children":[]}
        ],
        "texts":[{"binding":0},{"binding":1}],
        "bindings":[{"target":3,"sink":"text","expression":1},{"target":9,"sink":"text","expression":4}],
        "inputs":[{"name":5,"kind":"collection"}],
        "events":[{"target":8,"type":4,"action":0,"loop":0,"fields":[]}],
        "stateSlots":[{"initialExpression":0,"frameSlot":0}],
        "expressions":[
            {"instructions":[{"op":"constant","constant":1},{"op":"return"}]},
            {"instructions":[{"op":"loadState","state":0},{"op":"return"}]},
            {"instructions":[{"op":"constant","constant":0},{"op":"return"}]},
            {"instructions":[{"op":"loadRowField","field":6},{"op":"return"}]},
            {"instructions":[{"op":"loadRowField","field":7},{"op":"return"}]},
            {"instructions":[{"op":"loadRowField","field":8},{"op":"return"}]},
            {"instructions":[{"op":"loadRowField","field":9},{"op":"return"}]}
        ],
        "actions":[{"frameSlots":0,"instructions":[{"op":"evaluate","expression":4},{"op":"storeState","state":0},{"op":"return"}]}],
        "loops":[{"sourceExpression":2,"keyExpression":3,"itemSlot":0,"rowTemplate":4,"input":0}],
        "dependencyEdges":[{"source":{"kind":"state","handle":0},"target":{"kind":"binding","handle":0}}]
    })
}

fn event_slot_artifact() -> serde_json::Value {
    serde_json::json!({
        "version":"0.9", "rootNode":0,
        "strings":["div", "button", "click", "type", "unsupported"],
        "constants":[null],
        "nodes":[{"op":"element","tag":0,"children":[1,2]},{"op":"element","tag":1,"parent":0,"children":[]},{"op":"text","text":0,"parent":0}],
        "texts":[{"binding":0}],
        "bindings":[{"target":2,"sink":"text","expression":1}],
        "events":[{"target":1,"type":2,"action":0,"fields":[{"name":3,"slot":1}]}],
        "stateSlots":[{"initialExpression":0,"frameSlot":0}],
        "expressions":[
            {"instructions":[{"op":"constant","constant":0},{"op":"return"}]},
            {"instructions":[{"op":"loadState","state":0},{"op":"return"}]},
            {"instructions":[{"op":"loadFrame","slot":1},{"op":"return"}]}
        ],
        "actions":[{"frameSlots":2,"instructions":[{"op":"evaluate","expression":2},{"op":"storeState","state":0},{"op":"return"}]}],
        "dependencyEdges":[{"source":{"kind":"state","handle":0},"target":{"kind":"binding","handle":0}}]
    })
}

fn async_row_action_artifact() -> serde_json::Value {
    let mut app = keyed_row_artifact();
    app["strings"] =
        serde_json::json!(["div", "ul", "li", "button", "click", "items", "id", "title", "type"]);
    app["nodes"] = serde_json::json!([
        {"op":"element", "tag":0, "children":[1,3,7]},
        {"op":"element", "tag":1, "parent":0, "children":[2]},
        {"op":"loop", "loop":0, "parent":1},
        {"op":"text", "text":0, "parent":0},
        {"op":"element", "tag":2, "children":[5]},
        {"op":"element", "tag":3, "parent":4, "children":[6]},
        {"op":"text", "text":1, "parent":5},
        {"op":"text", "text":2, "parent":0}
    ]);
    app["texts"] = serde_json::json!([{"binding":0},{"binding":1},{"binding":2}]);
    app["bindings"] = serde_json::json!([
        {"target":3,"sink":"text","expression":1},
        {"target":6,"sink":"text","expression":4},
        {"target":7,"sink":"text","expression":6}
    ]);
    app["events"] = serde_json::json!([{"target":5,"type":4,"action":0,"loop":0,"fields":[{"name":8,"slot":0}]}]);
    app["stateSlots"] = serde_json::json!([
        {"initialExpression":0,"frameSlot":0}, {"initialExpression":0,"frameSlot":0}
    ]);
    app["expressions"] = serde_json::json!([
        {"instructions":[{"op":"constant","constant":1},{"op":"return"}]},
        {"instructions":[{"op":"loadState","state":0},{"op":"return"}]},
        {"instructions":[{"op":"constant","constant":0},{"op":"return"}]},
        {"instructions":[{"op":"loadRowField","field":6},{"op":"return"}]},
        {"instructions":[{"op":"loadRowField","field":7},{"op":"return"}]},
        {"instructions":[{"op":"constant","constant":2},{"op":"return"}]},
        {"instructions":[{"op":"loadState","state":1},{"op":"return"}]},
        {"instructions":[{"op":"loadFrame","slot":0},{"op":"return"}]}
    ]);
    app["constants"] = serde_json::json!([[], null, "data:text/plain,ok"]);
    app["actions"] = serde_json::json!([
        {"frameSlots":1,"instructions":[{"op":"call","action":1,"arguments":[7]},{"op":"return"}]},
        {"frameSlots":2,"parameterSlots":[0],"instructions":[
            {"op":"capabilityRequest","capability":"fetch","request":{"url":5,"method":"GET","decode":"text"},"successPc":1,"failurePc":3,"finallyPc":null,"resultSlot":1,"errorSlot":1},
            {"op":"call","action":2,"arguments":[7]},{"op":"return"},{"op":"return"}
        ]},
        {"frameSlots":1,"parameterSlots":[0],"instructions":[
            {"op":"evaluate","expression":4},{"op":"storeState","state":0},
            {"op":"evaluate","expression":7},{"op":"storeState","state":1},{"op":"return"}
        ]}
    ]);
    app["dependencyEdges"] = serde_json::json!([
        {"source":{"kind":"state","handle":0},"target":{"kind":"binding","handle":0}},
        {"source":{"kind":"state","handle":1},"target":{"kind":"binding","handle":2}}
    ]);
    app
}

/// One static button and three observable slots: branch result, inner finally,
/// and outer finally. Keeping this artifact small makes fetch behavior visible
/// without adding a test-only runtime API.
fn fetch_artifact(decode: &str, require_ok: bool, nested: bool) -> serde_json::Value {
    let failure_pc = if nested { 5 } else { 4 };
    let finally_pc = if nested { 11 } else { 7 };
    let mut instructions = vec![serde_json::json!({
        "op":"capabilityRequest", "capability":"fetch",
        "request":{"url":7,"method":"GET","decode":decode,"requireOk":require_ok},
        "successPc":1,"failurePc":failure_pc,"finallyPc":finally_pc,"resultSlot":1,"errorSlot":2
    })];
    if nested {
        instructions.push(serde_json::json!({
            "op":"capabilityRequest", "capability":"fetch",
            "request":{"url":7,"method":"GET","decode":"text"},
            "successPc":2,"failurePc":failure_pc,"finallyPc":8,"resultSlot":1,"errorSlot":2
        }));
    } else {
        instructions.push(serde_json::json!({"op":"evaluate","expression":8}));
        instructions.push(serde_json::json!({"op":"storeState","state":0}));
        instructions.push(serde_json::json!({"op":"return"}));
    }
    if nested {
        instructions.extend([
            serde_json::json!({"op":"evaluate","expression":8}),
            serde_json::json!({"op":"storeState","state":0}),
            serde_json::json!({"op":"return"}),
        ]);
    }
    instructions.extend([
        serde_json::json!({"op":"evaluate","expression":9}),
        serde_json::json!({"op":"storeState","state":0}),
        serde_json::json!({"op":"return"}),
        serde_json::json!({"op":"evaluate","expression":10}),
        serde_json::json!({"op":"storeState","state":1}),
        serde_json::json!({"op":"return"}),
        serde_json::json!({"op":"evaluate","expression":11}),
        serde_json::json!({"op":"storeState","state":2}),
        serde_json::json!({"op":"return"}),
    ]);
    serde_json::json!({
        "version":"0.9", "rootNode":0,
        "strings":["div","button","click"],
        "constants":[null,"url","success","failure","inner","outer"],
        "nodes":[
            {"op":"element","tag":0,"children":[1,2,3,4]},
            {"op":"element","tag":1,"parent":0,"children":[]},
            {"op":"text","text":0,"parent":0}, {"op":"text","text":1,"parent":0}, {"op":"text","text":2,"parent":0}
        ],
        "texts":[{"binding":0},{"binding":1},{"binding":2}],
        "bindings":[
            {"target":2,"sink":"text","expression":1}, {"target":3,"sink":"text","expression":2}, {"target":4,"sink":"text","expression":3}
        ],
        "events":[{"target":1,"type":2,"action":0,"fields":[]}],
        "stateSlots":[{"initialExpression":0,"frameSlot":0},{"initialExpression":0,"frameSlot":0},{"initialExpression":0,"frameSlot":0}],
        "expressions":[
            {"instructions":[{"op":"constant","constant":0},{"op":"return"}]},
            {"instructions":[{"op":"loadState","state":0},{"op":"return"}]}, {"instructions":[{"op":"loadState","state":1},{"op":"return"}]}, {"instructions":[{"op":"loadState","state":2},{"op":"return"}]},
            {"instructions":[{"op":"constant","constant":1},{"op":"return"}]}, {"instructions":[{"op":"constant","constant":2},{"op":"return"}]}, {"instructions":[{"op":"constant","constant":3},{"op":"return"}]}, {"instructions":[{"op":"constant","constant":1},{"op":"return"}]},
            {"instructions":[{"op":"loadFrame","slot":1},{"op":"return"}]}, {"instructions":[{"op":"loadFrame","slot":2},{"op":"return"}]}, {"instructions":[{"op":"constant","constant":4},{"op":"return"}]}, {"instructions":[{"op":"constant","constant":5},{"op":"return"}]}
        ],
        "actions":[{"frameSlots":3,"instructions":instructions}],
        "dependencyEdges":[
            {"source":{"kind":"state","handle":0},"target":{"kind":"binding","handle":0}}, {"source":{"kind":"state","handle":1},"target":{"kind":"binding","handle":1}}, {"source":{"kind":"state","handle":2},"target":{"kind":"binding","handle":2}}
        ]
    })
}

fn caller_continuation_artifact() -> serde_json::Value {
    let mut app = fetch_artifact("text", true, false);
    app["expressions"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"instructions":[{"op":"loadFrame","slot":0},{"op":"return"}]}));
    app["expressions"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"instructions":[{"op":"loadFrame","slot":1},{"op":"return"}]}));
    app["actions"] = serde_json::json!([
        {"frameSlots":2,"instructions":[
            {"op":"call","action":1,"arguments":[],"successPc":1,"failurePc":4,"resultSlot":0,"errorSlot":1},
            {"op":"evaluate","expression":12},{"op":"storeState","state":0},{"op":"return"},
            {"op":"evaluate","expression":13},{"op":"storeState","state":0},{"op":"return"}
        ]},
        {"frameSlots":2,"instructions":[
            {"op":"capabilityRequest","capability":"fetch","request":{"url":7,"method":"GET","decode":"text","requireOk":true},"successPc":1,"failurePc":2,"finallyPc":null,"resultSlot":0,"errorSlot":1},
            {"op":"return","outcome":"success","value":12}, {"op":"return","outcome":"failure","value":13}
        ]}
    ]);
    app
}

fn route_error_artifact() -> serde_json::Value {
    serde_json::json!({
        "version":"0.9", "rootNode":0,
        "strings":["div", "button", "click", "message", "Retry", "error", "status", "body"],
        "constants":[null, "Retry"],
        "nodes":[
            {"op":"element","tag":0,"children":[1,2,3,4]},
            {"op":"text","text":0,"parent":0},
            {"op":"text","text":1,"parent":0},
            {"op":"text","text":2,"parent":0},
            {"op":"element","tag":1,"parent":0,"children":[5]},
            {"op":"text","text":3,"parent":4}
        ],
        "texts":[{"binding":0},{"binding":1},{"binding":2},{"value":"Retry"}],
        "bindings":[{"target":1,"sink":"text","expression":0},{"target":2,"sink":"text","expression":1},{"target":3,"sink":"text","expression":2}],
        "events":[{"target":4,"type":2,"action":0,"fields":[]}],
        "stateSlots":[{"name":5,"initialExpression":1,"frameSlot":0}],
        "routeErrorState":0,
        "expressions":[
            {"instructions":[{"op":"loadState","state":0},{"op":"field","field":3},{"op":"return"}]},
            {"instructions":[{"op":"loadState","state":0},{"op":"field","field":6},{"op":"return"}]},
            {"instructions":[{"op":"loadState","state":0},{"op":"field","field":7},{"op":"return"}]},
            {"instructions":[{"op":"constant","constant":0},{"op":"return"}]}
        ],
        "actions":[{"routeRetry":true,"instructions":[{"op":"return"}]}]
    })
}

fn click_fetch(root: &Element) {
    root.query_selector("button")
        .unwrap()
        .unwrap()
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
}

async fn settle_fetch() {
    for _ in 0..5 {
        browser_tick().await;
    }
}

async fn browser_tick() {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window()
            .unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(resolve.unchecked_ref(), 0)
            .unwrap();
    });
    JsFuture::from(promise).await.unwrap();
}

fn initialize_rows(runtime: &PlecRuntime, rows: serde_json::Value) {
    runtime
        .initialize_input("items".into(), serde_wasm_bindgen::to_value(&rows).unwrap())
        .unwrap();
}

fn apply_delta(runtime: &PlecRuntime, delta: serde_json::Value) {
    runtime
        .apply_delta(serde_wasm_bindgen::to_value(&delta).unwrap())
        .unwrap();
}

fn static_output(root: &Element) -> String {
    root.first_element_child()
        .unwrap()
        .child_nodes()
        .item(1)
        .unwrap()
        .text_content()
        .unwrap_or_default()
}

#[wasm_bindgen_test]
fn rust_compiler_counter_fixture_mounts_and_updates_one_text_binding() {
    let runtime = PlecRuntime::new();
    let root = mount_root();
    runtime
        .load_application(serde_wasm_bindgen::to_value(&rust_counter_artifact()).unwrap())
        .unwrap();

    let mount_metrics: serde_json::Value =
        serde_wasm_bindgen::from_value(runtime.mount(root.clone()).unwrap()).unwrap();
    assert_eq!(root.text_content().unwrap(), "0");
    assert_eq!(mount_metrics["bindings"], 1);
    assert_eq!(mount_metrics["createdTexts"], 1);

    let button = root.query_selector("button").unwrap().unwrap();
    assert_eq!(button.child_nodes().length(), 1);
    let text = button.first_child().unwrap();
    button
        .clone()
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    assert_eq!(root.text_content().unwrap(), "1");
    assert!(text.is_same_node(button.first_child().as_ref()));
}

#[wasm_bindgen_test]
fn rust_local_action_fixture_updates_existing_text_node() {
    let runtime = PlecRuntime::new();
    let root = mount_root();
    load_and_mount(&runtime, rust_local_action_artifact(), &root);
    let button = root.query_selector("button").unwrap().unwrap();
    let text = button.first_child().unwrap();
    button
        .clone()
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    assert_eq!(button.text_content().as_deref(), Some("1"));
    assert!(text.is_same_node(button.first_child().as_ref()));
}

#[wasm_bindgen_test]
fn rust_component_fixture_refreshes_child_without_remounting() {
    let runtime = PlecRuntime::new();
    let root = mount_root();
    load_and_mount(&runtime, rust_component_artifact(), &root);
    let span = root.query_selector("span").unwrap().unwrap();
    let text = span.first_child().unwrap();
    assert_eq!(span.text_content().unwrap(), "one");
    root.query_selector("button")
        .unwrap()
        .unwrap()
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    let next = root.query_selector("span").unwrap().unwrap();
    assert_eq!(next.text_content().unwrap(), "two");
    assert!(span.is_same_node(Some(&next)));
    assert!(text.is_same_node(next.first_child().as_ref()));
}

#[wasm_bindgen_test]
fn component_slot_mounts_caller_owned_children_inside_the_callee_anchor() {
    let runtime = PlecRuntime::new();
    let root = mount_root();
    load_and_mount(&runtime, component_slot_artifact(), &root);
    let section = root.query_selector("section").unwrap().unwrap();
    let paragraph = section.query_selector("p").unwrap().unwrap();
    assert_eq!(paragraph.text_content().as_deref(), Some("Inside"));
    assert!(root.query_selector("main > p").unwrap().is_none());
}

#[wasm_bindgen_test]
fn rust_keyed_slot_fixture_retains_caller_row_identity_and_disposes_slots() {
    let runtime = PlecRuntime::new();
    let root = mount_root();
    load_and_mount(&runtime, rust_keyed_slot_component_artifact(), &root);
    apply_delta(
        &runtime,
        serde_json::json!({"type":"insert","input_id":"items","row_key":"one","row":{"id":"one","title":"One","done":false},"before_row_key":null}),
    );
    apply_delta(
        &runtime,
        serde_json::json!({"type":"insert","input_id":"items","row_key":"two","row":{"id":"two","title":"Two","done":false},"before_row_key":null}),
    );

    let first = root.query_selector("li").unwrap().unwrap();
    let first_node: web_sys::Node = first.clone().into();
    let button = first.query_selector("button").unwrap().unwrap();
    let text = button.first_child().unwrap();
    assert_eq!(
        first
            .query_selector("em")
            .unwrap()
            .unwrap()
            .text_content()
            .unwrap(),
        "Open"
    );

    apply_delta(
        &runtime,
        serde_json::json!({"type":"update","input_id":"items","row_key":"one","changes":{"title":"Updated","done":true}}),
    );
    assert!(first_node.is_same_node(
        root.query_selector("li")
            .unwrap()
            .as_ref()
            .map(|node| node.unchecked_ref())
    ));
    assert!(text.is_same_node(button.first_child().as_ref()));
    assert_eq!(button.text_content().unwrap(), "Updated");
    assert!(first.query_selector("strong").unwrap().is_some());
    assert_eq!(
        root.query_selector_all("li")
            .unwrap()
            .item(1)
            .unwrap()
            .text_content()
            .unwrap(),
        "TwoOpen"
    );

    button
        .clone()
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    assert_eq!(
        root.query_selector("p")
            .unwrap()
            .unwrap()
            .text_content()
            .unwrap(),
        "Updated"
    );

    apply_delta(
        &runtime,
        serde_json::json!({"type":"move","input_id":"items","row_key":"two","before_row_key":"one"}),
    );
    assert!(first_node.is_same_node(root.query_selector_all("li").unwrap().item(1).as_ref()));
    assert!(text.is_same_node(button.first_child().as_ref()));

    apply_delta(
        &runtime,
        serde_json::json!({"type":"remove","input_id":"items","row_key":"one"}),
    );
    assert!(first.parent_node().is_none());
    button
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    assert_eq!(
        root.query_selector("p")
            .unwrap()
            .unwrap()
            .text_content()
            .unwrap(),
        "Updated"
    );
}

#[wasm_bindgen_test]
fn rust_keyed_local_action_fixture_retains_row_identity() {
    let runtime = PlecRuntime::new();
    let root = mount_root();
    load_and_mount(&runtime, rust_keyed_local_action_artifact(), &root);
    apply_delta(
        &runtime,
        serde_json::json!({"type":"insert","input_id":"items","row_key":"one","row":{"id":"one","title":"One"},"before_row_key":null}),
    );
    apply_delta(
        &runtime,
        serde_json::json!({"type":"insert","input_id":"items","row_key":"two","row":{"id":"two","title":"Two"},"before_row_key":null}),
    );
    let first = root.query_selector("li").unwrap().unwrap();
    let first_node: web_sys::Node = first.clone().into();
    let button = first.query_selector("button").unwrap().unwrap();
    button
        .clone()
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    assert_eq!(
        root.query_selector("p")
            .unwrap()
            .unwrap()
            .text_content()
            .as_deref(),
        Some("One")
    );
    apply_delta(
        &runtime,
        serde_json::json!({"type":"move","input_id":"items","row_key":"two","before_row_key":"one"}),
    );
    assert!(first_node.is_same_node(root.query_selector_all("li").unwrap().item(1).as_ref()));
    let moved_button = root
        .query_selector_all("li")
        .unwrap()
        .item(1)
        .and_then(|row| row.dyn_into::<Element>().ok())
        .and_then(|row| row.query_selector("button").ok().flatten())
        .unwrap();
    let moved_button_node: web_sys::Node = moved_button.into();
    assert!(button.is_same_node(Some(&moved_button_node)));
}

#[wasm_bindgen_test]
fn rust_keyed_component_fixture_retains_rows_and_disposes_removed_child() {
    let runtime = PlecRuntime::new();
    let root = mount_root();
    load_and_mount(&runtime, rust_keyed_component_artifact(), &root);
    apply_delta(
        &runtime,
        serde_json::json!({"type":"insert","input_id":"items","row_key":"one","row":{"id":"one","title":"One"},"before_row_key":null}),
    );
    apply_delta(
        &runtime,
        serde_json::json!({"type":"insert","input_id":"items","row_key":"two","row":{"id":"two","title":"Two"},"before_row_key":null}),
    );

    let first = root.query_selector("li").unwrap().unwrap();
    let first_node: web_sys::Node = first.clone().into();
    let title = first.query_selector("span").unwrap().unwrap();
    let title_text = title.first_child().unwrap();
    let button = first.query_selector("button").unwrap().unwrap();

    apply_delta(
        &runtime,
        serde_json::json!({"type":"update","input_id":"items","row_key":"one","changes":{"title":"Updated"}}),
    );
    assert!(first_node.is_same_node(
        root.query_selector("li")
            .unwrap()
            .as_ref()
            .map(|node| node.unchecked_ref())
    ));
    assert!(title.is_same_node(
        first
            .query_selector("span")
            .unwrap()
            .as_ref()
            .map(|node| node.unchecked_ref())
    ));
    assert!(title_text.is_same_node(title.first_child().as_ref()));
    assert_eq!(title.text_content().unwrap(), "Updated");

    button
        .clone()
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    assert_eq!(first.text_content().unwrap(), "Updated1");

    apply_delta(
        &runtime,
        serde_json::json!({"type":"move","input_id":"items","row_key":"two","before_row_key":"one"}),
    );
    assert!(first_node.is_same_node(root.query_selector_all("li").unwrap().item(1).as_ref()));
    assert_eq!(first.text_content().unwrap(), "Updated1");

    apply_delta(
        &runtime,
        serde_json::json!({"type":"remove","input_id":"items","row_key":"one"}),
    );
    assert!(first.parent_node().is_none());
    button
        .clone()
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    assert_eq!(root.text_content().unwrap(), "Two0");
}

#[wasm_bindgen_test]
fn rust_keyed_callback_component_fixture_dispatches_parent_row_action() {
    let runtime = PlecRuntime::new();
    let root = mount_root();
    load_and_mount(&runtime, rust_keyed_callback_component_artifact(), &root);
    apply_delta(
        &runtime,
        serde_json::json!({"type":"insert","input_id":"items","row_key":"one","row":{"id":"one","title":"One"},"before_row_key":null}),
    );
    apply_delta(
        &runtime,
        serde_json::json!({"type":"insert","input_id":"items","row_key":"two","row":{"id":"two","title":"Two"},"before_row_key":null}),
    );
    let first = root.query_selector("button").unwrap().unwrap();
    let first_node: web_sys::Node = first.clone().into();
    first
        .clone()
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    assert_eq!(
        root.query_selector("p")
            .unwrap()
            .unwrap()
            .text_content()
            .unwrap(),
        "One"
    );
    apply_delta(
        &runtime,
        serde_json::json!({"type":"move","input_id":"items","row_key":"two","before_row_key":"one"}),
    );
    assert!(first_node.is_same_node(root.query_selector_all("button").unwrap().item(1).as_ref()));
    root.query_selector("button")
        .unwrap()
        .unwrap()
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    assert_eq!(
        root.query_selector("p")
            .unwrap()
            .unwrap()
            .text_content()
            .unwrap(),
        "Two"
    );
    apply_delta(
        &runtime,
        serde_json::json!({"type":"remove","input_id":"items","row_key":"one"}),
    );
    first
        .clone()
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    assert_eq!(
        root.query_selector("p")
            .unwrap()
            .unwrap()
            .text_content()
            .unwrap(),
        "Two"
    );
}

#[wasm_bindgen_test]
fn rust_collection_mutation_fixture_updates_one_keyed_row_without_remounting() {
    let runtime = PlecRuntime::new();
    let root = mount_root();
    load_and_mount(&runtime, rust_collection_mutation_artifact(), &root);
    root.query_selector("button")
        .unwrap()
        .unwrap()
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    let row = root.query_selector("li").unwrap().unwrap();
    let row_node: web_sys::Node = row.clone().into();
    let title = row.query_selector("span").unwrap().unwrap();
    let title_text = title.first_child().unwrap();
    assert_eq!(title.text_content().unwrap(), "One");

    root.query_selector_all("button")
        .unwrap()
        .item(1)
        .unwrap()
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    let next_row = root.query_selector("li").unwrap().unwrap();
    let next_title = next_row.query_selector("span").unwrap().unwrap();
    assert!(row_node.is_same_node(Some(&next_row)));
    assert!(title_text.is_same_node(next_title.first_child().as_ref()));
    assert_eq!(next_title.text_content().unwrap(), "Two");

    root.query_selector_all("button")
        .unwrap()
        .item(2)
        .unwrap()
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    assert!(row.parent_node().is_none());
    assert_eq!(root.text_content().unwrap(), "Add");
}

#[wasm_bindgen_test]
fn rust_nested_component_fixture_refreshes_grandchild_without_remounting() {
    let runtime = PlecRuntime::new();
    let root = mount_root();
    load_and_mount(&runtime, rust_nested_component_artifact(), &root);
    let section = root.query_selector("section").unwrap().unwrap();
    let span = root.query_selector("span").unwrap().unwrap();
    let text = span.first_child().unwrap();
    let buttons = root.query_selector_all("button").unwrap();

    buttons
        .item(1)
        .unwrap()
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    let next_section = root.query_selector("section").unwrap().unwrap();
    let next_span = root.query_selector("span").unwrap().unwrap();
    assert_eq!(next_span.text_content().unwrap(), "two");
    assert!(section.is_same_node(Some(&next_section)));
    assert!(span.is_same_node(Some(&next_span)));
    assert!(text.is_same_node(next_span.first_child().as_ref()));

    buttons
        .item(0)
        .unwrap()
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    assert_eq!(
        root.query_selector("div")
            .unwrap()
            .unwrap()
            .text_content()
            .unwrap(),
        "two1"
    );
}

#[wasm_bindgen_test]
fn rust_collection_rows_fixture_reconciles_keyed_rows_and_branch_listener() {
    let runtime = PlecRuntime::new();
    let root = mount_root();
    load_and_mount(&runtime, rust_collection_rows_artifact(), &root);
    apply_delta(
        &runtime,
        serde_json::json!({"type":"insert","input_id":"items","row_key":"one","row":{"id":"one","title":"One","done":true},"before_row_key":null}),
    );
    apply_delta(
        &runtime,
        serde_json::json!({"type":"insert","input_id":"items","row_key":"two","row":{"id":"two","title":"Two","done":false},"before_row_key":null}),
    );
    let first = root.query_selector("li").unwrap().unwrap();
    let first_node: web_sys::Node = first.clone().into();
    let title = first.first_child().unwrap();
    let button = first.query_selector("button").unwrap().unwrap();

    apply_delta(
        &runtime,
        serde_json::json!({"type":"update","input_id":"items","row_key":"one","changes":{"title":"Updated"}}),
    );
    assert!(first_node.is_same_node(
        root.query_selector("li")
            .unwrap()
            .as_ref()
            .map(|node| node.unchecked_ref())
    ));
    assert!(title.is_same_node(first.first_child().as_ref()));
    assert_eq!(first.text_content().unwrap(), "UpdatedDone");

    apply_delta(
        &runtime,
        serde_json::json!({"type":"update","input_id":"items","row_key":"one","changes":{"done":false}}),
    );
    assert!(first.query_selector("button").unwrap().is_none());
    assert!(!button.is_connected());
    apply_delta(
        &runtime,
        serde_json::json!({"type":"move","input_id":"items","row_key":"two","before_row_key":"one"}),
    );
    assert!(first_node.is_same_node(root.query_selector_all("li").unwrap().item(1).as_ref()));
    apply_delta(
        &runtime,
        serde_json::json!({"type":"remove","input_id":"items","row_key":"one"}),
    );
    assert!(!first.is_connected());
}

#[wasm_bindgen_test]
fn rust_static_conditional_fixture_replaces_branch_and_disposes_listener() {
    let runtime = PlecRuntime::new();
    let root = mount_root();
    load_and_mount(&runtime, rust_static_conditional_artifact(), &root);

    let false_branch = root
        .query_selector("[data-runtime-node='4']")
        .unwrap()
        .unwrap();
    assert!(root
        .query_selector("[data-runtime-node='2']")
        .unwrap()
        .is_none());
    false_branch
        .clone()
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();

    let true_branch = root
        .query_selector("[data-runtime-node='2']")
        .unwrap()
        .unwrap();
    assert!(false_branch.parent_node().is_none());
    false_branch
        .clone()
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    assert!(root
        .query_selector("[data-runtime-node='2']")
        .unwrap()
        .is_some());
    assert!(root
        .query_selector("[data-runtime-node='4']")
        .unwrap()
        .is_none());

    true_branch
        .clone()
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    let next_false_branch = root
        .query_selector("[data-runtime-node='4']")
        .unwrap()
        .unwrap();
    assert!(true_branch.parent_node().is_none());
    assert!(!false_branch.is_same_node(Some(&next_false_branch)));
}

#[wasm_bindgen_test]
fn static_listener_dispatches_once_and_dispose_removes_its_dom() {
    let runtime = PlecRuntime::new();
    runtime
        .load_application(
            serde_wasm_bindgen::to_value(&serde_json::json!({
                "version": "0.9",
                "rootNode": 0,
                "strings": ["div", "button", "click"],
                "constants": [false, true],
                "nodes": [
                    {"op": "element", "tag": 0, "children": [1, 2]},
                    {"op": "element", "tag": 1, "parent": 0, "children": []},
                    {"op": "text", "text": 0, "parent": 0}
                ],
                "texts": [{"binding": 0}],
                "bindings": [{"target": 2, "sink": "text", "expression": 1}],
                "events": [{"target": 1, "type": 2, "action": 0, "fields": []}],
                "stateSlots": [{"initialExpression": 0, "frameSlot": 0}],
                "expressions": [
                    {"instructions": [{"op": "constant", "constant": 0}, {"op": "return"}]},
                    {"instructions": [{"op": "loadState", "state": 0}, {"op": "return"}]},
                    {"instructions": [{"op": "constant", "constant": 1}, {"op": "return"}]}
                ],
                "actions": [{"frameSlots": 0, "instructions": [
                    {"op": "evaluate", "expression": 2},
                    {"op": "storeState", "state": 0},
                    {"op": "return"}
                ]}],
                "dependencyEdges": [{
                    "source": {"kind": "state", "handle": 0},
                    "target": {"kind": "binding", "handle": 0}
                }]
            }))
            .unwrap(),
        )
        .unwrap();
    let root = mount_root();
    runtime.mount(root.clone()).unwrap();
    // Remounting must tear down the old closure before attaching the new
    // concrete node's listener.
    runtime.mount(root.clone()).unwrap();
    let button = root.query_selector("button").unwrap().unwrap();
    button
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    assert_eq!(root.text_content().unwrap(), "true");
    runtime.dispose().unwrap();
    assert_eq!(root.child_element_count(), 0);
}

#[wasm_bindgen_test]
fn keyed_row_listener_survives_updates_and_moves_but_stale_row_callback_is_inert() {
    let runtime = PlecRuntime::new();
    let root = mount_root();
    load_and_mount(&runtime, keyed_row_artifact(), &root);
    initialize_rows(
        &runtime,
        serde_json::json!([
            {"id":"first", "title":"first title"},
            {"id":"second", "title":"second title"}
        ]),
    );

    let first = root
        .query_selector("[data-runtime-row-key='first'] button")
        .unwrap()
        .unwrap();
    first
        .clone()
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    assert_eq!(static_output(&root), "first title");

    apply_delta(
        &runtime,
        serde_json::json!({
            "type":"update", "input_id":"items", "row_key":"first", "changes":{"title":"updated title"}
        }),
    );
    first
        .clone()
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    assert_eq!(static_output(&root), "updated title");

    apply_delta(
        &runtime,
        serde_json::json!({
            "type":"move", "input_id":"items", "row_key":"first", "before_row_key":null
        }),
    );
    first
        .clone()
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    assert_eq!(static_output(&root), "updated title");

    apply_delta(
        &runtime,
        serde_json::json!({
            "type":"remove", "input_id":"items", "row_key":"first"
        }),
    );
    let second = root
        .query_selector("[data-runtime-row-key='second'] button")
        .unwrap()
        .unwrap();
    second
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    first
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    assert_eq!(static_output(&root), "second title");
}

#[wasm_bindgen_test]
fn static_conditional_replaces_branch_listeners_and_supports_no_alternate() {
    let runtime = PlecRuntime::new();
    let root = mount_root();
    load_and_mount(&runtime, static_conditional_artifact(true), &root);
    let false_branch = root
        .query_selector("[data-runtime-node='3']")
        .unwrap()
        .unwrap();
    false_branch
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    assert!(root
        .query_selector("[data-runtime-node='3']")
        .unwrap()
        .is_none());
    let true_branch = root
        .query_selector("[data-runtime-node='2']")
        .unwrap()
        .unwrap();
    true_branch
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    assert!(root
        .query_selector("[data-runtime-node='2']")
        .unwrap()
        .is_none());

    let no_alternate = PlecRuntime::new();
    let no_alternate_root = mount_root();
    load_and_mount(
        &no_alternate,
        static_conditional_artifact(false),
        &no_alternate_root,
    );
    assert!(no_alternate_root
        .query_selector("button")
        .unwrap()
        .is_none());
}

#[wasm_bindgen_test]
fn row_conditional_listener_replaces_only_its_own_keyed_row() {
    let runtime = PlecRuntime::new();
    let root = mount_root();
    load_and_mount(&runtime, row_conditional_artifact(), &root);
    initialize_rows(
        &runtime,
        serde_json::json!([
            {"id":"first", "title":"first", "enabled":true},
            {"id":"second", "title":"second", "enabled":false}
        ]),
    );
    let first_row = root
        .query_selector("[data-runtime-row-key='first']")
        .unwrap()
        .unwrap();
    let first_button = first_row.query_selector("button").unwrap().unwrap();
    first_button
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    assert_eq!(static_output(&root), "first");

    apply_delta(
        &runtime,
        serde_json::json!({
            "type":"update", "input_id":"items", "row_key":"first", "changes":{"enabled":false}
        }),
    );
    assert!(root
        .query_selector("[data-runtime-row-key='first'] button")
        .unwrap()
        .is_none());
    assert!(first_row.is_same_node(Some(
        &root
            .query_selector("[data-runtime-row-key='first']")
            .unwrap()
            .unwrap()
    )));

    apply_delta(
        &runtime,
        serde_json::json!({
            "type":"update", "input_id":"items", "row_key":"second", "changes":{"enabled":true}
        }),
    );
    let second_button = root
        .query_selector("[data-runtime-row-key='second'] button")
        .unwrap()
        .unwrap();
    second_button
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    assert_eq!(static_output(&root), "second");
}

#[wasm_bindgen_test]
fn nested_row_conditional_disposes_only_the_replaced_branch() {
    let runtime = PlecRuntime::new();
    let root = mount_root();
    load_and_mount(&runtime, nested_row_conditional_artifact(), &root);
    initialize_rows(
        &runtime,
        serde_json::json!([
            {"id":"first", "title":"first", "enabled":true, "active":true},
            {"id":"second", "title":"second", "enabled":true, "active":true}
        ]),
    );
    let first = root
        .query_selector("[data-runtime-row-key='first'] button")
        .unwrap()
        .unwrap();
    let second = root
        .query_selector("[data-runtime-row-key='second'] button")
        .unwrap()
        .unwrap();

    apply_delta(
        &runtime,
        serde_json::json!({
            "type":"update", "input_id":"items", "row_key":"first", "changes":{"active":false}
        }),
    );
    assert!(root
        .query_selector("[data-runtime-row-key='first'] button")
        .unwrap()
        .is_none());
    assert!(root
        .query_selector("[data-runtime-row-key='second'] button")
        .unwrap()
        .is_some());
    first
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    second
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    assert_eq!(static_output(&root), "second");
}

#[wasm_bindgen_test]
fn event_dispatch_exposes_only_declared_slots_and_rejects_unsupported_fields() {
    let runtime = PlecRuntime::new();
    let root = mount_root();
    load_and_mount(&runtime, event_slot_artifact(), &root);
    root.query_selector("button")
        .unwrap()
        .unwrap()
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    assert_eq!(root.text_content().unwrap(), "click");

    let invalid = PlecRuntime::new();
    let mut artifact = event_slot_artifact();
    artifact["events"][0]["fields"][0]["name"] = serde_json::json!(4);
    assert!(invalid
        .load_application(serde_wasm_bindgen::to_value(&artifact).unwrap())
        .is_err());
}

#[wasm_bindgen_test(async)]
async fn row_event_frame_survives_nested_call_and_fetch_continuation() {
    let runtime = PlecRuntime::new();
    let root = mount_root();
    load_and_mount(&runtime, async_row_action_artifact(), &root);
    initialize_rows(
        &runtime,
        serde_json::json!([{"id":"row", "title":"retained row"}]),
    );
    root.query_selector("button")
        .unwrap()
        .unwrap()
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    for _ in 0..5 {
        browser_tick().await;
    }
    assert_eq!(root.text_content().unwrap(), "retained rowclick");
}

#[wasm_bindgen_test(async)]
async fn disposing_a_typed_graph_aborts_and_discards_its_fetch() {
    set_plec_fetch_queue(r#"[{"pending":true}]"#);
    let runtime = PlecRuntime::new();
    let root = mount_root();
    load_and_mount(&runtime, async_row_action_artifact(), &root);
    initialize_rows(
        &runtime,
        serde_json::json!([{"id":"row", "title":"retained row"}]),
    );
    root.query_selector("button")
        .unwrap()
        .unwrap()
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    runtime.dispose().unwrap();
    for _ in 0..2 {
        browser_tick().await;
    }
    assert_eq!(plec_fetch_aborts(), 1);
    assert_eq!(root.text_content().unwrap_or_default(), "");
    restore_plec_fetch();
}

#[wasm_bindgen_test(async)]
async fn typed_fetch_decodes_json_text_and_empty_responses() {
    for (decode, spec, expected) in [
        (
            "json",
            r#"[{"body":"{\"ok\":true}","headers":{"content-type":"application/json"}}]"#,
            r#"{"ok":true}inner"#,
        ),
        ("text", r#"[{"body":"plain"}]"#, "plaininner"),
        ("empty", r#"[{"body":"ignored"}]"#, "inner"),
    ] {
        set_plec_fetch_queue(spec);
        let runtime = PlecRuntime::new();
        let root = mount_root();
        load_and_mount(&runtime, fetch_artifact(decode, true, false), &root);
        click_fetch(&root);
        settle_fetch().await;
        assert_eq!(root.text_content().unwrap(), expected);
        restore_plec_fetch();
    }
}

#[wasm_bindgen_test(async)]
async fn typed_fetch_routes_http_decode_network_and_abort_failures() {
    for (spec, expected) in [
        (
            r#"[{"status":400,"statusText":"Bad","body":"{\"reason\":\"no\"}","headers":{"content-type":"application/problem+json; charset=utf-8"}}]"#,
            "\"kind\":\"http\"",
        ),
        (
            r#"[{"body":"not json","headers":{"content-type":"application/json"}}]"#,
            "\"kind\":\"decode\"",
        ),
        (r#"[{"reject":"offline"}]"#, "\"kind\":\"network\""),
        (
            r#"[{"reject":"aborted","name":"AbortError"}]"#,
            "\"kind\":\"abort\"",
        ),
    ] {
        set_plec_fetch_queue(spec);
        let runtime = PlecRuntime::new();
        let root = mount_root();
        load_and_mount(&runtime, fetch_artifact("json", true, false), &root);
        click_fetch(&root);
        settle_fetch().await;
        let output = root.text_content().unwrap();
        assert!(output.contains(expected), "{output}");
        assert!(output.contains("\"url\":\"url\""), "{output}");
        assert!(output.ends_with("inner"), "{output}");
        restore_plec_fetch();
    }

    set_plec_fetch_queue(r#"[{"status":418,"body":"teapot"}]"#);
    let runtime = PlecRuntime::new();
    let root = mount_root();
    load_and_mount(&runtime, fetch_artifact("text", false, false), &root);
    click_fetch(&root);
    settle_fetch().await;
    assert_eq!(root.text_content().unwrap(), "teapotinner");
    restore_plec_fetch();
}

#[wasm_bindgen_test(async)]
async fn typed_fetch_runs_nested_finalizers_inner_to_outer() {
    set_plec_fetch_queue(r#"[{"body":"outer"},{"body":"inner"}]"#);
    let runtime = PlecRuntime::new();
    let root = mount_root();
    load_and_mount(&runtime, fetch_artifact("text", true, true), &root);
    click_fetch(&root);
    settle_fetch().await;
    assert_eq!(root.text_content().unwrap(), "innerinnerouter");
    restore_plec_fetch();
}

#[wasm_bindgen_test(async)]
async fn suspended_callee_resumes_its_caller_continuation() {
    set_plec_fetch_queue(r#"[{"body":"resumed"}]"#);
    let runtime = PlecRuntime::new();
    let root = mount_root();
    load_and_mount(&runtime, caller_continuation_artifact(), &root);
    click_fetch(&root);
    settle_fetch().await;
    assert_eq!(root.text_content().unwrap(), "resumed");
    restore_plec_fetch();
}

#[wasm_bindgen_test(async)]
async fn remount_and_typed_route_replacement_abort_stale_fetches() {
    set_plec_fetch_queue(r#"[{"pending":true}]"#);
    let runtime = PlecRuntime::new();
    let root = mount_root();
    load_and_mount(&runtime, fetch_artifact("text", true, false), &root);
    click_fetch(&root);
    runtime.mount(root.clone()).unwrap();
    settle_fetch().await;
    assert_eq!(plec_fetch_aborts(), 1);
    assert_eq!(root.text_content().unwrap_or_default(), "");
    restore_plec_fetch();

    set_plec_fetch_queue(r#"[{"pending":true}]"#);
    let runtime = PlecRuntime::new();
    let root = mount_root();
    let mut route_root = fetch_artifact("text", true, false);
    route_root["routeOutlets"] = serde_json::json!([{"id":"main","node":0}]);
    route_root["actions"][0]["routeLoader"] = serde_json::json!(true);
    route_root["actions"][0]["loaderResultState"] = serde_json::json!(0);
    route_root["actions"][0]["instructions"] = serde_json::json!([
        {"op":"capabilityRequest","capability":"fetch","request":{"url":7,"method":"GET","decode":"text","requireOk":true},"successPc":1,"failurePc":1,"finallyPc":null,"resultSlot":1,"errorSlot":2},
        {"op":"return"}
    ]);
    runtime
        .register_graph(
            "a".into(),
            serde_wasm_bindgen::to_value(&route_root).unwrap(),
        )
        .unwrap();
    runtime
        .register_graph(
            "b".into(),
            serde_wasm_bindgen::to_value(&fetch_artifact("text", true, false)).unwrap(),
        )
        .unwrap();
    let manifest = js_sys::JSON::parse(
        &serde_json::json!({
            "version":3,"rootGraphId":"a","routes":[
                {"id":"loading","path":"*","graphId":"a","outletId":"main","loaderAction":0},
                {"id":"next","path":"/next","graphId":"b","outletId":"main"}
            ]
        })
        .to_string(),
    )
    .unwrap();
    runtime.start(root.clone(), manifest).unwrap();
    runtime.navigate("/next".into(), false).unwrap();
    settle_fetch().await;
    assert_eq!(plec_fetch_aborts(), 1);
    assert_eq!(root.text_content().unwrap_or_default(), "");
    restore_plec_fetch();
}

#[wasm_bindgen_test(async)]
async fn typed_route_loader_writes_its_declared_result_state() {
    set_plec_fetch_queue(r#"[{"body":"loaded"}]"#);
    let runtime = PlecRuntime::new();
    let root = mount_root();
    let mut route = fetch_artifact("text", true, false);
    route["routeOutlets"] = serde_json::json!([{"id":"main","node":0}]);
    route["actions"][0]["routeLoader"] = serde_json::json!(true);
    route["actions"][0]["loaderResultState"] = serde_json::json!(0);
    route["actions"][0]["instructions"] = serde_json::json!([
        {"op":"capabilityRequest","capability":"fetch","request":{"url":7,"method":"GET","decode":"text","requireOk":true},"successPc":1,"failurePc":1,"finallyPc":null,"resultSlot":1,"errorSlot":2},
        {"op":"return"}
    ]);
    route["actions"][0]["instructions"] = serde_json::json!([
        {"op":"capabilityRequest","capability":"fetch","request":{"url":7,"method":"GET","decode":"text","requireOk":true},"successPc":1,"failurePc":1,"finallyPc":null,"resultSlot":1,"errorSlot":2},
        {"op":"return"}
    ]);
    runtime
        .register_graph("root".into(), serde_wasm_bindgen::to_value(&route).unwrap())
        .unwrap();
    runtime
        .register_graph("page".into(), serde_wasm_bindgen::to_value(&route).unwrap())
        .unwrap();
    let manifest = js_sys::JSON::parse(
        &serde_json::json!({
            "version":3,"rootGraphId":"root","routes":[
                {"id":"page","path":"*","graphId":"page","outletId":"main","loaderAction":0}
            ]
        })
        .to_string(),
    )
    .unwrap();
    runtime.start(root.clone(), manifest).unwrap();
    settle_fetch().await;
    assert_eq!(root.text_content().unwrap_or_default(), "loaded");
    restore_plec_fetch();
}

#[wasm_bindgen_test(async)]
async fn rust_route_async_fixture_navigates_and_disposes_stale_loader() {
    set_plec_fetch_queue(r#"[{"body":"loaded"}]"#);
    let runtime = PlecRuntime::new();
    let root = mount_root();
    let route = rust_route_async_artifact();
    runtime.register_graph("root".into(), serde_wasm_bindgen::to_value(&route).unwrap()).unwrap();
    runtime.register_graph("page".into(), serde_wasm_bindgen::to_value(&route).unwrap()).unwrap();
    let manifest = js_sys::JSON::parse(&serde_json::json!({
        "version":3,"rootGraphId":"root","routes":[
            {"id":"page","path":"*","graphId":"page","outletId":"main","loaderAction":0}
        ]
    }).to_string()).unwrap();
    runtime.start(root.clone(), manifest).unwrap();
    settle_fetch().await;
    assert_eq!(root.text_content().unwrap_or_default(), "loaded");
    restore_plec_fetch();

    set_plec_fetch_queue(r#"[{"status":500,"statusText":"Failed","body":"nope"}]"#);
    let runtime = PlecRuntime::new();
    let root = mount_root();
    let route = rust_route_async_artifact();
    runtime.register_graph("root".into(), serde_wasm_bindgen::to_value(&route).unwrap()).unwrap();
    runtime.register_graph("page".into(), serde_wasm_bindgen::to_value(&route).unwrap()).unwrap();
    runtime.register_graph("error".into(), serde_wasm_bindgen::to_value(&route_error_artifact()).unwrap()).unwrap();
    let manifest = js_sys::JSON::parse(&serde_json::json!({
        "version":3,"rootGraphId":"root","routes":[
            {"id":"page","path":"*","graphId":"page","errorGraphId":"error","outletId":"main","loaderAction":0}
        ]
    }).to_string()).unwrap();
    runtime.start(root.clone(), manifest).unwrap();
    settle_fetch().await;
    assert!(root.text_content().unwrap_or_default().contains("request failed (500)"));
    restore_plec_fetch();

    set_plec_fetch_queue(r#"[{"pending":true}]"#);
    let runtime = PlecRuntime::new();
    let root = mount_root();
    let route = rust_route_async_artifact();
    runtime.register_graph("root".into(), serde_wasm_bindgen::to_value(&route).unwrap()).unwrap();
    runtime.register_graph("page".into(), serde_wasm_bindgen::to_value(&route).unwrap()).unwrap();
    runtime.register_graph("next".into(), serde_wasm_bindgen::to_value(&fetch_artifact("text", true, false)).unwrap()).unwrap();
    let manifest = js_sys::JSON::parse(&serde_json::json!({
        "version":3,"rootGraphId":"root","routes":[
            {"id":"page","path":"/","graphId":"page","outletId":"main","loaderAction":0},
            {"id":"next","path":"/next","graphId":"next","outletId":"main"}
        ]
    }).to_string()).unwrap();
    runtime.start(root.clone(), manifest).unwrap();
    runtime.navigate("/next".into(), false).unwrap();
    settle_fetch().await;
    assert_eq!(plec_fetch_aborts(), 1);
    assert_eq!(root.text_content().unwrap_or_default(), "");
    restore_plec_fetch();
}

#[wasm_bindgen_test(async)]
async fn typed_route_error_receives_fetch_failure_and_retry_reloads() {
    set_plec_fetch_queue(
        r#"[{"status":418,"statusText":"Teapot","body":"short and stout"},{"body":"loaded"}]"#,
    );
    let runtime = PlecRuntime::new();
    let root = mount_root();
    let mut route = fetch_artifact("text", true, false);
    route["routeOutlets"] = serde_json::json!([{"id":"main","node":0}]);
    route["actions"][0]["routeLoader"] = serde_json::json!(true);
    route["actions"][0]["loaderResultState"] = serde_json::json!(0);
    route["actions"][0]["instructions"] = serde_json::json!([
        {"op":"capabilityRequest","capability":"fetch","request":{"url":7,"method":"GET","decode":"text","requireOk":true},"successPc":1,"failurePc":1,"finallyPc":null,"resultSlot":1,"errorSlot":2},
        {"op":"return"}
    ]);
    runtime
        .register_graph("root".into(), serde_wasm_bindgen::to_value(&route).unwrap())
        .unwrap();
    runtime
        .register_graph("page".into(), serde_wasm_bindgen::to_value(&route).unwrap())
        .unwrap();
    runtime
        .register_graph(
            "error".into(),
            serde_wasm_bindgen::to_value(&route_error_artifact()).unwrap(),
        )
        .unwrap();
    let manifest = js_sys::JSON::parse(&serde_json::json!({
        "version":3,"rootGraphId":"root","routes":[
            {"id":"page","path":"*","graphId":"page","errorGraphId":"error","outletId":"main","loaderAction":0}
        ]
    }).to_string()).unwrap();
    runtime.start(root.clone(), manifest).unwrap();
    settle_fetch().await;
    assert_eq!(
        root.text_content().unwrap_or_default(),
        "request failed (418)418short and stoutRetry"
    );
    click_fetch(&root);
    settle_fetch().await;
    assert_eq!(root.text_content().unwrap_or_default(), "loaded");
    restore_plec_fetch();
}
