use crate::dom::properties::*;
use crate::eval::{expression::*, typed_vm::*, value::*};
use crate::runtime::lifecycle::*;
use wasm_bindgen::JsCast;

pub(crate) fn typed_apply_binding(
    app: &TypedApplication,
    binding: &TypedBinding,
    node: &Node,
    states: &[RuntimeValue],
    row: Option<&HashMap<String, RuntimeValue>>,
    index: usize,
) -> Result<(), JsValue> {
    let value = typed_eval(app, binding.expression, states, row, index)?;
    typed_apply_value(app, &binding.sink, binding.name, node, value)
}

pub(crate) fn typed_apply_value(
    app: &TypedApplication,
    sink: &str,
    name: Option<usize>,
    node: &Node,
    value: RuntimeValue,
) -> Result<(), JsValue> {
    if sink == "text" {
        let value = typed_value_string(&value);
        if let Some(text) = node.dyn_ref::<web_sys::Text>() {
            text.set_data(&value);
        } else {
            node.set_text_content(Some(&value));
        }
        return Ok(());
    }
    let element: Element = node
        .clone()
        .dyn_into()
        .map_err(|_| JsValue::from_str("binding target is not element"))?;
    let name = name
        .and_then(|handle| app.strings.get(handle))
        .map(String::as_str)
        .unwrap_or("");
    if sink == "property" {
        if let Ok(input) = element.clone().dyn_into::<web_sys::HtmlInputElement>() {
            if name == "checked" {
                let checked = matches!(value, RuntimeValue::Bool(true));
                input.set_checked(checked);
                if !checked {
                    element.remove_attribute("checked")?;
                }
                return Ok(());
            }
            if name == "disabled" {
                let disabled = matches!(value, RuntimeValue::Bool(true));
                input.set_disabled(disabled);
                if !disabled {
                    element.remove_attribute("disabled")?;
                }
                return Ok(());
            }
        }
        js_sys::Reflect::set(
            &element,
            &JsValue::from_str(name),
            &serde_wasm_bindgen::to_value(&value)?,
        )
        .map_err(|_| JsValue::from_str("property write failed"))?;
    } else if name == "checked" || name == "disabled" {
        let enabled = matches!(value, RuntimeValue::Bool(true));
        if let Ok(input) = element.clone().dyn_into::<web_sys::HtmlInputElement>() {
            if name == "checked" {
                input.set_checked(enabled);
            } else {
                input.set_disabled(enabled);
            }
        }
        if enabled {
            element.set_attribute(name, "")?;
        } else {
            element.remove_attribute(name)?;
        }
    } else {
        element.set_attribute(
            if name == "className" { "class" } else { name },
            &typed_value_string(&value),
        )?;
    }
    Ok(())
}

pub(crate) fn typed_apply_spread(
    _app: &TypedApplication,
    sink: &str,
    node: &Node,
    value: RuntimeValue,
) -> Result<(), JsValue> {
    let values = match value {
        // A component invoked without props still observes an empty props bag
        // in React. Dynamic component references have no static target from
        // which lowering can synthesize that record, so preserve the same
        // behaviour at the DOM spread boundary.
        RuntimeValue::Null => return Ok(()),
        RuntimeValue::Record(values) => values,
        _ => return Err(JsValue::from_str("intrinsic props spread must evaluate to a record")),
    };
    let element: Element = node.clone().dyn_into()
        .map_err(|_| JsValue::from_str("binding target is not element"))?;
    if sink != "attribute" && sink != "property" {
        return Err(JsValue::from_str("unsupported intrinsic props spread sink"));
    }
    // Keep the previous keys on the DOM host. This makes a reactive spread
    // self-contained and lets the next application remove attributes that no
    // longer occur in the serializable bag.
    let previous = element
        .get_attribute("data-plec-spread-keys")
        .unwrap_or_default()
        .split('\u{1f}')
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect::<std::collections::HashSet<_>>();
    let current = values.keys().cloned().collect::<std::collections::HashSet<_>>();
    for stale in previous.difference(&current) {
        let name = if stale == "className" { "class" } else { stale };
        element.remove_attribute(name)?;
        let _ = js_sys::Reflect::set(&element, &JsValue::from_str(stale), &JsValue::UNDEFINED);
    }
    for (name, value) in values {
        if name == "checked" {
            if let Ok(input) = element.clone().dyn_into::<web_sys::HtmlInputElement>() {
                input.set_checked(matches!(value, RuntimeValue::Bool(true)));
            }
        } else if name == "disabled" {
            if let Ok(input) = element.clone().dyn_into::<web_sys::HtmlInputElement>() {
                input.set_disabled(matches!(value, RuntimeValue::Bool(true)));
            }
        } else if name == "value" {
            js_sys::Reflect::set(
                &element,
                &JsValue::from_str("value"),
                &serde_wasm_bindgen::to_value(&value)?,
            )
            .map_err(|_| JsValue::from_str("spread property write failed"))?;
        } else if matches!(value, RuntimeValue::Bool(false) | RuntimeValue::Null) {
            element.remove_attribute(if name == "className" { "class" } else { &name })?;
        } else {
            element.set_attribute(
                if name == "className" { "class" } else { &name },
                &typed_value_string(&value),
            )?;
        }
    }
    element.set_attribute(
        "data-plec-spread-keys",
        &current.into_iter().collect::<Vec<_>>().join("\u{1f}"),
    )?;
    Ok(())
}

pub(crate) fn apply_binding_with_context(
    node: &Node,
    binding: &Binding,
    expressions: &[Expression],
    scope: &HashMap<String, Value>,
    environment: &HashMap<String, Value>,
) -> Result<(), JsValue> {
    let value = binding
        .expression_id
        .as_ref()
        .and_then(|id| expressions.iter().find(|entry| entry.id == *id))
        .map(|entry| evaluate_with_context(&entry.expression, scope, environment))
        .unwrap_or(Value::Null);
    if binding.kind == "text" {
        node.clone()
            .dyn_into::<web_sys::Text>()
            .map_err(|_| JsValue::from_str("text binding target"))?
            .set_data(&value_string(Some(&value)));
    } else {
        let element: Element = node
            .clone()
            .dyn_into()
            .map_err(|_| JsValue::from_str("element binding target"))?;
        set_value(
            &element,
            binding.attribute_name.as_deref().unwrap_or_default(),
            &value,
        )?;
    }
    Ok(())
}

pub(crate) fn apply_binding_host(
    node: &Node,
    binding: &Binding,
    expressions: &[Expression],
    scope: &HashMap<String, Value>,
    handles: &HashMap<String, Node>,
) -> Result<(), JsValue> {
    let value = binding
        .expression_id
        .as_ref()
        .and_then(|id| expressions.iter().find(|entry| entry.id == *id))
        .map(|entry| evaluate(&resolve_host_reads(&entry.expression, handles), scope))
        .unwrap_or(Value::Null);
    if binding.kind == "text" {
        node.clone()
            .dyn_into::<web_sys::Text>()
            .map_err(|_| JsValue::from_str("text binding target"))?
            .set_data(&value_string(Some(&value)));
    } else {
        set_value(
            &node
                .clone()
                .dyn_into::<Element>()
                .map_err(|_| JsValue::from_str("element binding target"))?,
            binding.attribute_name.as_deref().unwrap_or_default(),
            &value,
        )?;
    }
    Ok(())
}

pub(crate) fn resolve_host_reads(value: &Value, handles: &HashMap<String, Node>) -> Value {
    if value.get("kind").and_then(Value::as_str) == Some("host-element-read") {
        let reference = value
            .get("refId")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let Some(element) = handles
            .get(reference)
            .and_then(|node| node.clone().dyn_into::<Element>().ok())
        else {
            return serde_json::json!({"kind":"literal","value":null});
        };
        let capability = value.get("capability").unwrap_or(&Value::Null);
        let result = match capability.get("kind").and_then(Value::as_str) {
            Some("property") => match capability.get("name").and_then(Value::as_str) {
                Some("value") => element
                    .clone()
                    .dyn_into::<web_sys::HtmlInputElement>()
                    .ok()
                    .map(|input| Value::String(input.value()))
                    .unwrap_or(Value::Null),
                Some("tagName") => Value::String(element.tag_name()),
                Some(name) => element
                    .get_attribute(name)
                    .map(Value::String)
                    .unwrap_or(Value::Bool(false)),
                _ => Value::Null,
            },
            Some("closest") => Value::Bool(
                element
                    .closest(
                        capability
                            .get("selector")
                            .and_then(Value::as_str)
                            .unwrap_or_default(),
                    )
                    .ok()
                    .flatten()
                    .is_some(),
            ),
            Some("is-active") => Value::Bool(
                element
                    .owner_document()
                    .and_then(|document| document.active_element())
                    .map(|active| active == element)
                    .unwrap_or(false),
            ),
            _ => Value::Null,
        };
        return serde_json::json!({"kind":"literal","value":result});
    }
    match value {
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|item| resolve_host_reads(item, handles))
                .collect(),
        ),
        Value::Object(values) => Value::Object(
            values
                .iter()
                .map(|(key, item)| (key.clone(), resolve_host_reads(item, handles)))
                .collect(),
        ),
        _ => value.clone(),
    }
}

pub(crate) fn apply_binding(
    node: &Node,
    binding: &Binding,
    expressions: &[Expression],
    scope: &HashMap<String, Value>,
) -> Result<(), JsValue> {
    let value = expressions
        .iter()
        .find(|entry| Some(&entry.id) == binding.expression_id.as_ref())
        .map(|entry| evaluate(&entry.expression, scope))
        .unwrap_or(Value::Null);
    if binding.kind == "text" {
        node.clone()
            .dyn_into::<web_sys::Text>()
            .map_err(|_| JsValue::from_str("text binding target"))?
            .set_data(&value_string(Some(&value)));
    } else {
        let element: Element = node
            .clone()
            .dyn_into()
            .map_err(|_| JsValue::from_str("element binding target"))?;
        set_value(
            &element,
            binding.attribute_name.as_deref().unwrap_or_default(),
            &value,
        )?;
    }
    Ok(())
}
