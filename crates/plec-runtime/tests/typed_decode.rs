use wasm_bindgen::JsValue;
use wasm_bindgen_test::wasm_bindgen_test;

wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn js(source: &str) -> JsValue {
    js_sys::JSON::parse(source).expect("test JSON parses")
}

#[wasm_bindgen_test]
fn component_application_decoder_handles_nested_tags_defaults_aliases_and_unknown_fields() {
    let source = r#"{
      "version":"0.10","rootComponent":0,"components":[{
        "rootNode":0,"strings":["div"],
        "nodes":[{"op":"element","tag":0,"parent":null}],
        "expressions":[{"instructions":[{"op":"filter","predicate":0,"item_slot":2,"index_slot":3}]}],
        "actions":[{"instructions":[
          {"op":"capabilityRequest","capability":"serverAction","request":{"action":0,"arguments":[]},"successPc":1,"failurePc":1,"resultSlot":0,"errorSlot":1},
          {"op":"return"}
        ]}],
        "parameters":[{"name":0,"callable":false}],
        "unknownFutureField":{"ignored":true}
      }]}
    "#;
    let decoded = plec_schema::typed::decode_component_application(&js(source)).unwrap();
    assert_eq!(decoded.version, "0.10");
    let app = &decoded.components[0];
    assert_eq!(app.version, "0.10");
    assert_eq!(app.id, "");
    assert!(app.constants.is_empty());
    assert!(
        matches!(app.nodes[0], plec_schema::typed::TypedNode::Element { ref namespace, .. } if namespace == "html")
    );
    assert!(matches!(
        app.expressions[0].instructions[0],
        plec_schema::typed::TypedExpressionInstruction::Filter {
            item_slot: 2,
            index_slot: Some(3),
            ..
        }
    ));
    assert!(matches!(
        app.actions[0].instructions[0],
        plec_schema::typed::TypedActionInstruction::CapabilityRequest {
            request: plec_schema::typed::TypedCapabilityRequest::ServerAction(_),
            ..
        }
    ));
}

#[wasm_bindgen_test]
fn component_application_decoder_rejects_missing_fields_bad_tags_and_alias_collisions() {
    assert!(plec_schema::typed::decode_component_application(&js(
        r#"{"version":"0.10","rootComponent":0}"#
    ))
    .is_err());
    assert!(plec_schema::typed::decode_component_application(&js(r#"{"version":"0.10","rootComponent":0,"components":[{"rootNode":0,"strings":[],"nodes":[{"op":"future"}]}]}"#)).is_err());
    assert!(plec_schema::typed::decode_component_application(&js(r#"{"version":"0.10","rootComponent":0,"components":[{"rootNode":0,"strings":[],"nodes":[{"op":"element","tag":0}],"expressions":[{"instructions":[{"op":"filter","predicate":0,"item_slot":1,"itemSlot":1}]}]}]}"#)).is_err());
}

#[wasm_bindgen_test]
fn delta_decoder_preserves_aliases_optional_nulls_and_runtime_value_types() {
    let delta = plec_schema::delta::decode_delta_js(&js(
        r#"{"type":"insert","instanceId":null,"input_id":"items","row_key":"a","row":{"id":"a","n":2},"before_row_key":null,"ignored":true}"#,
    ))
    .unwrap();
    match delta {
        plec_schema::delta::RuntimeDelta::Insert {
            instance_id,
            input_id,
            row_key,
            row,
            before_row_key,
        } => {
            assert_eq!(instance_id, None);
            assert_eq!(input_id, "items");
            assert_eq!(row_key, "a");
            assert_eq!(before_row_key, None);
            assert_eq!(
                row.get("id"),
                Some(&plec_schema::RuntimeValue::String("a".into()))
            );
            assert_eq!(row.get("n"), Some(&plec_schema::RuntimeValue::Number(2.0)));
        }
        _ => panic!("wrong decoded delta variant"),
    }
    assert!(plec_schema::delta::decode_delta_js(&js(
        r#"{"type":"remove","inputId":"a","input_id":"b","rowKey":"k"}"#,
    ))
    .is_err());
}

#[wasm_bindgen_test]
fn ssr_snapshot_decoder_keeps_strict_schema_and_generic_failure_fallback() {
    let value = js(r#"{
          "version":2,"revision":"rev","routes":[{"routeId":"home"}],
          "public":{"location":"/"},"loaders":[{"graphId":"g","action":0,"state":{"kind":"rejected","failure":{"kind":"runtime","message":"private details","extra":true}}}],
          "structure":{"graphs":{"root/outlet:main":{"graphId":"g"}}}
        }"#);
    let snapshot = plec_schema::ssr_decode::decode_snapshot(&value).unwrap();
    assert_eq!(snapshot.routes[0].phase, plec_ir::SsrRoutePhase::Active);
    assert!(snapshot.routes[0].params.is_empty());
    assert!(
        matches!(snapshot.loaders[0].state, plec_ir::SsrLoaderState::Rejected { ref failure } if *failure == plec_ir::PublicRouteLoaderFailure::generic())
    );
    let with_unknown = js(
        r#"{"version":2,"revision":"rev","routes":[],"public":{"location":"/"},"structure":{"graphs":{},"unexpected":true}}"#,
    );
    assert!(plec_schema::ssr_decode::decode_snapshot(&with_unknown).is_err());
}

#[wasm_bindgen_test]
fn route_manifest_decoders_preserve_runtime_and_ir_defaults() {
    let ir = plec_schema::routing::decode_ir_manifest(&js(
        r#"{"version":3,"rootGraphId":"root","routes":[{"id":"home","path":"","graphId":"home","outletId":"main","meta":{"title":"Home"}}]}"#,
    ))
    .unwrap();
    assert_eq!(ir.version, 3);
    assert_eq!(ir.revision, "");
    assert_eq!(ir.routes[0].pending_mode, "replace");
    assert_eq!(
        ir.routes[0].meta.as_ref().unwrap().title.as_deref(),
        Some("Home")
    );

    let runtime = plec_schema::routing::decode_runtime_manifest(&js(
        r#"{"rootGraphId":"root","routes":[{"id":"home","path":"","graphId":"home","outletId":"main"}]}"#,
    ))
    .unwrap();
    assert_eq!(runtime.version, None);
    assert_eq!(runtime.routes[0].pending_mode, "replace");

    assert!(plec_schema::routing::decode_ir_manifest(&js(
        r#"{"version":3,"rootGraphId":"root","routes":[{"id":"home","path":"","graphId":"home","outletId":"main","meta":{"title":7}}]}"#,
    ))
    .is_err());
}

#[wasm_bindgen_test]
fn direct_numeric_decoders_reject_overflow_fraction_and_sign_mismatch() {
    assert_eq!(
        plec_schema::js_decode::u32(&JsValue::from_f64(u32::MAX as f64)).unwrap(),
        u32::MAX
    );
    assert!(plec_schema::js_decode::u32(&JsValue::from_f64(u32::MAX as f64 + 1.0)).is_err());
    assert!(
        plec_schema::js_decode::usize(&JsValue::from_f64(18_446_744_073_709_551_616.0)).is_err()
    );
    assert_eq!(
        plec_schema::js_decode::i64(&JsValue::from_f64(-9_223_372_036_854_775_808.0)).unwrap(),
        i64::MIN
    );
    assert!(plec_schema::js_decode::i64(&JsValue::from_f64(9_223_372_036_854_775_808.0)).is_err());
    assert!(plec_schema::js_decode::usize(&JsValue::from_f64(1.5)).is_err());
}

#[wasm_bindgen_test]
fn normalization_matches_json_number_coercions_without_a_json_decode_tree() {
    let non_finite =
        plec_client::runtime::normalize_json_value(&JsValue::from_f64(f64::NAN), 0).unwrap();
    assert!(non_finite.is_null());
    let negative_zero =
        plec_client::runtime::normalize_json_value(&JsValue::from_f64(-0.0), 0).unwrap();
    assert_eq!(negative_zero.as_f64(), Some(0.0));
    assert!(!negative_zero.as_f64().unwrap().is_sign_negative());
}
