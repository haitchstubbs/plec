#![cfg(target_arch = "wasm32")]

//! Boundary coverage for the documented untrusted-input limits (see
//! docs/security-limits.md): hostile artifact, host-input, and deeply nested
//! payloads must fail predictably at the WASM boundary before excessive
//! allocation or recursion.

use plec_client::runtime::TypedRuntime;
use plec_eval::eval::typed_eval;
use plec_ir::limits::{
    MAX_ARTIFACT_JSON_BYTES, MAX_HOST_INPUT_JSON_BYTES, MAX_SNAPSHOT_JSON_BYTES, MAX_VALUE_DEPTH,
};
use plec_runtime::PlecRuntime;
use plec_schema::delta::UpdateMetrics;
use plec_schema::typed::TypedApplication;
use wasm_bindgen::JsValue;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn js_from_json(json: &str) -> JsValue {
    js_sys::JSON::parse(json).expect("test payload should be valid JSON")
}

fn error_string(error: JsValue) -> String {
    error.as_string().unwrap_or_default()
}

#[wasm_bindgen_test]
fn host_inputs_within_limit_are_accepted() {
    let runtime = PlecRuntime::new();
    let inputs = serde_json::json!({
        "location.pathname": "/",
        "count": 3,
        "rows": [{"id": "a"}]
    });
    runtime
        .set_host_inputs(serde_wasm_bindgen::to_value(&inputs).unwrap())
        .expect("ordinary host inputs must stay accepted");
}

#[wasm_bindgen_test]
fn oversized_host_inputs_are_rejected_before_decode() {
    let runtime = PlecRuntime::new();
    let payload = format!(
        r#"{{"rows":[{{"pad":"{}"}}]}}"#,
        "x".repeat(MAX_HOST_INPUT_JSON_BYTES + 1024)
    );
    let error = runtime
        .set_host_inputs(js_from_json(&payload))
        .expect_err("oversized host inputs must be rejected");
    assert!(
        error_string(error).contains("exceeds byte limit"),
        "unexpected error"
    );
}

#[wasm_bindgen_test]
fn deeply_nested_host_inputs_are_rejected_before_deserialization() {
    let runtime = PlecRuntime::new();
    // The JS-value normalization boundary runs before RuntimeValue
    // deserialization, so deeply nested host input cannot grow the WASM
    // stack while decoding.
    let depth = 200;
    let payload = format!(
        r#"{{"v":{}}}"#,
        format_args!("{}1{}", "[".repeat(depth), "]".repeat(depth))
    );
    let error = runtime
        .set_host_inputs(js_from_json(&payload))
        .expect_err("over-deep host inputs must be rejected");
    assert!(
        error_string(error).contains("decode depth limit"),
        "unexpected error"
    );
}

#[wasm_bindgen_test]
fn oversized_application_artifacts_are_rejected() {
    let runtime = PlecRuntime::new();
    let artifact = format!(
        r#"{{"version":"0.10","rootComponent":0,"components":[{{"id":"App","version":"0.10","rootNode":0,"strings":["{}"],"nodes":[{{"op":"element","tag":0}}]}}]}}"#,
        "x".repeat(MAX_ARTIFACT_JSON_BYTES + 1024)
    );
    let error = runtime
        .load_application(js_from_json(&artifact))
        .expect_err("oversized application artifact must be rejected");
    assert!(
        error_string(error).contains("exceeds byte limit"),
        "unexpected error"
    );
}

#[wasm_bindgen_test]
fn deeply_nested_application_artifacts_are_rejected_before_deserialization() {
    let runtime = PlecRuntime::new();
    let depth = 200;
    let artifact = format!(
        r#"{{"version":"0.10","rootComponent":0,"components":[{{"id":"App","version":"0.10","rootNode":0,"strings":[],"constants":[{}],"nodes":[{{"op":"element","tag":0}}]}}]}}"#,
        format_args!("{}1{}", "[".repeat(depth), "]".repeat(depth))
    );
    let error = runtime
        .load_application(js_from_json(&artifact))
        .expect_err("over-deep application artifact must be rejected");
    assert!(
        error_string(error).contains("decode depth limit"),
        "unexpected error"
    );
}

#[wasm_bindgen_test]
fn runtime_value_depth_limit_rejects_shallow_but_over_deep_constants() {
    // The parser depth guard allows more nesting than the documented runtime
    // value limit; validation must still reject constants beyond
    // MAX_VALUE_DEPTH after decode.
    fn nested(depth: usize) -> String {
        let mut value = "1".to_string();
        for _ in 0..depth {
            value = format!("[{value}]");
        }
        value
    }
    let artifact = format!(
        r#"{{"version":"0.10","rootComponent":0,"components":[{{"id":"App","version":"0.10","rootNode":0,"strings":[],"constants":[{}],"nodes":[{{"op":"element","tag":0}}]}}]}}"#,
        nested(MAX_VALUE_DEPTH + 8)
    );
    let runtime = PlecRuntime::new();
    let error = runtime
        .load_application(js_from_json(&artifact))
        .expect_err("over-deep constants must be rejected by validation");
    assert!(
        error_string(error).contains("nesting exceeds limit"),
        "unexpected error"
    );
}

#[wasm_bindgen_test]
fn oversized_ssr_snapshots_are_rejected_before_decode() {
    let runtime = PlecRuntime::new();
    let payload = format!(
        r#"{{"pad":"{}"}}"#,
        "x".repeat(MAX_SNAPSHOT_JSON_BYTES + 1024)
    );
    // The snapshot decode path is exercised through start_adopt_snapshot,
    // which first requires a manifest; craft a minimal v3 manifest and a
    // mismatched-but-oversized snapshot payload.
    let manifest =
        js_from_json(r#"{"version":3,"revision":"rev-1","rootGraphId":"app#Index","routes":[]}"#);
    let root = web_sys::window()
        .unwrap()
        .document()
        .unwrap()
        .create_element("div")
        .unwrap();
    let error = runtime
        .start_adopt_snapshot(root, manifest, js_from_json(&payload))
        .expect_err("oversized snapshot payload must fail closed");
    assert!(
        error_string(error) == "mismatch:ssr-snapshot-payload",
        "unexpected error"
    );
}

#[wasm_bindgen_test]
fn small_envelope_artifact_loads_through_bounded_decode() {
    let runtime = PlecRuntime::new();
    let artifact = r#"{
        "version": "0.10", "rootComponent": 0,
        "components": [{
            "id": "App", "version": "0.10", "rootNode": 0,
            "strings": ["div"], "constants": [],
            "nodes": [{"op": "element", "tag": 0}]
        }]
    }"#;
    runtime
        .load_application(js_from_json(artifact))
        .expect("a small legitimate artifact must load");
    // Same payload delivered the way the compiler/host glue delivers it:
    // serde-serialized into a JS value first (serde maps arrive as JS Map).
    let parsed: serde_json::Value = serde_json::from_str(artifact).unwrap();
    let via_serde = serde_wasm_bindgen::to_value(&parsed).unwrap();
    PlecRuntime::new()
        .load_application(via_serde)
        .expect("a serde-serialized artifact must load");
}

#[wasm_bindgen_test]
fn nested_js_maps_preserve_json_fields() {
    let outer = js_sys::Map::new();
    let rows = js_sys::Array::new();
    let row = js_sys::Map::new();
    row.set(&JsValue::from_str("id"), &JsValue::from_str("a"));
    rows.push(&row);
    outer.set(&JsValue::from_str("rows"), &rows);

    let normalized = plec_client::runtime::normalize_json_value(&outer, 0)
        .expect("nested maps should normalize");
    let json: serde_json::Value =
        serde_json::from_str(&String::from(js_sys::JSON::stringify(&normalized).unwrap())).unwrap();

    assert_eq!(json["rows"][0]["id"], "a");
}

#[wasm_bindgen_test]
fn nested_undefined_normalizes_to_null() {
    let input = js_sys::Object::new();
    js_sys::Reflect::set(&input, &JsValue::from_str("optional"), &JsValue::UNDEFINED).unwrap();

    let normalized = plec_client::runtime::normalize_json_value(&input, 0)
        .expect("undefined field should normalize");
    let json: serde_json::Value =
        serde_json::from_str(&String::from(js_sys::JSON::stringify(&normalized).unwrap())).unwrap();

    assert!(json["optional"].is_null());
}

fn bounded_application() -> TypedApplication {
    serde_json::from_str(
        r#"{
        "version": "0.10", "rootNode": 0, "strings": ["div"],
        "constants": [[1, 2, 3]],
        "nodes": [{"op": "element", "tag": 0}],
        "expressions": [], "actions": []
    }"#,
    )
    .unwrap()
}

#[wasm_bindgen_test]
fn backward_jump_loop_exhausts_expression_budget() {
    let mut app = bounded_application();
    app.expressions = serde_json::from_value(serde_json::json!([
        {"instructions": [{"op": "jump", "target": 0}]}
    ]))
    .unwrap();
    let error = typed_eval(&app, 0, &[], None, 0)
        .expect_err("crafted backward-jump loop must exhaust the budget");
    assert!(
        error
            .as_string()
            .unwrap_or_default()
            .contains("execution budget exceeded"),
        "{error:?}"
    );
}

#[wasm_bindgen_test]
fn self_referential_map_terminates_without_overflow() {
    let mut app = bounded_application();
    app.expressions = serde_json::from_value(serde_json::json!([
        {"instructions": [
            {"op": "constant", "constant": 0},
            {"op": "map", "mapper": 1, "itemSlot": 0},
            {"op": "return"}
        ]},
        {"instructions": [
            {"op": "constant", "constant": 0},
            {"op": "map", "mapper": 1, "itemSlot": 0},
            {"op": "return"}
        ]}
    ]))
    .unwrap();
    let error = typed_eval(&app, 0, &[], None, 0).expect_err("self-referential map must terminate");
    let message = error.as_string().unwrap_or_default();
    assert!(
        message.contains("execution budget exceeded") || message.contains("nesting exceeds limit"),
        "{message}"
    );
}

#[wasm_bindgen_test]
fn crafted_backward_jump_loop_exhausts_action_budget() {
    let mut app = bounded_application();
    app.actions = serde_json::from_value(serde_json::json!([
        {"frameSlots": 0, "instructions": [{"op": "jump", "target": 0}]}
    ]))
    .unwrap();
    let mut runtime = TypedRuntime::new(app).unwrap();
    let mut metrics = UpdateMetrics::default();
    let error = runtime
        .execute_action(0, &[], None, None, &mut metrics)
        .expect_err("crafted action loop must exhaust the budget");
    assert!(
        error
            .as_string()
            .unwrap_or_default()
            .contains("execution budget exceeded"),
        "{error:?}"
    );
}

#[wasm_bindgen_test]
fn self_requeuing_reaction_terminates_within_drain_depth() {
    let app: TypedApplication = serde_json::from_str(
        r#"{
        "version": "0.10", "rootNode": 0, "strings": ["div"],
        "constants": [null, 1],
        "nodes": [{"op": "element", "tag": 0}],
        "stateSlots": [{"initialExpression": 0, "frameSlot": 0}],
        "expressions": [
            {"instructions": [{"op": "constant", "constant": 0}, {"op": "return"}]},
            {"instructions": [{"op": "constant", "constant": 1}, {"op": "return"}]}
        ],
        "reactions": [{"dependencies": [0], "action": 0}],
        "dependencyEdges": [
            {"source": {"kind": "state", "handle": 0}, "target": {"kind": "reaction", "handle": 0}}
        ],
        "actions": [{
            "instructions": [
                {"op": "evaluate", "expression": 1},
                {"op": "storeState", "state": 0},
                {"op": "return"}
            ]
        }]
    }"#,
    )
    .unwrap();
    let mut runtime = TypedRuntime::new(app).unwrap();
    let mut metrics = UpdateMetrics::default();
    let error = runtime
        .execute_action(0, &[], None, None, &mut metrics)
        .expect_err("self-requeuing reaction must terminate");
    let message = error.as_string().unwrap_or_default();
    assert!(
        message.contains("reaction drain depth exceeds limit")
            || message.contains("reaction execution budget exceeded"),
        "{message}"
    );
}
