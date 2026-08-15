use crate::runtime::lifecycle::*;

pub(crate) fn typed_eval(
    app: &TypedApplication,
    program: usize,
    states: &[RuntimeValue],
    row: Option<&HashMap<String, RuntimeValue>>,
    row_index: usize,
) -> Result<RuntimeValue, JsValue> {
    typed_eval_frame(app, program, states, row, row_index, &[], &[])
}

pub(crate) fn typed_eval_frame(
    app: &TypedApplication,
    program: usize,
    states: &[RuntimeValue],
    row: Option<&HashMap<String, RuntimeValue>>,
    _row_index: usize,
    frame: &[RuntimeValue],
    event: &[RuntimeValue],
) -> Result<RuntimeValue, JsValue> {
    let instructions = &app
        .expressions
        .get(program)
        .ok_or_else(|| JsValue::from_str("expression handle out of range"))?
        .instructions;
    let mut stack = Vec::<RuntimeValue>::new();
    let mut pc = 0usize;
    while pc < instructions.len() {
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
                let value = app.host_slots.get(*host).and_then(|slot| {
                    (slot.kind == "cookie").then(|| slot.name.and_then(|name| app.strings.get(name)).and_then(|name| app.host_inputs.get(name)).cloned()).flatten()
                }).unwrap_or(RuntimeValue::Null);
                stack.push(value)
            }
            TypedExpressionInstruction::Field { field } => {
                let object = stack
                    .pop()
                    .ok_or_else(|| JsValue::from_str("expression stack underflow: field"))?;
                let field = app.strings.get(*field).map(String::as_str).unwrap_or("");
                stack.push(match object {
                    RuntimeValue::Record(value) => value.get(field).cloned().unwrap_or(RuntimeValue::Null),
                    RuntimeValue::Array(value) if field == "length" => RuntimeValue::Number(value.len() as f64),
                    RuntimeValue::String(value) if field == "length" => RuntimeValue::Number(value.chars().count() as f64),
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
                for (index, item) in source.array().unwrap_or(&[]).iter().cloned().enumerate() {
                    let object = item.record().cloned().unwrap_or_default();
                    let value = typed_eval(app, *predicate, states, Some(&object), index)?;
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
                    "trim" => RuntimeValue::String(typed_value_string(parts.first().unwrap_or(&RuntimeValue::Null))
                        .trim()
                        .to_owned()),
                    "lower" => RuntimeValue::String(typed_value_string(parts.first().unwrap_or(&RuntimeValue::Null))
                        .to_lowercase()),
                    "upper" => RuntimeValue::String(typed_value_string(parts.first().unwrap_or(&RuntimeValue::Null))
                        .to_uppercase()),
                    "includes" => RuntimeValue::Bool(typed_value_string(parts.first().unwrap_or(&RuntimeValue::Null))
                        .contains(&typed_value_string(
                            parts.get(1).unwrap_or(&RuntimeValue::Null),
                        ))),
                    _ => RuntimeValue::String(parts.iter().map(typed_value_string).collect::<String>()),
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
            TypedExpressionInstruction::MakeRecord { fields } => {
                if stack.len() < fields.len() {
                    return Err(JsValue::from_str("expression stack underflow: makeRecord"));
                }
                let mut values = (0..fields.len()).filter_map(|_| stack.pop()).collect::<Vec<_>>();
                values.reverse();
                let record = fields
                    .iter()
                    .zip(values)
                    .filter_map(|(field, value)| app.strings.get(*field).cloned().map(|field| (field, value)))
                    .collect();
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
                    "add" => RuntimeValue::String(format!(
                        "{}{}",
                        typed_value_string(&left),
                        typed_value_string(&right)
                    )),
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
            _ => {
                return Err(JsValue::from_str(
                    "unsupported typed expression instruction",
                ))
            }
        };
        pc += 1;
    }
    Ok(stack.pop().unwrap_or(RuntimeValue::Null))
}

pub(crate) fn typed_truthy(value: &RuntimeValue) -> bool {
    match value {
        RuntimeValue::Null => false,
        RuntimeValue::Bool(value) => *value,
        RuntimeValue::Number(value) => *value != 0.0,
        RuntimeValue::String(value) => !value.is_empty(),
        RuntimeValue::Array(value) => !value.is_empty(),
        RuntimeValue::Record(_) => true,
    }
}

pub(crate) fn typed_value_string(value: &RuntimeValue) -> String {
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
            "version": "0.9", "rootNode": 0, "strings": ["div"],
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
            typed_eval(&app, 0, &[], None, 0).unwrap(),
            RuntimeValue::Array(vec![
                RuntimeValue::String("first".into()),
                RuntimeValue::String("second".into()),
                RuntimeValue::String("third".into()),
            ]),
        );
    }
}
