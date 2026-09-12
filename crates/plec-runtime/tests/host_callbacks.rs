#![cfg(target_arch = "wasm32")]

//! Host component callback props (wasm-runtime-a2n.3): callable props cross
//! to the provider as runtime-owned functions; invoking one executes the
//! compiled action through the normal pipeline, and after disposal (or when
//! the runtime's graph generation no longer matches) the retained function
//! rejects instead of executing.

use plec_runtime::PlecRuntime;
use wasm_bindgen::JsValue;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

/// Root div with a host component carrying one callable prop (`onActivate`)
/// plus a text binding that renders the state the callback increments.
fn host_callback_artifact() -> JsValue {
    let artifact = serde_json::json!({
        "version": "0.10",
        "rootComponent": 0,
        "components": [{
            "id": "host-callbacks.tsx#App",
            "rootNode": 0,
            "strings": ["div", "onActivate"],
            "constants": [0, 1],
            "nodes": [
                {"op": "element", "tag": 0, "parent": null, "children": [1, 2]},
                {"op": "hostComponent", "provider": "test", "component": "Widget",
                 "parent": 0,
                 "props": [{"kind": "callable", "name": 1, "action": 0}]},
                {"op": "text", "text": 0, "parent": 0}
            ],
            "texts": [{"binding": 0}],
            "bindings": [{"target": 2, "sink": "text", "expression": 1}],
            "inputs": [],
            "events": [],
            "stateSlots": [{"initialExpression": 0, "frameSlot": 0}],
            "expressions": [
                {"instructions": [{"op": "constant", "constant": 0}, {"op": "return"}]},
                {"instructions": [{"op": "loadState", "state": 0}, {"op": "return"}]},
                {"instructions": [
                    {"op": "loadState", "state": 0},
                    {"op": "constant", "constant": 1},
                    {"op": "binary", "kind": "add"},
                    {"op": "return"}
                ]}
            ],
            "actions": [{"frameSlots": 1, "instructions": [
                {"op": "evaluate", "expression": 2},
                {"op": "storeState", "state": 0},
                {"op": "return"}
            ]}],
            "loops": [],
            "dependencyEdges": [
                {"source": {"kind": "state", "handle": 0}, "target": {"kind": "binding", "handle": 0}}
            ]
        }]
    });
    serde_wasm_bindgen::to_value(&artifact).expect("artifact JSON")
}

/// Installs a registry whose provider records the `onActivate` function it
/// received and exposes it on `globalThis.__captured_host_callback`.
fn install_capturing_provider() {
    js_sys::eval(
        r#"
(() => {
  globalThis.__captured_host_callback = null;
  globalThis.__test_host_registry = {
    resolve: (provider, component) =>
      provider === 'test' && component === 'Widget'
        ? {
            mount: (boundary, props) => {
              globalThis.__captured_host_callback = props.onActivate;
              const span = document.createElement('span');
              span.setAttribute('data-provider-owned', 'true');
              boundary.replaceChildren(span);
              return span;
            },
            update: () => {},
            dispose: () => {},
          }
        : undefined,
  };
})()
"#,
    )
    .expect("capturing registry snippet evaluates");
}

fn fresh_root() -> web_sys::Element {
    web_sys::window()
        .expect("window")
        .document()
        .expect("document")
        .create_element("div")
        .expect("root element")
}

fn text_content(root: &web_sys::Element) -> String {
    root.text_content().unwrap_or_default()
}

#[wasm_bindgen_test]
fn host_callback_props_execute_actions_and_update_the_dom() {
    install_capturing_provider();
    let runtime = PlecRuntime::new();
    let registry = js_sys::Reflect::get(&js_sys::global(), &"__test_host_registry".into())
        .expect("registry installed");
    runtime.set_host_registry(registry).expect("registry set");
    runtime
        .load_application(host_callback_artifact())
        .expect("artifact loads");
    let root = fresh_root();
    runtime.mount(root.clone()).expect("mount");

    assert_eq!(
        text_content(&root),
        "0",
        "state binding renders the initial value"
    );

    let captured = || -> JsValue {
        js_sys::Reflect::get(&js_sys::global(), &"__captured_host_callback".into())
            .expect("captured callback exists")
    };
    assert!(
        wasm_bindgen::JsCast::is_instance_of::<js_sys::Function>(&captured()),
        "onActivate must cross to the provider as a plain function"
    );

    js_sys::Function::from(captured())
        .call0(&JsValue::NULL)
        .expect("callback invocation succeeds");
    js_sys::Function::from(captured())
        .call0(&JsValue::NULL)
        .expect("second invocation succeeds");
    assert_eq!(
        text_content(&root),
        "2",
        "each callback invocation must run the action and update the binding"
    );

    // A plain-JSON payload is part of the contract; non-serializable
    // payloads reject deterministically instead of executing.
    let bad = js_sys::Function::from(captured())
        .call1(&JsValue::NULL, &js_sys::global())
        .expect_err("non-serializable payload must reject");
    assert!(
        bad.as_string()
            .is_some_and(|message| message.contains("payload")),
        "payload rejection must be deterministic, got: {bad:?}"
    );
}

#[wasm_bindgen_test]
fn host_callback_props_reject_after_dispose() {
    install_capturing_provider();
    let runtime = PlecRuntime::new();
    let registry = js_sys::Reflect::get(&js_sys::global(), &"__test_host_registry".into())
        .expect("registry installed");
    runtime.set_host_registry(registry).expect("registry set");
    runtime
        .load_application(host_callback_artifact())
        .expect("artifact loads");
    let root = fresh_root();
    runtime.mount(root).expect("mount");

    let captured = || -> js_sys::Function {
        js_sys::Function::from(
            js_sys::Reflect::get(&js_sys::global(), &"__captured_host_callback".into())
                .expect("captured callback exists"),
        )
    };
    captured()
        .call0(&JsValue::NULL)
        .expect("pre-dispose invoke");

    // The provider may retain the function past disposal; the retained
    // reference must reject instead of executing against freed state.
    runtime.dispose().expect("dispose");
    let error = captured()
        .call0(&JsValue::NULL)
        .expect_err("retained callback must reject after dispose");
    assert!(
        error
            .as_string()
            .is_some_and(|message| message.contains("stale host callback")),
        "post-dispose invocation must reject as stale, got: {error:?}"
    );
}

#[wasm_bindgen_test]
fn host_callback_props_reject_after_graph_replacement() {
    install_capturing_provider();
    let runtime = PlecRuntime::new();
    let registry = js_sys::Reflect::get(&js_sys::global(), &"__test_host_registry".into())
        .expect("registry installed");
    runtime.set_host_registry(registry).expect("registry set");
    runtime
        .load_application(host_callback_artifact())
        .expect("first artifact loads");
    runtime.mount(fresh_root()).expect("first mount");
    let callback = js_sys::Function::from(
        js_sys::Reflect::get(&js_sys::global(), &"__captured_host_callback".into())
            .expect("first callback exists"),
    );

    // Loading a replacement drains the prior typed instance forest. The
    // provider can still retain its old function, but it must not reach the
    // new graph generation.
    runtime
        .load_application(host_callback_artifact())
        .expect("replacement artifact loads");
    let error = callback
        .call0(&JsValue::NULL)
        .expect_err("old generation callback must reject");
    assert!(
        error
            .as_string()
            .is_some_and(|message| message.contains("stale host callback")),
        "old graph callback must reject as stale, got: {error:?}"
    );
}

/// Keyed row host callback: action reads the live row title, proving callback
/// ownership follows the row rather than a stale mount-time value.
fn keyed_host_callback_artifact() -> JsValue {
    let artifact = serde_json::json!({
        "version": "0.10",
        "rootComponent": 0,
        "components": [{
            "id": "host-callbacks.tsx#Rows",
            "rootNode": 0,
            "strings": ["div", "li", "rows", "id", "title", "onActivate"],
            "constants": [0, []],
            "nodes": [
                {"op": "element", "tag": 0, "parent": null, "children": [1, 2]},
                {"op": "loop", "loop": 0, "parent": 0},
                {"op": "text", "text": 0, "parent": 0},
                {"op": "element", "tag": 1, "parent": null, "children": [4]},
                {"op": "hostComponent", "provider": "test", "component": "RowWidget",
                 "parent": 3,
                 "props": [{"kind": "callable", "name": 5, "action": 0}]}
            ],
            "texts": [{"binding": 0}],
            "bindings": [{"target": 2, "sink": "text", "expression": 1}],
            "inputs": [{"name": 2, "kind": "collection"}],
            "events": [],
            "stateSlots": [{"initialExpression": 0, "frameSlot": 0}],
            "expressions": [
                {"instructions": [{"op": "constant", "constant": 0}, {"op": "return"}]},
                {"instructions": [{"op": "loadState", "state": 0}, {"op": "return"}]},
                {"instructions": [{"op": "constant", "constant": 1}, {"op": "return"}]},
                {"instructions": [{"op": "loadRowField", "field": 3}, {"op": "return"}]},
                {"instructions": [{"op": "loadRowField", "field": 4}, {"op": "return"}]}
            ],
            "actions": [{"frameSlots": 1, "instructions": [
                {"op": "evaluate", "expression": 4},
                {"op": "storeState", "state": 0},
                {"op": "return"}
            ]}],
            "loops": [{"sourceExpression": 2, "keyExpression": 3, "itemSlot": 0, "rowTemplate": 3, "input": 0}],
            "dependencyEdges": [
                {"source": {"kind": "state", "handle": 0}, "target": {"kind": "binding", "handle": 0}}
            ]
        }]
    });
    serde_wasm_bindgen::to_value(&artifact).expect("artifact JSON")
}

#[wasm_bindgen_test]
fn keyed_host_callbacks_read_live_rows_and_reject_after_row_disposal() {
    js_sys::eval(
        r#"
(() => {
  globalThis.__row_callback = null;
  globalThis.__test_host_registry = {
    resolve: (provider, component) =>
      provider === 'test' && component === 'RowWidget'
        ? {
            mount: (boundary, props) => {
              globalThis.__row_callback = props.onActivate;
              return boundary;
            },
            update: () => {},
            dispose: () => {},
          }
        : undefined,
  };
})()
"#,
    )
    .expect("row provider registry installs");
    let runtime = PlecRuntime::new();
    runtime
        .set_host_registry(
            js_sys::Reflect::get(&js_sys::global(), &"__test_host_registry".into())
                .expect("registry exists"),
        )
        .expect("registry set");
    runtime
        .load_application(keyed_host_callback_artifact())
        .expect("row artifact loads");
    let root = fresh_root();
    runtime.mount(root.clone()).expect("mount");
    runtime
        .initialize_input(
            "rows".into(),
            serde_wasm_bindgen::to_value(&serde_json::json!([
                {"id": "row-a", "title": "first"}
            ]))
            .unwrap(),
        )
        .expect("row mounts");
    let callback = || -> js_sys::Function {
        js_sys::Function::from(
            js_sys::Reflect::get(&js_sys::global(), &"__row_callback".into())
                .expect("row callback exists"),
        )
    };
    callback().call0(&JsValue::NULL).expect("first callback");
    assert_eq!(text_content(&root), "first");

    runtime
        .apply_deltas(
            serde_wasm_bindgen::to_value(&serde_json::json!([
                {"type": "update", "inputId": "rows", "rowKey": "row-a", "changes": {"title": "second"}}
            ]))
            .unwrap(),
        )
        .expect("row updates");
    callback().call0(&JsValue::NULL).expect("updated callback");
    assert_eq!(
        text_content(&root),
        "second",
        "callback must read the row's current values after a keyed update"
    );

    runtime
        .apply_deltas(
            serde_wasm_bindgen::to_value(&serde_json::json!([
                {"type": "remove", "inputId": "rows", "rowKey": "row-a"}
            ]))
            .unwrap(),
        )
        .expect("row removes");
    let error = callback()
        .call0(&JsValue::NULL)
        .expect_err("removed row callback must reject");
    assert!(
        error
            .as_string()
            .is_some_and(|message| message.contains("stale host callback")),
        "removed row callback must reject as stale, got: {error:?}"
    );
}
