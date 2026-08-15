#![cfg(target_arch = "wasm32")]

use plec_runtime::PlecRuntime;
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;
use web_sys::{Element, Event};

wasm_bindgen_test_configure!(run_in_browser);

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
    app["strings"] = serde_json::json!(["div", "ul", "li", "button", "click", "items", "id", "title", "enabled"]);
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


fn initialize_rows(runtime: &PlecRuntime, rows: serde_json::Value) {
    runtime.initialize_input(
        "items".into(),
        serde_wasm_bindgen::to_value(&rows).unwrap(),
    ).unwrap();
}

fn apply_delta(runtime: &PlecRuntime, delta: serde_json::Value) {
    runtime.apply_delta(serde_wasm_bindgen::to_value(&delta).unwrap()).unwrap();
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
    initialize_rows(&runtime, serde_json::json!([
        {"id":"first", "title":"first title"},
        {"id":"second", "title":"second title"}
    ]));

    let first = root.query_selector("[data-runtime-row-key='first'] button").unwrap().unwrap();
    first.clone().dyn_into::<web_sys::EventTarget>().unwrap()
        .dispatch_event(&Event::new("click").unwrap()).unwrap();
    assert_eq!(static_output(&root), "first title");

    apply_delta(&runtime, serde_json::json!({
        "type":"update", "input_id":"items", "row_key":"first", "changes":{"title":"updated title"}
    }));
    first.clone().dyn_into::<web_sys::EventTarget>().unwrap()
        .dispatch_event(&Event::new("click").unwrap()).unwrap();
    assert_eq!(static_output(&root), "updated title");

    apply_delta(&runtime, serde_json::json!({
        "type":"move", "input_id":"items", "row_key":"first", "before_row_key":null
    }));
    first.clone().dyn_into::<web_sys::EventTarget>().unwrap()
        .dispatch_event(&Event::new("click").unwrap()).unwrap();
    assert_eq!(static_output(&root), "updated title");

    apply_delta(&runtime, serde_json::json!({
        "type":"remove", "input_id":"items", "row_key":"first"
    }));
    let second = root.query_selector("[data-runtime-row-key='second'] button").unwrap().unwrap();
    second.dyn_into::<web_sys::EventTarget>().unwrap()
        .dispatch_event(&Event::new("click").unwrap()).unwrap();
    first.dyn_into::<web_sys::EventTarget>().unwrap()
        .dispatch_event(&Event::new("click").unwrap()).unwrap();
    assert_eq!(static_output(&root), "second title");
}

#[wasm_bindgen_test]
fn static_conditional_replaces_branch_listeners_and_supports_no_alternate() {
    let runtime = PlecRuntime::new();
    let root = mount_root();
    load_and_mount(&runtime, static_conditional_artifact(true), &root);
    let false_branch = root.query_selector("[data-runtime-node='3']").unwrap().unwrap();
    false_branch.dyn_into::<web_sys::EventTarget>().unwrap()
        .dispatch_event(&Event::new("click").unwrap()).unwrap();
    assert!(root.query_selector("[data-runtime-node='3']").unwrap().is_none());
    let true_branch = root.query_selector("[data-runtime-node='2']").unwrap().unwrap();
    true_branch.dyn_into::<web_sys::EventTarget>().unwrap()
        .dispatch_event(&Event::new("click").unwrap()).unwrap();
    assert!(root.query_selector("[data-runtime-node='2']").unwrap().is_none());

    let no_alternate = PlecRuntime::new();
    let no_alternate_root = mount_root();
    load_and_mount(&no_alternate, static_conditional_artifact(false), &no_alternate_root);
    assert!(no_alternate_root.query_selector("button").unwrap().is_none());
}

#[wasm_bindgen_test]
fn row_conditional_listener_replaces_only_its_own_keyed_row() {
    let runtime = PlecRuntime::new();
    let root = mount_root();
    load_and_mount(&runtime, row_conditional_artifact(), &root);
    initialize_rows(&runtime, serde_json::json!([
        {"id":"first", "title":"first", "enabled":true},
        {"id":"second", "title":"second", "enabled":false}
    ]));
    let first_row = root.query_selector("[data-runtime-row-key='first']").unwrap().unwrap();
    let first_button = first_row.query_selector("button").unwrap().unwrap();
    first_button.dyn_into::<web_sys::EventTarget>().unwrap()
        .dispatch_event(&Event::new("click").unwrap()).unwrap();
    assert_eq!(static_output(&root), "first");

    apply_delta(&runtime, serde_json::json!({
        "type":"update", "input_id":"items", "row_key":"first", "changes":{"enabled":false}
    }));
    assert!(root.query_selector("[data-runtime-row-key='first'] button").unwrap().is_none());
    assert!(first_row.is_same_node(Some(&root.query_selector("[data-runtime-row-key='first']").unwrap().unwrap())));

    apply_delta(&runtime, serde_json::json!({
        "type":"update", "input_id":"items", "row_key":"second", "changes":{"enabled":true}
    }));
    let second_button = root.query_selector("[data-runtime-row-key='second'] button").unwrap().unwrap();
    second_button.dyn_into::<web_sys::EventTarget>().unwrap()
        .dispatch_event(&Event::new("click").unwrap()).unwrap();
    assert_eq!(static_output(&root), "second");
}
