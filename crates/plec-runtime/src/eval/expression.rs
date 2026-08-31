use crate::eval::value::*;
use crate::runtime::lifecycle::*;

pub(crate) fn expression_value_with_context(
    id: &Option<String>,
    expressions: &[Expression],
    scope: &HashMap<String, Value>,
    environment: &HashMap<String, Value>,
) -> Value {
    id.as_ref()
        .and_then(|id| expressions.iter().find(|entry| entry.id == *id))
        .map(|entry| evaluate_with_context(&entry.expression, scope, environment))
        .unwrap_or(Value::Null)
}

pub(crate) fn expression_value(
    id: &Option<String>,
    expressions: &[Expression],
    scope: &HashMap<String, Value>,
) -> Value {
    id.as_ref()
        .and_then(|id| expressions.iter().find(|entry| entry.id == *id))
        .map(|entry| evaluate(&entry.expression, scope))
        .unwrap_or(Value::Null)
}

pub(crate) fn evaluate(expression: &Value, scope: &HashMap<String, Value>) -> Value {
    let Some(kind) = expression.get("kind").and_then(Value::as_str) else {
        return Value::Null;
    };
    let left = || evaluate(expression.get("left").unwrap_or(&Value::Null), scope);
    let right = || evaluate(expression.get("right").unwrap_or(&Value::Null), scope);
    match kind {
        "literal" => expression.get("value").cloned().unwrap_or(Value::Null),
        "identifier" => scope
            .get(
                expression
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            )
            .cloned()
            .unwrap_or(Value::Null),
        "member" => {
            let object = evaluate(expression.get("object").unwrap_or(&Value::Null), scope);
            let property = expression
                .get("property")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if property == "length" {
                if let Some(values) = object.as_array() {
                    number(values.len() as f64)
                } else if let Some(value) = object.as_str() {
                    number(value.chars().count() as f64)
                } else {
                    Value::Null
                }
            } else {
                object.get(property).cloned().unwrap_or(Value::Null)
            }
        }
        "conditional" => {
            if truthy(&evaluate(
                expression.get("test").unwrap_or(&Value::Null),
                scope,
            )) {
                evaluate(expression.get("consequent").unwrap_or(&Value::Null), scope)
            } else {
                evaluate(expression.get("alternate").unwrap_or(&Value::Null), scope)
            }
        }
        "logical" => {
            let value = left();
            match expression.get("op").and_then(Value::as_str) {
                Some("&&") => {
                    if truthy(&value) {
                        right()
                    } else {
                        value
                    }
                }
                Some("||") => {
                    if truthy(&value) {
                        value
                    } else {
                        right()
                    }
                }
                Some("??") => {
                    if value.is_null() {
                        right()
                    } else {
                        value
                    }
                }
                _ => Value::Null,
            }
        }
        "unary" => {
            let value = evaluate(expression.get("argument").unwrap_or(&Value::Null), scope);
            match expression.get("op").and_then(Value::as_str) {
                Some("!") => Value::Bool(!truthy(&value)),
                Some("-") => number(-value.as_f64().unwrap_or(0.0)),
                Some("+") => number(value.as_f64().unwrap_or(0.0)),
                _ => Value::Null,
            }
        }
        "binary" => {
            let a = left();
            let b = right();
            match expression.get("op").and_then(Value::as_str) {
                Some("+") => {
                    if a.is_string() || b.is_string() {
                        Value::String(format!(
                            "{}{}",
                            value_string(Some(&a)),
                            value_string(Some(&b))
                        ))
                    } else {
                        number(a.as_f64().unwrap_or(0.0) + b.as_f64().unwrap_or(0.0))
                    }
                }
                Some("-") => number(a.as_f64().unwrap_or(0.0) - b.as_f64().unwrap_or(0.0)),
                Some("*") => number(a.as_f64().unwrap_or(0.0) * b.as_f64().unwrap_or(0.0)),
                Some("/") => number(a.as_f64().unwrap_or(0.0) / b.as_f64().unwrap_or(0.0)),
                Some("%") => number(a.as_f64().unwrap_or(0.0) % b.as_f64().unwrap_or(0.0)),
                Some("!==") => Value::Bool(a != b),
                Some("==") | Some("===") => Value::Bool(a == b),
                Some("!=") => Value::Bool(a != b),
                Some(">") => Value::Bool(a.as_f64() > b.as_f64()),
                Some(">=") => Value::Bool(a.as_f64() >= b.as_f64()),
                Some("<") => Value::Bool(a.as_f64() < b.as_f64()),
                Some("<=") => Value::Bool(a.as_f64() <= b.as_f64()),
                _ => Value::Null,
            }
        }
        "template" => Value::String(
            expression
                .get("parts")
                .and_then(Value::as_array)
                .map(|parts| {
                    parts
                        .iter()
                        .map(|part| {
                            if part.is_string() {
                                part.as_str().unwrap_or_default().into()
                            } else {
                                value_string(Some(&evaluate(part, scope)))
                            }
                        })
                        .collect()
                })
                .unwrap_or_default(),
        ),
        "array" => Value::Array(
            expression
                .get("items")
                .and_then(Value::as_array)
                .map(|items| {
                    let mut values = Vec::new();
                    for item in items {
                        if item.get("kind").and_then(Value::as_str) == Some("spread") {
                            values.extend(
                                evaluate(item.get("value").unwrap_or(&Value::Null), scope)
                                    .as_array()
                                    .cloned()
                                    .unwrap_or_default(),
                            );
                        } else {
                            values.push(evaluate(item, scope));
                        }
                    }
                    values
                })
                .unwrap_or_default(),
        ),
        "object" => {
            let mut result = serde_json::Map::new();
            let properties = expression
                .get("properties")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_else(|| {
                    expression
                        .get("entries")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default()
                });
            for property in properties {
                match property.get("kind").and_then(Value::as_str) {
                    Some("spread") => {
                        if let Some(object) =
                            evaluate(property.get("value").unwrap_or(&Value::Null), scope)
                                .as_object()
                        {
                            for (key, value) in object {
                                result.insert(key.clone(), value.clone());
                            }
                        }
                    }
                    Some("entry") => {
                        if let Some(key) = property.get("key").and_then(Value::as_str) {
                            result.insert(
                                key.into(),
                                evaluate(property.get("value").unwrap_or(&Value::Null), scope),
                            );
                        }
                    }
                    _ => {
                        if let Some(key) = property.get("key").and_then(Value::as_str) {
                            result.insert(
                                key.into(),
                                evaluate(property.get("value").unwrap_or(&Value::Null), scope),
                            );
                        }
                    }
                }
            }
            Value::Object(result)
        }
        "intrinsic" => {
            if expression.get("name").and_then(Value::as_str) == Some("encodeURIComponent") {
                return Value::String(
                    js_sys::encode_uri_component(&value_string(
                        expression
                            .get("args")
                            .and_then(Value::as_array)
                            .and_then(|args| args.first())
                            .map(|arg| evaluate(arg, scope))
                            .as_ref(),
                    ))
                    .into(),
                );
            }
            Value::String(
                expression
                    .get("args")
                    .and_then(Value::as_array)
                    .map(|args| {
                        args.iter()
                            .map(|arg| evaluate(arg, scope))
                            .filter(|value| truthy(value))
                            .map(|value| value_string(Some(&value)))
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .unwrap_or_default(),
            )
        }
        "method" => {
            let receiver = evaluate(expression.get("receiver").unwrap_or(&Value::Null), scope);
            let args = expression
                .get("args")
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .map(|value| evaluate(value, scope))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            match expression.get("name").and_then(Value::as_str) {
                Some("trim") => Value::String(value_string(Some(&receiver)).trim().into()),
                Some("toLowerCase") => Value::String(value_string(Some(&receiver)).to_lowercase()),
                Some("toUpperCase") => Value::String(value_string(Some(&receiver)).to_uppercase()),
                Some("includes") => Value::Bool(
                    args.first()
                        .map(|value| {
                            value_string(Some(&receiver)).contains(&value_string(Some(value)))
                        })
                        .unwrap_or(false),
                ),
                _ => Value::Null,
            }
        }
        "collection" => {
            let source = evaluate(expression.get("source").unwrap_or(&Value::Null), scope);
            let item_name = expression
                .get("itemName")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let index_name = expression.get("indexName").and_then(Value::as_str);
            let body = expression.get("expression").unwrap_or(&Value::Null);
            let values = source.as_array().cloned().unwrap_or_default();
            let mapped = values
                .into_iter()
                .enumerate()
                .filter_map(|(index, item)| {
                    let mut item_scope = scope.clone();
                    item_scope.insert(item_name.into(), item.clone());
                    if let Some(name) = index_name {
                        item_scope.insert(name.into(), number(index as f64));
                    }
                    let value = evaluate(body, &item_scope);
                    match expression.get("op").and_then(Value::as_str) {
                        Some("filter") if truthy(&value) => Some(item),
                        Some("map") => Some(value),
                        _ => None,
                    }
                })
                .collect();
            Value::Array(mapped)
        }
        _ => Value::Null,
    }
}

pub(crate) fn evaluate_with_context(
    expression: &Value,
    scope: &HashMap<String, Value>,
    environment: &HashMap<String, Value>,
) -> Value {
    fn replace(value: &Value, environment: &HashMap<String, Value>) -> Value {
        if value.get("kind").and_then(Value::as_str) == Some("context") {
            return serde_json::json!({"kind":"literal","value":environment.get(value.get("contextId").and_then(Value::as_str).unwrap_or_default()).cloned().unwrap_or(Value::Null)});
        }
        match value {
            Value::Array(values) => {
                Value::Array(values.iter().map(|v| replace(v, environment)).collect())
            }
            Value::Object(values) => Value::Object(
                values
                    .iter()
                    .map(|(k, v)| (k.clone(), replace(v, environment)))
                    .collect(),
            ),
            _ => value.clone(),
        }
    }
    evaluate(&replace(expression, environment), scope)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn evaluates_ordered_record_spreads_and_all_value_primitives() {
        let expression = serde_json::json!({"kind":"object","properties":[
         {"kind":"entry","key":"title","value":{"kind":"template","parts":["Todo: ",{"kind":"member","object":{"kind":"identifier","name":"todo"},"property":"title"}]}},
         {"kind":"spread","value":{"kind":"identifier","name":"todo"}},
         {"kind":"entry","key":"open","value":{"kind":"unary","op":"!","argument":{"kind":"member","object":{"kind":"identifier","name":"todo"},"property":"done"}}}
        ]});
        let scope = HashMap::from([(
            "todo".into(),
            serde_json::json!({"title":"Overridden","done":false,"rank":2}),
        )]);
        assert_eq!(
            evaluate(&expression, &scope),
            serde_json::json!({"title":"Overridden","done":false,"rank":2,"open":true})
        );
    }

    #[test]
    fn evaluates_collection_predicates_with_named_row_scopes() {
        let expression = serde_json::json!({"kind":"collection","op":"filter","source":{"kind":"identifier","name":"todos"},"itemName":"todo","expression":{"kind":"method","name":"includes","receiver":{"kind":"method","name":"toLowerCase","receiver":{"kind":"member","object":{"kind":"identifier","name":"todo"},"property":"title"}},"args":[{"kind":"method","name":"trim","receiver":{"kind":"identifier","name":"search"}}]}});
        let scope = HashMap::from([
            (
                "todos".into(),
                serde_json::json!([{"title":"Ship Plec"},{"title":"Write docs"}]),
            ),
            ("search".into(), serde_json::json!("ship")),
        ]);
        assert_eq!(
            evaluate(&expression, &scope),
            serde_json::json!([{"title":"Ship Plec"}])
        );
    }

    #[test]
    fn resolves_context_values_from_the_nearest_environment() {
        let expression = serde_json::json!({"kind":"member","object":{"kind":"context","contextId":"theme"},"property":"tone"});
        let outer = HashMap::from([("theme".into(), serde_json::json!({"tone":"outer"}))]);
        let inner = HashMap::from([("theme".into(), serde_json::json!({"tone":"inner"}))]);
        assert_eq!(
            evaluate_with_context(&expression, &HashMap::new(), &outer),
            serde_json::json!("outer")
        );
        assert_eq!(
            evaluate_with_context(&expression, &HashMap::new(), &inner),
            serde_json::json!("inner")
        );
    }
}
