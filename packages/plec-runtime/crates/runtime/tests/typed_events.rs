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
