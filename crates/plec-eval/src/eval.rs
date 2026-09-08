use plec_dom::platform::window;
use plec_schema::delta::RuntimeValue;
use plec_schema::typed::TypedApplication;
use plec_schema::typed::TypedExpressionInstruction;
use std::collections::HashMap;
use wasm_bindgen::JsValue;

pub fn typed_eval(
    app: &TypedApplication,
    cookie_policy: Option<&plec_dom::cookie::CookiePolicyMap>,
    program: usize,
    states: &[RuntimeValue],
    row: Option<&HashMap<String, RuntimeValue>>,
    row_index: usize,
) -> Result<RuntimeValue, JsValue> {
    typed_eval_frame(app, cookie_policy, program, states, row, row_index, &[], &[])
}

#[allow(clippy::too_many_arguments)]
pub fn typed_eval_frame(
    app: &TypedApplication,
    cookie_policy: Option<&plec_dom::cookie::CookiePolicyMap>,
    program: usize,
    states: &[RuntimeValue],
    row: Option<&HashMap<String, RuntimeValue>>,
    _row_index: usize,
    frame: &[RuntimeValue],
    event: &[RuntimeValue],
) -> Result<RuntimeValue, JsValue> {
    let mut fuel = plec_ir::limits::MAX_EXPRESSION_STEPS;
    typed_eval_bounded(
        app,
        cookie_policy,
        program,
        states,
        row,
        frame,
        event,
        &mut fuel,
        0,
    )
}

/// Execution-budget wrapper: the interpreter shares one fuel counter across
/// every nested Filter/Map evaluation, so crafted loops or self-referential
/// predicates exhaust a documented budget instead of pinning the tab or
/// overflowing the stack.
#[allow(clippy::too_many_arguments)]
fn typed_eval_bounded(
    app: &TypedApplication,
    cookie_policy: Option<&plec_dom::cookie::CookiePolicyMap>,
    program: usize,
    states: &[RuntimeValue],
    row: Option<&HashMap<String, RuntimeValue>>,
    frame: &[RuntimeValue],
    event: &[RuntimeValue],
    fuel: &mut usize,
    nesting: usize,
) -> Result<RuntimeValue, JsValue> {
    if nesting > plec_ir::limits::MAX_EVAL_NESTING {
        return Err(JsValue::from_str("expression nesting exceeds limit"));
    }
    let instructions = &app
        .expressions
        .get(program)
        .ok_or_else(|| JsValue::from_str("expression handle out of range"))?
        .instructions;
    let mut stack = Vec::<RuntimeValue>::new();
    let mut pc = 0usize;
    while pc < instructions.len() {
        if *fuel == 0 {
            return Err(JsValue::from_str("expression execution budget exceeded"));
        }
        *fuel -= 1;
        let instruction = &instructions[pc];
        match instruction {
            TypedExpressionInstruction::Constant { constant } => stack.push(
                app.constants
                    .get(*constant)
                    .cloned()
                    .unwrap_or(RuntimeValue::Null),
            ),
            TypedExpressionInstruction::LoadState { state } => {
                stack.push(states.get(*state).cloned().unwrap_or(RuntimeValue::Null))
            }
            TypedExpressionInstruction::LoadRef { reference } => stack.push(
                app.ref_values
                    .get(*reference)
                    .cloned()
                    .unwrap_or(RuntimeValue::Null),
            ),
            TypedExpressionInstruction::LoadProp { prop } => stack.push(
                app.runtime_props
                    .get(*prop)
                    .cloned()
                    .unwrap_or(RuntimeValue::Null),
            ),
            TypedExpressionInstruction::LoadRowRecord => {
                stack.push(RuntimeValue::Record(row.cloned().unwrap_or_default()))
            }
            TypedExpressionInstruction::LoadRowField { field } => {
                let field = app.strings.get(*field).map(String::as_str).unwrap_or("");
                stack.push(if field.is_empty() {
                    RuntimeValue::Record(row.cloned().unwrap_or_default())
                } else {
                    row.and_then(|value| value.get(field))
                        .cloned()
                        .unwrap_or(RuntimeValue::Null)
                });
            }
            TypedExpressionInstruction::LoadEventField { field } => {
                stack.push(event.get(*field).cloned().unwrap_or(RuntimeValue::Null))
            }
            TypedExpressionInstruction::LoadFrame { slot } => {
                stack.push(frame.get(*slot).cloned().unwrap_or(RuntimeValue::Null))
            }
            TypedExpressionInstruction::LoadHost { host } => {
                let value = app
                    .host_slots
                    .get(*host)
                    .and_then(|slot| match slot.kind.as_str() {
                        // A policy denial (or any read failure) resolves the
                        // slot to Null. Falling back to host inputs here would
                        // leak host/SSR-adopted values past the capability
                        // gate; the host grants `getSync` explicitly or the
                        // slot stays empty.
                        "cookie" => slot
                            .name
                            .and_then(|name| app.strings.get(name))
                            .and_then(|name| {
                                plec_dom::cookie::read_sync_cookie(cookie_policy, name).ok()
                            }),
                        "location" => app
                            .host_inputs
                            .get("location.pathname")
                            .cloned()
                            .map(|pathname| {
                                RuntimeValue::Record(HashMap::from([
                                    ("pathname".into(), pathname),
                                    (
                                        "search".into(),
                                        app.host_inputs
                                            .get("location.search")
                                            .cloned()
                                            .unwrap_or_default(),
                                    ),
                                    (
                                        "hash".into(),
                                        app.host_inputs
                                            .get("location.hash")
                                            .cloned()
                                            .unwrap_or_default(),
                                    ),
                                ]))
                            })
                            .or_else(|| {
                                window().ok().and_then(|window| {
                                    let location = window.location();
                                    Some(RuntimeValue::Record(HashMap::from([
                                        (
                                            "pathname".into(),
                                            RuntimeValue::String(location.pathname().ok()?),
                                        ),
                                        (
                                            "search".into(),
                                            RuntimeValue::String(location.search().ok()?),
                                        ),
                                        (
                                            "hash".into(),
                                            RuntimeValue::String(location.hash().ok()?),
                                        ),
                                    ])))
                                })
                            }),
                        "mediaQuery" => slot
                            .query
                            .and_then(|query| app.strings.get(query))
                            .and_then(|query| {
                                window().ok()?.match_media(query).ok()?.map(|media| {
                                    RuntimeValue::Record(HashMap::from([(
                                        "matches".into(),
                                        RuntimeValue::Bool(media.matches()),
                                    )]))
                                })
                            }),
                        "currentYear" => Some(RuntimeValue::Number(
                            js_sys::Date::new_0().get_full_year() as f64,
                        )),
                        "loaderData" => app.host_inputs.get("loaderData").cloned(),
                        _ => None,
                    })
                    .unwrap_or(RuntimeValue::Null);
                stack.push(value)
            }
            TypedExpressionInstruction::Field { field } => {
                let object = stack
                    .pop()
                    .ok_or_else(|| JsValue::from_str("expression stack underflow: field"))?;
                let field = app.strings.get(*field).map(String::as_str).unwrap_or("");
                stack.push(match object {
                    RuntimeValue::Record(value) => {
                        value.get(field).cloned().unwrap_or(RuntimeValue::Null)
                    }
                    RuntimeValue::Array(value) if field == "length" => {
                        RuntimeValue::Number(value.len() as f64)
                    }
                    RuntimeValue::String(value) if field == "length" => {
                        RuntimeValue::Number(value.chars().count() as f64)
                    }
                    _ => RuntimeValue::Null,
                });
            }
            TypedExpressionInstruction::Index => {
                let key = stack
                    .pop()
                    .ok_or_else(|| JsValue::from_str("expression stack underflow: index key"))?;
                let object = stack
                    .pop()
                    .ok_or_else(|| JsValue::from_str("expression stack underflow: index object"))?;
                let key = match key {
                    RuntimeValue::String(value) => value,
                    RuntimeValue::Number(value) if value.is_finite() && value.fract() == 0.0 => {
                        value.to_string()
                    }
                    _ => String::new(),
                };
                stack.push(match object {
                    RuntimeValue::Record(value) => {
                        value.get(&key).cloned().unwrap_or(RuntimeValue::Null)
                    }
                    RuntimeValue::Array(value) => key
                        .parse::<usize>()
                        .ok()
                        .and_then(|index| value.get(index))
                        .cloned()
                        .unwrap_or(RuntimeValue::Null),
                    RuntimeValue::String(value) => key
                        .parse::<usize>()
                        .ok()
                        .and_then(|index| value.chars().nth(index))
                        .map(|value| RuntimeValue::String(value.to_string()))
                        .unwrap_or(RuntimeValue::Null),
                    _ => RuntimeValue::Null,
                });
            }
            TypedExpressionInstruction::Filter { predicate, .. }
            | TypedExpressionInstruction::Map {
                mapper: predicate, ..
            } => {
                let source = stack
                    .pop()
                    .ok_or_else(|| JsValue::from_str("expression stack underflow: collection"))?;
                let mut output = Vec::new();
                for (_index, item) in source.array().unwrap_or(&[]).iter().cloned().enumerate() {
                    let object = item.record().cloned().unwrap_or_default();
                    let value = typed_eval_bounded(
                        app,
                        cookie_policy,
                        *predicate,
                        states,
                        Some(&object),
                        frame,
                        event,
                        fuel,
                        nesting + 1,
                    )?;
                    if matches!(instruction, TypedExpressionInstruction::Map { .. }) {
                        output.push(value);
                    } else if typed_truthy(&value) {
                        output.push(RuntimeValue::Record(object));
                    }
                }
                stack.push(RuntimeValue::Array(output));
            }
            TypedExpressionInstruction::String { kind, count } => {
                if stack.len() < *count {
                    return Err(JsValue::from_str("expression stack underflow: string"));
                }
                let mut parts = (0..*count).filter_map(|_| stack.pop()).collect::<Vec<_>>();
                parts.reverse();
                let value = match kind.as_str() {
                    "trim" => RuntimeValue::String(
                        typed_value_string(parts.first().unwrap_or(&RuntimeValue::Null))
                            .trim()
                            .to_owned(),
                    ),
                    "lower" => RuntimeValue::String(
                        typed_value_string(parts.first().unwrap_or(&RuntimeValue::Null))
                            .to_lowercase(),
                    ),
                    "upper" => RuntimeValue::String(
                        typed_value_string(parts.first().unwrap_or(&RuntimeValue::Null))
                            .to_uppercase(),
                    ),
                    "encodeUriComponent" => RuntimeValue::String(
                        js_sys::encode_uri_component(&typed_value_string(
                            parts.first().unwrap_or(&RuntimeValue::Null),
                        ))
                        .into(),
                    ),
                    "jsonStringify" => RuntimeValue::String(
                        serde_json::to_string(parts.first().unwrap_or(&RuntimeValue::Null))
                            .unwrap_or_default(),
                    ),
                    "includes" => RuntimeValue::Bool(
                        typed_value_string(parts.first().unwrap_or(&RuntimeValue::Null)).contains(
                            &typed_value_string(parts.get(1).unwrap_or(&RuntimeValue::Null)),
                        ),
                    ),
                    _ => RuntimeValue::String(
                        parts.iter().map(typed_value_string).collect::<String>(),
                    ),
                };
                stack.push(value);
            }
            TypedExpressionInstruction::MakeArray { count, spreads } => {
                if stack.len() < *count {
                    return Err(JsValue::from_str("expression stack underflow: makeArray"));
                }
                let mut values = (0..*count).filter_map(|_| stack.pop()).collect::<Vec<_>>();
                values.reverse();
                let mut output = Vec::new();
                for (index, value) in values.into_iter().enumerate() {
                    if spreads.get(index).copied().unwrap_or(false) {
                        output.extend(value.array().unwrap_or(&[]).iter().cloned());
                    } else {
                        output.push(value);
                    }
                }
                stack.push(RuntimeValue::Array(output));
            }
            TypedExpressionInstruction::MakeRecord { fields, spreads } => {
                if stack.len() < fields.len() {
                    return Err(JsValue::from_str("expression stack underflow: makeRecord"));
                }
                let mut values = (0..fields.len())
                    .filter_map(|_| stack.pop())
                    .collect::<Vec<_>>();
                values.reverse();
                let mut record = HashMap::new();
                for (index, (field, value)) in fields.iter().zip(values).enumerate() {
                    if spreads.get(index).copied().unwrap_or(false) {
                        if let RuntimeValue::Record(values) = value {
                            record.extend(values);
                        }
                    } else if let Some(field) = app.strings.get(*field) {
                        record.insert(field.clone(), value);
                    }
                }
                stack.push(RuntimeValue::Record(record));
            }
            TypedExpressionInstruction::OmitFields { fields } => {
                let value = stack
                    .pop()
                    .ok_or_else(|| JsValue::from_str("expression stack underflow: omitFields"))?;
                let mut record = value.record().cloned().unwrap_or_default();
                for field in fields {
                    if let Some(name) = app.strings.get(*field) {
                        record.remove(name);
                    }
                }
                stack.push(RuntimeValue::Record(record));
            }
            TypedExpressionInstruction::Binary { kind } => {
                let right = stack
                    .pop()
                    .ok_or_else(|| JsValue::from_str("expression stack underflow: binary"))?;
                let left = stack
                    .pop()
                    .ok_or_else(|| JsValue::from_str("expression stack underflow: binary"))?;
                let result = match kind.as_str() {
                    "equal" => RuntimeValue::Bool(left == right),
                    "notEqual" => RuntimeValue::Bool(left != right),
                    "instanceofError" => RuntimeValue::Bool(matches!(
                        left,
                        RuntimeValue::Record(ref value)
                            if value.contains_key("kind") || value.contains_key("message")
                    )),
                    "and" => {
                        if typed_truthy(&left) {
                            right
                        } else {
                            left
                        }
                    }
                    "or" => {
                        if typed_truthy(&left) {
                            left
                        } else {
                            right
                        }
                    }
                    "coalesce" => {
                        if left.is_null() {
                            right
                        } else {
                            left
                        }
                    }
                    "add" => match (left, right) {
                        (RuntimeValue::Number(left), RuntimeValue::Number(right)) => {
                            RuntimeValue::Number(left + right)
                        }
                        (left, right) => RuntimeValue::String(format!(
                            "{}{}",
                            typed_value_string(&left),
                            typed_value_string(&right)
                        )),
                    },
                    "subtract" => RuntimeValue::Number(left.number() - right.number()),
                    "multiply" => RuntimeValue::Number(left.number() * right.number()),
                    "divide" => RuntimeValue::Number(left.number() / right.number()),
                    "greater" => RuntimeValue::Bool(left.number() > right.number()),
                    "greaterEqual" => RuntimeValue::Bool(left.number() >= right.number()),
                    "less" => RuntimeValue::Bool(left.number() < right.number()),
                    "lessEqual" => RuntimeValue::Bool(left.number() <= right.number()),
                    _ => RuntimeValue::Null,
                };
                stack.push(result);
            }
            TypedExpressionInstruction::Unary { kind } => {
                let value = stack
                    .pop()
                    .ok_or_else(|| JsValue::from_str("expression stack underflow: unary"))?;
                stack.push(match kind.as_str() {
                    "not" => RuntimeValue::Bool(!typed_truthy(&value)),
                    "minus" => RuntimeValue::Number(-value.number()),
                    _ => value,
                });
            }
            TypedExpressionInstruction::JumpIfFalse { target } => {
                if !typed_truthy(
                    &stack.pop().ok_or_else(|| {
                        JsValue::from_str("expression stack underflow: jumpIfFalse")
                    })?,
                ) {
                    pc = *target;
                    continue;
                }
            }
            TypedExpressionInstruction::JumpIfTrue { target } => {
                if typed_truthy(
                    &stack.pop().ok_or_else(|| {
                        JsValue::from_str("expression stack underflow: jumpIfTrue")
                    })?,
                ) {
                    pc = *target;
                    continue;
                }
            }
            TypedExpressionInstruction::Jump { target } => {
                pc = *target;
                continue;
            }
            TypedExpressionInstruction::Return => {
                return Ok(stack.pop().unwrap_or(RuntimeValue::Null))
            }
        };
        pc += 1;
    }
    Ok(stack.pop().unwrap_or(RuntimeValue::Null))
}

pub fn typed_truthy(value: &RuntimeValue) -> bool {
    match value {
        RuntimeValue::Null => false,
        RuntimeValue::Bool(value) => *value,
        RuntimeValue::Number(value) => *value != 0.0,
        RuntimeValue::String(value) => !value.is_empty(),
        RuntimeValue::Array(value) => !value.is_empty(),
        RuntimeValue::Record(_) => true,
    }
}

pub fn typed_value_string(value: &RuntimeValue) -> String {
    match value {
        RuntimeValue::String(value) => value.clone(),
        RuntimeValue::Null => String::new(),
        RuntimeValue::Bool(value) => value.to_string(),
        RuntimeValue::Number(value) => value.to_string(),
        RuntimeValue::Array(_) | RuntimeValue::Record(_) => {
            serde_json::to_string(value).unwrap_or_default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn make_array_flattens_only_marked_spreads() {
        let app = serde_json::from_value(serde_json::json!({
            "version": "0.10", "rootNode": 0, "strings": ["div"],
            "constants": [["first", "second"], "third"],
            "nodes": [{"op": "element", "tag": 0}],
            "expressions": [{"instructions": [
                {"op": "constant", "constant": 0},
                {"op": "constant", "constant": 1},
                {"op": "makeArray", "count": 2, "spreads": [true, false]},
                {"op": "return"}
            ]}]
        }))
        .unwrap();

        assert_eq!(
            typed_eval(&app, None, 0, &[], None, 0).unwrap(),
            RuntimeValue::Array(vec![
                RuntimeValue::String("first".into()),
                RuntimeValue::String("second".into()),
                RuntimeValue::String("third".into()),
            ]),
        );
    }

    #[test]
    fn add_preserves_numeric_values() {
        let app = serde_json::from_value(serde_json::json!({
            "version": "0.10", "rootNode": 0, "strings": [],
            "constants": [0.0, 1.0],
            "nodes": [{"op": "element", "tag": 0}],
            "expressions": [{"instructions": [
                {"op": "constant", "constant": 0},
                {"op": "constant", "constant": 1},
                {"op": "binary", "kind": "add"},
                {"op": "return"}
            ]}]
        }))
        .unwrap();

        assert_eq!(
            typed_eval(&app, None, 0, &[], None, 0).unwrap(),
            RuntimeValue::Number(1.0)
        );
    }

    #[test]
    fn computed_row_records_and_operators_are_executable() {
        let app = serde_json::from_value(serde_json::json!({
            "version": "0.10", "rootNode": 0, "strings": ["count"],
            "constants": [2.0], "nodes": [{"op": "element", "tag": 0}],
            "expressions": [{"instructions": [
                {"op": "loadRowRecord"}, {"op": "constant", "constant": 0},
                {"op": "index"}, {"op": "constant", "constant": 0},
                {"op": "binary", "kind": "greaterEqual"}, {"op": "return"}
            ]}]
        }))
        .unwrap();
        let row = HashMap::from([("2".into(), RuntimeValue::Number(3.0))]);
        assert_eq!(
            typed_eval(&app, None, 0, &[], Some(&row), 0).unwrap(),
            RuntimeValue::Bool(true)
        );
    }

    // Host test binaries cannot exercise error paths: wasm-bindgen's
    // non-wasm stubs panic on any JsValue operation. Backward-jump and
    // self-referential budget tests live in
    // crates/plec-runtime/tests/untrusted_input_limits.rs (browser suite).

    #[test]
    fn nested_filter_within_nesting_limit_evaluates() {
        let app = serde_json::from_value(serde_json::json!({
            "version": "0.10", "rootNode": 0, "strings": [],
            "constants": [[1, 2, 3]],
            "nodes": [{"op": "element", "tag": 0}],
            "expressions": [
                {"instructions": [
                    {"op": "constant", "constant": 0},
                    {"op": "filter", "predicate": 1, "itemSlot": 0},
                    {"op": "return"}
                ]},
                {"instructions": [
                    {"op": "loadRowRecord"},
                    {"op": "return"}
                ]}
            ]
        }))
        .unwrap();

        assert_eq!(
            typed_eval(&app, None, 0, &[], None, 0).unwrap(),
            RuntimeValue::Array(vec![
                RuntimeValue::Record(HashMap::new()),
                RuntimeValue::Record(HashMap::new()),
                RuntimeValue::Record(HashMap::new()),
            ])
        );
    }
}
