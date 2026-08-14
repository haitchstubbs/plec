use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Delta {
    Update {
        input_id: String,
        row_key: String,
        changes: HashMap<String, Value>,
    },
    Insert {
        input_id: String,
        row_key: String,
        row: HashMap<String, Value>,
        before_row_key: Option<String>,
    },
    Remove {
        input_id: String,
        row_key: String,
    },
    Move {
        input_id: String,
        row_key: String,
        before_row_key: Option<String>,
    },
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMetrics {
    pub dom_operations: u32,
    pub nodes_touched: u32,
    pub bindings_touched: u32,
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

    pub fn json_body(&self) -> Result<String, wasm_bindgen::JsValue> {
        serde_json::to_string(self)
            .map_err(|error| wasm_bindgen::JsValue::from_str(&error.to_string()))
    }
}

pub fn runtime_from_json(value: Value) -> Result<RuntimeValue, wasm_bindgen::JsValue> {
    serde_json::from_value(value)
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error.to_string()))
}
