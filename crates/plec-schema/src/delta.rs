use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use plec_ir::SsrSnapshotValue;

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Delta {
    Update {
        #[serde(default)]
        instance_id: Option<String>,
        #[serde(rename = "inputId", alias = "input_id")]
        input_id: String,
        #[serde(rename = "rowKey", alias = "row_key")]
        row_key: String,
        changes: HashMap<String, Value>,
    },
    Insert {
        #[serde(default)]
        instance_id: Option<String>,
        #[serde(rename = "inputId", alias = "input_id")]
        input_id: String,
        #[serde(rename = "rowKey", alias = "row_key")]
        row_key: String,
        row: HashMap<String, Value>,
        #[serde(rename = "beforeRowKey", alias = "before_row_key")]
        before_row_key: Option<String>,
    },
    Remove {
        #[serde(default)]
        instance_id: Option<String>,
        #[serde(rename = "inputId", alias = "input_id")]
        input_id: String,
        #[serde(rename = "rowKey", alias = "row_key")]
        row_key: String,
    },
    Move {
        #[serde(default)]
        instance_id: Option<String>,
        #[serde(rename = "inputId", alias = "input_id")]
        input_id: String,
        #[serde(rename = "rowKey", alias = "row_key")]
        row_key: String,
        #[serde(rename = "beforeRowKey", alias = "before_row_key")]
        before_row_key: Option<String>,
    },
}

/// Keep the browser delta protocol compact without changing its ordering
/// semantics: only adjacent field updates for the same row are merged.
pub fn coalesce_deltas(deltas: Vec<Delta>) -> Vec<Delta> {
    let mut result: Vec<Delta> = Vec::with_capacity(deltas.len());
    for delta in deltas {
        if let (
            Delta::Update {
                input_id,
                row_key,
                changes,
                ..
            },
            Some(Delta::Update {
                input_id: previous_input,
                row_key: previous_key,
                changes: previous_changes,
                ..
            }),
        ) = (&delta, result.last_mut())
        {
            if input_id == previous_input && row_key == previous_key {
                previous_changes.extend(changes.clone());
                continue;
            }
        }
        result.push(delta);
    }
    result
}

impl Delta {
    pub fn input_id(&self) -> &str {
        match self {
            Self::Update { input_id, .. }
            | Self::Insert { input_id, .. }
            | Self::Remove { input_id, .. }
            | Self::Move { input_id, .. } => input_id,
        }
    }

    pub fn instance_id(&self) -> Option<&str> {
        match self {
            Self::Update { instance_id, .. }
            | Self::Insert { instance_id, .. }
            | Self::Remove { instance_id, .. }
            | Self::Move { instance_id, .. } => instance_id.as_deref(),
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMetrics {
    #[serde(default)]
    pub reconciliation_us: f64,
    pub dom_operations: u32,
    pub nodes_touched: u32,
    pub bindings_touched: u32,
    pub prop_writes: u32,
    pub row_inserts: u32,
    pub row_removes: u32,
    pub row_moves: u32,
    /// Physical relocation cost: how many DOM nodes a row move displaced,
    /// distinct from the logical `row_moves` count and from `dom_operations`
    /// (one mutation call per staged node plus one range splice).
    #[serde(default)]
    pub dom_nodes_moved: u32,
    pub wasm_dom_us: f64,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MountMetrics {
    pub decode_us: f64,
    pub static_mount_us: f64,
    pub row_program_execute_us: f64,
    pub row_state_registration_us: f64,
    pub fragment_append_us: f64,
    pub program_compile_us: f64,
    pub program_revision: Option<String>,
    pub instruction_count: u32,
    pub compiled_expression_count: u32,
    pub field_slot_count: u32,
    pub binding_program_count: u32,
    pub average_row_program_execute_us: f64,
    pub row_count: u32,
    pub created_elements: u32,
    pub created_texts: u32,
    pub bindings: u32,
    pub dom_operations: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RuntimeValue {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<RuntimeValue>),
    Record(HashMap<String, RuntimeValue>),
}

impl Default for RuntimeValue {
    fn default() -> Self {
        Self::Null
    }
}

impl RuntimeValue {
    pub fn truthy(&self) -> bool {
        match self {
            Self::Null => false,
            Self::Bool(value) => *value,
            Self::Number(value) => *value != 0.0,
            Self::String(value) => !value.is_empty(),
            Self::Array(value) => !value.is_empty(),
            Self::Record(_) => true,
        }
    }

    /// The canonical text form used for DOM bindings, keys, and string
    /// expression operators.
    pub fn dom_string(&self) -> String {
        match self {
            Self::String(value) => value.clone(),
            Self::Null => String::new(),
            Self::Bool(value) => value.to_string(),
            Self::Number(value) => value.to_string(),
            Self::Array(_) | Self::Record(_) => serde_json::to_string(self).unwrap_or_default(),
        }
    }

    /// Estimated live byte size used by bounded evaluator stacks. The walk is
    /// iterative because validated values may still be deeply nested.
    pub fn estimated_size_bytes(&self) -> usize {
        let mut bytes = 0usize;
        let mut pending = vec![self];
        while let Some(value) = pending.pop() {
            match value {
                Self::Null | Self::Bool(_) | Self::Number(_) => bytes += 8,
                Self::String(value) => bytes += value.len(),
                Self::Array(values) => {
                    bytes += 16 + 8 * values.len();
                    pending.extend(values.iter());
                }
                Self::Record(values) => {
                    bytes += 16 + 8 * values.len();
                    pending.extend(values.values());
                }
            }
        }
        bytes
    }

    /// Converts a transport JSON value without applying an input-size limit.
    /// Callers accepting untrusted input must use `runtime_from_json` instead.
    pub fn from_json_value(value: Value) -> Self {
        match value {
            Value::Null => Self::Null,
            Value::Bool(value) => Self::Bool(value),
            Value::Number(value) => Self::Number(value.as_f64().unwrap_or(0.0)),
            Value::String(value) => Self::String(value),
            Value::Array(values) => {
                Self::Array(values.into_iter().map(Self::from_json_value).collect())
            }
            Value::Object(values) => Self::Record(
                values
                    .into_iter()
                    .map(|(name, value)| (name, Self::from_json_value(value)))
                    .collect(),
            ),
        }
    }

    /// Converts to JSON using the SSR loader contract: non-finite numbers
    /// become null because JSON cannot represent them.
    pub fn into_json_value(self) -> Value {
        match self {
            Self::Null => Value::Null,
            Self::Bool(value) => Value::Bool(value),
            Self::Number(value) => serde_json::Number::from_f64(value)
                .map(Value::Number)
                .unwrap_or(Value::Null),
            Self::String(value) => Value::String(value),
            Self::Array(values) => {
                Value::Array(values.into_iter().map(Self::into_json_value).collect())
            }
            Self::Record(values) => Value::Object(
                values
                    .into_iter()
                    .map(|(name, value)| (name, value.into_json_value()))
                    .collect(),
            ),
        }
    }

    pub fn from_ssr_snapshot(value: &SsrSnapshotValue) -> Self {
        match value {
            SsrSnapshotValue::Null => Self::Null,
            SsrSnapshotValue::Bool(value) => Self::Bool(*value),
            SsrSnapshotValue::Number(value) => Self::Number(*value),
            SsrSnapshotValue::String(value) => Self::String(value.clone()),
            SsrSnapshotValue::Array(values) => {
                Self::Array(values.iter().map(Self::from_ssr_snapshot).collect())
            }
            SsrSnapshotValue::Record(values) => Self::Record(
                values
                    .iter()
                    .map(|(name, value)| (name.clone(), Self::from_ssr_snapshot(value)))
                    .collect(),
            ),
        }
    }

    pub fn into_ssr_snapshot(self) -> SsrSnapshotValue {
        match self {
            Self::Null => SsrSnapshotValue::Null,
            Self::Bool(value) => SsrSnapshotValue::Bool(value),
            Self::Number(value) => SsrSnapshotValue::Number(value),
            Self::String(value) => SsrSnapshotValue::String(value),
            Self::Array(values) => {
                SsrSnapshotValue::Array(values.into_iter().map(Self::into_ssr_snapshot).collect())
            }
            Self::Record(values) => SsrSnapshotValue::Record(
                values
                    .into_iter()
                    .map(|(name, value)| (name, value.into_ssr_snapshot()))
                    .collect(),
            ),
        }
    }
    /// Bounds a decoded untrusted value tree before it can drive further
    /// allocation. Fails closed on nesting depth, total tree size, and
    /// oversized strings (see `limits`).
    pub fn check_limits(&self) -> Result<(), &'static str> {
        use crate::limits::{MAX_VALUE_DEPTH, MAX_VALUE_NODES, MAX_VALUE_STRING_BYTES};
        let mut stack: Vec<(&RuntimeValue, usize)> = vec![(self, 0)];
        let mut nodes = 0usize;
        while let Some((value, depth)) = stack.pop() {
            if depth > MAX_VALUE_DEPTH {
                return Err("runtime value nesting exceeds limit");
            }
            nodes += 1;
            if nodes > MAX_VALUE_NODES {
                return Err("runtime value size exceeds limit");
            }
            match value {
                RuntimeValue::String(value) => {
                    if value.len() > MAX_VALUE_STRING_BYTES {
                        return Err("runtime value string exceeds limit");
                    }
                }
                RuntimeValue::Array(values) => {
                    stack.extend(values.iter().map(|value| (value, depth + 1)));
                }
                RuntimeValue::Record(values) => {
                    nodes += values.len();
                    if nodes > MAX_VALUE_NODES {
                        return Err("runtime value size exceeds limit");
                    }
                    stack.extend(values.values().map(|value| (value, depth + 1)));
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub fn record(&self) -> Option<&HashMap<String, RuntimeValue>> {
        if let Self::Record(value) = self {
            Some(value)
        } else {
            None
        }
    }

    pub fn array(&self) -> Option<&[RuntimeValue]> {
        if let Self::Array(value) = self {
            Some(value)
        } else {
            None
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    pub fn number(&self) -> f64 {
        if let Self::Number(value) = self {
            *value
        } else {
            0.0
        }
    }

    pub fn json_body(&self) -> Result<String, String> {
        fn write(value: &RuntimeValue, output: &mut String) -> Result<(), String> {
            match value {
                RuntimeValue::Null => output.push_str("null"),
                RuntimeValue::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
                RuntimeValue::Number(value) if value.is_finite() => {
                    output.push_str(&value.to_string())
                }
                RuntimeValue::Number(_) => {
                    return Err("JSON cannot encode a non-finite number".into())
                }
                RuntimeValue::String(value) => {
                    output.push('"');
                    for character in value.chars() {
                        match character {
                            '"' => output.push_str("\\\""),
                            '\\' => output.push_str("\\\\"),
                            '\n' => output.push_str("\\n"),
                            '\r' => output.push_str("\\r"),
                            '\t' => output.push_str("\\t"),
                            character if character <= '\u{1f}' => {
                                output.push_str(&format!("\\u{:04x}", character as u32))
                            }
                            character => output.push(character),
                        }
                    }
                    output.push('"');
                }
                RuntimeValue::Array(values) => {
                    output.push('[');
                    for (index, value) in values.iter().enumerate() {
                        if index > 0 {
                            output.push(',');
                        }
                        write(value, output)?;
                    }
                    output.push(']');
                }
                RuntimeValue::Record(values) => {
                    output.push('{');
                    for (index, (key, value)) in values.iter().enumerate() {
                        if index > 0 {
                            output.push(',');
                        }
                        write(&RuntimeValue::String(key.clone()), output)?;
                        output.push(':');
                        write(value, output)?;
                    }
                    output.push('}');
                }
            }
            Ok(())
        }
        let mut output = String::new();
        write(self, &mut output)?;
        Ok(output)
    }
}

pub fn runtime_from_json(value: Value) -> Result<RuntimeValue, String> {
    let runtime: RuntimeValue = serde_json::from_value(value).map_err(|error| error.to_string())?;
    runtime.check_limits().map_err(str::to_owned)?;
    Ok(runtime)
}

/// JSON transport variants share RuntimeValue's coercion and stack-accounting
/// rules without requiring the SSR evaluator to allocate a second value tree.
pub fn json_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64().map(|value| value != 0.0).unwrap_or(false),
        Value::String(value) => !value.is_empty(),
        Value::Array(values) => !values.is_empty(),
        Value::Object(_) => true,
    }
}

pub fn json_dom_string(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Null => String::new(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value
            .as_f64()
            .map(|value| value.to_string())
            .unwrap_or_default(),
        Value::Array(_) | Value::Object(_) => serde_json::to_string(value).unwrap_or_default(),
    }
}

pub fn json_estimated_size_bytes(value: &Value) -> usize {
    let mut bytes = 0usize;
    let mut pending = vec![value];
    while let Some(value) = pending.pop() {
        match value {
            Value::Null | Value::Bool(_) | Value::Number(_) => bytes += 8,
            Value::String(value) => bytes += value.len(),
            Value::Array(values) => {
                bytes += 16 + 8 * values.len();
                pending.extend(values.iter());
            }
            Value::Object(values) => {
                bytes += 16 + 8 * values.len();
                pending.extend(values.values());
            }
        }
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_browser_camel_case_deltas_and_coalesces_adjacent_updates() {
        let deltas: Vec<Delta> = serde_json::from_value(serde_json::json!([
            {"type":"update","inputId":"todos","rowKey":"a","changes":{"title":"A"}},
            {"type":"update","inputId":"todos","rowKey":"a","changes":{"done":true}},
            {"type":"remove","inputId":"todos","rowKey":"b"}
        ]))
        .unwrap();
        let coalesced = coalesce_deltas(deltas);
        assert_eq!(coalesced.len(), 2);
        let value = serde_json::to_value(&coalesced[0]).unwrap();
        assert_eq!(value["inputId"], "todos");
        assert_eq!(value["rowKey"], "a");
        assert_eq!(value["changes"]["done"], true);
    }

    #[test]
    fn runtime_value_coercions_match_existing_contract() {
        let cases = [
            (RuntimeValue::Null, false, ""),
            (RuntimeValue::Bool(false), false, "false"),
            (RuntimeValue::Bool(true), true, "true"),
            (RuntimeValue::Number(0.0), false, "0"),
            (RuntimeValue::Number(2.5), true, "2.5"),
            (RuntimeValue::String(String::new()), false, ""),
            (RuntimeValue::Array(Vec::new()), false, "[]"),
            (RuntimeValue::Record(HashMap::new()), true, "{}"),
        ];

        for (value, truthy, string) in cases {
            assert_eq!(value.truthy(), truthy);
            assert_eq!(value.dom_string(), string);
        }
    }

    #[test]
    fn size_accounting_is_iterative_and_matches_json_transport() {
        let value = RuntimeValue::Array(vec![
            RuntimeValue::String("hi".into()),
            RuntimeValue::Array(vec![RuntimeValue::Bool(true)]),
        ]);
        let json = serde_json::json!(["hi", [true]]);

        assert_eq!(value.estimated_size_bytes(), 66);
        assert_eq!(json_estimated_size_bytes(&json), 66);
    }

    #[test]
    fn json_and_snapshot_conversions_preserve_their_contracts() {
        let json = serde_json::json!({"items":[true, 2], "name":"Plec"});
        let runtime = RuntimeValue::from_json_value(json.clone());
        assert_eq!(
            runtime.clone().into_json_value(),
            serde_json::json!({"items":[true, 2.0], "name":"Plec"})
        );
        assert_eq!(
            RuntimeValue::Number(f64::NAN).into_json_value(),
            Value::Null
        );

        let snapshot = runtime.clone().into_ssr_snapshot();
        assert_eq!(RuntimeValue::from_ssr_snapshot(&snapshot), runtime);
        assert_eq!(
            RuntimeValue::from_ssr_snapshot(&SsrSnapshotValue::Number(f64::INFINITY))
                .into_json_value(),
            Value::Null
        );
    }

    #[test]
    fn json_transport_coercions_preserve_json_number_rendering() {
        let json = serde_json::json!({"value":[1]});
        let runtime = RuntimeValue::from_json_value(json.clone());

        assert_eq!(json_truthy(&json), runtime.truthy());
        assert_eq!(json_dom_string(&json), r#"{"value":[1]}"#);
        assert_eq!(runtime.dom_string(), r#"{"value":[1.0]}"#);
    }
}
