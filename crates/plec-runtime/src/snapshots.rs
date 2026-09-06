use super::lifecycle::decode_untrusted_json;
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
            return Err(JsValue::from_str(
                "snapshot shape path depth exceeds limit",
            ));
        }
    }
    Ok(())
}

/// Applies the documented runtime-value budgets (depth, node count, string
/// bytes) to a decoded snapshot payload before it can be retained as a
/// projection or drive projection allocation. Iterative, so it cannot
/// overflow the stack on an already-decoded deep tree.
fn check_value_budget(value: &Value) -> Result<(), JsValue> {
    use plec_schema::limits::{MAX_VALUE_DEPTH, MAX_VALUE_NODES, MAX_VALUE_STRING_BYTES};
    let mut stack: Vec<(&Value, usize)> = vec![(value, 0)];
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
            Value::String(string) => {
                if string.len() > MAX_VALUE_STRING_BYTES {
                    return Err(JsValue::from_str("runtime value string exceeds limit"));
                }
            }
            Value::Array(values) => stack.extend(values.iter().map(|value| (value, depth + 1))),
            Value::Object(map) => {
                nodes += map.len();
                if nodes > MAX_VALUE_NODES {
                    return Err(JsValue::from_str("runtime value size exceeds limit"));
                }
                stack.extend(map.values().map(|value| (value, depth + 1)));
            }
            _ => {}
        }
    }
    Ok(())
}

pub(crate) enum SnapshotProjection {
    #[allow(dead_code)]
    Value(Vec<Value>),
    Collection {
        keys: Vec<String>,
        rows: HashMap<String, HashMap<String, Value>>,
    },
}

pub(crate) struct SnapshotInput {
    pub(crate) shape: SnapshotShape,
    pub(crate) projection: SnapshotProjection,
}

fn read_path(value: &Value, path: &[String]) -> Value {
    let mut current = value;
    for segment in path {
        let Some(next) = current.as_object().and_then(|object| object.get(segment)) else {
            return Value::Null;
        };
        current = next;
    }
    current.clone()
}

fn js_key(value: Value) -> String {
    match value {
        Value::String(value) => value,
        Value::Null => "null".into(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Array(values) => values.into_iter().map(js_key).collect::<Vec<_>>().join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

fn project(value: Value, shape: &SnapshotShape) -> Result<SnapshotProjection, JsValue> {
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
                .as_array()
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
                let source = row.as_object().ok_or_else(|| {
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
    previous: &HashMap<String, Value>,
    next: &HashMap<String, Value>,
) -> HashMap<String, Value> {
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
            changes.insert(key.clone(), Value::Null);
        }
    }
    changes
}

fn reconcile(
    input_id: &str,
    previous: &SnapshotProjection,
    next: &SnapshotProjection,
    shape: &SnapshotShape,
) -> Vec<Delta> {
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
            deltas.push(Delta::Remove {
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
            deltas.push(Delta::Insert {
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
            deltas.push(Delta::Update {
                instance_id: None,
                input_id: input_id.into(),
                row_key: key.clone(),
                changes,
            });
        }
        if *order_sensitive && previous_keys.get(index) != Some(key) {
            deltas.push(Delta::Move {
                instance_id: None,
                input_id: input_id.into(),
                row_key: key.clone(),
                before_row_key,
            });
        }
    }
    coalesce_deltas(deltas)
}

#[wasm_bindgen::prelude::wasm_bindgen]
impl PlecRuntime {
    pub fn initialize_snapshot_input(
        &self,
        input_id: String,
        value: JsValue,
        shape: JsValue,
    ) -> Result<JsValue, JsValue> {
        // Both payloads decode through the shared bounded path (JS-value
        // normalization + byte ceiling + depth-guarded JSON parse) and the
        // decoded tree passes the runtime-value budgets before any state is
        // touched, so a hostile payload fails closed without mutating the
        // snapshot table or reconciling rows.
        let value_json = decode_untrusted_json(
            &value,
            plec_schema::limits::MAX_HOST_INPUT_JSON_BYTES,
            "snapshot value",
        )?;
        check_value_budget(&value_json)?;
        let shape_json = decode_untrusted_json(
            &shape,
            plec_schema::limits::MAX_HOST_INPUT_JSON_BYTES,
            "snapshot shape",
        )?;
        let shape: SnapshotShape = serde_json::from_value(shape_json).map_err(error)?;
        validate_shape(&shape)?;
        let projection = project(value_json, &shape)?;
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
        let value: Value = decode_untrusted_json(
            &value,
            plec_schema::limits::MAX_HOST_INPUT_JSON_BYTES,
            "snapshot value",
        )?;
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
            let value = serde_wasm_bindgen::to_value(&deltas).map_err(error)?;
            serde_wasm_bindgen::from_value(self.apply_deltas(value)?).map_err(error)?
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
            serde_json::json!([
                {"id":"a","title":"A","done":false},
                {"id":"b","title":"B","done":false}
            ]),
            &shape,
        )
        .unwrap();
        let next = project(
            serde_json::json!([
                {"id":"b","title":"B","done":false},
                {"id":"a","title":"A","done":true},
                {"id":"c","title":"C","done":false}
            ]),
            &shape,
        )
        .unwrap();
        let deltas = reconcile("todos", &previous, &next, &shape);
        let json = serde_json::to_value(deltas).unwrap();
        assert!(json
            .as_array()
            .unwrap()
            .iter()
            .any(|delta| delta["type"] == "update" && delta["rowKey"] == "a"));
        assert!(json
            .as_array()
            .unwrap()
            .iter()
            .any(|delta| delta["type"] == "move" && delta["rowKey"] == "b"));
        assert!(json
            .as_array()
            .unwrap()
            .iter()
            .any(|delta| delta["type"] == "insert" && delta["rowKey"] == "c"));
    }

    #[test]
    fn non_collection_snapshots_do_not_emit_row_deltas() {
        let shape = SnapshotShape::Object {
            observed_paths: vec![vec!["name".into()]],
        };
        let previous = project(serde_json::json!({"name":"before"}), &shape).unwrap();
        let next = project(serde_json::json!({"name":"after"}), &shape).unwrap();
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
        let within = serde_json::json!({"a": [1, "two", null, true]});
        assert!(check_value_budget(&within).is_ok());
        let mut current = serde_json::json!(0);
        for _ in 0..=MAX_VALUE_DEPTH {
            current = serde_json::json!([current]);
        }
        assert!(check_value_budget(&current).is_err());
        let oversized = serde_json::json!((0..=MAX_VALUE_NODES).map(|i| i as u32).collect::<Vec<u32>>());
        assert!(check_value_budget(&oversized).is_err());
        let long_string = "x".repeat(MAX_VALUE_STRING_BYTES + 1);
        assert!(check_value_budget(&serde_json::json!(long_string)).is_err());
    }
}
