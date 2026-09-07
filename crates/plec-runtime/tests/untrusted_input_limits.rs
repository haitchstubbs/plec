#![cfg(target_arch = "wasm32")]

//! Boundary coverage for the documented untrusted-input limits (see
//! docs/security-limits.md): hostile artifact, host-input, and deeply nested
//! payloads must fail predictably at the WASM boundary before excessive
//! allocation or recursion.

use plec_client::runtime::TypedRuntime;
use plec_eval::eval::typed_eval;
use plec_ir::limits::{
    MAX_ARTIFACT_JSON_BYTES, MAX_HOST_INPUT_JSON_BYTES, MAX_LOOP_ROWS, MAX_MOUNT_DEPTH,
    MAX_SNAPSHOT_JSON_BYTES, MAX_SNAPSHOT_SHAPE_PATHS, MAX_SNAPSHOT_SHAPE_PATH_SEGMENTS,
    MAX_VALUE_DEPTH, MAX_VALUE_NODES,
};
use plec_runtime::PlecRuntime;
use plec_schema::delta::UpdateMetrics;
use plec_schema::typed::TypedApplication;
use std::collections::HashMap;
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

fn scalar_shape() -> JsValue {
    js_from_json(r#"{"kind":"scalar"}"#)
}

#[wasm_bindgen_test]
fn oversized_snapshot_input_values_are_rejected() {
    let runtime = PlecRuntime::new();
    let payload = format!(
        r#"{{"pad":"{}"}}"#,
        "x".repeat(MAX_HOST_INPUT_JSON_BYTES + 1024)
    );
    let error = runtime
        .initialize_snapshot_input("value".into(), js_from_json(&payload), scalar_shape())
        .expect_err("oversized snapshot value must be rejected at initialize");
    assert!(
        error_string(error).contains("exceeds byte limit"),
        "unexpected error"
    );
    runtime
        .initialize_snapshot_input(
            "value".into(),
            js_from_json(r#"{"pad":"small"}"#),
            scalar_shape(),
        )
        .expect("a small snapshot value must initialize");
    let error = runtime
        .apply_input_snapshot("value".into(), js_from_json(&payload))
        .expect_err("oversized snapshot value must be rejected at apply");
    assert!(
        error_string(error).contains("exceeds byte limit"),
        "unexpected error"
    );
}

#[wasm_bindgen_test]
fn deeply_nested_snapshot_input_values_are_rejected() {
    let runtime = PlecRuntime::new();
    let depth = 200;
    let payload = format!("{}1{}", "[".repeat(depth), "]".repeat(depth));
    let error = runtime
        .initialize_snapshot_input("value".into(), js_from_json(&payload), scalar_shape())
        .expect_err("over-deep snapshot value must be rejected");
    assert!(
        error_string(error).contains("decode depth limit"),
        "unexpected error"
    );
    runtime
        .initialize_snapshot_input(
            "value".into(),
            js_from_json(r#"{"pad":"small"}"#),
            scalar_shape(),
        )
        .expect("a small snapshot value must initialize");
    let error = runtime
        .apply_input_snapshot("value".into(), js_from_json(&payload))
        .expect_err("over-deep snapshot value must be rejected at apply");
    assert!(
        error_string(error).contains("decode depth limit"),
        "unexpected error"
    );
}

#[wasm_bindgen_test]
fn snapshot_input_node_budget_is_enforced() {
    let runtime = PlecRuntime::new();
    // Just over MAX_VALUE_NODES scalars stays well below the 1 MiB byte
    // envelope, so the structural node budget is what rejects this payload.
    let payload = format!(
        "[{}]",
        (0..MAX_VALUE_NODES)
            .map(|_| "0")
            .collect::<Vec<_>>()
            .join(",")
    );
    let error = runtime
        .initialize_snapshot_input("value".into(), js_from_json(&payload), scalar_shape())
        .expect_err("structurally excessive snapshot value must be rejected");
    assert!(
        error_string(error).contains("runtime value size exceeds limit"),
        "unexpected error"
    );
}

#[wasm_bindgen_test]
fn snapshot_shape_path_limits_are_enforced() {
    let runtime = PlecRuntime::new();
    let shape = format!(
        r#"{{"kind":"object","observed_paths":[{}]}}"#,
        (0..=MAX_SNAPSHOT_SHAPE_PATHS)
            .map(|index| format!(r#"["path{index}"]"#))
            .collect::<Vec<_>>()
            .join(",")
    );
    let error = runtime
        .initialize_snapshot_input(
            "value".into(),
            js_from_json(r#"{"a":1}"#),
            js_from_json(&shape),
        )
        .expect_err("snapshot shape beyond the path limit must be rejected");
    assert!(
        error_string(error).contains("snapshot shape path count exceeds limit"),
        "unexpected error"
    );
    let segments = MAX_SNAPSHOT_SHAPE_PATH_SEGMENTS + 1;
    let deep_shape = format!(
        r#"{{"kind":"object","observed_paths":[[{}]]}}"#,
        vec![r#""a""#; segments].join(",")
    );
    let error = runtime
        .initialize_snapshot_input(
            "value".into(),
            js_from_json(r#"{"a":1}"#),
            js_from_json(&deep_shape),
        )
        .expect_err("snapshot shape beyond the path depth limit must be rejected");
    assert!(
        error_string(error).contains("snapshot shape path depth exceeds limit"),
        "unexpected error"
    );
}

#[wasm_bindgen_test]
fn failed_snapshot_apply_preserves_previous_projection() {
    let runtime = PlecRuntime::new();
    let shape = js_from_json(r#"{"kind":"object","observedPaths":[["name"]]}"#);
    runtime
        .initialize_snapshot_input("value".into(), js_from_json(r#"{"name":"A"}"#), shape)
        .expect("snapshot initialization must succeed");
    let oversized = format!(
        r#"{{"pad":"{}"}}"#,
        "x".repeat(MAX_HOST_INPUT_JSON_BYTES + 1024)
    );
    let error = runtime
        .apply_input_snapshot("value".into(), js_from_json(&oversized))
        .expect_err("oversized snapshot apply must fail closed");
    assert!(
        error_string(error).contains("exceeds byte limit"),
        "unexpected error"
    );
    // Re-applying the identical projection must produce zero deltas: the
    // failed apply cannot have replaced the retained projection.
    let metrics = runtime
        .apply_input_snapshot("value".into(), js_from_json(r#"{"name":"A"}"#))
        .expect("re-applying the retained projection must succeed");
    let json: serde_json::Value = serde_wasm_bindgen::from_value(metrics).unwrap();
    assert_eq!(json["domOperations"], 0, "no deltas expected: {json}");
}

fn snapshot_application() -> PlecRuntime {
    // Collection-shaped snapshot inputs reconcile live typed inputs, which
    // requires a loaded (not mounted) application.
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
        .expect("snapshot test application must load");
    runtime
}

#[wasm_bindgen_test]
fn snapshot_inputs_within_limits_round_trip() {
    let runtime = snapshot_application();
    let shape = js_from_json(
        r#"{"kind":"collection","key_expression":"todo.id","observed_row_paths":[["title"]]}"#,
    );
    runtime
        .initialize_snapshot_input(
            "todos".into(),
            js_from_json(r#"[{"id":"a","title":"A"}]"#),
            shape.clone(),
        )
        .expect("legitimate snapshot must initialize");
    runtime
        .apply_input_snapshot(
            "todos".into(),
            js_from_json(r#"[{"id":"a","title":"B"},{"id":"b","title":"B"}]"#),
        )
        .expect("legitimate snapshot apply must succeed");
    // The same payloads delivered through serde (JS Map objects) must take
    // the same bounded path.
    let shape_value: serde_json::Value =
        serde_json::from_str(r#"{"kind":"object","observed_paths":[["name"]]}"#).unwrap();
    runtime
        .initialize_snapshot_input(
            "profile".into(),
            serde_wasm_bindgen::to_value(&serde_json::json!({"name":"a"})).unwrap(),
            serde_wasm_bindgen::to_value(&shape_value).unwrap(),
        )
        .expect("serde-delivered snapshot must initialize");
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

#[wasm_bindgen_test]
fn wide_js_array_exhausts_normalization_budget() {
    let input =
        js_sys::Array::new_with_length((plec_schema::limits::MAX_DECODE_JS_NODES + 1) as u32);
    let error = plec_client::runtime::normalize_json_value(&input, 0)
        .expect_err("wide JS array must exhaust budget");
    let message = error.as_string().unwrap_or_default();
    assert!(
        message.contains("decode width limit"),
        "expected width limit error, got: {message}"
    );
}

#[wasm_bindgen_test]
fn wide_js_map_exhausts_normalization_budget() {
    let map = js_sys::Map::new();
    // Use an array inside the map that pushes total nodes over the budget
    let array = js_sys::Array::new_with_length(plec_schema::limits::MAX_DECODE_JS_NODES as u32);
    map.set(&JsValue::from_str("items"), &array);
    let error = plec_client::runtime::normalize_json_value(&map, 0)
        .expect_err("wide JS map contents must exhaust budget");
    let message = error.as_string().unwrap_or_default();
    assert!(
        message.contains("decode width limit"),
        "expected width limit error, got: {message}"
    );
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

#[wasm_bindgen_test]
fn loop_projection_beyond_row_limit_is_rejected_before_mutation() {
    let mut runtime = TypedRuntime::new(bounded_application()).unwrap();
    let parent = web_sys::window()
        .unwrap()
        .document()
        .unwrap()
        .create_element("div")
        .unwrap()
        .into();
    let projection = (0..=MAX_LOOP_ROWS)
        .map(|index| (index.to_string(), HashMap::new()))
        .collect::<Vec<_>>();
    let mut metrics = UpdateMetrics::default();
    let error = runtime
        .reconcile_loop(0, &parent, projection, &mut metrics)
        .expect_err("projection beyond the loop row limit must fail closed");
    assert!(
        error_string(error).contains("LOOP_ROW_LIMIT_EXCEEDED"),
        "unexpected error"
    );
}

fn envelope_artifact(component: &str) -> String {
    format!(
        r#"{{"version":"0.10","rootComponent":0,"components":[{{"id":"App","version":"0.10","rootNode":0,"strings":["div"],{component}}}]}}"#
    )
}

#[wasm_bindgen_test]
fn load_application_rejects_self_child_node_graph() {
    // A self-referential child recursed unbounded during mount before
    // topology validation; it must now be rejected at the load boundary.
    let artifact =
        envelope_artifact(r#""nodes":[{"op":"element","tag":0,"parent":null,"children":[0]}]"#);
    let error = PlecRuntime::new()
        .load_application(js_from_json(&artifact))
        .expect_err("self-child node graph must be rejected");
    assert!(
        error_string(error).contains("node ownership is cyclic or shared"),
        "unexpected error"
    );
}

#[wasm_bindgen_test]
fn load_application_rejects_unrooted_node_graph() {
    let artifact = envelope_artifact(
        r#""nodes":[{"op":"element","tag":0,"parent":null,"children":[]},{"op":"element","tag":0,"parent":null,"children":[]}]"#,
    );
    let error = PlecRuntime::new()
        .load_application(js_from_json(&artifact))
        .expect_err("unrooted nodes must be rejected");
    assert!(
        error_string(error).contains("node graph contains unrooted nodes"),
        "unexpected error"
    );
}

#[wasm_bindgen_test]
fn deep_linear_node_chain_fails_at_mount_depth_limit() {
    // Topology validation accepts an acyclic chain, so one node past
    // MAX_MOUNT_DEPTH must exhaust the documented mount budget instead of
    // overflowing the WASM stack during instantiate_node recursion.
    let depth = MAX_MOUNT_DEPTH + 1;
    let mut nodes = String::new();
    for index in 0..depth {
        let parent = if index == 0 {
            "null".to_string()
        } else {
            (index - 1).to_string()
        };
        let children = if index + 1 < depth {
            format!("[{}]", index + 1)
        } else {
            "[]".to_string()
        };
        nodes.push_str(&format!(
            r#"{{"op":"element","tag":0,"parent":{parent},"children":{children}}},"#
        ));
    }
    nodes.pop();
    let app: TypedApplication = serde_json::from_str(&format!(
        r#"{{
        "version": "0.10", "rootNode": 0, "strings": ["div"],
        "nodes": [{nodes}],
        "expressions": [], "actions": []
    }}"#,
    ))
    .unwrap();
    let mut runtime = TypedRuntime::new(app).unwrap();
    let root = web_sys::window()
        .unwrap()
        .document()
        .unwrap()
        .create_element("div")
        .unwrap()
        .into();
    let error = runtime
        .mount(root)
        .err()
        .expect("chain deeper than the mount budget must fail closed");
    assert!(
        error_string(error).contains("mount depth exceeds limit"),
        "unexpected error"
    );
}

#[wasm_bindgen_test]
fn node_chain_within_mount_depth_limit_still_mounts() {
    // The budget must reject only pathological shapes: a chain exactly at
    // MAX_MOUNT_DEPTH is still legitimate output and mounts cleanly.
    let depth = MAX_MOUNT_DEPTH;
    let mut nodes = String::new();
    for index in 0..depth {
        let parent = if index == 0 {
            "null".to_string()
        } else {
            (index - 1).to_string()
        };
        let children = if index + 1 < depth {
            format!("[{}]", index + 1)
        } else {
            "[]".to_string()
        };
        nodes.push_str(&format!(
            r#"{{"op":"element","tag":0,"parent":{parent},"children":{children}}},"#
        ));
    }
    nodes.pop();
    let app: TypedApplication = serde_json::from_str(&format!(
        r#"{{
        "version": "0.10", "rootNode": 0, "strings": ["div"],
        "nodes": [{nodes}],
        "expressions": [], "actions": []
    }}"#,
    ))
    .unwrap();
    let mut runtime = TypedRuntime::new(app).unwrap();
    let root = web_sys::window()
        .unwrap()
        .document()
        .unwrap()
        .create_element("div")
        .unwrap()
        .into();
    runtime
        .mount(root)
        .expect("chain within the mount budget must mount");
}

#[wasm_bindgen_test]
fn load_application_rejects_out_of_range_expression_jump_target() {
    let artifact = envelope_artifact(
        r#""nodes":[{"op":"element","tag":0,"parent":null,"children":[]}],"expressions":[{"instructions":[{"op":"jump","target":9}]}]"#,
    );
    let error = PlecRuntime::new()
        .load_application(js_from_json(&artifact))
        .expect_err("out-of-range expression jump target must be rejected");
    assert!(
        error_string(error).contains("expression jump target out of range"),
        "unexpected error"
    );
}

#[wasm_bindgen_test]
fn self_tail_call_action_terminates_within_call_depth() {
    // Tail calls previously ran through native recursion with a fresh fuel
    // budget per level, so a self-tail-call action could recurse without
    // any bound. They now share the continuation machinery and its depth
    // budget.
    let mut app = bounded_application();
    app.actions = serde_json::from_value(serde_json::json!([
        {"frameSlots": 0, "instructions": [{"op": "call", "action": 0, "arguments": []}]}
    ]))
    .unwrap();
    let mut runtime = TypedRuntime::new(app).unwrap();
    let mut metrics = UpdateMetrics::default();
    let error = runtime
        .execute_action(0, &[], None, None, &mut metrics)
        .expect_err("self-tail-call action must terminate");
    assert!(
        error
            .as_string()
            .unwrap_or_default()
            .contains("action call depth exceeds limit"),
        "{error:?}"
    );
}
