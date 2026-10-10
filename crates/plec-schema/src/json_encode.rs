//! Small, deterministic JSON encoders for the runtime's recursive value type.
//! These replace the browser runtime's generic Serde serializers while the
//! native builds retain their Serde implementations for Rust-owned formats.

use crate::RuntimeValue;
use std::fmt::Write as _;

pub fn runtime_value(value: &RuntimeValue) -> String {
    let mut output = String::new();
    write_runtime_value(&mut output, value);
    output
}

pub fn runtime_values(values: &[RuntimeValue]) -> String {
    let mut output = String::new();
    output.push('[');
    for (index, value) in values.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        write_runtime_value(&mut output, value);
    }
    output.push(']');
    output
}

fn write_runtime_value(output: &mut String, value: &RuntimeValue) {
    match value {
        RuntimeValue::Null => output.push_str("null"),
        RuntimeValue::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        RuntimeValue::Number(value) if !value.is_finite() => output.push_str("null"),
        RuntimeValue::Number(value) => {
            let mut buffer = zmij::Buffer::new();
            output.push_str(buffer.format_finite(*value));
        }
        RuntimeValue::String(value) => write_string(output, value),
        RuntimeValue::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                write_runtime_value(output, value);
            }
            output.push(']');
        }
        RuntimeValue::Record(values) => {
            output.push('{');
            for (index, (name, value)) in values.iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                write_string(output, name);
                output.push(':');
                write_runtime_value(output, value);
            }
            output.push('}');
        }
    }
}

fn write_string(output: &mut String, value: &str) {
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0c}' => output.push_str("\\f"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character <= '\u{1f}' => {
                let _ = write!(output, "\\u{:04x}", character as u32);
            }
            character => output.push(character),
        }
    }
    output.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn runtime_encoder_matches_native_serde_json_for_json_values() {
        let values = [
            RuntimeValue::Null,
            RuntimeValue::Bool(true),
            RuntimeValue::Number(1.25),
            RuntimeValue::Number(-0.0),
            RuntimeValue::Number(1.0e21),
            RuntimeValue::Number(1.0e-7),
            RuntimeValue::Number(f64::NAN),
            RuntimeValue::String("quotes \" slash \\ controls\0\u{08}\u{0c}\n\r\t and ☃".into()),
            RuntimeValue::Array(vec![RuntimeValue::Number(3.0), RuntimeValue::Null]),
            RuntimeValue::Record(HashMap::from([
                ("first".into(), RuntimeValue::String("one".into())),
                ("second".into(), RuntimeValue::Number(-42.5)),
            ])),
        ];
        for value in &values {
            assert_eq!(runtime_value(value), serde_json::to_string(value).unwrap());
            if matches!(value, RuntimeValue::Array(_) | RuntimeValue::Record(_)) {
                assert_eq!(value.dom_string(), serde_json::to_string(value).unwrap());
            }
        }
        for number in [
            0.0,
            -0.0,
            1.0,
            -1.0,
            0.000001,
            0.0000001,
            1.0e20,
            1.0e21,
            f64::MIN_POSITIVE,
            f64::MAX,
            f64::MIN,
            9_007_199_254_740_992.0,
        ] {
            let value = RuntimeValue::Number(number);
            assert_eq!(
                runtime_value(&value),
                serde_json::to_string(&value).unwrap()
            );
        }
        assert_eq!(
            runtime_values(&values),
            serde_json::to_string(&values).unwrap()
        );
    }
}
