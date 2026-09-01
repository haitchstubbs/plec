#![cfg(target_arch = "wasm32")]

use plec_runtime::PlecRuntime;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;
use web_sys::{Element, Event};

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen::prelude::wasm_bindgen(inline_js = r#"
let originalFetch;
let fetchQueue = [];
let aborts = 0;
let domMutationCounts = { append: 0, insertBefore: 0, remove: 0 };
const originalAppendChild = Node.prototype.appendChild;
const originalInsertBefore = Node.prototype.insertBefore;
const originalRemoveChild = Node.prototype.removeChild;
Node.prototype.appendChild = function (...args) { domMutationCounts.append++; return originalAppendChild.apply(this, args); };
Node.prototype.insertBefore = function (...args) { domMutationCounts.insertBefore++; return originalInsertBefore.apply(this, args); };
Node.prototype.removeChild = function (...args) { domMutationCounts.remove++; return originalRemoveChild.apply(this, args); };
export function resetPlecDomMutations() { domMutationCounts = { append: 0, insertBefore: 0, remove: 0 }; }
export function plecDomMutations() { return JSON.stringify(domMutationCounts); }
export function setPlecFetchQueue(specs) {
  originalFetch ??= window.fetch;
  fetchQueue = JSON.parse(specs);
  aborts = 0;
  window.fetch = (input, init) => {
    const spec = fetchQueue.shift();
    if (spec.reject) return Promise.reject(Object.assign(new Error(spec.reject), { name: spec.name || 'TypeError' }));
    const signal = input instanceof Request ? input.signal : init?.signal;
    if (spec.pending) return new Promise((_resolve, reject) => signal.addEventListener('abort', () => { aborts++; reject(Object.assign(new Error('aborted'), { name: 'AbortError' })); }));
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
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = resetPlecDomMutations)]
    fn reset_plec_dom_mutations();
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = plecDomMutations)]
    fn plec_dom_mutations() -> String;
}

struct FetchMockGuard;

impl Drop for FetchMockGuard {
    fn drop(&mut self) {
        restore_plec_fetch();
    }
}

fn install_plec_fetch_queue(specs: &str) -> FetchMockGuard {
    set_plec_fetch_queue(specs);
    FetchMockGuard
}

struct BrowserLocationGuard {
    href: String,
}

impl Drop for BrowserLocationGuard {
    fn drop(&mut self) {
        let Some(window) = web_sys::window() else {
            return;
        };
        let _ = window.history().and_then(|history| {
            history.replace_state_with_url(&JsValue::NULL, "", Some(&self.href))
        });
    }
}

fn reset_browser_location() -> BrowserLocationGuard {
    reset_browser_location_to("/")
}

fn reset_browser_location_to(path: &str) -> BrowserLocationGuard {
    let window = web_sys::window().unwrap();
    let location = window.location();
    let href = format!(
        "{}{}{}",
        location.pathname().unwrap(),
        location.search().unwrap(),
        location.hash().unwrap(),
    );
    window
        .history()
        .unwrap()
        .replace_state_with_url(&JsValue::NULL, "", Some(path))
        .unwrap();
    BrowserLocationGuard { href }
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

fn rust_general_async_actions_artifact() -> serde_json::Value {
    serde_json::from_str(include_str!("fixtures/rust-general-async-actions-0.9.json"))
        .expect("Rust general async fixture should be valid JSON")
}

fn component_slot_artifact() -> serde_json::Value {
    serde_json::json!({
        "version":"0.10", "rootComponent":0,
        "components":[
            {"id":"App","rootNode":0,"strings":["main"],"constants":[],
             "nodes":[
                {"op":"element","tag":0,"parent":null,"children":[2]},
                {"op":"component","component":2,"parent":null,"props":[],"children":[]},
                {"op":"component","component":1,"parent":0,"props":[],"children":[1]}
             ],"texts":[],"bindings":[],"propPrograms":[],"events":[],"inputs":[],"stateSlots":[],"parameters":[],"expressions":[],"actions":[],"loops":[],"dependencyEdges":[]},
            {"id":"Frame","rootNode":0,"strings":["section"],"constants":[],
             "nodes":[{"op":"element","tag":0,"parent":null,"children":[1]},{"op":"slot","parent":0}],
             "texts":[],"bindings":[],"propPrograms":[],"events":[],"inputs":[],"stateSlots":[],"parameters":[],"expressions":[],"actions":[],"loops":[],"dependencyEdges":[]},
            {"id":"Child","rootNode":0,"strings":["p"],"constants":[],
             "nodes":[{"op":"element","tag":0,"parent":null,"children":[1]},{"op":"text","text":0,"parent":0}],
             "texts":[{"value":"Inside"}],"bindings":[],"propPrograms":[],"events":[],"inputs":[],"stateSlots":[],"parameters":[],"expressions":[],"actions":[],"loops":[],"dependencyEdges":[]}
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
            {"instructions":[{"op":"loadState","state":0},{"op":"return"}]},
            {"instructions":[{"op":"loadState","state":0},{"op":"unary","kind":"not"},{"op":"return"}]}
        ],
        "actions":[
            {"frameSlots":0,"instructions":[{"op":"evaluate","expression":3},{"op":"storeState","state":0},{"op":"return"}]},
            {"frameSlots":0,"instructions":[{"op":"evaluate","expression":3},{"op":"storeState","state":0},{"op":"return"}]}
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

/// A layout that passes a location-derived prop into a child component whose
/// bindings read that prop. The `<a>` has no href on purpose: the raw
/// `a[href]` navigation sweep must not be able to produce any of the expected
/// DOM state, so only graph propagation can satisfy the assertions.
fn navigation_prop_artifact() -> serde_json::Value {
    serde_json::json!({
        "version":"0.10", "rootComponent":0,
        "components":[
            {"id":"App","rootNode":0,
             "strings":["main","pathname","div","nav"],
             "constants":[],
             "hostSlots":[{"kind":"location","query":null,"name":null}],
             "nodes":[
                {"op":"element","tag":2,"parent":null,"children":[1,2]},
                {"op":"component","component":1,"parent":0,"props":[{"kind":"value","name":1,"expression":0}],"children":[]},
                {"op":"element","tag":3,"parent":0,"children":[]}
             ],
             "texts":[],"bindings":[],"propPrograms":[],"events":[],"inputs":[],"stateSlots":[],"parameters":[],
             "expressions":[{"instructions":[{"op":"loadHost","host":0},{"op":"field","field":1},{"op":"return"}]}],
             "actions":[],"loops":[],
             "routeOutlets":[{"id":"main","node":2}],
             "dependencyEdges":[]},
            {"id":"Nav","rootNode":0,
             "strings":["pathname","a","aria-current","span"],
             "constants":["/next","page",null],
             "nodes":[
                {"op":"element","tag":1,"parent":null,"children":[1,2]},
                {"op":"text","text":0,"parent":0},
                {"op":"element","tag":3,"parent":0,"children":[]}
             ],
             "texts":[{"binding":0}],
             "bindings":[
                {"target":1,"sink":"text","expression":0},
                {"target":0,"sink":"attribute","name":2,"expression":1}
             ],
             "propPrograms":[],"events":[],"inputs":[],"stateSlots":[],
             "parameters":[{"name":0,"callable":false,"component":false}],
             "expressions":[
                {"instructions":[{"op":"loadProp","prop":0},{"op":"return"}]},
                {"instructions":[
                    {"op":"loadProp","prop":0},
                    {"op":"constant","constant":0},
                    {"op":"binary","kind":"equal"},
                    {"op":"jumpIfFalse","target":6},
                    {"op":"constant","constant":1},
                    {"op":"jump","target":7},
                    {"op":"constant","constant":2},
                    {"op":"return"}
                ]}
             ],
             "actions":[],"loops":[],
             "dependencyEdges":[
                {"source":{"kind":"prop","handle":0},"target":{"kind":"binding","handle":0},"loop":null},
                {"source":{"kind":"prop","handle":0},"target":{"kind":"binding","handle":1},"loop":null}
             ]}
        ]
    })
}

fn navigation_route_artifact() -> serde_json::Value {
    serde_json::json!({
        "version":"0.10", "rootComponent":0,
        "components":[
            {"id":"Page","rootNode":0,"strings":["p"],"constants":[],
             "nodes":[{"op":"element","tag":0,"parent":null,"children":[1]},{"op":"text","text":0,"parent":0}],
             "texts":[{"value":"Route body"}],"bindings":[],"propPrograms":[],"events":[],"inputs":[],"stateSlots":[],"parameters":[],"expressions":[],"actions":[],"loops":[],"dependencyEdges":[]}
        ]
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

fn apply_deltas(runtime: &PlecRuntime, deltas: serde_json::Value) -> serde_json::Value {
    serde_wasm_bindgen::from_value(
        runtime
            .apply_deltas(serde_wasm_bindgen::to_value(&deltas).unwrap())
            .unwrap(),
    )
    .unwrap()
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
fn keyed_value_batch_updates_only_its_field_bindings_without_row_mutation() {
    let runtime = PlecRuntime::new();
    let root = mount_root();
    load_and_mount(&runtime, keyed_row_artifact(), &root);
    apply_delta(
        &runtime,
        serde_json::json!({"type":"insert","input_id":"items","row_key":"one","row":{"id":"one","title":"One"},"before_row_key":null}),
    );
    apply_delta(
        &runtime,
        serde_json::json!({"type":"insert","input_id":"items","row_key":"two","row":{"id":"two","title":"Two"},"before_row_key":null}),
    );
    let first = root.query_selector("li").unwrap().unwrap();
    reset_plec_dom_mutations();

    let metrics = apply_deltas(
        &runtime,
        serde_json::json!([
            {"type":"update","input_id":"items","row_key":"one","changes":{"title":"One+"}},
            {"type":"update","input_id":"items","row_key":"two","changes":{"title":"Two+"}}
        ]),
    );
    let mutations: serde_json::Value = serde_json::from_str(&plec_dom_mutations()).unwrap();

    assert_eq!(mutations["append"], 0);
    assert_eq!(mutations["insertBefore"], 0);
    assert_eq!(mutations["remove"], 0);
    assert_eq!(metrics["rowInserts"], 0);
    assert_eq!(metrics["rowRemoves"], 0);
    assert_eq!(metrics["rowMoves"], 0);
    assert_eq!(metrics["bindingsTouched"], 2);
    assert_eq!(root.query_selector_all("li").unwrap().length(), 2);
    assert!(first.is_same_node(
        root.query_selector("li")
            .unwrap()
            .as_ref()
            .map(|node| node.unchecked_ref())
    ));
    assert_eq!(
        root.query_selector("li")
            .unwrap()
            .unwrap()
            .text_content()
            .as_deref(),
        Some("One+")
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

#[wasm_bindgen_test(async)]
async fn rust_general_async_actions_fixture_preserves_frame_and_finally_lifecycle() {
    {
        let _fetch = install_plec_fetch_queue(r#"[{"body":"{\"id\":\"one\",\"title\":\"New\"}"}]"#);
        let runtime = PlecRuntime::new();
        let root = mount_root();
        load_and_mount(&runtime, rust_general_async_actions_artifact(), &root);
        initialize_rows(&runtime, serde_json::json!([{"id":"one","title":"Old"}]));
        let row = root.query_selector("li").unwrap().unwrap();
        let text = row.first_child().unwrap();
        root.query_selector("button")
            .unwrap()
            .unwrap()
            .dyn_into::<web_sys::EventTarget>()
            .unwrap()
            .dispatch_event(&Event::new("click").unwrap())
            .unwrap();
        settle_fetch().await;
        let next = root.query_selector("li").unwrap().unwrap();
        assert!(row.is_same_node(Some(&next)));
        assert!(text.is_same_node(next.first_child().as_ref()));
        assert_eq!(next.text_content().unwrap(), "New");
        assert_eq!(
            root.query_selector_all("p")
                .unwrap()
                .item(1)
                .unwrap()
                .text_content()
                .unwrap(),
            "true"
        );
    }

    {
        let _fetch = install_plec_fetch_queue(r#"[{"reject":"offline"}]"#);
        let runtime = PlecRuntime::new();
        let root = mount_root();
        load_and_mount(&runtime, rust_general_async_actions_artifact(), &root);
        initialize_rows(&runtime, serde_json::json!([{"id":"one","title":"Old"}]));
        root.query_selector("button")
            .unwrap()
            .unwrap()
            .dyn_into::<web_sys::EventTarget>()
            .unwrap()
            .dispatch_event(&Event::new("click").unwrap())
            .unwrap();
        settle_fetch().await;
        assert_eq!(
            root.query_selector("p")
                .unwrap()
                .unwrap()
                .text_content()
                .unwrap(),
            "network request failed"
        );
        assert_eq!(
            root.query_selector_all("p")
                .unwrap()
                .item(1)
                .unwrap()
                .text_content()
                .unwrap(),
            "true"
        );
    }

    {
        let _fetch = install_plec_fetch_queue(r#"[{"pending":true}]"#);
        let runtime = PlecRuntime::new();
        let root = mount_root();
        load_and_mount(&runtime, rust_general_async_actions_artifact(), &root);
        root.query_selector("button")
            .unwrap()
            .unwrap()
            .dyn_into::<web_sys::EventTarget>()
            .unwrap()
            .dispatch_event(&Event::new("click").unwrap())
            .unwrap();
        runtime.dispose().unwrap();
        settle_fetch().await;
        assert_eq!(plec_fetch_aborts(), 1);
        assert_eq!(root.text_content().unwrap_or_default(), "");
    }
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
fn rust_component_registry_routes_without_legacy_renderer() {
    let runtime = PlecRuntime::new();
    let root = mount_root();
    runtime
        .register_graph(
            "rust-component.tsx#App".into(),
            serde_wasm_bindgen::to_value(&rust_component_artifact()).unwrap(),
        )
        .unwrap();
    let manifest =
        js_sys::JSON::parse(r#"{"version":3,"rootGraphId":"rust-component.tsx#App","routes":[]}"#)
            .unwrap();
    runtime.start(root.clone(), manifest).unwrap();
    assert_eq!(root.text_content().unwrap(), "one");
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
    let _fetch = install_plec_fetch_queue(r#"[{"body":"ok"}]"#);
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
    assert_eq!(
        root.text_content().unwrap(),
        "retained rowretained rowclick"
    );
}

#[wasm_bindgen_test(async)]
async fn disposing_a_typed_graph_aborts_and_discards_its_fetch() {
    let _fetch = install_plec_fetch_queue(r#"[{"pending":true}]"#);
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
        let _fetch = install_plec_fetch_queue(spec);
        let runtime = PlecRuntime::new();
        let root = mount_root();
        load_and_mount(&runtime, fetch_artifact(decode, true, false), &root);
        click_fetch(&root);
        settle_fetch().await;
        assert_eq!(root.text_content().unwrap(), expected);
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
        let _fetch = install_plec_fetch_queue(spec);
        let runtime = PlecRuntime::new();
        let root = mount_root();
        load_and_mount(&runtime, fetch_artifact("json", true, false), &root);
        click_fetch(&root);
        settle_fetch().await;
        let output = root.text_content().unwrap();
        assert!(output.contains(expected), "{output}");
        assert!(output.contains("\"url\":\"url\""), "{output}");
        assert!(output.ends_with("inner"), "{output}");
    }

    let _fetch = install_plec_fetch_queue(r#"[{"status":418,"body":"teapot"}]"#);
    let runtime = PlecRuntime::new();
    let root = mount_root();
    load_and_mount(&runtime, fetch_artifact("text", false, false), &root);
    click_fetch(&root);
    settle_fetch().await;
    assert_eq!(root.text_content().unwrap(), "teapotinner");
}

#[wasm_bindgen_test(async)]
async fn typed_fetch_runs_nested_finalizers_inner_to_outer() {
    let _fetch = install_plec_fetch_queue(r#"[{"body":"outer"},{"body":"inner"}]"#);
    let runtime = PlecRuntime::new();
    let root = mount_root();
    load_and_mount(&runtime, fetch_artifact("text", true, true), &root);
    click_fetch(&root);
    settle_fetch().await;
    assert_eq!(root.text_content().unwrap(), "innerinnerouter");
}

#[wasm_bindgen_test(async)]
async fn suspended_callee_resumes_its_caller_continuation() {
    let _fetch = install_plec_fetch_queue(r#"[{"body":"resumed"}]"#);
    let runtime = PlecRuntime::new();
    let root = mount_root();
    load_and_mount(&runtime, caller_continuation_artifact(), &root);
    click_fetch(&root);
    settle_fetch().await;
    assert_eq!(root.text_content().unwrap(), "resumed");
}

#[wasm_bindgen_test(async)]
async fn remount_and_typed_route_replacement_abort_stale_fetches() {
    let _location = reset_browser_location();
    let fetch = install_plec_fetch_queue(r#"[{"pending":true}]"#);
    let runtime = PlecRuntime::new();
    let root = mount_root();
    load_and_mount(&runtime, fetch_artifact("text", true, false), &root);
    click_fetch(&root);
    runtime.mount(root.clone()).unwrap();
    settle_fetch().await;
    assert_eq!(plec_fetch_aborts(), 1);
    assert_eq!(root.text_content().unwrap_or_default(), "");
    drop(fetch);

    let _fetch = install_plec_fetch_queue(r#"[{"pending":true}]"#);
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
}

#[wasm_bindgen_test]
fn navigation_refreshes_location_props_in_child_components() {
    let _location = reset_browser_location();
    let runtime = PlecRuntime::new();
    let root = mount_root();
    runtime
        .register_graph(
            "root".into(),
            serde_wasm_bindgen::to_value(&navigation_prop_artifact()).unwrap(),
        )
        .unwrap();
    runtime
        .register_graph(
            "page".into(),
            serde_wasm_bindgen::to_value(&navigation_route_artifact()).unwrap(),
        )
        .unwrap();
    let manifest = js_sys::JSON::parse(
        &serde_json::json!({
            "version":3,"rootGraphId":"root","routes":[
                {"id":"index","path":"*","graphId":"page","outletId":"main"}
            ]
        })
        .to_string(),
    )
    .unwrap();
    runtime.start(root.clone(), manifest).unwrap();
    let link = root.query_selector("a").unwrap().unwrap();
    assert_eq!(link.text_content().as_deref(), Some("/"));
    assert_eq!(link.get_attribute("aria-current").as_deref(), Some(""));
    runtime.navigate("/next".into(), false).unwrap();
    let link = root.query_selector("a").unwrap().unwrap();
    assert_eq!(link.text_content().as_deref(), Some("/next"));
    assert_eq!(link.get_attribute("aria-current").as_deref(), Some("page"));
}

#[wasm_bindgen_test(async)]
async fn typed_route_loader_writes_its_declared_result_state() {
    let _location = reset_browser_location();
    let _fetch = install_plec_fetch_queue(r#"[{"body":"loaded"}]"#);
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
}

#[wasm_bindgen_test(async)]
async fn rust_route_async_fixture_navigates_and_disposes_stale_loader() {
    let _location = reset_browser_location();
    let fetch = install_plec_fetch_queue(r#"[{"body":"loaded"}]"#);
    let runtime = PlecRuntime::new();
    let root = mount_root();
    let route = rust_route_async_artifact();
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
    drop(fetch);

    let fetch = install_plec_fetch_queue(r#"[{"status":500,"statusText":"Failed","body":"nope"}]"#);
    let runtime = PlecRuntime::new();
    let root = mount_root();
    let route = rust_route_async_artifact();
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
    assert!(root
        .text_content()
        .unwrap_or_default()
        .contains("request failed (500)"));
    drop(fetch);

    let _fetch = install_plec_fetch_queue(r#"[{"pending":true}]"#);
    let runtime = PlecRuntime::new();
    let root = mount_root();
    let route = rust_route_async_artifact();
    runtime
        .register_graph("root".into(), serde_wasm_bindgen::to_value(&route).unwrap())
        .unwrap();
    runtime
        .register_graph("page".into(), serde_wasm_bindgen::to_value(&route).unwrap())
        .unwrap();
    runtime
        .register_graph(
            "next".into(),
            serde_wasm_bindgen::to_value(&fetch_artifact("text", true, false)).unwrap(),
        )
        .unwrap();
    let manifest = js_sys::JSON::parse(
        &serde_json::json!({
            "version":3,"rootGraphId":"root","routes":[
                {"id":"page","path":"/","graphId":"page","outletId":"main","loaderAction":0},
                {"id":"next","path":"/next","graphId":"next","outletId":"main"}
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
}

#[wasm_bindgen_test(async)]
async fn typed_route_error_receives_fetch_failure_and_retry_reloads() {
    let _location = reset_browser_location();
    let _fetch = install_plec_fetch_queue(
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
}

// --- SSR adoption ownership index -------------------------------------------

/// Server-rendered shape for `rust-nested-component-0.10.json`: the root
/// "main" graph calls Section -> Grandchild, so adoption must claim the root
/// scope plus one nested sibling range per component call. Marker paths
/// mirror the server's `renderNode` emission exactly.
fn nested_adoption_html(duplicate_text_marker: bool) -> String {
    let text_marker = "<!--plec:text:root/component:1/component:1:2-->";
    let span_marker = if duplicate_text_marker {
        format!("{text_marker}{text_marker}")
    } else {
        text_marker.to_owned()
    };
    let mut html = String::new();
    html.push_str("<main data-plec-node=\"root/node:0\">");
    html.push_str("<!--plec:component:root:1-->");
    html.push_str("<section data-plec-node=\"root/component:1/node:0\">");
    html.push_str("<!--plec:component:root/component:1:1-->");
    html.push_str("<div data-plec-node=\"root/component:1/component:1/node:0\">");
    html.push_str("<span data-plec-node=\"root/component:1/component:1/node:1\">");
    html.push_str(&span_marker);
    html.push_str("one</span>");
    html.push_str("<button data-plec-node=\"root/component:1/component:1/node:3\">");
    html.push_str("<!--plec:text:root/component:1/component:1:4-->0</button>");
    html.push_str("</div>");
    html.push_str("<!--plec:component-end:root/component:1:1-->");
    html.push_str("</section>");
    html.push_str("<!--plec:component-end:root:1-->");
    html.push_str("<button data-plec-node=\"root/node:2\"></button>");
    html.push_str("</main>");
    html
}

fn start_adopt_fixture(runtime: &PlecRuntime, root: &Element, html: &str) -> Result<(), JsValue> {
    root.set_inner_html(html);
    runtime
        .register_graph(
            "rust-nested-component.tsx#App".into(),
            serde_wasm_bindgen::to_value(&rust_nested_component_artifact()).unwrap(),
        )
        .unwrap();
    let manifest = js_sys::JSON::parse(
        r#"{"version":3,"rootGraphId":"rust-nested-component.tsx#App","routes":[]}"#,
    )
    .unwrap();
    runtime.start_adopt(root.clone(), manifest)
}

#[wasm_bindgen_test]
fn adopted_nested_components_claim_scoped_indexes_and_stay_live() {
    let runtime = PlecRuntime::new();
    let root = mount_root();
    start_adopt_fixture(&runtime, &root, &nested_adoption_html(false)).unwrap();

    // Claims bound the intended nodes: the grandchild span carries the
    // propagated prop and its button owns the click action plus the state
    // text binding, all updated in place on the server DOM.
    let span = root.query_selector("span").unwrap().unwrap();
    let button = root.query_selector("div button").unwrap().unwrap();
    assert_eq!(span.text_content().unwrap(), "one");
    let button_text = button.text_content().unwrap();
    assert_eq!(button_text, "0", "button text before click: {button_text}");
    button
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    assert_eq!(
        root.query_selector("span")
            .unwrap()
            .unwrap()
            .text_content()
            .unwrap(),
        "one"
    );
    assert_eq!(
        root.query_selector("div button")
            .unwrap()
            .unwrap()
            .text_content()
            .unwrap(),
        "1"
    );

    // Scoped index construction: this fixture performs three adoptions over
    // ~31 owned nodes (root scope 15, section range 9, grandchild range 7).
    // Rebuilding a full-root comment map per component level would walk at
    // least 45 nodes.
    // The visit counter is process-global, so reset it to measure only this
    // fixture's walks instead of inheriting earlier tests' adoptions.
    runtime.reset_adoption_index_walks();
    start_adopt_fixture(&runtime, &mount_root(), &nested_adoption_html(false)).unwrap();
    let walks = runtime.adoption_index_walks();
    assert!(
        walks < 45,
        "nested adoption must not re-walk the adoption root (walked {walks})"
    );
}

#[wasm_bindgen_test]
fn adopted_duplicate_marker_fails_instead_of_silently_claiming() {
    let runtime = PlecRuntime::new();
    let error =
        start_adopt_fixture(&runtime, &mount_root(), &nested_adoption_html(true)).unwrap_err();
    let message = error.as_string().unwrap_or_default();
    assert!(
        message.contains("duplicate:ssr-marker:plec:text:root/component:1/component:1:2"),
        "unexpected adoption error: {message}"
    );
}

#[wasm_bindgen_test]
fn adopted_missing_and_mismatched_markers_fail_closed_with_unchanged_codes() {
    // Absent element marker.
    let html =
        nested_adoption_html(false).replace("<button data-plec-node=\"root/node:2\"></button>", "");
    let error = start_adopt_fixture(&PlecRuntime::new(), &mount_root(), &html).unwrap_err();
    assert!(error
        .as_string()
        .unwrap_or_default()
        .contains("missing:ssr-node:root/node:2"));

    // Absent text marker, surfaced from a nested scoped index.
    let html =
        nested_adoption_html(false).replace("<!--plec:text:root/component:1/component:1:2-->", "");
    let error = start_adopt_fixture(&PlecRuntime::new(), &mount_root(), &html).unwrap_err();
    assert!(
        error
            .as_string()
            .unwrap_or_default()
            .contains("missing:ssr-text:root/component:1/component:1:2"),
        "unexpected adoption error: {}",
        error.as_string().unwrap_or_default()
    );

    // Absent component boundary.
    let html = nested_adoption_html(false).replace("<!--plec:component:root:1-->", "");
    let error = start_adopt_fixture(&PlecRuntime::new(), &mount_root(), &html).unwrap_err();
    assert!(error
        .as_string()
        .unwrap_or_default()
        .contains("missing:ssr-component:root:1"));

    // Wrong element tag.
    let html = nested_adoption_html(false)
        .replace(
            "<main data-plec-node=\"root/node:0\">",
            "<section data-plec-node=\"root/node:0\">",
        )
        .replace("</main>", "</section>");
    let error = start_adopt_fixture(&PlecRuntime::new(), &mount_root(), &html).unwrap_err();
    assert!(error
        .as_string()
        .unwrap_or_default()
        .contains("mismatch:ssr-tag:root/node:0"));
}

// ---------------------------------------------------------------------------
// SSR snapshot import pipeline
//
// One text node whose static binding reads the `loaderData` host slot (a
// direct host-input read). The snapshot seeds a public export named
// `loaderData`, so adoption must recompute the binding from the imported
// value instead of a blank initialiser.
// ---------------------------------------------------------------------------

fn snapshot_fixture_artifact() -> serde_json::Value {
    serde_json::json!({
        "version": "0.10",
        "rootComponent": 0,
        "components": [{
            "id": "ssr-snapshot.tsx#App",
            "rootNode": 0,
            "strings": ["main", "p"],
            "constants": [],
            "nodes": [
                {"op": "element", "tag": 0, "parent": null, "children": [1]},
                {"op": "element", "tag": 1, "parent": 0, "children": [2]},
                {"op": "text", "text": 0, "parent": 1}
            ],
            "texts": [{"binding": 0}],
            "bindings": [{"target": 2, "sink": "text", "expression": 0}],
            "propPrograms": [],
            "events": [],
            "inputs": [],
            "hostSlots": [{"kind": "loaderData"}],
            "stateSlots": [],
            "parameters": [],
            "expressions": [
                {"instructions": [{"op": "loadHost", "host": 0}, {"op": "return"}]}
            ],
            "actions": [],
            "loops": [],
            "dependencyEdges": [],
            "routeOutlets": [{"id": "main", "node": 0}]
        }]
    })
}

/// The route page graph mounted into the root layout's outlet: an empty span
/// with no bindings, claimed at the `root/outlet:main` instance path.
fn snapshot_route_artifact() -> serde_json::Value {
    serde_json::json!({
        "version": "0.10",
        "rootComponent": 0,
        "components": [{
            "id": "ssr-snapshot.tsx#Home",
            "rootNode": 0,
            "strings": ["span"],
            "constants": [],
            "nodes": [{"op": "element", "tag": 0, "parent": null, "children": []}],
            "texts": [],
            "bindings": [],
            "propPrograms": [],
            "events": [],
            "inputs": [],
            "hostSlots": [],
            "stateSlots": [],
            "parameters": [],
            "expressions": [],
            "actions": [],
            "loops": [],
            "dependencyEdges": [],
            "routeOutlets": []
        }]
    })
}

fn snapshot_fixture(
    revision: &str,
    export_value: &str,
    location: &str,
    structure_graph: &str,
) -> serde_json::Value {
    snapshot_chain_fixture(
        revision,
        export_value,
        location,
        structure_graph,
        serde_json::json!({}),
    )
}

fn snapshot_chain_fixture(
    revision: &str,
    export_value: &str,
    location: &str,
    structure_graph: &str,
    params: serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "version": 1,
        "revision": revision,
        "routes": [{"routeId": "ssr-snapshot.tsx#Home", "params": params, "phase": "active"}],
        "public": {"location": location, "exports": {
            "loaderData": {
                "value": export_value,
                "declaration": {
                    "name": "loaderData",
                    "sourceOwner": "server",
                    "valueIsSerializable": true,
                    "explicitlyPublic": true
                }
            }
        }},
        "loaders": [],
        "structure": {"graphs": {"root/outlet:main": {"graphId": structure_graph}}}
    })
}

fn start_snapshot_fixture(
    runtime: &PlecRuntime,
    root: &Element,
    server_text: &str,
    snapshot: serde_json::Value,
) -> Result<(), JsValue> {
    start_snapshot_route_fixture(runtime, root, server_text, snapshot, "")
}

fn start_snapshot_route_fixture(
    runtime: &PlecRuntime,
    root: &Element,
    server_text: &str,
    snapshot: serde_json::Value,
    route_path: &str,
) -> Result<(), JsValue> {
    root.set_inner_html(&format!(
        "<main data-plec-node=\"root/node:0\"><p data-plec-node=\"root/node:1\">\
         <!--plec:text:root:2-->{server_text}</p>\
         <span data-plec-node=\"root/outlet:main/node:0\"></span></main>"
    ));
    runtime
        .register_graph(
            "ssr-snapshot.tsx#Home".into(),
            serde_wasm_bindgen::to_value(&snapshot_route_artifact()).unwrap(),
        )
        .unwrap();
    // Register the root application last: snapshot structure validation reads
    // the most recently registered 0.10 application.
    runtime
        .register_graph(
            "ssr-snapshot.tsx#App".into(),
            serde_wasm_bindgen::to_value(&snapshot_fixture_artifact()).unwrap(),
        )
        .unwrap();
    let manifest = js_sys::JSON::parse(&format!(
        r#"{{"version":3,"revision":"rev-1","rootGraphId":"ssr-snapshot.tsx#App",
            "routes":[{{"id":"ssr-snapshot.tsx#Home","path":"{route_path}",
            "graphId":"ssr-snapshot.tsx#Home","outletId":"main"}}]}}"#,
    ))
    .unwrap();
    let snapshot = serde_wasm_bindgen::to_value(&snapshot).unwrap();
    runtime.start_adopt_snapshot(root.clone(), manifest, snapshot)
}

fn snapshot_root_text(root: &Element) -> String {
    root.query_selector("p")
        .unwrap()
        .unwrap()
        .text_content()
        .unwrap_or_default()
}

#[wasm_bindgen_test]
fn snapshot_import_seeds_public_exports_before_adoption() {
    let _location = reset_browser_location();
    let runtime = PlecRuntime::new();
    let root = mount_root();
    start_snapshot_fixture(
        &runtime,
        &root,
        "seeded-value",
        snapshot_fixture("rev-1", "seeded-value", "/", "ssr-snapshot.tsx#App"),
    )
    .unwrap();
    // The recomputed binding evaluates from the imported export, matching the
    // server DOM exactly: seeded state, no divergence.
    assert_eq!(snapshot_root_text(&root), "seeded-value");
    assert_eq!(runtime.ssr_text_divergences(), 0);
}

#[wasm_bindgen_test]
fn snapshot_binding_divergence_is_allowed_and_reported() {
    let _location = reset_browser_location();
    let runtime = PlecRuntime::new();
    let root = mount_root();
    start_snapshot_fixture(
        &runtime,
        &root,
        "server-value",
        snapshot_fixture("rev-1", "seeded-value", "/", "ssr-snapshot.tsx#App"),
    )
    .unwrap();
    // Ordinary binding-value divergence never fails adoption: recompute
    // consequences win, and the divergence is observable for dev reporting.
    assert_eq!(snapshot_root_text(&root), "seeded-value");
    assert_eq!(runtime.ssr_text_divergences(), 1);
}

#[wasm_bindgen_test]
fn snapshot_version_and_revision_gates_have_dedicated_codes() {
    let _location = reset_browser_location();
    let mut snapshot = snapshot_fixture("rev-1", "x", "/", "ssr-snapshot.tsx#App");
    snapshot["version"] = serde_json::json!(999);
    let error =
        start_snapshot_fixture(&PlecRuntime::new(), &mount_root(), "x", snapshot).unwrap_err();
    assert_eq!(
        error.as_string().unwrap_or_default(),
        "unsupported:ssr-snapshot-version"
    );

    let snapshot = snapshot_fixture("other-revision", "x", "/", "ssr-snapshot.tsx#App");
    let error =
        start_snapshot_fixture(&PlecRuntime::new(), &mount_root(), "x", snapshot).unwrap_err();
    assert_eq!(error.as_string().unwrap_or_default(), "stale-revision");
}

#[wasm_bindgen_test]
fn snapshot_payload_and_structure_failures_fail_closed() {
    let _location = reset_browser_location();
    // Unparseable payload: a bare string cannot decode as a snapshot.
    let runtime = PlecRuntime::new();
    let root = mount_root();
    root.set_inner_html("<main data-plec-node=\"root/node:0\"></main>");
    runtime
        .register_graph(
            "ssr-snapshot.tsx#App".into(),
            serde_wasm_bindgen::to_value(&snapshot_fixture_artifact()).unwrap(),
        )
        .unwrap();
    let manifest = js_sys::JSON::parse(
        r#"{"version":3,"revision":"rev-1","rootGraphId":"ssr-snapshot.tsx#App","routes":[]}"#,
    )
    .unwrap();
    let error = runtime
        .start_adopt_snapshot(root.clone(), manifest, JsValue::from_str("not-a-snapshot"))
        .unwrap_err();
    assert_eq!(
        error.as_string().unwrap_or_default(),
        "mismatch:ssr-snapshot-payload"
    );

    // Structure referencing an unknown component graph.
    let snapshot = snapshot_fixture("rev-1", "x", "/", "missing.tsx#Nope");
    let error =
        start_snapshot_fixture(&PlecRuntime::new(), &mount_root(), "x", snapshot).unwrap_err();
    assert!(error
        .as_string()
        .unwrap_or_default()
        .starts_with("mismatch:ssr-snapshot:unknown ssr snapshot graph"));
}

#[wasm_bindgen_test]
fn snapshot_location_mismatch_fails_closed() {
    let _location = reset_browser_location();
    let snapshot = snapshot_fixture("rev-1", "x", "/other-page", "ssr-snapshot.tsx#App");
    let error =
        start_snapshot_fixture(&PlecRuntime::new(), &mount_root(), "x", snapshot).unwrap_err();
    assert_eq!(
        error.as_string().unwrap_or_default(),
        "mismatch:ssr-location"
    );
}

#[wasm_bindgen_test]
fn snapshot_param_route_chain_adopts_with_imported_params() {
    let _location = reset_browser_location_to("/projects/p1");
    let runtime = PlecRuntime::new();
    let root = mount_root();
    let snapshot = snapshot_chain_fixture(
        "rev-1",
        "seeded-value",
        "/projects/p1",
        "ssr-snapshot.tsx#App",
        serde_json::json!({"projectId": "p1"}),
    );
    // The URL-derived chain agrees with the imported chain, including the
    // decoded $param value, so adoption proceeds instead of falling back.
    start_snapshot_route_fixture(
        &runtime,
        &root,
        "seeded-value",
        snapshot,
        "projects/$projectId",
    )
    .unwrap();
    assert_eq!(snapshot_root_text(&root), "seeded-value");
    assert_eq!(runtime.ssr_text_divergences(), 0);
}

#[wasm_bindgen_test]
fn snapshot_param_value_mismatch_fails_closed() {
    let _location = reset_browser_location_to("/projects/p1");
    let snapshot = snapshot_chain_fixture(
        "rev-1",
        "x",
        "/projects/p1",
        "ssr-snapshot.tsx#App",
        serde_json::json!({"projectId": "other"}),
    );
    let error = start_snapshot_route_fixture(
        &PlecRuntime::new(),
        &mount_root(),
        "x",
        snapshot,
        "projects/$projectId",
    )
    .unwrap_err();
    assert_eq!(
        error.as_string().unwrap_or_default(),
        "mismatch:ssr-route-chain:params:0:projectId"
    );
}

#[wasm_bindgen_test]
fn snapshot_chain_route_disagreement_fails_closed() {
    let _location = reset_browser_location_to("/about");
    // The URL resolves to no manifest route while the snapshot claims the
    // home page: the transferred cause contradicts the browser URL.
    let snapshot = snapshot_fixture("rev-1", "x", "/about", "ssr-snapshot.tsx#App");
    let error =
        start_snapshot_fixture(&PlecRuntime::new(), &mount_root(), "x", snapshot).unwrap_err();
    assert_eq!(
        error.as_string().unwrap_or_default(),
        "mismatch:ssr-route-chain:length:1:0"
    );
}

#[wasm_bindgen_test]
fn snapshot_non_active_phase_fails_closed() {
    let _location = reset_browser_location();
    let mut snapshot = snapshot_fixture("rev-1", "x", "/", "ssr-snapshot.tsx#App");
    snapshot["routes"][0]["phase"] = serde_json::json!("pending");
    let error =
        start_snapshot_fixture(&PlecRuntime::new(), &mount_root(), "x", snapshot).unwrap_err();
    assert_eq!(
        error.as_string().unwrap_or_default(),
        "mismatch:ssr-route-chain:phase:0:pending"
    );
}

#[wasm_bindgen_test]
fn abandon_adoption_purges_seeded_host_inputs() {
    let _location = reset_browser_location();
    let runtime = PlecRuntime::new();
    let root = mount_root();
    start_snapshot_fixture(
        &runtime,
        &root,
        "seeded-value",
        snapshot_fixture("rev-1", "seeded-value", "/", "ssr-snapshot.tsx#App"),
    )
    .unwrap();
    runtime.abandon_adoption();
    // A subsequent adoption without a snapshot recomputes from blank host
    // inputs: the seeded export must not leak into the fallback remount.
    start_adopt_snapshot_fixture_without_snapshot(&runtime, &root).unwrap();
    assert_eq!(snapshot_root_text(&root), "");
    assert_eq!(runtime.ssr_text_divergences(), 0);
}

fn start_adopt_snapshot_fixture_without_snapshot(
    runtime: &PlecRuntime,
    root: &Element,
) -> Result<(), JsValue> {
    root.set_inner_html(
        "<main data-plec-node=\"root/node:0\"><p data-plec-node=\"root/node:1\">\
         <!--plec:text:root:2-->ignored</p></main>",
    );
    let manifest = js_sys::JSON::parse(
        r#"{"version":3,"revision":"rev-1","rootGraphId":"ssr-snapshot.tsx#App","routes":[]}"#,
    )
    .unwrap();
    runtime.start_adopt_snapshot(root.clone(), manifest, wasm_bindgen::JsValue::UNDEFINED)
}

// ---------------------------------------------------------------------------
// SSR conditional adoption
//
// One static conditional in the route page graph (p vs span branch) plus a
// toggle button outside the region. The snapshot's branch record is the
// ownership cause: adoption claims the marked region with the recorded side,
// and branch flips run through the normal reconcile path.
// ---------------------------------------------------------------------------

/// The route page graph. The conditional's node handle is 1; its branches are
/// the p (2, consequent) and span (3, alternate) elements. Initial state is
/// false, so a client reconcile always starts on the alternate side.
fn conditional_route_artifact() -> serde_json::Value {
    serde_json::json!({
        "version": "0.10",
        "rootComponent": 0,
        "components": [{
            "id": "ssr-conditional.tsx#Home",
            "rootNode": 0,
            "strings": ["section", "p", "span", "button", "click"],
            "constants": [false],
            "nodes": [
                {"op": "element", "tag": 0, "children": [1, 4]},
                {"op": "conditional", "test": 1, "parent": 0, "consequent": 2, "alternate": 3},
                {"op": "element", "tag": 1, "parent": 0, "children": []},
                {"op": "element", "tag": 2, "parent": 0, "children": []},
                {"op": "element", "tag": 3, "parent": 0, "children": []}
            ],
            "texts": [],
            "bindings": [],
            "propPrograms": [],
            "events": [{"target": 4, "type": 4, "action": 0, "fields": []}],
            "inputs": [],
            "hostSlots": [],
            "stateSlots": [{"initialExpression": 0, "frameSlot": 0}],
            "parameters": [],
            "expressions": [
                {"instructions": [{"op": "constant", "constant": 0}, {"op": "return"}]},
                {"instructions": [{"op": "loadState", "state": 0}, {"op": "return"}]},
                {"instructions": [{"op": "loadState", "state": 0}, {"op": "unary", "kind": "not"}, {"op": "return"}]}
            ],
            "actions": [{"frameSlots": 0, "instructions": [
                {"op": "evaluate", "expression": 2},
                {"op": "storeState", "state": 0},
                {"op": "return"}
            ]}],
            "loops": [],
            "dependencyEdges": [{"source": {"kind": "state", "handle": 0}, "target": {"kind": "conditional", "handle": 1}}],
            "routeOutlets": []
        }]
    })
}

/// The server markup the route graph renders for one recorded branch side.
fn conditional_server_dom(selected: &str) -> String {
    let inner = match selected {
        "consequent" => "<p data-plec-node=\"root/outlet:main/node:2\"></p>",
        "alternate" => "<span data-plec-node=\"root/outlet:main/node:3\"></span>",
        _ => "",
    };
    format!(
        "<!--plec:conditional:root/outlet:main:1-->{inner}\
         <!--plec:conditional-end:root/outlet:main:1-->\
         <button data-plec-node=\"root/outlet:main/node:4\"></button>"
    )
}

/// Snapshot structure with branch records for the conditional route instance.
fn conditional_snapshot(branches: serde_json::Value) -> serde_json::Value {
    let mut snapshot = snapshot_chain_fixture(
        "rev-1",
        "x",
        "/",
        "ssr-snapshot.tsx#App",
        serde_json::json!({}),
    );
    // Nested instance ids embed the parent segment escaped, mirroring
    // graph_instance_id exactly.
    snapshot["structure"]["graphs"]["root%2Foutlet:main/outlet:main"] = serde_json::json!({
        "graphId": "ssr-conditional.tsx#Home",
        "branches": branches,
    });
    snapshot
}

fn start_conditional_fixture(
    runtime: &PlecRuntime,
    root: &Element,
    outlet_dom: &str,
    branches: serde_json::Value,
) -> Result<(), JsValue> {
    root.set_inner_html(&format!(
        "<main data-plec-node=\"root/node:0\"><p data-plec-node=\"root/node:1\">\
         <!--plec:text:root:2-->ignored</p>\
         <section data-plec-node=\"root/outlet:main/node:0\">{outlet_dom}</section></main>"
    ));
    runtime
        .register_graph(
            "ssr-conditional.tsx#Home".into(),
            serde_wasm_bindgen::to_value(&conditional_route_artifact()).unwrap(),
        )
        .unwrap();
    runtime
        .register_graph(
            "ssr-snapshot.tsx#App".into(),
            serde_wasm_bindgen::to_value(&snapshot_fixture_artifact()).unwrap(),
        )
        .unwrap();
    let manifest = js_sys::JSON::parse(
        r#"{"version":3,"revision":"rev-1","rootGraphId":"ssr-snapshot.tsx#App",
            "routes":[{"id":"ssr-snapshot.tsx#Home","path":"",
            "graphId":"ssr-conditional.tsx#Home","outletId":"main"}]}"#,
    )
    .unwrap();
    let snapshot = serde_wasm_bindgen::to_value(&conditional_snapshot(branches)).unwrap();
    runtime.start_adopt_snapshot(root.clone(), manifest, snapshot)
}

fn conditional_toggle(root: &Element) -> web_sys::EventTarget {
    root.query_selector("[data-plec-node='root/outlet:main/node:4']")
        .unwrap()
        .unwrap()
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
}

#[wasm_bindgen_test]
fn ssr_conditional_adopts_recorded_branch_and_flips_through_reconcile() {
    let _location = reset_browser_location();
    let runtime = PlecRuntime::new();
    let root = mount_root();
    start_conditional_fixture(
        &runtime,
        &root,
        &conditional_server_dom("alternate"),
        serde_json::json!([{"node": 1, "selected": "alternate"}]),
    )
    .unwrap();
    // The server-rendered alternate branch survived adoption untouched.
    let adopted_span = root
        .query_selector("[data-plec-node='root/outlet:main/node:3']")
        .unwrap()
        .unwrap();
    assert!(root
        .query_selector("[data-plec-node='root/outlet:main/node:2']")
        .unwrap()
        .is_none());
    // The recomputed initial test value (false) agrees with the recorded
    // side, so the first toggle flips the region through the normal
    // reconcile path: the span is replaced by a freshly instantiated
    // consequent p (client nodes carry data-runtime-node markers).
    conditional_toggle(&root)
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    let flip_one = root
        .query_selector("[data-runtime-node='2']")
        .unwrap()
        .unwrap();
    assert!(adopted_span.parent_node().is_none());
    assert!(root
        .query_selector("[data-plec-node='root/outlet:main/node:3']")
        .unwrap()
        .is_none());
    // And back: the region keeps flipping without remounting the graph.
    conditional_toggle(&root)
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    assert!(flip_one.parent_node().is_none());
    assert!(root
        .query_selector("[data-runtime-node='3']")
        .unwrap()
        .is_some());
}

#[wasm_bindgen_test]
fn ssr_conditional_divergence_keeps_recorded_side_until_first_reconcile() {
    let _location = reset_browser_location();
    let runtime = PlecRuntime::new();
    let root = mount_root();
    // The record claims the consequent while the recomputed initial test
    // value (false) selects the alternate: adoption proceeds, the recorded
    // side stays mounted, and the divergence resolves on the first flip.
    start_conditional_fixture(
        &runtime,
        &root,
        &conditional_server_dom("consequent"),
        serde_json::json!([{"node": 1, "selected": "consequent"}]),
    )
    .unwrap();
    let adopted_p = root
        .query_selector("[data-plec-node='root/outlet:main/node:2']")
        .unwrap()
        .unwrap();
    conditional_toggle(&root)
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    // false -> true recomputes to the recorded consequent: no flip yet.
    assert!(adopted_p.parent_node().is_some());
    conditional_toggle(&root)
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    assert!(adopted_p.parent_node().is_none());
    assert!(root
        .query_selector("[data-runtime-node='3']")
        .unwrap()
        .is_some());
}

#[wasm_bindgen_test]
fn ssr_conditional_without_branch_record_fails_closed() {
    let _location = reset_browser_location();
    let error = start_conditional_fixture(
        &PlecRuntime::new(),
        &mount_root(),
        &conditional_server_dom("alternate"),
        serde_json::json!([]),
    )
    .unwrap_err();
    assert_eq!(
        error.as_string().unwrap_or_default(),
        "missing:ssr-branch:root/outlet:main:1"
    );
}

#[wasm_bindgen_test]
fn ssr_conditional_with_missing_markers_fails_closed() {
    let _location = reset_browser_location();
    // No start marker: the ownership region cannot be located.
    let error = start_conditional_fixture(
        &PlecRuntime::new(),
        &mount_root(),
        "<span data-plec-node=\"root/outlet:main/node:3\"></span>",
        serde_json::json!([{"node": 1, "selected": "alternate"}]),
    )
    .unwrap_err();
    assert_eq!(
        error.as_string().unwrap_or_default(),
        "missing:ssr-branch:root/outlet:main:1"
    );

    // Start marker present but the region is unterminated.
    let error = start_conditional_fixture(
        &PlecRuntime::new(),
        &mount_root(),
        "<!--plec:conditional:root/outlet:main:1-->\
         <span data-plec-node=\"root/outlet:main/node:3\"></span>",
        serde_json::json!([{"node": 1, "selected": "alternate"}]),
    )
    .unwrap_err();
    assert_eq!(
        error.as_string().unwrap_or_default(),
        "missing:ssr-branch-end:root/outlet:main:1"
    );
}

#[wasm_bindgen_test]
fn ssr_conditional_side_disagreement_fails_closed() {
    let _location = reset_browser_location();
    // The record claims the alternate while the markers enclose the
    // consequent's p: the ownership cause contradicts the markup.
    let error = start_conditional_fixture(
        &PlecRuntime::new(),
        &mount_root(),
        &conditional_server_dom("consequent"),
        serde_json::json!([{"node": 1, "selected": "alternate"}]),
    )
    .unwrap_err();
    assert_eq!(
        error.as_string().unwrap_or_default(),
        "mismatch:ssr-branch:root/outlet:main:1"
    );

    // A none record over a non-empty region is the same structural lie.
    let error = start_conditional_fixture(
        &PlecRuntime::new(),
        &mount_root(),
        &conditional_server_dom("alternate"),
        serde_json::json!([{"node": 1, "selected": "none"}]),
    )
    .unwrap_err();
    assert_eq!(
        error.as_string().unwrap_or_default(),
        "mismatch:ssr-branch:root/outlet:main:1"
    );
}

#[wasm_bindgen_test]
fn ssr_conditional_none_branch_adopts_empty_region_and_flips() {
    let _location = reset_browser_location();
    let runtime = PlecRuntime::new();
    let root = mount_root();
    start_conditional_fixture(
        &runtime,
        &root,
        &conditional_server_dom("none"),
        serde_json::json!([{"node": 1, "selected": "none"}]),
    )
    .unwrap();
    assert!(root
        .query_selector("[data-plec-node='root/outlet:main/node:2']")
        .unwrap()
        .is_none());
    assert!(root
        .query_selector("[data-plec-node='root/outlet:main/node:3']")
        .unwrap()
        .is_none());
    // The empty region still flips into a mounted branch afterwards.
    conditional_toggle(&root)
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    assert!(root
        .query_selector("[data-runtime-node='2']")
        .unwrap()
        .is_some());
}

// ---------------------------------------------------------------------------
// SSR keyed loop adoption
//
// The demo todos shape: a keyed loop whose source is state seeded from the
// transferred `loaderData` export. The server renders real rows (loop
// markers + `data-runtime-row-key`), the snapshot records ordered keys as
// the ownership record, and adoption claims the row DOM into `TypedLoopRows`.
// A collection-input variant adopts its empty loop and receives rows through
// the normal delta path afterwards.
// ---------------------------------------------------------------------------

/// The route page graph. Node 2 is the loop anchor; node 3 is the row
/// template (title binding + row button). The loop source reads state 0,
/// whose initialiser is the `loaderData` host slot, so the server's rows and
/// the client projection derive from the same imported cause.
fn loop_route_artifact() -> serde_json::Value {
    serde_json::json!({
        "version": "0.10",
        "rootComponent": 0,
        "components": [{
            "id": "ssr-loop.tsx#Home",
            "rootNode": 0,
            "strings": ["section", "ul", "li", "title", "click", "button", "Pick", "p", "id"],
            "constants": [""],
            "nodes": [
                {"op": "element", "tag": 0, "parent": null, "children": [1, 7]},
                {"op": "element", "tag": 1, "parent": 0, "children": [2]},
                {"op": "loop", "loop": 0, "parent": 1},
                {"op": "element", "tag": 2, "parent": null, "children": [4, 5]},
                {"op": "text", "text": 0, "parent": 3},
                {"op": "element", "tag": 5, "parent": 3, "children": [6]},
                {"op": "text", "text": 1, "parent": 5},
                {"op": "element", "tag": 7, "parent": 0, "children": [8]},
                {"op": "text", "text": 2, "parent": 7}
            ],
            "texts": [{"binding": 0}, {"value": "Pick"}, {"binding": 1}],
            "bindings": [
                {"target": 4, "sink": "text", "expression": 1},
                {"target": 8, "sink": "text", "expression": 3}
            ],
            "propPrograms": [],
            "events": [{"target": 5, "type": 4, "action": 0, "loop": 0, "fields": []}],
            "inputs": [],
            "hostSlots": [{"kind": "loaderData"}],
            "stateSlots": [
                {"initialExpression": 0, "frameSlot": 0},
                {"initialExpression": 2, "frameSlot": 1}
            ],
            "parameters": [],
            "expressions": [
                {"instructions": [{"op": "loadHost", "host": 0}, {"op": "return"}]},
                {"instructions": [{"op": "loadRowField", "field": 3}, {"op": "return"}]},
                {"instructions": [{"op": "constant", "constant": 0}, {"op": "return"}]},
                {"instructions": [{"op": "loadState", "state": 1}, {"op": "return"}]},
                {"instructions": [{"op": "loadRowField", "field": 8}, {"op": "return"}]}
            ],
            "actions": [{"frameSlots": 0, "instructions": [
                {"op": "evaluate", "expression": 1},
                {"op": "storeState", "state": 1},
                {"op": "return"}
            ]}],
            "loops": [{"sourceExpression": 0, "keyExpression": 4, "itemSlot": 0, "rowTemplate": 3, "input": null}],
            "dependencyEdges": [
                {"source": {"kind": "state", "handle": 0}, "target": {"kind": "loop", "handle": 0}},
                {"source": {"kind": "state", "handle": 1}, "target": {"kind": "binding", "handle": 1}},
                {"source": {"kind": "rowField", "handle": 3, "loop": 0}, "target": {"kind": "binding", "handle": 0}}
            ],
            "routeOutlets": []
        }]
    })
}

/// Rows the `loaderData` export transfers; the loop recomputes keys from it.
fn loop_rows_json(pairs: &[(&str, &str)]) -> serde_json::Value {
    serde_json::json!(pairs
        .iter()
        .map(|(id, title)| serde_json::json!({"id": id, "title": title}))
        .collect::<Vec<_>>())
}

/// The server markup the route graph renders: ordered keyed rows between
/// loop markers, row roots stamped `data-runtime-row-key`, plus the empty
/// static output paragraph.
fn loop_server_dom(rows: &[(&str, &str)]) -> String {
    let list = rows
        .iter()
        .map(|(key, title)| {
            let rp = format!("root/outlet:main/loop:2/key:{key}");
            format!(
                "<!--plec:loop:{rp}-->\
                 <li data-runtime-row-key=\"{key}\" data-plec-node=\"{rp}/node:3\">\
                 <!--plec:text:{rp}:4-->{title}\
                 <button data-plec-node=\"{rp}/node:5\">\
                 <!--plec:text:{rp}:6-->Pick</button></li>\
                 <!--plec:loop-end:{rp}-->"
            )
        })
        .collect::<String>();
    format!(
        "<ul data-plec-node=\"root/outlet:main/node:1\">{list}</ul>\
         <p data-plec-node=\"root/outlet:main/node:7\">\
         <!--plec:text:root/outlet:main:8--></p>"
    )
}

/// Snapshot with the route instance's ordered loop keys and the public
/// `loaderData` export the state initialiser reads.
fn loop_snapshot(keys: &[&str], rows: serde_json::Value) -> serde_json::Value {
    let mut snapshot = snapshot_chain_fixture(
        "rev-1",
        "x",
        "/",
        "ssr-snapshot.tsx#App",
        serde_json::json!({}),
    );
    snapshot["public"]["exports"]["loaderData"]["value"] = rows;
    snapshot["structure"]["graphs"]["root%2Foutlet:main/outlet:main"] = serde_json::json!({
        "graphId": "ssr-loop.tsx#Home",
        "loops": [{"node": 2, "keys": keys}],
    });
    snapshot
}

fn start_loop_fixture(
    runtime: &PlecRuntime,
    root: &Element,
    outlet_dom: &str,
    snapshot: serde_json::Value,
) -> Result<(), JsValue> {
    root.set_inner_html(&format!(
        "<main data-plec-node=\"root/node:0\"><p data-plec-node=\"root/node:1\">\
         <!--plec:text:root:2-->ignored</p>\
         <section data-plec-node=\"root/outlet:main/node:0\">{outlet_dom}</section></main>"
    ));
    runtime
        .register_graph(
            "ssr-loop.tsx#Home".into(),
            serde_wasm_bindgen::to_value(&loop_route_artifact()).unwrap(),
        )
        .unwrap();
    runtime
        .register_graph(
            "ssr-snapshot.tsx#App".into(),
            serde_wasm_bindgen::to_value(&snapshot_fixture_artifact()).unwrap(),
        )
        .unwrap();
    let manifest = js_sys::JSON::parse(
        r#"{"version":3,"revision":"rev-1","rootGraphId":"ssr-snapshot.tsx#App",
            "routes":[{"id":"ssr-snapshot.tsx#Home","path":"",
            "graphId":"ssr-loop.tsx#Home","outletId":"main"}]}"#,
    )
    .unwrap();
    let snapshot = serde_wasm_bindgen::to_value(&snapshot).unwrap();
    runtime.start_adopt_snapshot(root.clone(), manifest, snapshot)
}

fn loop_row<'a>(root: &Element, key: &str) -> web_sys::Element {
    root.query_selector(&format!("[data-runtime-row-key='{key}']"))
        .unwrap()
        .unwrap_or_else(|| panic!("adopted row {key} missing"))
}

#[wasm_bindgen_test]
fn ssr_keyed_loop_adopts_server_rows_and_keeps_row_actions_live() {
    let _location = reset_browser_location();
    let runtime = PlecRuntime::new();
    let root = mount_root();
    let rows = loop_rows_json(&[("one", "One"), ("two", "Two")]);
    start_loop_fixture(
        &runtime,
        &root,
        &loop_server_dom(&[("one", "One"), ("two", "Two")]),
        loop_snapshot(&["one", "two"], rows),
    )
    .unwrap();
    // The server-rendered rows survived: keys claimed, values intact, and
    // the row-root attribute the event delegation depends on is present.
    let one = loop_row(&root, "one");
    let two = loop_row(&root, "two");
    assert_eq!(one.text_content().unwrap(), "OnePick");
    assert_eq!(two.text_content().unwrap(), "TwoPick");
    // Clicking an adopted row's button runs the row-scoped action through
    // event delegation and updates the static output binding, with the other
    // row's DOM untouched (no remount).
    let one_node: web_sys::Node = one.clone().into();
    two.query_selector("button")
        .unwrap()
        .unwrap()
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    assert_eq!(
        root.query_selector("[data-plec-node='root/outlet:main/node:7']")
            .unwrap()
            .unwrap()
            .text_content(),
        Some("Two".to_string())
    );
    assert!(one.parent_node().is_some());
    assert!(one_node.is_same_node(Some(loop_row(&root, "one").unchecked_ref())));
}

#[wasm_bindgen_test]
fn ssr_keyed_loop_missing_snapshot_record_fails_closed() {
    let _location = reset_browser_location();
    let mut snapshot = loop_snapshot(&["one"], loop_rows_json(&[("one", "One")]));
    snapshot["structure"]["graphs"]["root%2Foutlet:main/outlet:main"]
        .as_object_mut()
        .unwrap()
        .remove("loops");
    let error = start_loop_fixture(
        &PlecRuntime::new(),
        &mount_root(),
        &loop_server_dom(&[("one", "One")]),
        snapshot,
    )
    .unwrap_err();
    assert_eq!(
        error.as_string().unwrap_or_default(),
        "missing:ssr-loop:root/outlet:main:2"
    );
}

#[wasm_bindgen_test]
fn ssr_keyed_loop_projection_mismatches_fail_closed_per_row() {
    let cases: &[(&str, serde_json::Value, &[&str], &str)] = &[
        (
            // The transferred keys name a row the imported state lacks.
            "missing",
            loop_rows_json(&[("one", "One")]),
            &["one", "two"],
            "missing:ssr-row:two",
        ),
        (
            // The imported state has a row the transferred keys do not.
            "extra",
            loop_rows_json(&[("one", "One"), ("two", "Two")]),
            &["one"],
            "extra:ssr-row:two",
        ),
        (
            // Same keys, different order: identity ordering is the record.
            "order",
            loop_rows_json(&[("one", "One"), ("two", "Two")]),
            &["two", "one"],
            "mismatch:ssr-row-order:root/outlet:main:2",
        ),
        (
            // A duplicated imported key is a hard error, client-side too.
            "duplicate",
            loop_rows_json(&[("one", "One"), ("one", "Again")]),
            &["one", "two"],
            "duplicate:ssr-row-key:one",
        ),
    ];
    for (name, rows, keys, expected) in cases {
        let error = start_loop_fixture(
            &PlecRuntime::new(),
            &mount_root(),
            &loop_server_dom(&[("one", "One"), ("two", "Two")]),
            loop_snapshot(keys, rows.clone()),
        )
        .unwrap_err()
        .as_string()
        .unwrap_or_default();
        assert_eq!(&error, expected, "case {name}");
    }
}

#[wasm_bindgen_test]
fn ssr_keyed_loop_missing_dom_row_fails_closed_at_claim() {
    let _location = reset_browser_location();
    // Keys and projection agree on two rows, but the server only rendered
    // one: the claim cannot invent the missing row DOM.
    let error = start_loop_fixture(
        &PlecRuntime::new(),
        &mount_root(),
        &loop_server_dom(&[("one", "One")]),
        loop_snapshot(
            &["one", "two"],
            loop_rows_json(&[("one", "One"), ("two", "Two")]),
        ),
    )
    .unwrap_err();
    assert_eq!(
        error.as_string().unwrap_or_default(),
        "missing:ssr-node:root/outlet:main/loop:2/key:two/node:3"
    );
}

#[wasm_bindgen_test]
fn ssr_keyed_loop_duplicate_dom_row_markers_fail_closed() {
    let _location = reset_browser_location();
    // Two server rows claim the same key segment: the ownership index
    // rejects the duplicated marker instead of last-wins claiming.
    let error = start_loop_fixture(
        &PlecRuntime::new(),
        &mount_root(),
        &loop_server_dom(&[("one", "One"), ("one", "One")]),
        loop_snapshot(&["one"], loop_rows_json(&[("one", "One")])),
    )
    .unwrap_err();
    assert_eq!(
        error.as_string().unwrap_or_default(),
        "duplicate:ssr-marker:plec:loop:root/outlet:main/loop:2/key:one"
    );
}

/// The collection-input variant: the loop source compiles to an empty
/// literal because rows arrive through the input, so the server renders no
/// rows and the snapshot records an empty key list. Adoption claims the
/// empty loop; rows then flow through the normal delta path.
fn collection_loop_route_artifact() -> serde_json::Value {
    let mut app = loop_route_artifact();
    let component = &mut app["components"][0];
    component["strings"] = serde_json::json!([
        "section", "ul", "li", "title", "click", "button", "Pick", "p", "id", "items"
    ]);
    component["inputs"] = serde_json::json!([{"name": 9, "kind": "collection"}]);
    component["hostSlots"] = serde_json::json!([]);
    component["stateSlots"] = serde_json::json!([{"initialExpression": 2, "frameSlot": 0}]);
    component["actions"] = serde_json::json!([{"frameSlots": 0, "instructions": [
        {"op": "evaluate", "expression": 1},
        {"op": "storeState", "state": 0},
        {"op": "return"}
    ]}]);
    component["expressions"] = serde_json::json!([
        {"instructions": [{"op": "makeArray", "count": 0, "spreads": []}, {"op": "return"}]},
        {"instructions": [{"op": "loadRowField", "field": 3}, {"op": "return"}]},
        {"instructions": [{"op": "constant", "constant": 0}, {"op": "return"}]},
        {"instructions": [{"op": "loadState", "state": 0}, {"op": "return"}]},
        {"instructions": [{"op": "loadRowField", "field": 8}, {"op": "return"}]}
    ]);
    component["loops"] = serde_json::json!([{"sourceExpression": 0, "keyExpression": 4, "itemSlot": 0, "rowTemplate": 3, "input": 0}]);
    component["dependencyEdges"] = serde_json::json!([
        {"source": {"kind": "rowField", "handle": 3, "loop": 0}, "target": {"kind": "binding", "handle": 0}},
        {"source": {"kind": "state", "handle": 0}, "target": {"kind": "binding", "handle": 1}}
    ]);
    app
}

fn start_collection_loop_fixture(runtime: &PlecRuntime, root: &Element) -> Result<(), JsValue> {
    let mut snapshot = snapshot_chain_fixture(
        "rev-1",
        "x",
        "/",
        "ssr-snapshot.tsx#App",
        serde_json::json!({}),
    );
    snapshot["structure"]["graphs"]["root%2Foutlet:main/outlet:main"] = serde_json::json!({
        "graphId": "ssr-loop.tsx#Home",
        "loops": [{"node": 2, "keys": []}],
    });
    root.set_inner_html(
        "<main data-plec-node=\"root/node:0\"><p data-plec-node=\"root/node:1\">\
         <!--plec:text:root:2-->ignored</p>\
         <section data-plec-node=\"root/outlet:main/node:0\">\
         <ul data-plec-node=\"root/outlet:main/node:1\"></ul>\
         <p data-plec-node=\"root/outlet:main/node:7\">\
         <!--plec:text:root/outlet:main:8--></p></section></main>",
    );
    runtime
        .register_graph(
            "ssr-loop.tsx#Home".into(),
            serde_wasm_bindgen::to_value(&collection_loop_route_artifact()).unwrap(),
        )
        .unwrap();
    runtime
        .register_graph(
            "ssr-snapshot.tsx#App".into(),
            serde_wasm_bindgen::to_value(&snapshot_fixture_artifact()).unwrap(),
        )
        .unwrap();
    let manifest = js_sys::JSON::parse(
        r#"{"version":3,"revision":"rev-1","rootGraphId":"ssr-snapshot.tsx#App",
            "routes":[{"id":"ssr-snapshot.tsx#Home","path":"",
            "graphId":"ssr-loop.tsx#Home","outletId":"main"}]}"#,
    )
    .unwrap();
    runtime.start_adopt_snapshot(
        root.clone(),
        manifest,
        serde_wasm_bindgen::to_value(&snapshot).unwrap(),
    )
}

#[wasm_bindgen_test]
fn ssr_collection_loop_adopts_empty_and_receives_rows_through_deltas() {
    let _location = reset_browser_location();
    let runtime = PlecRuntime::new();
    let root = mount_root();
    start_collection_loop_fixture(&runtime, &root).unwrap();
    assert!(root.query_selector("li").unwrap().is_none());
    // Post-adoption input hydration reconciles rows into the claimed loop
    // parent with the same keyed-row identity the mount path produces.
    initialize_rows(&runtime, loop_rows_json(&[("one", "One"), ("two", "Two")]));
    let one = loop_row(&root, "one");
    let one_node: web_sys::Node = one.clone().into();
    assert_eq!(one.text_content().unwrap(), "OnePick");

    // A one-row field update is targeted: no structural DOM mutation, the
    // other row untouched, and the changed row keeps its element identity.
    reset_plec_dom_mutations();
    apply_delta(
        &runtime,
        serde_json::json!({"type":"update","input_id":"items","row_key":"one","changes":{"title":"Updated"}}),
    );
    let mutations = plec_dom_mutations();
    assert_eq!(mutations, "{\"append\":0,\"insertBefore\":0,\"remove\":0}");
    let one_after = loop_row(&root, "one");
    assert!(one_node.is_same_node(Some(one_after.unchecked_ref())));
    assert_eq!(one_after.text_content().unwrap(), "UpdatedPick");
    assert_eq!(loop_row(&root, "two").text_content().unwrap(), "TwoPick");

    // Add, move, and remove flow through the adopted loop's reconcile.
    apply_delta(
        &runtime,
        serde_json::json!({"type":"insert","input_id":"items","row_key":"three","row":{"id":"three","title":"Three"},"before_row_key":"one"}),
    );
    assert_eq!(
        loop_row(&root, "three").text_content().unwrap(),
        "ThreePick"
    );
    apply_delta(
        &runtime,
        serde_json::json!({"type":"move","input_id":"items","row_key":"two","before_row_key":"one"}),
    );
    // Order after the insert and move: three, two, one.
    assert!(loop_row(&root, "two").is_same_node(
        root.query_selector_all("li")
            .unwrap()
            .item(1)
            .as_ref()
            .map(|node| node.unchecked_ref())
    ));
    apply_delta(
        &runtime,
        serde_json::json!({"type":"remove","input_id":"items","row_key":"one"}),
    );
    assert!(!one_after.is_connected());
    assert!(root
        .query_selector("[data-runtime-row-key='one']")
        .unwrap()
        .is_none());
}

// ---------------------------------------------------------------------------
// SSR row-scoped conditional adoption
//
// A conditional inside the row template adopts through the loop slice: the
// server encloses the rendered branch in the shared boundary grammar, the
// claim recomputes the test and validates the region, and the registered
// row region flips through normal state reconciliation afterwards.
// ---------------------------------------------------------------------------

fn row_conditional_route_artifact() -> serde_json::Value {
    serde_json::json!({
        "version": "0.10",
        "rootComponent": 0,
        "components": [{
            "id": "ssr-loop.tsx#Home",
            "rootNode": 0,
            "strings": ["section", "ul", "li", "title", "click", "button", "Done", "p", "id", "toggle", ""],
            "constants": ["", true],
            "nodes": [
                {"op": "element", "tag": 0, "parent": null, "children": [1, 8, 9]},
                {"op": "element", "tag": 1, "parent": 0, "children": [2]},
                {"op": "loop", "loop": 0, "parent": 1},
                {"op": "element", "tag": 2, "parent": null, "children": [4, 5]},
                {"op": "text", "text": 0, "parent": 3},
                {"op": "conditional", "test": 5, "parent": 3, "consequent": 6, "alternate": null},
                {"op": "element", "tag": 5, "parent": null, "children": [7]},
                {"op": "text", "text": 1, "parent": 6},
                {"op": "element", "tag": 5, "parent": 0, "children": []},
                {"op": "element", "tag": 7, "parent": 0, "children": [10]},
                {"op": "text", "text": 2, "parent": 9}
            ],
            "texts": [{"binding": 0}, {"value": "Done"}, {"binding": 1}],
            "bindings": [
                {"target": 4, "sink": "text", "expression": 1},
                {"target": 10, "sink": "text", "expression": 3}
            ],
            "propPrograms": [],
            "events": [
                {"target": 6, "type": 4, "action": 0, "loop": 0, "fields": []},
                {"target": 8, "type": 4, "action": 1, "fields": []}
            ],
            "inputs": [],
            "hostSlots": [{"kind": "loaderData"}],
            "stateSlots": [
                {"initialExpression": 0, "frameSlot": 0},
                {"initialExpression": 2, "frameSlot": 1},
                {"initialExpression": 7, "frameSlot": 2}
            ],
            "parameters": [],
            "expressions": [
                {"instructions": [{"op": "loadHost", "host": 0}, {"op": "return"}]},
                {"instructions": [{"op": "loadRowField", "field": 3}, {"op": "return"}]},
                {"instructions": [{"op": "constant", "constant": 0}, {"op": "return"}]},
                {"instructions": [{"op": "loadState", "state": 1}, {"op": "return"}]},
                {"instructions": [{"op": "loadRowField", "field": 8}, {"op": "return"}]},
                {"instructions": [{"op": "loadState", "state": 2}, {"op": "return"}]},
                {"instructions": [{"op": "loadState", "state": 2}, {"op": "unary", "kind": "not"}, {"op": "return"}]},
                {"instructions": [{"op": "constant", "constant": 1}, {"op": "return"}]}
            ],
            "actions": [
                {"frameSlots": 0, "instructions": [
                    {"op": "evaluate", "expression": 1},
                    {"op": "storeState", "state": 1},
                    {"op": "return"}
                ]},
                {"frameSlots": 0, "instructions": [
                    {"op": "evaluate", "expression": 6},
                    {"op": "storeState", "state": 2},
                    {"op": "return"}
                ]}
            ],
            "loops": [{"sourceExpression": 0, "keyExpression": 4, "itemSlot": 0, "rowTemplate": 3, "input": null}],
            "dependencyEdges": [
                {"source": {"kind": "state", "handle": 0}, "target": {"kind": "loop", "handle": 0}},
                {"source": {"kind": "state", "handle": 1}, "target": {"kind": "binding", "handle": 1}},
                {"source": {"kind": "state", "handle": 2}, "target": {"kind": "conditional", "handle": 5}},
                {"source": {"kind": "rowField", "handle": 3, "loop": 0}, "target": {"kind": "binding", "handle": 0}}
            ],
            "routeOutlets": []
        }]
    })
}

/// Row markup whose template contains the conditional region. `branch`
/// selects whether the server rendered the consequent button inside the
/// region boundaries.
fn row_conditional_server_dom(with_button: bool) -> String {
    let rows = [("one", "One"), ("two", "Two")]
        .iter()
        .map(|(key, title)| {
            let rp = format!("root/outlet:main/loop:2/key:{key}");
            let region = if with_button {
                format!(
                    "<!--plec:conditional:{rp}:5-->\
                     <button data-plec-node=\"{rp}/node:6\">\
                     <!--plec:text:{rp}:7-->Done</button>\
                     <!--plec:conditional-end:{rp}:5-->"
                )
            } else {
                format!(
                    "<!--plec:conditional:{rp}:5-->\
                     <!--plec:conditional-end:{rp}:5-->"
                )
            };
            format!(
                "<!--plec:loop:{rp}-->\
                 <li data-runtime-row-key=\"{key}\" data-plec-node=\"{rp}/node:3\">\
                 <!--plec:text:{rp}:4-->{title}{region}</li>\
                 <!--plec:loop-end:{rp}-->"
            )
        })
        .collect::<String>();
    format!(
        "<ul data-plec-node=\"root/outlet:main/node:1\">{rows}</ul>\
         <button data-plec-node=\"root/outlet:main/node:8\"></button>\
         <p data-plec-node=\"root/outlet:main/node:9\">\
         <!--plec:text:root/outlet:main:10--></p>"
    )
}

fn start_row_conditional_fixture(
    runtime: &PlecRuntime,
    root: &Element,
    outlet_dom: &str,
) -> Result<(), JsValue> {
    root.set_inner_html(&format!(
        "<main data-plec-node=\"root/node:0\"><p data-plec-node=\"root/node:1\">\
         <!--plec:text:root:2-->ignored</p>\
         <section data-plec-node=\"root/outlet:main/node:0\">{outlet_dom}</section></main>"
    ));
    runtime
        .register_graph(
            "ssr-loop.tsx#Home".into(),
            serde_wasm_bindgen::to_value(&row_conditional_route_artifact()).unwrap(),
        )
        .unwrap();
    runtime
        .register_graph(
            "ssr-snapshot.tsx#App".into(),
            serde_wasm_bindgen::to_value(&snapshot_fixture_artifact()).unwrap(),
        )
        .unwrap();
    let manifest = js_sys::JSON::parse(
        r#"{"version":3,"revision":"rev-1","rootGraphId":"ssr-snapshot.tsx#App",
            "routes":[{"id":"ssr-snapshot.tsx#Home","path":"",
            "graphId":"ssr-loop.tsx#Home","outletId":"main"}]}"#,
    )
    .unwrap();
    let mut snapshot = snapshot_chain_fixture(
        "rev-1",
        "x",
        "/",
        "ssr-snapshot.tsx#App",
        serde_json::json!({}),
    );
    snapshot["public"]["exports"]["loaderData"]["value"] =
        loop_rows_json(&[("one", "One"), ("two", "Two")]);
    snapshot["structure"]["graphs"]["root%2Foutlet:main/outlet:main"] = serde_json::json!({
        "graphId": "ssr-loop.tsx#Home",
        "loops": [{"node": 2, "keys": ["one", "two"]}],
    });
    runtime.start_adopt_snapshot(
        root.clone(),
        manifest,
        serde_wasm_bindgen::to_value(&snapshot).unwrap(),
    )
}

fn graph_toggle_button(root: &Element) -> web_sys::EventTarget {
    root.query_selector("[data-plec-node='root/outlet:main/node:8']")
        .unwrap()
        .unwrap()
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
}

#[wasm_bindgen_test]
fn ssr_row_conditional_adopts_region_and_flips_through_reconcile() {
    let _location = reset_browser_location();
    let runtime = PlecRuntime::new();
    let root = mount_root();
    // Toggle state starts true, so the server rendered the consequent
    // button inside every row's conditional region.
    start_row_conditional_fixture(&runtime, &root, &row_conditional_server_dom(true)).unwrap();
    let one = loop_row(&root, "one");
    assert_eq!(one.text_content().unwrap(), "OneDone");
    // The claimed region's button runs the row action through the
    // conditional listener owner.
    one.query_selector("button")
        .unwrap()
        .unwrap()
        .dyn_into::<web_sys::EventTarget>()
        .unwrap()
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    assert_eq!(
        root.query_selector("[data-plec-node='root/outlet:main/node:9']")
            .unwrap()
            .unwrap()
            .text_content(),
        Some("One".to_string())
    );
    // The toggle flips state 2, and the adopted row regions reconcile to the
    // empty branch: server buttons are removed, not remounted.
    let adopted_button = one.query_selector("button").unwrap().unwrap();
    graph_toggle_button(&root)
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    assert!(!adopted_button.is_connected());
    assert_eq!(one.text_content().unwrap(), "One");
    // And back: freshly instantiated client buttons appear in the regions.
    graph_toggle_button(&root)
        .dispatch_event(&Event::new("click").unwrap())
        .unwrap();
    assert!(loop_row(&root, "one")
        .query_selector("[data-runtime-node='6']")
        .unwrap()
        .is_some());
}

#[wasm_bindgen_test]
fn ssr_row_conditional_branch_disagreement_fails_closed() {
    let _location = reset_browser_location();
    // Toggle state is true, so the recomputed selection claims the
    // consequent — but the server rendered an empty region. The ownership
    // cause contradicts the markup: fail closed per row template.
    let error = start_row_conditional_fixture(
        &PlecRuntime::new(),
        &mount_root(),
        &row_conditional_server_dom(false),
    )
    .unwrap_err();
    assert_eq!(
        error.as_string().unwrap_or_default(),
        "mismatch:ssr-row-branch:root/outlet:main/loop:2/key:one:5"
    );
}

/// The demo todos shape: the row template is a component call whose value
/// props read the row (`title`, `done`), and the child renders a text
/// binding plus a prop-driven conditional. Regression: a row-scoped
/// component call must stay owned by its row — a graph-level entry would
/// let static refresh sweeps re-evaluate its props without the row (empty
/// record, `done` falsy), blanking the text and flipping the branch.
fn component_row_route_artifact() -> serde_json::Value {
    serde_json::json!({
        "version": "0.10",
        "rootComponent": 0,
        "components": [
            {
                "id": "ssr-loop.tsx#Home",
                "rootNode": 0,
                "strings": ["section", "ul", "li", "title", "done", "id"],
                "constants": [],
                "nodes": [
                    {"op": "element", "tag": 0, "parent": null, "children": [1]},
                    {"op": "element", "tag": 1, "parent": 0, "children": [2]},
                    {"op": "loop", "loop": 0, "parent": 1},
                    {"op": "component", "component": 1, "parent": null, "props": [
                        {"kind": "value", "name": 3, "expression": 0},
                        {"kind": "value", "name": 4, "expression": 1}
                    ]}
                ],
                "texts": [],
                "bindings": [],
                "propPrograms": [],
                "events": [],
                "inputs": [],
                "hostSlots": [{"kind": "loaderData"}],
                "stateSlots": [],
                "parameters": [],
                "expressions": [
                    {"instructions": [{"op": "loadRowField", "field": 3}, {"op": "return"}]},
                    {"instructions": [{"op": "loadRowField", "field": 4}, {"op": "return"}]},
                    {"instructions": [{"op": "loadHost", "host": 0}, {"op": "return"}]},
                    {"instructions": [{"op": "loadRowField", "field": 5}, {"op": "return"}]}
                ],
                "actions": [],
                "loops": [{"sourceExpression": 2, "keyExpression": 3, "itemSlot": 0, "rowTemplate": 3, "input": null}],
                "dependencyEdges": [],
                "routeOutlets": []
            },
            {
                "id": "ssr-loop.tsx#Row",
                "rootNode": 0,
                "strings": ["li", "span", "button", "title", "done", "Done"],
                "constants": [],
                "nodes": [
                    {"op": "element", "tag": 0, "parent": null, "children": [1, 3]},
                    {"op": "element", "tag": 1, "parent": 0, "children": [2]},
                    {"op": "text", "text": 0, "parent": 1},
                    {"op": "conditional", "test": 1, "parent": 0, "consequent": 4, "alternate": null},
                    {"op": "element", "tag": 2, "parent": null, "children": [5]},
                    {"op": "text", "text": 1, "parent": 4}
                ],
                "texts": [{"binding": 0}, {"value": "Done"}],
                "bindings": [{"target": 2, "sink": "text", "expression": 0}],
                "propPrograms": [],
                "events": [],
                "inputs": [],
                "hostSlots": [],
                "stateSlots": [],
                "parameters": [{"name": 3, "callable": false}, {"name": 4, "callable": false}],
                "expressions": [
                    {"instructions": [{"op": "loadProp", "prop": 0}, {"op": "return"}]},
                    {"instructions": [{"op": "loadProp", "prop": 1}, {"op": "return"}]}
                ],
                "actions": [],
                "loops": [],
                "dependencyEdges": [
                    {"source": {"kind": "prop", "handle": 0}, "target": {"kind": "binding", "handle": 0}},
                    {"source": {"kind": "prop", "handle": 1}, "target": {"kind": "conditional", "handle": 3}}
                ],
                "routeOutlets": []
            }
        ]
    })
}

fn component_row_server_dom() -> String {
    let rp = "root/outlet:main/loop:2/key:one";
    format!(
        "<ul data-plec-node=\"root/outlet:main/node:1\">\
         <!--plec:loop:{rp}-->\
         <!--plec:component:{rp}:3-->\
         <li data-runtime-row-key=\"one\" data-plec-node=\"{rp}/component:3/node:0\">\
         <span data-plec-node=\"{rp}/component:3/node:1\">\
         <!--plec:text:{rp}/component:3:2-->One</span>\
         <!--plec:conditional:{rp}/component:3:3-->\
         <button data-plec-node=\"{rp}/component:3/node:4\">\
         <!--plec:text:{rp}/component:3:5-->Done</button>\
         <!--plec:conditional-end:{rp}/component:3:3-->\
         </li>\
         <!--plec:component-end:{rp}:3-->\
         <!--plec:loop-end:{rp}--></ul>"
    )
}

#[wasm_bindgen_test]
fn ssr_component_row_template_keeps_server_content_and_props() {
    let _location = reset_browser_location();
    let runtime = PlecRuntime::new();
    let root = mount_root();
    root.set_inner_html(&format!(
        "<main data-plec-node=\"root/node:0\"><p data-plec-node=\"root/node:1\">\
         <!--plec:text:root:2-->ignored</p>\
         <section data-plec-node=\"root/outlet:main/node:0\">{}</section></main>",
        component_row_server_dom()
    ));
    runtime
        .register_graph(
            "ssr-loop.tsx#Home".into(),
            serde_wasm_bindgen::to_value(&component_row_route_artifact()).unwrap(),
        )
        .unwrap();
    runtime
        .register_graph(
            "ssr-snapshot.tsx#App".into(),
            serde_wasm_bindgen::to_value(&snapshot_fixture_artifact()).unwrap(),
        )
        .unwrap();
    let manifest = js_sys::JSON::parse(
        r#"{"version":3,"revision":"rev-1","rootGraphId":"ssr-snapshot.tsx#App",
            "routes":[{"id":"ssr-snapshot.tsx#Home","path":"",
            "graphId":"ssr-loop.tsx#Home","outletId":"main"}]}"#,
    )
    .unwrap();
    let mut snapshot = snapshot_chain_fixture(
        "rev-1",
        "x",
        "/",
        "ssr-snapshot.tsx#App",
        serde_json::json!({}),
    );
    snapshot["public"]["exports"]["loaderData"]["value"] =
        serde_json::json!([{"id": "one", "title": "One", "done": true}]);
    snapshot["structure"]["graphs"]["root%2Foutlet:main/outlet:main"] = serde_json::json!({
        "graphId": "ssr-loop.tsx#Home",
        "loops": [{"node": 2, "keys": ["one"]}],
    });
    runtime
        .start_adopt_snapshot(
            root.clone(),
            manifest,
            serde_wasm_bindgen::to_value(&snapshot).unwrap(),
        )
        .unwrap();
    // The claimed row keeps the server-rendered title and branch: row-scoped
    // component props were not recomputed without their row by any sweep.
    let row = loop_row(&root, "one");
    assert_eq!(row.text_content().unwrap(), "OneDone");
    assert!(row.query_selector("button").unwrap().is_some());
}

// ---------------------------------------------------------------------------
// Loader state transfer: SSR executes route loaders, the browser resumes.
//
// The route page reads the `loaderData` host slot, so a resolved imported
// outcome must reach its initialiser without any client fetch, and a
// rejected outcome must restore the error phase the server rendered.
// ---------------------------------------------------------------------------

fn loader_layout_artifact() -> serde_json::Value {
    serde_json::json!({
        "version": "0.10",
        "rootComponent": 0,
        "components": [{
            "id": "loader-transfer.tsx#Root",
            "rootNode": 0,
            "strings": ["main", "p"],
            "constants": [],
            "nodes": [
                {"op": "element", "tag": 0, "parent": null, "children": [1]},
                {"op": "element", "tag": 1, "parent": 0, "children": [2]},
                {"op": "text", "text": 0, "parent": 1}
            ],
            "texts": [{"value": "layout"}],
            "bindings": [],
            "propPrograms": [],
            "events": [],
            "inputs": [],
            "hostSlots": [],
            "stateSlots": [],
            "parameters": [],
            "expressions": [],
            "actions": [],
            "loops": [],
            "dependencyEdges": [],
            "routeOutlets": [{"id": "main", "node": 0}]
        }]
    })
}

fn loader_page_artifact() -> serde_json::Value {
    serde_json::json!({
        "version": "0.10",
        "rootComponent": 0,
        "components": [{
            "id": "loader-transfer.tsx#Page",
            "rootNode": 0,
            "strings": ["p"],
            "constants": [null, "/api/data"],
            "nodes": [
                {"op": "element", "tag": 0, "parent": null, "children": [1]},
                {"op": "text", "text": 0, "parent": 0}
            ],
            "texts": [{"binding": 0}],
            "bindings": [{"target": 1, "sink": "text", "expression": 0}],
            "propPrograms": [],
            "events": [],
            "inputs": [],
            "hostSlots": [{"kind": "loaderData"}],
            "stateSlots": [{"initialExpression": 1, "frameSlot": 0}],
            "parameters": [],
            "expressions": [
                {"instructions": [{"op": "loadHost", "host": 0}, {"op": "return"}]},
                {"instructions": [{"op": "constant", "constant": 0}, {"op": "return"}]},
                {"instructions": [{"op": "constant", "constant": 1}, {"op": "return"}]}
            ],
            "actions": [{
                "frameSlots": 2,
                "loaderResultState": 0,
                "routeLoader": true,
                "instructions": [
                    {"op": "capabilityRequest", "capability": "fetch", "request": {"url": 2, "method": "GET", "decode": "responseJson", "requireOk": true}, "successPc": 1, "failurePc": 2, "resultSlot": 0, "errorSlot": 1},
                    {"op": "return"},
                    {"op": "return", "outcome": "failure"}
                ]
            }],
            "loops": [],
            "dependencyEdges": [
                {"source": {"kind": "state", "handle": 0}, "target": {"kind": "binding", "handle": 0}}
            ],
            "routeOutlets": []
        }]
    })
}

fn loader_error_artifact() -> serde_json::Value {
    serde_json::json!({
        "version": "0.10",
        "rootComponent": 0,
        "components": [{
            "id": "loader-transfer.tsx#Error",
            "rootNode": 0,
            "strings": ["div", "button", "click", "Retry", "message", "kind"],
            "constants": [null, "Retry"],
            "nodes": [
                {"op": "element", "tag": 0, "parent": null, "children": [1, 2]},
                {"op": "text", "text": 0, "parent": 0},
                {"op": "element", "tag": 1, "parent": 0, "children": [3]},
                {"op": "text", "text": 2, "parent": 2}
            ],
            "texts": [{"binding": 0}, {"binding": 1}, {"value": "Retry"}],
            "bindings": [
                {"target": 1, "sink": "text", "expression": 1},
                {"target": 2, "sink": "text", "expression": 2}
            ],
            "propPrograms": [],
            "events": [{"target": 2, "type": 2, "action": 0}],
            "inputs": [],
            "hostSlots": [],
            "stateSlots": [{"initialExpression": 0, "frameSlot": 0}],
            "parameters": [],
            "expressions": [
                {"instructions": [{"op": "constant", "constant": 0}, {"op": "return"}]},
                {"instructions": [{"op": "loadState", "state": 0}, {"op": "field", "field": 4}, {"op": "return"}]},
                {"instructions": [{"op": "loadState", "state": 0}, {"op": "field", "field": 5}, {"op": "return"}]}
            ],
            "actions": [{"routeRetry": true, "instructions": [{"op": "return"}]}],
            "loops": [],
            "dependencyEdges": [],
            "routeErrorState": 0,
            "routeOutlets": []
        }]
    })
}

fn loader_plain_artifact() -> serde_json::Value {
    serde_json::json!({
        "version": "0.10",
        "rootComponent": 0,
        "components": [{
            "id": "loader-transfer.tsx#Next",
            "rootNode": 0,
            "strings": ["p"],
            "constants": [],
            "nodes": [
                {"op": "element", "tag": 0, "parent": null, "children": [1]},
                {"op": "text", "text": 0, "parent": 0}
            ],
            "texts": [{"value": "next-page"}],
            "bindings": [],
            "propPrograms": [],
            "events": [],
            "inputs": [],
            "hostSlots": [],
            "stateSlots": [],
            "parameters": [],
            "expressions": [],
            "actions": [],
            "loops": [],
            "dependencyEdges": [],
            "routeOutlets": []
        }]
    })
}

fn loader_manifest(error_graph: bool) -> JsValue {
    let mut page = serde_json::json!({
        "id": "page",
        "path": "todos",
        "graphId": "loader-transfer.tsx#Page",
        "outletId": "main",
        "loaderAction": 0
    });
    if error_graph {
        page["errorGraphId"] = serde_json::json!("loader-transfer.tsx#Error");
    }
    serde_wasm_bindgen::to_value(&serde_json::json!({
        "version": 3,
        "revision": "rev-1",
        "rootGraphId": "loader-transfer.tsx#Root",
        "routes": [
            page,
            {"id": "next", "path": "next", "graphId": "loader-transfer.tsx#Next", "outletId": "main"}
        ]
    }))
    .unwrap()
}

fn register_loader_graphs(runtime: &PlecRuntime) {
    runtime
        .register_graph(
            "loader-transfer.tsx#Root".into(),
            serde_wasm_bindgen::to_value(&loader_layout_artifact()).unwrap(),
        )
        .unwrap();
    runtime
        .register_graph(
            "loader-transfer.tsx#Page".into(),
            serde_wasm_bindgen::to_value(&loader_page_artifact()).unwrap(),
        )
        .unwrap();
    runtime
        .register_graph(
            "loader-transfer.tsx#Error".into(),
            serde_wasm_bindgen::to_value(&loader_error_artifact()).unwrap(),
        )
        .unwrap();
    runtime
        .register_graph(
            "loader-transfer.tsx#Next".into(),
            serde_wasm_bindgen::to_value(&loader_plain_artifact()).unwrap(),
        )
        .unwrap();
}

/// The server HTML for the loader route: layout marker plus the page's
/// single bound text under the `root/outlet:main` instance path.
fn loader_route_html(server_text: &str) -> String {
    format!(
        "<main data-plec-node=\"root/node:0\"><p data-plec-node=\"root/node:1\">\
         <!--plec:text:root:2-->layout</p>\
         <p data-plec-node=\"root/outlet:main/node:0\">\
         <!--plec:text:root/outlet:main:1-->{server_text}</p></main>"
    )
}

fn loader_route_error_html(message: &str) -> String {
    format!(
        "<main data-plec-node=\"root/node:0\"><p data-plec-node=\"root/node:1\">\
         <!--plec:text:root:2-->layout</p>\
         <div data-plec-node=\"root/outlet:main/node:0\">\
         <!--plec:text:root/outlet:main:1-->{message}\
         <button data-plec-node=\"root/outlet:main/node:2\">\
         <!--plec:text:root/outlet:main:3-->Retry</button></div></main>"
    )
}

fn start_loader_transfer(
    runtime: &PlecRuntime,
    root: &Element,
    html: &str,
    phase: &str,
    loaders: serde_json::Value,
) -> Result<(), JsValue> {
    root.set_inner_html(html);
    register_loader_graphs(runtime);
    let snapshot = serde_wasm_bindgen::to_value(&serde_json::json!({
        "version": 1,
        "revision": "rev-1",
        "routes": [{"routeId": "page", "params": {}, "phase": phase}],
        "public": {"location": "/todos", "exports": {}},
        "loaders": loaders,
        "structure": {"graphs": {"root/outlet:main": {"graphId": "loader-transfer.tsx#Root"}}}
    }))
    .unwrap();
    runtime.start_adopt_snapshot(
        root.clone(),
        loader_manifest(phase == "error").into(),
        snapshot,
    )
}

fn loader_route_text(root: &Element) -> String {
    root.text_content().unwrap_or_default()
}

#[wasm_bindgen_test]
fn loader_snapshot_import_resumes_without_client_refetch() {
    let _location = reset_browser_location_to("/todos");
    // An empty fetch queue makes any client loader refetch fail loudly, so
    // the imported value rendering proves the loader never re-ran.
    let _fetch = install_plec_fetch_queue(r#"[]"#);
    let runtime = PlecRuntime::new();
    let root = mount_root();
    start_loader_transfer(
        &runtime,
        &root,
        &loader_route_html("loaded-value"),
        "active",
        serde_json::json!([{
            "graphId": "loader-transfer.tsx#Page",
            "action": 0,
            "state": {"kind": "resolved", "value": "loaded-value"}
        }]),
    )
    .unwrap();
    assert_eq!(loader_route_text(&root), "layoutloaded-value");
    assert_eq!(runtime.ssr_text_divergences(), 0);
}

#[wasm_bindgen_test]
fn loader_snapshot_import_without_outcome_fails_closed() {
    let _location = reset_browser_location_to("/todos");
    let snapshot = start_loader_transfer(
        &PlecRuntime::new(),
        &mount_root(),
        &loader_route_html("x"),
        "active",
        serde_json::json!([]),
    )
    .unwrap_err();
    assert_eq!(
        snapshot.as_string().unwrap_or_default(),
        "mismatch:ssr-route-chain:phase:0:loader"
    );
}

#[wasm_bindgen_test]
fn legacy_loader_adoption_without_snapshot_fails_closed() {
    let _location = reset_browser_location_to("/todos");
    let runtime = PlecRuntime::new();
    let root = mount_root();
    root.set_inner_html(&loader_route_html("x"));
    register_loader_graphs(&runtime);
    let error = runtime
        .start_adopt(root.clone(), loader_manifest(true).into())
        .unwrap_err();
    assert!(error
        .as_string()
        .unwrap_or_default()
        .starts_with("mismatch:ssr-loader:loader-transfer.tsx#Page#action:0"));
}

#[wasm_bindgen_test(async)]
async fn rejected_loader_snapshot_restores_error_phase_and_retry_runs_loader() {
    let _location = reset_browser_location_to("/todos");
    let runtime = PlecRuntime::new();
    let root = mount_root();
    start_loader_transfer(
        &runtime,
        &root,
        &loader_route_error_html("fetch failed"),
        "error",
        serde_json::json!([{
            "graphId": "loader-transfer.tsx#Page",
            "action": 0,
            "state": {"kind": "rejected", "message": "fetch failed"}
        }]),
    )
    .unwrap();
    // The recorded error restored the error phase from the snapshot instead
    // of refetching: the imported message is what the error graph renders.
    assert!(loader_route_text(&root).contains("fetch failed"));

    // Retry from the restored phase re-enters the loader and succeeds.
    let _fetch = install_plec_fetch_queue(r#"[{"body":"\"retry-value\""}]"#);
    click_fetch(&root);
    settle_fetch().await;
    assert!(loader_route_text(&root).contains("retry-value"));
}

#[wasm_bindgen_test(async)]
async fn adopted_loader_route_runs_loader_on_fresh_navigation() {
    let _location = reset_browser_location_to("/todos");
    let runtime = PlecRuntime::new();
    let root = mount_root();
    start_loader_transfer(
        &runtime,
        &root,
        &loader_route_html("loaded-value"),
        "active",
        serde_json::json!([{
            "graphId": "loader-transfer.tsx#Page",
            "action": 0,
            "state": {"kind": "resolved", "value": "loaded-value"}
        }]),
    )
    .unwrap();
    assert!(loader_route_text(&root).contains("loaded-value"));

    // Away to a plain route, then back: the fresh page instance is not the
    // imported one, so its loader executes normally.
    runtime.navigate("/next".into(), false).unwrap();
    assert!(loader_route_text(&root).contains("next-page"));
    let _fetch = install_plec_fetch_queue(r#"[{"body":"\"second-value\""}]"#);
    runtime.navigate("/todos".into(), false).unwrap();
    settle_fetch().await;
    assert!(loader_route_text(&root).contains("second-value"));
}
