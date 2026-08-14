use crate::eval::expression::*;
use crate::runtime::lifecycle::*;

pub(crate) fn index_elements(app: &Application) -> HashMap<String, ElementNode> {
    app.elements
        .iter()
        .cloned()
        .map(|node| (node.id.clone(), node))
        .collect()
}

pub(crate) fn index_texts(app: &Application) -> HashMap<String, TextNode> {
    app.texts
        .iter()
        .cloned()
        .map(|node| (node.id.clone(), node))
        .collect()
}

pub(crate) fn index_contexts(app: &Application) -> HashMap<String, ContextScope> {
    app.contexts
        .iter()
        .cloned()
        .map(|node| (node.id.clone(), node))
        .collect()
}

pub(crate) fn index_loops(app: &Application) -> HashMap<String, Loop> {
    app.loops
        .iter()
        .cloned()
        .map(|node| (node.id.clone(), node))
        .collect()
}

pub(crate) fn index_conditionals(app: &Application) -> HashMap<String, Conditional> {
    app.conditionals
        .iter()
        .cloned()
        .map(|node| (node.id.clone(), node))
        .collect()
}

/** Node ids owned by a conditional branch. Removing these registrations before
 * mounting the opposite branch prevents listeners/bindings from retaining
 * detached DOM targets. */
pub(crate) fn conditional_branch_node_ids(
    app: &Application,
    conditional: &Conditional,
) -> HashSet<String> {
    fn visit(app: &Application, id: &str, output: &mut HashSet<String>) {
        if !output.insert(id.into()) {
            return;
        }
        if let Some(element) = app.elements.iter().find(|entry| entry.id == id) {
            for child in &element.children {
                visit(app, child, output);
            }
        }
        if let Some(conditional) = app.conditionals.iter().find(|entry| entry.id == id) {
            for child in conditional
                .consequent
                .iter()
                .chain(conditional.alternate.iter())
            {
                visit(app, child, output);
            }
        }
    }
    let mut output = HashSet::new();
    for child in conditional
        .consequent
        .iter()
        .chain(conditional.alternate.iter())
    {
        visit(app, child, &mut output);
    }
    output
}

pub(crate) fn context_defaults(app: &Application) -> HashMap<String, Value> {
    app.context_definitions
        .iter()
        .map(|definition| {
            (
                definition.id.clone(),
                expression_value(
                    &Some(definition.default_expression_id.clone()),
                    &app.expressions,
                    &HashMap::new(),
                ),
            )
        })
        .collect()
}

pub(crate) fn find_loop(app: &Application, input: &str) -> Result<Loop, JsValue> {
    app.loops
        .iter()
        .find(|entry| {
            entry.input_id.as_deref() == Some(input) || entry.query_id.as_deref() == Some(input)
        })
        .cloned()
        .ok_or_else(|| JsValue::from_str("loop missing"))
}

pub(crate) fn row_scope(item_name: &str, row: &HashMap<String, Value>) -> HashMap<String, Value> {
    let value = Value::Object(
        row.iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    );
    HashMap::from([(item_name.into(), value)])
}
