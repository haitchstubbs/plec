use super::lifecycle::decode_untrusted_js;
use super::PlecRuntime;
use plec_client::prelude::*;
use plec_dom::platform::now;
use serde::Deserialize;

#[derive(Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum SnapshotShape {
    Scalar,
    Object {
        #[serde(default)]
        observed_paths: Vec<Vec<String>>,
    },
    Collection {
        key_expression: String,
        #[serde(default = "default_order_sensitive")]
        order_sensitive: bool,
        #[serde(default)]
        observed_row_paths: Vec<Vec<String>>,
    },
}

fn default_order_sensitive() -> bool {
    true
}

fn decode_shape(value: &JsValue) -> Result<SnapshotShape, JsValue> {
    use plec_schema::js_decode::{boolean, string, ObjectDecoder};
    let mut object = ObjectDecoder::new(value)?;
    let kind = string(&object.get("kind")?)?;
    match kind.as_str() {
        "scalar" => Ok(SnapshotShape::Scalar),
        "object" => {
            let observed_paths = if object.has("observed_paths") {
                decode_paths(object.get("observed_paths")?)?
            } else {
                Vec::new()
            };
            Ok(SnapshotShape::Object { observed_paths })
        }
        "collection" => {
            let key_expression = string(&object.get("key_expression")?)?;
            let order_sensitive = if object.has("order_sensitive") {
                boolean(&object.get("order_sensitive")?)?
            } else {
                default_order_sensitive()
            };
            let observed_row_paths = if object.has("observed_row_paths") {
                decode_paths(object.get("observed_row_paths")?)?
            } else {
                Vec::new()
            };
            Ok(SnapshotShape::Collection {
                key_expression,
                order_sensitive,
                observed_row_paths,
            })
        }
        _ => Err(JsValue::from_str("unknown variant for enum SnapshotShape")),
    }
}

fn decode_paths(value: JsValue) -> Result<Vec<Vec<String>>, JsValue> {
    use plec_schema::js_decode::{array, string};
    array(&value)?
        .iter()
        .map(|path| array(&path)?.iter().map(string).collect())
        .collect()
}

/// Rejects pathological snapshot input shapes before the projection can use
/// them to drive allocation. The shape arrives from the host like any other
/// facade payload, so its observed-path tables get their own structural caps
/// beyond the decode byte envelope.
fn validate_shape(shape: &SnapshotShape) -> Result<(), JsValue> {
    use plec_schema::limits::{MAX_SNAPSHOT_SHAPE_PATHS, MAX_SNAPSHOT_SHAPE_PATH_SEGMENTS};
    let paths = match shape {
        SnapshotShape::Scalar => return Ok(()),
        SnapshotShape::Object { observed_paths } => observed_paths,
        SnapshotShape::Collection {
            key_expression,
            observed_row_paths,
            ..
        } => {
            if key_expression.is_empty() {
                return Err(JsValue::from_str("snapshot key expression is empty"));
            }
            observed_row_paths
        }
    };
    if paths.len() > MAX_SNAPSHOT_SHAPE_PATHS {
        return Err(JsValue::from_str("snapshot shape path count exceeds limit"));
    }
    for path in paths {
        if path.len() > MAX_SNAPSHOT_SHAPE_PATH_SEGMENTS {
            return Err(JsValue::from_str("snapshot shape path depth exceeds limit"));
        }
    }
    Ok(())
}

/// Applies the documented runtime-value budgets (depth, node count, string
/// bytes) to a decoded snapshot payload before it can be retained as a
/// projection or drive projection allocation. Iterative, so it cannot
/// overflow the stack on an already-decoded deep tree.
fn check_value_budget(value: &RuntimeValue) -> Result<(), JsValue> {
    use plec_schema::limits::{MAX_VALUE_DEPTH, MAX_VALUE_NODES, MAX_VALUE_STRING_BYTES};
    let mut stack: Vec<(&RuntimeValue, usize)> = vec![(value, 0)];
    let mut nodes = 0usize;
    while let Some((value, depth)) = stack.pop() {
        if depth > MAX_VALUE_DEPTH {
            return Err(JsValue::from_str("runtime value nesting exceeds limit"));
        }
        nodes += 1;
        if nodes > MAX_VALUE_NODES {
            return Err(JsValue::from_str("runtime value size exceeds limit"));
        }
        match value {
            RuntimeValue::String(string) => {
                if string.len() > MAX_VALUE_STRING_BYTES {
                    return Err(JsValue::from_str("runtime value string exceeds limit"));
                }
            }
            RuntimeValue::Array(values) => {
                stack.extend(values.iter().map(|value| (value, depth + 1)))
            }
            RuntimeValue::Record(map) => {
                nodes += map.len();
                if nodes > MAX_VALUE_NODES {
                    return Err(JsValue::from_str("runtime value size exceeds limit"));
                }
                stack.extend(map.values().map(|value| (value, depth + 1)));
            }
            RuntimeValue::Null | RuntimeValue::Bool(_) | RuntimeValue::Number(_) => {}
        }
    }
    Ok(())
}

pub(crate) enum SnapshotProjection {
    #[allow(dead_code)]
    Value(Vec<RuntimeValue>),
    Collection {
        keys: Vec<String>,
        rows: HashMap<String, HashMap<String, RuntimeValue>>,
    },
}

pub(crate) struct SnapshotInput {
    pub(crate) shape: SnapshotShape,
    pub(crate) projection: SnapshotProjection,
}

fn read_path(value: &RuntimeValue, path: &[String]) -> RuntimeValue {
    let mut current = value;
    for segment in path {
        let Some(next) = current.record().and_then(|object| object.get(segment)) else {
            return RuntimeValue::Null;
        };
        current = next;
    }
    current.clone()
}

fn js_key(value: RuntimeValue) -> String {
    match value {
        RuntimeValue::String(value) => value,
        RuntimeValue::Null => "null".into(),
        RuntimeValue::Bool(value) => value.to_string(),
        RuntimeValue::Number(value) => value.to_string(),
        RuntimeValue::Array(values) => values.into_iter().map(js_key).collect::<Vec<_>>().join(","),
        RuntimeValue::Record(_) => "[object Object]".into(),
    }
}

fn project(value: RuntimeValue, shape: &SnapshotShape) -> Result<SnapshotProjection, JsValue> {
    match shape {
        SnapshotShape::Scalar => Ok(SnapshotProjection::Value(vec![value])),
        SnapshotShape::Object { observed_paths } => {
            Ok(SnapshotProjection::Value(if observed_paths.is_empty() {
                vec![value]
            } else {
                observed_paths
                    .iter()
                    .map(|path| read_path(&value, path))
                    .collect()
            }))
        }
        SnapshotShape::Collection {
            key_expression,
            observed_row_paths,
            ..
        } => {
            let rows = value
                .array()
                .ok_or_else(|| JsValue::from_str("snapshot collection must be an array"))?;
            let key_path = key_expression
                .split('.')
                .skip(1)
                .map(str::to_owned)
                .collect::<Vec<_>>();
            let mut keys = Vec::with_capacity(rows.len());
            let mut projected = HashMap::with_capacity(rows.len());
            for row in rows {
                let key = js_key(read_path(row, &key_path));
                let source = row.record().ok_or_else(|| {
                    JsValue::from_str("snapshot collection row must be an object")
                })?;
                let fields = if observed_row_paths.is_empty() {
                    source
                        .iter()
                        .map(|(key, value)| (key.clone(), value.clone()))
                        .collect()
                } else {
                    observed_row_paths
                        .iter()
                        .filter_map(|path| {
                            path.first()
                                .map(|field| (field.clone(), read_path(row, path)))
                        })
                        .collect()
                };
                keys.push(key.clone());
                projected.insert(key, fields);
            }
            Ok(SnapshotProjection::Collection {
                keys,
                rows: projected,
            })
        }
    }
}

fn diff(
    previous: &HashMap<String, RuntimeValue>,
    next: &HashMap<String, RuntimeValue>,
) -> HashMap<String, RuntimeValue> {
    let mut changes = HashMap::new();
    for (key, value) in next {
        if previous.get(key) != Some(value) {
            changes.insert(key.clone(), value.clone());
        }
    }
    // JSON has no `undefined`; a removed observed field becomes null at the
    // WASM boundary rather than being silently omitted.
    for key in previous.keys() {
        if !next.contains_key(key) {
            changes.insert(key.clone(), RuntimeValue::Null);
        }
    }
    changes
}

fn reconcile(
    input_id: &str,
    previous: &SnapshotProjection,
    next: &SnapshotProjection,
    shape: &SnapshotShape,
) -> Vec<plec_schema::delta::RuntimeDelta> {
    let (
        SnapshotProjection::Collection {
            keys: previous_keys,
            rows: previous_rows,
        },
        SnapshotProjection::Collection {
            keys: next_keys,
            rows: next_rows,
        },
        SnapshotShape::Collection {
            order_sensitive, ..
        },
    ) = (previous, next, shape)
    else {
        return Vec::new();
    };
    let previous_set = previous_keys
        .iter()
        .collect::<std::collections::HashSet<_>>();
    let next_set = next_keys.iter().collect::<std::collections::HashSet<_>>();
    let mut deltas = Vec::new();
    for key in previous_keys {
        if !next_set.contains(key) {
            deltas.push(plec_schema::delta::RuntimeDelta::Remove {
                instance_id: None,
                input_id: input_id.into(),
                row_key: key.clone(),
            });
        }
    }
    for (index, key) in next_keys.iter().enumerate() {
        let before_row_key = next_keys.get(index + 1).cloned();
        let row = next_rows.get(key).expect("projected key exists");
        if !previous_set.contains(key) {
            deltas.push(plec_schema::delta::RuntimeDelta::Insert {
                instance_id: None,
                input_id: input_id.into(),
                row_key: key.clone(),
                row: row.clone(),
                before_row_key,
            });
            continue;
        }
        let changes = diff(
            previous_rows
                .get(key)
                .expect("previous projected key exists"),
            row,
        );
        if !changes.is_empty() {
            deltas.push(plec_schema::delta::RuntimeDelta::Update {
                instance_id: None,
                input_id: input_id.into(),
                row_key: key.clone(),
                changes,
            });
        }
        if *order_sensitive && previous_keys.get(index) != Some(key) {
            deltas.push(plec_schema::delta::RuntimeDelta::Move {
                instance_id: None,
                input_id: input_id.into(),
                row_key: key.clone(),
                before_row_key,
            });
        }
    }
    plec_schema::delta::coalesce_runtime_deltas(deltas)
}

#[wasm_bindgen::prelude::wasm_bindgen]
impl PlecRuntime {
    pub fn initialize_snapshot_input(
        &self,
        input_id: String,
        value: JsValue,
        shape: JsValue,
    ) -> Result<JsValue, JsValue> {
        // Both payloads pass through bounded JS normalization and a stringify
        // byte check, then decode directly into typed runtime values. Values
        // pass runtime budgets before any state is touched, so hostile input
        // fails closed without mutating snapshots or reconciling rows.
        let value_json = decode_untrusted_js(
            &value,
            plec_schema::limits::MAX_HOST_INPUT_JSON_BYTES,
            "snapshot value",
        )?;
        let decoded_value = plec_schema::js_decode::runtime_value(&value_json)?;
        check_value_budget(&decoded_value)?;
        let shape_json = decode_untrusted_js(
            &shape,
            plec_schema::limits::MAX_HOST_INPUT_JSON_BYTES,
            "snapshot shape",
        )?;
        let shape = decode_shape(&shape_json)?;
        validate_shape(&shape)?;
        let projection = project(decoded_value, &shape)?;
        let metrics = if matches!(shape, SnapshotShape::Collection { .. }) {
            self.initialize_input(input_id.clone(), value)?
        } else {
            serde_wasm_bindgen::to_value(&UpdateMetrics::default()).map_err(error)?
        };
        self.snapshot_inputs
            .borrow_mut()
            .insert(input_id, SnapshotInput { shape, projection });
        Ok(metrics)
    }

    pub fn apply_input_snapshot(
        &self,
        input_id: String,
        value: JsValue,
    ) -> Result<JsValue, JsValue> {
        let started = now();
        let value = decode_untrusted_js(
            &value,
            plec_schema::limits::MAX_HOST_INPUT_JSON_BYTES,
            "snapshot value",
        )?;
        let value = plec_schema::js_decode::runtime_value(&value)?;
        check_value_budget(&value)?;
        let deltas = {
            let mut snapshots = self.snapshot_inputs.borrow_mut();
            let snapshot = snapshots
                .get_mut(&input_id)
                .ok_or_else(|| JsValue::from_str("snapshot input is not initialized"))?;
            let next = project(value, &snapshot.shape)?;
            let deltas = reconcile(&input_id, &snapshot.projection, &next, &snapshot.shape);
            snapshot.projection = next;
            deltas
        };
        let mut metrics = if deltas.is_empty() {
            UpdateMetrics::default()
        } else {
            self.state.apply_typed_deltas_with_metrics(deltas)?
        };
        metrics.reconciliation_us = (now() - started) * 1000.0;
        serde_wasm_bindgen::to_value(&metrics).map_err(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collection_shape() -> SnapshotShape {
        SnapshotShape::Collection {
            key_expression: "todo.id".into(),
            order_sensitive: true,
            observed_row_paths: vec![vec!["title".into()], vec!["done".into()]],
        }
    }

    #[test]
    fn reconciles_observed_fields_and_row_order() {
        let shape = collection_shape();
        let previous = project(
            runtime_from_json(serde_json::json!([
                {"id":"a","title":"A","done":false},
                {"id":"b","title":"B","done":false}
            ]))
            .unwrap(),
            &shape,
        )
        .unwrap();
        let next = project(
            runtime_from_json(serde_json::json!([
                {"id":"b","title":"B","done":false},
                {"id":"a","title":"A","done":true},
                {"id":"c","title":"C","done":false}
            ]))
            .unwrap(),
            &shape,
        )
        .unwrap();
        let deltas = reconcile("todos", &previous, &next, &shape);
        assert!(deltas.iter().any(|delta| matches!(delta,
            plec_schema::delta::RuntimeDelta::Update { row_key, .. } if row_key == "a")));
        assert!(deltas.iter().any(|delta| matches!(delta,
            plec_schema::delta::RuntimeDelta::Move { row_key, .. } if row_key == "b")));
        assert!(deltas.iter().any(|delta| matches!(delta,
            plec_schema::delta::RuntimeDelta::Insert { row_key, .. } if row_key == "c")));
    }

    #[test]
    fn non_collection_snapshots_do_not_emit_row_deltas() {
        let shape = SnapshotShape::Object {
            observed_paths: vec![vec!["name".into()]],
        };
        let previous = project(
            runtime_from_json(serde_json::json!({"name":"before"})).unwrap(),
            &shape,
        )
        .unwrap();
        let next = project(
            runtime_from_json(serde_json::json!({"name":"after"})).unwrap(),
            &shape,
        )
        .unwrap();
        assert!(reconcile("value", &previous, &next, &shape).is_empty());
    }

    // validate_shape/check_value_budget fail with JsValue errors, which only
    // exist on wasm32; these assertions run in the browser wasm suite.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test::wasm_bindgen_test]
    fn shape_validation_rejects_path_table_excess() {
        use plec_schema::limits::{MAX_SNAPSHOT_SHAPE_PATHS, MAX_SNAPSHOT_SHAPE_PATH_SEGMENTS};
        let valid = SnapshotShape::Object {
            observed_paths: vec![vec!["a".into(), "b".into()]],
        };
        assert!(validate_shape(&valid).is_ok());
        assert!(validate_shape(&SnapshotShape::Scalar).is_ok());
        let too_many = SnapshotShape::Object {
            observed_paths: (0..=MAX_SNAPSHOT_SHAPE_PATHS)
                .map(|index| vec![index.to_string()])
                .collect(),
        };
        assert!(validate_shape(&too_many).is_err());
        let too_deep = SnapshotShape::Object {
            observed_paths: vec![vec!["segment".into(); MAX_SNAPSHOT_SHAPE_PATH_SEGMENTS + 1]],
        };
        assert!(validate_shape(&too_deep).is_err());
        let empty_key = SnapshotShape::Collection {
            key_expression: String::new(),
            order_sensitive: true,
            observed_row_paths: Vec::new(),
        };
        assert!(validate_shape(&empty_key).is_err());
    }

    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test::wasm_bindgen_test]
    fn value_budget_rejects_deep_oversized_and_long_string_trees() {
        use plec_schema::limits::{MAX_VALUE_DEPTH, MAX_VALUE_NODES, MAX_VALUE_STRING_BYTES};
        let within = runtime_from_json(serde_json::json!({"a": [1, "two", null, true]})).unwrap();
        assert!(check_value_budget(&within).is_ok());
        let mut current = serde_json::json!(0);
        for _ in 0..=MAX_VALUE_DEPTH {
            current = serde_json::json!([current]);
        }
        assert!(check_value_budget(&runtime_from_json(current).unwrap()).is_err());
        let oversized = serde_json::json!((0..=MAX_VALUE_NODES)
            .map(|i| i as u32)
            .collect::<Vec<u32>>());
        assert!(check_value_budget(&runtime_from_json(oversized).unwrap()).is_err());
        let long_string = "x".repeat(MAX_VALUE_STRING_BYTES + 1);
        assert!(
            check_value_budget(&runtime_from_json(serde_json::json!(long_string)).unwrap())
                .is_err()
        );
    }
}
