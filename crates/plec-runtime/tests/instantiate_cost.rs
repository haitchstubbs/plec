//! Hot-path instantiation cost measurement for structural address emission
//! (wasm-runtime-ixk.2 acceptance criterion 6). This is a measurement, not a
//! regression gate: the numbers are printed so before/after medians can be
//! recorded on the protocol migration issue. The assertions only guard
//! against pathological accidents so the test stays cheap and meaningful.

use plec_runtime::PlecRuntime;
use wasm_bindgen_test::*;
use web_sys::{window, Element};

wasm_bindgen_test_configure!(run_in_browser);

/// A list row with a few elements and two bound texts: the shape the
/// structural address emission cost is amortized over.
fn list_artifact() -> serde_json::Value {
    serde_json::json!({
        "rootNode": 0,
        "strings": ["div", "ul", "li", "span", "em", "items", "id", "title", "done"],
        "constants": [[]],
        "nodes": [
            {"op":"element", "tag":0, "children":[1]},
            {"op":"element", "tag":1, "parent":0, "children":[2]},
            {"op":"loop", "loop":0, "parent":1},
            {"op":"element", "tag":2, "children":[4, 5]},
            {"op":"element", "tag":3, "parent":3, "children":[6]},
            {"op":"element", "tag":4, "parent":3, "children":[7]},
            {"op":"text", "text":0, "parent":4},
            {"op":"text", "text":1, "parent":5}
        ],
        "texts": [{"binding":0}, {"binding":1}],
        "bindings": [
            {"target":6, "sink":"text", "expression":0},
            {"target":7, "sink":"text", "expression":1}
        ],
        "inputs": [{"name":5, "kind":"collection"}],
        "events": [],
        "stateSlots": [],
        "expressions": [
            {"instructions":[{"op":"loadRowField","field":7},{"op":"return"}]},
            {"instructions":[{"op":"loadRowField","field":8},{"op":"return"}]},
            {"instructions":[{"op":"constant","constant":0},{"op":"return"}]},
            {"instructions":[{"op":"loadRowField","field":6},{"op":"return"}]}
        ],
        "actions": [],
        "loops": [{"sourceExpression":2,"keyExpression":3,"itemSlot":0,"rowTemplate":3,"input":0}],
        "dependencyEdges": []
    })
}

fn rows(count: usize) -> serde_json::Value {
    (0..count)
        .map(|index| {
            serde_json::json!({
                "id": format!("row-{index}"),
                "title": format!("Title {index}"),
                "done": index % 2 == 0,
            })
        })
        .collect()
}

fn now_ms() -> f64 {
    window()
        .expect("window")
        .performance()
        .expect("performance")
        .now()
}

fn median(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    samples[samples.len() / 2]
}

fn mount_root() -> Element {
    window()
        .expect("window")
        .document()
        .expect("document")
        .create_element("div")
        .expect("div")
}

/// Wrap the bare single-graph artifact in the IR 0.10 component-application
/// envelope `load_application` accepts.
fn component_application(graph: &serde_json::Value) -> serde_json::Value {
    let mut graph = graph.clone();
    let fields = graph.as_object_mut().unwrap();
    fields.remove("version");
    fields.insert(
        "id".into(),
        serde_json::Value::String("instantiate-cost.tsx#List".into()),
    );
    serde_json::json!({
        "version": "0.10",
        "rootComponent": 0,
        "components": [graph]
    })
}

#[wasm_bindgen_test]
fn measures_list_instantiation_cost_with_structural_addresses() {
    const ROWS: usize = 200;
    const RUNS: usize = 15;
    let artifact = component_application(&list_artifact());
    let payload = rows(ROWS);
    let mut mount_samples = Vec::new();
    let mut one_row_samples = Vec::new();
    for run in 0..RUNS {
        let runtime = PlecRuntime::new();
        let root = mount_root();
        runtime
            .load_application(serde_wasm_bindgen::to_value(&artifact).unwrap())
            .unwrap();
        runtime.mount(root.clone()).unwrap();
        let start = now_ms();
        runtime
            .initialize_input(
                "items".into(),
                serde_wasm_bindgen::to_value(&payload).unwrap(),
            )
            .unwrap();
        let after_mount = now_ms();
        mount_samples.push(after_mount - start);
        if run == 0 {
            let mounted = root.query_selector_all("li").unwrap().length();
            assert_eq!(mounted as usize, ROWS, "row elements must be mounted");
        }
        // The "change one list" hot path: one row insert after mount.
        let start = now_ms();
        runtime
            .apply_delta(
                serde_wasm_bindgen::to_value(&serde_json::json!({
                    "type":"insert",
                    "input_id":"items",
                    "row_key":"row-extra",
                    "row":{"id":"row-extra","title":"Extra","done":false},
                    "before_row_key":null
                }))
                .unwrap(),
            )
            .unwrap();
        one_row_samples.push(now_ms() - start);
    }
    let mount_median = median(mount_samples);
    let one_row_median = median(one_row_samples);
    web_sys::console::log_1(
        &format!(
            "[instantiate-cost] rows={ROWS} runs={RUNS} median_mount_ms={mount_median:.3} median_one_row_insert_ms={one_row_median:.3}"
        )
        .into(),
    );
    // Pathological-accident guard only; the recorded medians are the data.
    assert!(
        mount_median < 1_000.0,
        "list mount regressed: {mount_median}"
    );
    assert!(
        one_row_median < 100.0,
        "one-row insert regressed: {one_row_median}"
    );
}
