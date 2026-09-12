#![cfg(target_arch = "wasm32")]

//! Runtime-local host provider registry contract (wasm-runtime-a2n.4):
//! provider authority is scoped per PlecRuntime — two runtimes may bind the
//! same provider id to different implementations, a registry replaced after
//! mount cannot redirect update/dispose away from the lifecycle that
//! mounted, unknown providers fail deterministically, and top-level dispose
//! drains only that runtime's provider handles.

use plec_runtime::PlecRuntime;
use wasm_bindgen::JsValue;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn eval_bool(expression: &str) -> bool {
    js_sys::eval(expression)
        .expect("expression evaluates")
        .as_bool()
        .expect("expression is boolean")
}

/// A keyed loop with one host icon per row — the same compiled shape as the
/// lucide icon-in-a-list case, refreshed through the row component-refresh
/// queue so the host boundary update path runs.
fn icon_artifact() -> JsValue {
    serde_wasm_bindgen::to_value(&icon_artifact_json()).expect("artifact JSON")
}

fn icon_artifact_json() -> serde_json::Value {
    serde_json::json!({
        "version": "0.10",
        "rootComponent": 0,
        "components": [{
            "id": "registry-scope.tsx#List",
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
    })
}

fn fresh_root(tag: &str) -> web_sys::Element {
    web_sys::window()
        .expect("window")
        .document()
        .expect("document")
        .create_element(tag)
        .expect("root element")
}

/// Installs a JS provider implementation named `name` that records every
/// lifecycle call on `globalThis.__scope_calls[name]` and stamps its name on
/// the boundary it owns. Returns the registry resolving `lucide/Beaker` to
/// exactly this implementation.
fn scoped_registry_js(name: &str) -> JsValue {
    let source = format!(
        r#"
(() => {{
  const calls = {{ mount: 0, update: 0, dispose: 0 }};
  globalThis.__scope_calls ??= {{}};
  globalThis.__scope_calls[{name:?}] = calls;
  const lifecycle = {{
    mount: (boundary, props) => {{
      calls.mount += 1;
      const span = document.createElement('span');
      span.setAttribute('data-impl', {name:?});
      span.setAttribute('class', String(props.className ?? ''));
      boundary.replaceChildren(span);
      return span;
    }},
    update: (handle) => {{
      calls.update += 1;
      handle.setAttribute('data-updated-by', {name:?});
    }},
    dispose: () => {{
      calls.dispose += 1;
    }},
  }};
  return {{
    resolve: (provider, component) =>
      provider === 'lucide' && component === 'Beaker' ? lifecycle : undefined,
  }};
}})()
"#
    );
    js_sys::eval(&source).expect("scoped registry snippet evaluates")
}

#[wasm_bindgen_test]
fn two_runtimes_resolve_the_same_provider_id_independently() {
    let runtime_a = PlecRuntime::new();
    runtime_a
        .set_host_registry(scoped_registry_js("implA"))
        .expect("registry A installs");
    let runtime_b = PlecRuntime::new();
    runtime_b
        .set_host_registry(scoped_registry_js("implB"))
        .expect("registry B installs");

    let root_a = fresh_root("div");
    let root_b = fresh_root("div");
    runtime_a
        .load_application(icon_artifact())
        .expect("artifact loads under runtime A");
    runtime_b
        .load_application(icon_artifact())
        .expect("artifact loads under runtime B");
    runtime_a.mount(root_a.clone()).expect("mount A");
    runtime_b.mount(root_b.clone()).expect("mount B");

    for runtime in [&runtime_a, &runtime_b] {
        runtime
            .initialize_input(
                "rows".into(),
                serde_wasm_bindgen::to_value(&serde_json::json!([{"id": "row-a"}])).unwrap(),
            )
            .expect("row mount");
    }

    let impl_a = root_a
        .query_selector("[data-impl]")
        .expect("query works")
        .expect("runtime A provider content exists");
    let impl_b = root_b
        .query_selector("[data-impl]")
        .expect("query works")
        .expect("runtime B provider content exists");
    assert_eq!(
        impl_a.get_attribute("data-impl").as_deref(),
        Some("implA"),
        "runtime A must resolve its own implementation"
    );
    assert_eq!(
        impl_b.get_attribute("data-impl").as_deref(),
        Some("implB"),
        "runtime B must resolve its own implementation for the same provider id"
    );
}

#[wasm_bindgen_test]
fn registry_replacement_after_mount_cannot_redirect_lifecycle() {
    let runtime = PlecRuntime::new();
    runtime
        .set_host_registry(scoped_registry_js("first"))
        .expect("initial registry installs");
    runtime
        .load_application(icon_artifact())
        .expect("artifact loads");
    let root = fresh_root("div");
    runtime.mount(root).expect("mount");
    runtime
        .initialize_input(
            "rows".into(),
            serde_wasm_bindgen::to_value(&serde_json::json!([{"id": "row-a"}])).unwrap(),
        )
        .expect("row mount");

    // Replace the registry after the provider already mounted.
    runtime
        .set_host_registry(scoped_registry_js("second"))
        .expect("replacement registry installs");

    runtime
        .apply_deltas(
            serde_wasm_bindgen::to_value(&serde_json::json!([
                {"type": "update", "inputId": "rows", "rowKey": "row-a", "changes": {}}
            ]))
            .unwrap(),
        )
        .expect("row update");

    assert!(
        eval_bool("globalThis.__scope_calls.first.update === 1"),
        "update must route to the lifecycle captured at mount"
    );
    assert!(
        eval_bool("globalThis.__scope_calls.second.update === 0"),
        "replacement registry must not receive updates for mounted handles"
    );

    runtime.dispose().expect("dispose");
    assert!(
        eval_bool("globalThis.__scope_calls.first.dispose === 1"),
        "dispose must route to the lifecycle captured at mount"
    );
    assert!(
        eval_bool("globalThis.__scope_calls.second.dispose === 0"),
        "replacement registry must not receive dispose for mounted handles"
    );
}

#[wasm_bindgen_test]
fn unknown_host_component_fails_deterministically() {
    js_sys::eval("globalThis.__scope_calls = {};").expect("reset calls");
    let runtime = PlecRuntime::new();
    runtime
        .set_host_registry(scoped_registry_js("onlyKnownComponent"))
        .expect("registry installs");
    // The scoped registry resolves only lucide/Beaker; this artifact asks
    // for lucide/Missing.
    let mut artifact_json = single_host_artifact("Beaker");
    artifact_json["components"][0]["nodes"][1]["component"] = serde_json::json!("Missing");
    runtime
        .load_application(serde_wasm_bindgen::to_value(&artifact_json).unwrap())
        .expect("artifact loads; failure surfaces at mount");
    let root = fresh_root("div");
    let error = runtime.mount(root).expect_err("unknown provider must fail");
    assert!(
        error
            .as_string()
            .is_some_and(|message| message.contains("unknown host component: lucide/Missing")),
        "unknown host component must fail deterministically, got: {error:?}"
    );
}

/// Root-level host component (no loop): the host mount runs eagerly on
/// `mount`, so failure paths surface immediately.
fn single_host_artifact(component: &str) -> serde_json::Value {
    serde_json::json!({
        "version": "0.10",
        "rootComponent": 0,
        "components": [{
            "id": "registry-scope.tsx#Single",
            "rootNode": 0,
            "strings": ["div", "className"],
            "constants": ["size-4"],
            "nodes": [
                {"op": "element", "tag": 0, "parent": null, "children": [1]},
                {"op": "hostComponent", "provider": "lucide", "component": component,
                 "parent": 0,
                 "props": [{"kind": "value", "name": 1, "expression": 0}]}
            ],
            "texts": [],
            "bindings": [],
            "inputs": [],
            "events": [],
            "stateSlots": [],
            "expressions": [
                {"instructions": [{"op": "constant", "constant": 0}, {"op": "return"}]}
            ],
            "actions": [],
            "loops": [],
            "dependencyEdges": []
        }]
    })
}

#[wasm_bindgen_test]
fn missing_registry_fails_closed_at_mount() {
    let runtime = PlecRuntime::new();
    runtime
        .load_application(serde_wasm_bindgen::to_value(&single_host_artifact("Beaker")).unwrap())
        .expect("artifact loads");
    let root = fresh_root("div");
    let error = runtime
        .mount(root)
        .expect_err("mount without a registry must fail");
    assert!(
        error
            .as_string()
            .is_some_and(|message| message.contains("host component registry is not installed")),
        "absent registry must fail closed, got: {error:?}"
    );
}

#[wasm_bindgen_test]
fn top_level_dispose_drains_only_its_own_provider_handles() {
    js_sys::eval("globalThis.__scope_calls = {};").expect("reset calls");
    let runtime_a = PlecRuntime::new();
    runtime_a
        .set_host_registry(scoped_registry_js("drainA"))
        .expect("registry A installs");
    let runtime_b = PlecRuntime::new();
    runtime_b
        .set_host_registry(scoped_registry_js("drainB"))
        .expect("registry B installs");

    let root_a = fresh_root("div");
    let root_b = fresh_root("div");
    runtime_a
        .load_application(icon_artifact())
        .expect("artifact A loads");
    runtime_b
        .load_application(icon_artifact())
        .expect("artifact B loads");
    runtime_a.mount(root_a).expect("mount A");
    runtime_b.mount(root_b).expect("mount B");
    for runtime in [&runtime_a, &runtime_b] {
        runtime
            .initialize_input(
                "rows".into(),
                serde_wasm_bindgen::to_value(&serde_json::json!([{"id": "row-a"}])).unwrap(),
            )
            .expect("row mount");
    }

    runtime_a.dispose().expect("dispose A");
    assert!(
        eval_bool("globalThis.__scope_calls.drainA.dispose === 1"),
        "disposed runtime must drain its provider handles"
    );
    assert!(
        eval_bool("globalThis.__scope_calls.drainB.dispose === 0"),
        "disposing one runtime must not drain another runtime's handles"
    );
}
