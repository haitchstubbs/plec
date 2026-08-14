use crate::eval::{expression::*, value::*};
use crate::runtime::lifecycle::*;

pub(crate) fn apply_prop_program(
    node: &Node,
    program: &PropProgram,
    expressions: &[Expression],
    scope: &HashMap<String, Value>,
) -> Result<(), JsValue> {
    let element: Element = node
        .clone()
        .dyn_into()
        .map_err(|_| JsValue::from_str("prop program target"))?;
    for write in &program.writes {
        if write.kind == "event" || write.kind == "ref" {
            continue;
        }
        if write.kind == "spread" {
            let value = expression_value(&write.expression_id, expressions, scope);
            let Some(record) = value.as_object() else {
                continue;
            };
            for (name, value) in record {
                if name == "children" || name.starts_with("on") {
                    continue;
                }
                set_value(&element, name, value)?;
            }
            continue;
        }
        let value = write
            .static_value
            .as_ref()
            .map(|value| Value::String(value.clone()))
            .unwrap_or_else(|| expression_value(&write.expression_id, expressions, scope));
        set_value(&element, &write.name, &value)?;
    }
    Ok(())
}

pub(crate) fn set_value(element: &Element, name: &str, value: &Value) -> Result<(), JsValue> {
    let name = if name == "className" { "class" } else { name };
    if name == "checked" {
        if let Ok(input) = element.clone().dyn_into::<web_sys::HtmlInputElement>() {
            input.set_checked(value.as_bool().unwrap_or(false));
            return Ok(());
        }
    }
    if name == "indeterminate" {
        if let Ok(input) = element.clone().dyn_into::<web_sys::HtmlInputElement>() {
            input.set_indeterminate(value.as_bool().unwrap_or(false));
            return Ok(());
        }
    }
    if name == "value" {
        if let Ok(input) = element.clone().dyn_into::<web_sys::HtmlInputElement>() {
            input.set_value(&value_string(Some(value)));
            return Ok(());
        }
    }
    if value.as_bool() == Some(false) || value.is_null() {
        element.remove_attribute(name)?;
    } else {
        element.set_attribute(name, &value_string(Some(value)))?;
    }
    Ok(())
}
