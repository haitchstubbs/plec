#![cfg(target_arch = "wasm32")]

//! Host-component provider boundary contract: props must cross into the JS
//! provider as JSON-plain objects with every declared prop intact, on both
//! the mount and the refresh-update path. The default serde-wasm-bindgen
//! serializer emits Rust maps as ES6 `Map` instances, which object spread and
//! `Object.entries` read as empty — silently dropping `className`, `href`,
//! and every other prop at the provider boundary (the lucide icon class
//! regression).

use plec_runtime::PlecRuntime;
use wasm_bindgen::JsValue;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

const REGISTRY_JS: &str = r#"
(() => {
  const calls = { mountProps: null, updateProps: null };
  const apply = (element, props) => {
    for (const [key, value] of Object.entries(props || {})) {
      element.setAttribute(key === 'className' ? 'class' : key, String(value));
    }
  };
  globalThis.__test_host_calls = calls;
  globalThis.__plec_host_components = {
    mount: (provider, component, boundary, props) => {
      calls.mountProps = props;
      const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
      apply(svg, props);
      boundary.replaceChildren(svg);
      return {
        element: svg,
        attributes: new Set(Object.entries(props || {}).map(
          ([key]) => (key === 'className' ? 'class' : key)
        )),
      };
    },
    update: (handle, props) => {
      calls.updateProps = props;
      apply(handle.element, props);
    },
    dispose: () => {},
  };
})()
"#;

fn install_recording_provider() {
    js_sys::eval(REGISTRY_JS).expect("provider registry snippet must evaluate");
}

fn calls() -> js_sys::Object {
    js_sys::Reflect::get(&js_sys::global(), &"__test_host_calls".into())
        .expect("recording registry installed")
        .into()
}

fn recorded_props(slot: &str) -> String {
    let value = js_sys::Reflect::get(&calls().into(), &slot.into()).expect(slot);
    js_sys::JSON::stringify(&value)
        .expect("JSON.stringify works")
        .into()
}

/// A loop row containing one host icon with a constant `className` prop: the
/// compiled shape of an icon inside a keyed list, refreshed through the same
/// row component-refresh queue the application uses.
fn icon_artifact() -> JsValue {
    let artifact = serde_json::json!({
        "version": "0.10",
        "rootComponent": 0,
        "components": [{
            "id": "host-props.tsx#List",
            "rootNode": 0,
            "strings": ["ul", "li", "rows", "className", "id"],
            "constants": ["size-4 shrink-0", []],
            "nodes": [
                {"op": "element", "tag": 0, "parent": null, "children": [1]},
                {"op": "loop", "loop": 0, "parent": 0},
                {"op": "element", "tag": 1, "parent": null, "children": [3]},
                {"op": "hostComponent", "provider": "lucide", "component": "Beaker",
                 "parent": 2,
                 "props": [{"kind": "value", "name": 3, "expression": 0}]}
            ],
            "texts": [],
            "bindings": [],
            "inputs": [{"name": 2, "kind": "collection"}],
            "events": [],
            "stateSlots": [],
            "expressions": [
                {"instructions": [{"op": "constant", "constant": 0}, {"op": "return"}]},
                {"instructions": [{"op": "constant", "constant": 1}, {"op": "return"}]},
                {"instructions": [{"op": "loadRowField", "field": 4}, {"op": "return"}]}
            ],
            "actions": [],
            "loops": [{"sourceExpression": 1, "keyExpression": 2, "itemSlot": 0, "rowTemplate": 2, "input": 0}],
            "dependencyEdges": []
        }]
    });
    serde_wasm_bindgen::to_value(&artifact).expect("artifact JSON")
}

#[wasm_bindgen_test]
fn host_props_reach_the_provider_as_plain_objects_on_mount_and_update() {
    install_recording_provider();
    let runtime = PlecRuntime::new();
    runtime
        .load_application(icon_artifact())
        .expect("artifact with a host component must load");
    let root = web_sys::window()
        .expect("window")
        .document()
        .expect("document")
        .create_element("div")
        .expect("div");
    runtime.mount(root.clone()).expect("mount");

    runtime
        .initialize_input(
            "rows".into(),
            serde_wasm_bindgen::to_value(&serde_json::json!([{"id": "row-a"}])).unwrap(),
        )
        .expect("row mount");

    // The mount path must deliver a JSON-plain props object. A props `Map`
    // stringifies as `{}` — exactly the silent drop this test guards.
    let mount_props = recorded_props("mountProps");
    assert_eq!(
        mount_props, r#"{"className":"size-4 shrink-0"}"#,
        "mount props must arrive as a plain object with className intact, got: {mount_props}"
    );
    // The refresh path (row component-refresh queue → host boundary update)
    // must deliver the same plain shape, or a later refresh would strip the
    // attributes the mount applied.
    let update_props = recorded_props("updateProps");
    assert_eq!(
        update_props, r#"{"className":"size-4 shrink-0"}"#,
        "refresh props must arrive as a plain object with className intact, got: {update_props}"
    );
    // The user-visible contract: the provider-owned svg keeps its class.
    let svg = root
        .query_selector("svg")
        .expect("query_selector works")
        .expect("provider-owned svg exists");
    assert_eq!(
        svg.get_attribute("class").as_deref(),
        Some("size-4 shrink-0"),
        "icon class must survive mount and refresh"
    );
}
