use wasm_bindgen::JsValue;
use wasm_bindgen_test::wasm_bindgen_test;

wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn js_value(value: &serde_json::Value) -> JsValue {
    js_sys::JSON::parse(&value.to_string()).expect("benchmark fixture is valid JSON")
}

fn normalize(value: &JsValue, max_bytes: usize) -> JsValue {
    let value = plec_client::runtime::normalize_json_value(value, 0).unwrap();
    let text = js_sys::JSON::stringify(&value).unwrap();
    assert!(String::from(text).len() <= max_bytes);
    value
}

fn browser_heap_bytes() -> Option<f64> {
    let performance = web_sys::window()?.performance()?;
    let memory = js_sys::Reflect::get(performance.as_ref(), &JsValue::from_str("memory")).ok()?;
    js_sys::Reflect::get(&memory, &JsValue::from_str("usedJSHeapSize"))
        .ok()?
        .as_f64()
}

fn report(name: &str, iterations: usize, elapsed_ms: f64, heap_delta: Option<f64>) {
    let heap = heap_delta
        .map(|bytes| format!("{bytes:.0}"))
        .unwrap_or_else(|| "unavailable".into());
    println!(
        "decode-bench {name}: iterations={iterations} total_ms={elapsed_ms:.3} per_decode_us={:.3} retained_js_heap_delta_bytes={heap}",
        elapsed_ms * 1000.0 / iterations as f64,
    );
}

#[wasm_bindgen_test]
fn reports_bounded_artifact_and_ssr_decode_latency_and_retained_heap() {
    let constants = (0..256)
        .map(|index| {
            serde_json::json!({
                "id": index,
                "label": format!("constant-{index}"),
                "nested": [true, null, {"value": index}]
            })
        })
        .collect::<Vec<_>>();
    let artifact = js_value(&serde_json::json!({
        "version":"0.10",
        "rootComponent":0,
        "components":[{
            "id":"bench",
            "rootNode":0,
            "strings":["div"],
            "constants":constants,
            "nodes":[{"op":"element","tag":0,"parent":null}],
            "expressions":[{"instructions":[]}],
            "actions":[{"instructions":[{"op":"return"}]}]
        }]
    }));
    let exports = (0..128)
        .map(|index| {
            (
                format!("export-{index}"),
                serde_json::json!({
                    "value":{"level":[index, "snapshot-value", {"ok":true}]},
                    "declaration":{
                        "name":format!("export-{index}"),
                        "sourceOwner":"server",
                        "valueIsSerializable":true,
                        "explicitlyPublic":true
                    }
                }),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    let snapshot = js_value(&serde_json::json!({
        "version":2,
        "revision":"bench-revision",
        "routes":[{"routeId":"bench","params":{"id":"a"},"phase":"active"}],
        "public":{"location":"/bench","exports":exports},
        "loaders":[],
        "structure":{"graphs":{"root/outlet:main":{"graphId":"bench","branches":[],"loops":[]}},"nested":{}}
    }));

    let window = web_sys::window().expect("browser window");
    let performance = window.performance().expect("browser performance API");
    let iterations = 64;
    let heap_before = browser_heap_bytes();
    let start = performance.now();
    let mut retained = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let normalized = normalize(&artifact, plec_schema::limits::MAX_ARTIFACT_JSON_BYTES);
        let decoded = plec_schema::typed::decode_component_application(&normalized).unwrap();
        decoded.validate().unwrap();
        retained.push(decoded);
    }
    let elapsed = performance.now() - start;
    let heap_after = browser_heap_bytes();
    report(
        "artifact",
        iterations,
        elapsed,
        heap_before
            .zip(heap_after)
            .map(|(before, after)| after - before),
    );
    assert_eq!(retained.len(), iterations);
    drop(retained);

    let heap_before = browser_heap_bytes();
    let start = performance.now();
    let mut retained = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let normalized = normalize(&snapshot, plec_schema::limits::MAX_SNAPSHOT_JSON_BYTES);
        retained.push(plec_schema::ssr_decode::decode_snapshot(&normalized).unwrap());
    }
    let elapsed = performance.now() - start;
    let heap_after = browser_heap_bytes();
    report(
        "ssr_snapshot",
        iterations,
        elapsed,
        heap_before
            .zip(heap_after)
            .map(|(before, after)| after - before),
    );
    assert_eq!(retained.len(), iterations);
}
