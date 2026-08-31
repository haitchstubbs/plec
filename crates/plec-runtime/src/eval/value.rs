use crate::runtime::lifecycle::*;

pub(crate) fn number(value: f64) -> Value {
    serde_json::Number::from_f64(value)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

pub(crate) fn value_string(value: Option<&Value>) -> String {
    match value.unwrap_or(&Value::Null) {
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Null => String::new(),
        value => value.to_string(),
    }
}

pub(crate) fn parse_initial_state(value: &str) -> Value {
    serde_json::from_str(value).unwrap_or_else(|_| match value {
        "undefined" => Value::Null,
        other => Value::String(other.into()),
    })
}

pub(crate) fn truthy(value: &Value) -> bool {
    !value.is_null() && value.as_bool() != Some(false) && value.as_str() != Some("")
}
