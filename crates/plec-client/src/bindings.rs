//! Typed binding sinks. These are the only binding appliers: the legacy
//! string-id graph scheme's binding host was removed with
//! wasm-runtime-ixk.7.

use plec_eval::eval::*;
use std::collections::HashMap;
use wasm_bindgen::{prelude::*, JsCast};

use plec_ir::sink::{is_safe_attribute_name, is_safe_attribute_value, is_safe_property_name};
use plec_schema::delta::RuntimeValue;
use plec_schema::typed::{TypedApplication, TypedBinding};
use web_sys::{Element, Node};

pub fn typed_apply_binding(
    app: &TypedApplication,
    cookie_policy: Option<&plec_dom::cookie::CookiePolicyMap>,
    binding: &TypedBinding,
    node: &Node,
    states: &[RuntimeValue],
    row: Option<&HashMap<String, RuntimeValue>>,
    index: usize,
) -> Result<(), JsValue> {
    let value = typed_eval(app, cookie_policy, binding.expression, states, row, index)?;
    typed_apply_value(app, &binding.sink, binding.name, node, value)
}

pub fn typed_apply_value(
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
    // DOM-sink policy (plec_ir::sink). Names came from the validated artifact,
    // so a rejection here means substituted or tampered IR: fail closed.
    if sink == "property" {
        if !is_safe_property_name(name) {
            return Err(JsValue::from_str("unsafe property binding sink"));
        }
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
        return Ok(());
    }
    if sink != "attribute" {
        return Err(JsValue::from_str("unsupported binding sink"));
    }
    if !is_safe_attribute_name(name) {
        return Err(JsValue::from_str("unsafe attribute binding sink"));
    }
    if name == "checked" || name == "disabled" {
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
        let value = typed_value_string(&value);
        if !is_safe_attribute_value(name, &value) {
            return Err(JsValue::from_str("unsafe attribute binding value"));
        }
        element.set_attribute(if name == "className" { "class" } else { name }, &value)?;
    }
    Ok(())
}

pub fn typed_apply_spread(
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
        _ => {
            return Err(JsValue::from_str(
                "intrinsic props spread must evaluate to a record",
            ))
        }
    };
    let element: Element = node
        .clone()
        .dyn_into()
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
    // DOM-sink policy (plec_ir::sink). Spread keys come from evaluated app
    // data, so hostile keys (event-handler casing, srcdoc, script URL
    // schemes) are skipped instead of applied, mirroring the SSR serializer.
    // Only policy-approved keys join the bookkeeping set so stale removal
    // never resurrects a sink the policy rejected.
    let mut applied = std::collections::HashSet::new();
    for (name, value) in &values {
        if !is_safe_attribute_name(name) {
            continue;
        }
        if !matches!(value, RuntimeValue::Bool(false) | RuntimeValue::Null)
            && !is_safe_attribute_value(name, &typed_value_string(value))
        {
            continue;
        }
        applied.insert(name.clone());
    }
    for stale in previous.difference(&applied) {
        let name = if stale == "className" { "class" } else { stale };
        if !is_safe_attribute_name(stale) {
            continue;
        }
        element.remove_attribute(name)?;
        if is_safe_property_name(stale) {
            let _ = js_sys::Reflect::set(&element, &JsValue::from_str(stale), &JsValue::UNDEFINED);
        }
    }
    for (name, value) in values {
        if !applied.contains(&name) {
            continue;
        }
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
        &applied.into_iter().collect::<Vec<_>>().join("\u{1f}"),
    )?;
    Ok(())
}
