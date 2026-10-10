//! Narrow browser-facing encoders for values and runtime metrics.

use crate::delta::{MountMetrics, RuntimeValue, UpdateMetrics};
use js_sys::{Array, Map, Object, Reflect};
use wasm_bindgen::JsValue;

/// Encodes a RuntimeValue using the same map convention as
/// `serde_wasm_bindgen::to_value`: records become `Map` by default, while
/// callers that explicitly request JSON-compatible values get plain objects.
pub fn runtime_value(value: &RuntimeValue, records_as_objects: bool) -> Result<JsValue, JsValue> {
    Ok(match value {
        RuntimeValue::Null => JsValue::NULL,
        RuntimeValue::Bool(value) => JsValue::from_bool(*value),
        RuntimeValue::Number(value) => JsValue::from_f64(*value),
        RuntimeValue::String(value) => JsValue::from_str(value),
        RuntimeValue::Array(values) => {
            let output = Array::new();
            for value in values {
                output.push(&runtime_value(value, records_as_objects)?);
            }
            output.into()
        }
        RuntimeValue::Record(values) if records_as_objects => {
            let output = Object::new();
            for (key, value) in values {
                Reflect::set(
                    &output,
                    &JsValue::from_str(key),
                    &runtime_value(value, records_as_objects)?,
                )?;
            }
            output.into()
        }
        RuntimeValue::Record(values) => {
            let output = Map::new();
            for (key, value) in values {
                output.set(
                    &JsValue::from_str(key),
                    &runtime_value(value, records_as_objects)?,
                );
            }
            output.into()
        }
    })
}

pub fn string_array(values: &[String]) -> JsValue {
    let output = Array::new();
    for value in values {
        output.push(&JsValue::from_str(value));
    }
    output.into()
}

pub fn update_metrics(value: &UpdateMetrics) -> Result<JsValue, JsValue> {
    let object = Object::new();
    set_number(&object, "reconciliationUs", value.reconciliation_us)?;
    set_number(&object, "domOperations", value.dom_operations as f64)?;
    set_number(&object, "nodesTouched", value.nodes_touched as f64)?;
    set_number(&object, "bindingsTouched", value.bindings_touched as f64)?;
    set_number(&object, "propWrites", value.prop_writes as f64)?;
    set_number(&object, "rowInserts", value.row_inserts as f64)?;
    set_number(&object, "rowRemoves", value.row_removes as f64)?;
    set_number(&object, "rowMoves", value.row_moves as f64)?;
    set_number(&object, "domNodesMoved", value.dom_nodes_moved as f64)?;
    set_number(&object, "wasmDomUs", value.wasm_dom_us)?;
    Ok(object.into())
}

pub fn mount_metrics(value: &MountMetrics) -> Result<JsValue, JsValue> {
    let object = Object::new();
    set_number(&object, "decodeUs", value.decode_us)?;
    set_number(&object, "staticMountUs", value.static_mount_us)?;
    set_number(&object, "rowProgramExecuteUs", value.row_program_execute_us)?;
    set_number(
        &object,
        "rowStateRegistrationUs",
        value.row_state_registration_us,
    )?;
    set_number(&object, "fragmentAppendUs", value.fragment_append_us)?;
    set_number(&object, "programCompileUs", value.program_compile_us)?;
    set_value(
        &object,
        "programRevision",
        value
            .program_revision
            .as_ref()
            .map(|revision| JsValue::from_str(revision))
            .unwrap_or(JsValue::UNDEFINED),
    )?;
    set_number(&object, "instructionCount", value.instruction_count as f64)?;
    set_number(
        &object,
        "compiledExpressionCount",
        value.compiled_expression_count as f64,
    )?;
    set_number(&object, "fieldSlotCount", value.field_slot_count as f64)?;
    set_number(
        &object,
        "bindingProgramCount",
        value.binding_program_count as f64,
    )?;
    set_number(
        &object,
        "averageRowProgramExecuteUs",
        value.average_row_program_execute_us,
    )?;
    set_number(&object, "rowCount", value.row_count as f64)?;
    set_number(&object, "createdElements", value.created_elements as f64)?;
    set_number(&object, "createdTexts", value.created_texts as f64)?;
    set_number(&object, "bindings", value.bindings as f64)?;
    set_number(&object, "domOperations", value.dom_operations as f64)?;
    Ok(object.into())
}

fn set_number(object: &Object, name: &str, value: f64) -> Result<(), JsValue> {
    set_value(object, name, JsValue::from_f64(value))
}

fn set_value(object: &Object, name: &str, value: JsValue) -> Result<(), JsValue> {
    if Reflect::set(object, &JsValue::from_str(name), &value)? {
        Ok(())
    } else {
        Err(JsValue::from_str("JavaScript field assignment failed"))
    }
}
