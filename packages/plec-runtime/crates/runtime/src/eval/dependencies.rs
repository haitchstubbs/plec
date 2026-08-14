use crate::runtime::lifecycle::*;

pub(crate) fn program_dependencies(
    program: &PropProgram,
    expressions: &HashMap<&str, &Value>,
) -> HashSet<String> {
    let mut fields = HashSet::new();
    for write in &program.writes {
        if let Some(id) = write.expression_id.as_deref() {
            if let Some(expression) = expressions.get(id) {
                collect_dependencies(expression, &mut fields);
            }
        }
    }
    fields
}

pub(crate) fn binding_dependencies(
    binding: &Binding,
    expressions: &HashMap<&str, &Value>,
) -> HashSet<String> {
    let mut fields = HashSet::new();
    if let Some(expression_id) = binding.expression_id.as_deref() {
        if let Some(expression) = expressions.get(expression_id) {
            collect_dependencies(expression, &mut fields);
        }
    }
    fields
}

pub(crate) fn collect_dependencies(expression: &Value, fields: &mut HashSet<String>) {
    let Some(kind) = expression.get("kind").and_then(Value::as_str) else {
        return;
    };
    match kind {
        "member" => {
            let object = expression.get("object").unwrap_or(&Value::Null);
            if object.get("kind").and_then(Value::as_str) == Some("identifier")
                && object.get("name").and_then(Value::as_str) == Some("todo")
            {
                if let Some(property) = expression.get("property").and_then(Value::as_str) {
                    fields.insert(property.into());
                }
            } else {
                collect_dependencies(object, fields);
            }
        }
        "identifier" => {
            if let Some(name) = expression.get("name").and_then(Value::as_str) {
                if name != "todo" {
                    fields.insert(name.into());
                }
            }
        }
        "unary" => collect_dependencies(expression.get("argument").unwrap_or(&Value::Null), fields),
        "conditional" => {
            for key in ["test", "consequent", "alternate"] {
                collect_dependencies(expression.get(key).unwrap_or(&Value::Null), fields);
            }
        }
        "binary" | "logical" => {
            for key in ["left", "right"] {
                collect_dependencies(expression.get(key).unwrap_or(&Value::Null), fields);
            }
        }
        "template" => {
            for part in expression
                .get("parts")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                collect_dependencies(part, fields);
            }
        }
        "array" | "intrinsic" => {
            for item in expression
                .get("items")
                .or_else(|| expression.get("args"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                collect_dependencies(item, fields);
            }
        }
        "object" => {
            for entry in expression
                .get("entries")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                collect_dependencies(entry.get("value").unwrap_or(&Value::Null), fields);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn collects_only_the_row_fields_used_by_an_expression() {
        let expression = serde_json::json!({"kind":"conditional","test":{"kind":"member","object":{"kind":"identifier","name":"todo"},"property":"done"},"consequent":{"kind":"member","object":{"kind":"identifier","name":"todo"},"property":"title"},"alternate":{"kind":"literal","value":"Open"}});
        let mut fields = HashSet::new();
        collect_dependencies(&expression, &mut fields);
        assert_eq!(
            fields,
            HashSet::from(["done".to_owned(), "title".to_owned()])
        );
    }
}
