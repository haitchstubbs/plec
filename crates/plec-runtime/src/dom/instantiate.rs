use crate::dom::{bindings::*, properties::*};
use crate::eval::{expression::*, value::*};
use crate::runtime::lifecycle::*;

pub(crate) fn instantiate(
    doc: &Document,
    id: &str,
    elements: &HashMap<String, ElementNode>,
    texts: &HashMap<String, TextNode>,
    contexts: &HashMap<String, ContextScope>,
    loops: &HashMap<String, Loop>,
    conditionals: &HashMap<String, Conditional>,
    bindings: &[Binding],
    prop_programs: &[PropProgram],
    expressions: &[Expression],
    events: &[EventBinding],
    scope: &HashMap<String, Value>,
    environment: &HashMap<String, Value>,
    nodes: &mut HashMap<String, Node>,
) -> Result<Node, JsValue> {
    if let Some(conditional) = conditionals.get(id) {
        let fragment: Node = doc.create_document_fragment().into();
        let selected_true = expressions
            .iter()
            .find(|entry| entry.id == conditional.expression_id)
            .map(|entry| evaluate_with_context(&entry.expression, scope, environment))
            .filter(truthy)
            .is_some();
        let start: Node = doc
            .create_comment(&format!(
                "plec:conditional:{}:{}",
                id,
                if selected_true { 1 } else { 0 }
            ))
            .into();
        let end: Node = doc
            .create_comment(&format!("plec:conditional-end:{}", id))
            .into();
        fragment.append_child(&start)?;
        let selected = if selected_true {
            &conditional.consequent
        } else {
            &conditional.alternate
        };
        for child_id in selected {
            let child = instantiate(
                doc,
                child_id,
                elements,
                texts,
                contexts,
                loops,
                conditionals,
                bindings,
                prop_programs,
                expressions,
                events,
                scope,
                environment,
                nodes,
            )?;
            fragment.append_child(&child)?;
        }
        fragment.append_child(&end)?;
        nodes.insert(id.into(), start);
        return Ok(fragment);
    }
    if let Some(provider) = contexts.get(id) {
        let fragment: Node = doc.create_document_fragment().into();
        let mut child_environment = environment.clone();
        let value = expression_value_with_context(
            &provider.value_expression_id,
            expressions,
            scope,
            environment,
        );
        child_environment.insert(provider.context_id.clone(), value);
        for child in &provider.children {
            let child = instantiate(
                doc,
                child,
                elements,
                texts,
                contexts,
                loops,
                conditionals,
                bindings,
                prop_programs,
                expressions,
                events,
                scope,
                &child_environment,
                nodes,
            )?;
            fragment.append_child(&child)?;
        }
        return Ok(fragment);
    }
    if let Some(loop_node) = loops.get(id) {
        if loop_node.query_id.is_some() || loop_node.input_id.is_some() {
            return Ok(doc.create_text_node("").into());
        }
        let fragment: Node = doc.create_document_fragment().into();
        for row in &loop_node.rows {
            let child = instantiate(
                doc,
                &row.root_element_id,
                elements,
                texts,
                contexts,
                loops,
                conditionals,
                bindings,
                prop_programs,
                expressions,
                events,
                scope,
                environment,
                nodes,
            )?;
            fragment.append_child(&child)?;
        }
        return Ok(fragment);
    }
    if let Some(text) = texts.get(id) {
        let node: Node = doc
            .create_text_node(text.static_value.as_deref().unwrap_or(""))
            .into();
        if let Some(binding) = bindings.iter().find(|entry| entry.target_id == id) {
            apply_binding_with_context(&node, binding, expressions, scope, environment)?;
        }
        nodes.insert(id.into(), node.clone());
        return Ok(node);
    }
    let element = elements
        .get(id)
        .ok_or_else(|| JsValue::from_str("node missing"))?;
    // SVG descendants must share the SVG namespace; HTML-created `path` and
    // `svg` nodes do not paint even though their attributes are present.
    let svg_tags = [
        "svg", "path", "circle", "rect", "line", "polyline", "polygon", "ellipse", "g",
    ];
    let node = if svg_tags.contains(&element.tag.as_str()) {
        doc.create_element_ns(Some("http://www.w3.org/2000/svg"), &element.tag)?
    } else {
        doc.create_element(&element.tag)?
    };
    node.set_attribute("data-runtime-node", id)?;
    for attribute in &element.attributes {
        if let Some(value) = &attribute.static_value {
            set_value(&node, &attribute.name, &Value::String(value.clone()))?;
        }
    }
    for binding in bindings.iter().filter(|entry| entry.target_id == id) {
        apply_binding_with_context(
            &node.clone().into(),
            binding,
            expressions,
            scope,
            environment,
        )?;
    }
    for program in prop_programs.iter().filter(|entry| entry.target_id == id) {
        apply_prop_program(&node.clone().into(), program, expressions, scope)?;
    }
    for event in events.iter().filter(|entry| entry.target_id == id) {
        node.set_attribute("data-runtime-action", &event.action_id)?;
        node.set_attribute("data-runtime-event", &event.event_type)?;
        if let Some(field) = &event.field {
            node.set_attribute("data-runtime-field", field)?;
        }
    }
    for child in &element.children {
        let child = instantiate(
            doc,
            child,
            elements,
            texts,
            contexts,
            loops,
            conditionals,
            bindings,
            prop_programs,
            expressions,
            events,
            scope,
            environment,
            nodes,
        )?;
        node.append_child(&child)?;
    }
    let node: Node = node.into();
    nodes.insert(id.into(), node.clone());
    Ok(node)
}
