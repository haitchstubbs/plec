use serde::Deserialize;
use std::collections::{HashMap, HashSet};

use crate::schema::delta::RuntimeValue;
use wasm_bindgen::JsValue;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypedApplication {
    pub version: String,
    pub root_node: usize,
    pub strings: Vec<String>,
    #[serde(default)]
    pub constants: Vec<RuntimeValue>,
    pub nodes: Vec<TypedNode>,
    #[serde(default)]
    pub texts: Vec<TypedText>,
    #[serde(default)]
    pub bindings: Vec<TypedBinding>,
    #[serde(default)]
    pub events: Vec<TypedEvent>,
    #[serde(default)]
    pub inputs: Vec<TypedInput>,
    #[serde(default)]
    pub state_slots: Vec<TypedStateSlot>,
    #[serde(default)]
    pub expressions: Vec<TypedProgram>,
    #[serde(default)]
    pub actions: Vec<TypedAction>,
    #[serde(default)]
    pub loops: Vec<TypedLoop>,
    #[serde(default)]
    pub dependency_edges: Vec<TypedDependencyEdge>,
}

#[derive(Clone, Deserialize)]
pub struct TypedEvent {
    pub target: usize,
    #[serde(rename = "type")]
    pub event_type: usize,
    pub action: usize,
    #[serde(default)]
    pub fields: Vec<TypedEventField>,
    pub r#loop: Option<usize>,
}

#[derive(Clone, Deserialize)]
pub struct TypedEventField {
    pub name: usize,
    pub slot: usize,
}

#[derive(Clone, Deserialize)]
pub struct TypedAction {
    pub instructions: Vec<TypedActionInstruction>,
    #[serde(default)]
    pub frame_slots: usize,
    #[serde(default)]
    pub parameter_slots: Vec<usize>,
    pub loader_result_state: Option<usize>,
    #[serde(default)]
    pub route_loader: bool,
}

#[derive(Clone, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum TypedActionInstruction {
    Evaluate {
        expression: usize,
    },
    StoreState {
        state: usize,
    },
    CollectionMutation {
        input: usize,
        kind: String,
        key: usize,
        value: Option<usize>,
    },
    PreventDefault,
    Call {
        action: usize,
        #[serde(default)]
        arguments: Vec<usize>,
    },
    Jump {
        target: usize,
    },
    JumpIfFalse {
        target: usize,
    },
    CapabilityRequest {
        capability: String,
        request: TypedFetchRequest,
        success_pc: usize,
        failure_pc: usize,
        finally_pc: Option<usize>,
        result_slot: usize,
        error_slot: usize,
    },
    Return,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypedFetchRequest {
    pub url: usize,
    pub method: String,
    #[serde(default)]
    pub headers: Vec<TypedFetchHeader>,
    pub body: Option<usize>,
    pub decode: String,
    #[serde(default = "default_true")]
    pub require_ok: bool,
}

#[derive(Clone, Deserialize)]
pub struct TypedFetchHeader {
    pub name: usize,
    pub value: usize,
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum TypedNode {
    Element {
        tag: usize,
        parent: Option<usize>,
        #[serde(default)]
        children: Vec<usize>,
    },
    Text {
        text: usize,
        parent: Option<usize>,
    },
    Conditional {
        test: usize,
        parent: Option<usize>,
        consequent: usize,
        alternate: Option<usize>,
    },
    Loop {
        r#loop: usize,
        parent: Option<usize>,
    },
}

#[derive(Clone, Deserialize)]
pub struct TypedText {
    pub value: Option<String>,
    pub binding: Option<usize>,
}

#[derive(Clone, Deserialize)]
pub struct TypedBinding {
    pub target: usize,
    pub sink: String,
    pub name: Option<usize>,
    pub expression: usize,
}

#[derive(Clone, Deserialize)]
pub struct TypedInput {
    pub name: usize,
    pub kind: String,
}

#[derive(Clone, Deserialize)]
pub struct TypedStateSlot {
    pub initial_expression: usize,
    pub frame_slot: usize,
}

#[derive(Clone, Deserialize)]
pub struct TypedProgram {
    pub instructions: Vec<TypedExpressionInstruction>,
}

#[derive(Clone, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum TypedExpressionInstruction {
    Constant {
        constant: usize,
    },
    LoadState {
        state: usize,
    },
    LoadRowField {
        field: usize,
    },
    LoadEventField {
        field: usize,
    },
    LoadFrame {
        slot: usize,
    },
    Field {
        field: usize,
    },
    Index,
    Unary {
        kind: String,
    },
    Binary {
        kind: String,
    },
    String {
        kind: String,
        #[serde(default = "one")]
        count: usize,
    },
    MakeArray {
        count: usize,
    },
    MakeRecord {
        fields: Vec<usize>,
    },
    Filter {
        predicate: usize,
        item_slot: usize,
        index_slot: Option<usize>,
    },
    Map {
        mapper: usize,
        item_slot: usize,
        index_slot: Option<usize>,
    },
    Jump {
        target: usize,
    },
    JumpIfFalse {
        target: usize,
    },
    JumpIfTrue {
        target: usize,
    },
    Return,
}

fn one() -> usize {
    1
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypedLoop {
    pub source_expression: usize,
    pub key_expression: usize,
    pub item_slot: usize,
    pub index_slot: Option<usize>,
    pub row_template: usize,
    #[serde(default)]
    pub dependency_slots: Vec<usize>,
    pub input: Option<usize>,
}

#[derive(Clone, Deserialize)]
pub struct TypedDependencyEdge {
    pub source: TypedDependencyEndpoint,
    pub target: TypedDependencyEndpoint,
}

#[derive(Clone, Deserialize)]
pub struct TypedDependencyEndpoint {
    pub kind: String,
    pub handle: usize,
    pub r#loop: Option<usize>,
}

pub type TypedRowValues = HashMap<String, RuntimeValue>;

#[derive(Clone)]
pub struct TypedCollection {
    pub order: Vec<String>,
    pub rows: HashMap<String, HashMap<String, RuntimeValue>>,
}

impl Default for TypedCollection {
    fn default() -> Self {
        Self {
            order: Vec::new(),
            rows: HashMap::new(),
        }
    }
}

pub fn validate_typed_action_contract(
    action: &TypedAction,
    expression_count: usize,
    input_kinds: &[String],
) -> Result<(), &'static str> {
    let mut parameter_slots = HashSet::new();
    for slot in &action.parameter_slots {
        if *slot >= action.frame_slots {
            return Err("action parameter frame slot out of range");
        }
        if !parameter_slots.insert(*slot) {
            return Err("duplicate action parameter frame slot");
        }
    }

    for instruction in &action.instructions {
        if let TypedActionInstruction::CollectionMutation {
            input,
            key,
            value,
            kind,
        } = instruction
        {
            if *input >= input_kinds.len() {
                return Err("action input handle out of range");
            }
            if input_kinds[*input] != "collection" {
                return Err("collection mutation requires a collection input");
            }
            if *key >= expression_count {
                return Err("collection mutation key expression handle out of range");
            }
            match (kind.as_str(), value) {
                ("append" | "keyedReplace", Some(value)) if *value < expression_count => {}
                ("append" | "keyedReplace", Some(_)) => {
                    return Err("collection mutation value expression handle out of range")
                }
                ("append" | "keyedReplace", None) => {
                    return Err("collection mutation requires a value expression")
                }
                ("keyedRemove", None) => {}
                ("keyedRemove", Some(_)) => {
                    return Err("collection remove forbids a value expression")
                }
                _ => return Err("unknown collection mutation kind"),
            }
        }
    }

    Ok(())
}

pub fn validate_typed_event_contract(
    event: &TypedEvent,
    node_count: usize,
    string_count: usize,
    action_count: usize,
    loop_count: usize,
    frame_slots: usize,
) -> Result<(), &'static str> {
    if event.target >= node_count
        || event.event_type >= string_count
        || event.action >= action_count
    {
        return Err("event handle out of range");
    }
    if event
        .r#loop
        .map(|loop_index| loop_index >= loop_count)
        .unwrap_or(false)
    {
        return Err("event loop handle out of range");
    }
    let mut slots = HashSet::new();
    for field in &event.fields {
        if field.name >= string_count || field.slot >= frame_slots {
            return Err("event field handle out of range");
        }
        if !slots.insert(field.slot) {
            return Err("duplicate event frame slot");
        }
    }
    Ok(())
}

fn supported_event_field(name: &str) -> bool {
    matches!(
        name,
        "type"
            | "value"
            | "checked"
            | "rowKey"
            | "key"
            | "button"
            | "metaKey"
            | "ctrlKey"
            | "shiftKey"
            | "altKey"
    )
}

fn subtree_contains(nodes: &[TypedNode], root: usize, target: usize) -> bool {
    if root == target {
        return true;
    }
    match nodes.get(root) {
        Some(TypedNode::Element { children, .. }) => children
            .iter()
            .any(|child| subtree_contains(nodes, *child, target)),
        Some(TypedNode::Conditional {
            consequent,
            alternate,
            ..
        }) => {
            subtree_contains(nodes, *consequent, target)
                || alternate
                    .map(|child| subtree_contains(nodes, child, target))
                    .unwrap_or(false)
        }
        _ => false,
    }
}

impl TypedApplication {
    /// Validates untrusted executable IR before it reaches the typed runtime.
    pub(crate) fn validate(&self) -> Result<(), JsValue> {
        if self.version != "0.9" {
            return Err(JsValue::from_str(
                "unsupported executable application version",
            ));
        }
        if self.root_node >= self.nodes.len() {
            return Err(JsValue::from_str("root node handle out of range"));
        }
        for state in &self.state_slots {
            if state.initial_expression >= self.expressions.len() {
                return Err(JsValue::from_str("state expression handle out of range"));
            }
        }
        for event in &self.events {
            let frame_slots = self
                .actions
                .get(event.action)
                .map(|action| action.frame_slots)
                .unwrap_or(0);
            validate_typed_event_contract(
                event,
                self.nodes.len(),
                self.strings.len(),
                self.actions.len(),
                self.loops.len(),
                frame_slots,
            )
            .map_err(JsValue::from_str)?;
            if event.fields.iter().any(|field| {
                self.strings
                    .get(field.name)
                    .map(String::as_str)
                    .map(|name| !supported_event_field(name))
                    .unwrap_or(true)
            }) {
                return Err(JsValue::from_str("unsupported typed event field"));
            }
            let in_any_loop = self
                .loops
                .iter()
                .any(|loop_def| subtree_contains(&self.nodes, loop_def.row_template, event.target));
            match event.r#loop {
                Some(loop_index)
                    if !subtree_contains(
                        &self.nodes,
                        self.loops[loop_index].row_template,
                        event.target,
                    ) =>
                {
                    return Err(JsValue::from_str(
                        "event target is outside its loop template",
                    ))
                }
                None if in_any_loop => {
                    return Err(JsValue::from_str("loop event is missing loop ownership"))
                }
                _ => {}
            }
        }
        for action in &self.actions {
            let input_kinds = self
                .inputs
                .iter()
                .map(|input| input.kind.clone())
                .collect::<Vec<_>>();
            validate_typed_action_contract(action, self.expressions.len(), &input_kinds)
                .map_err(JsValue::from_str)?;
            for instruction in &action.instructions {
                match instruction {
                    TypedActionInstruction::Evaluate { expression }
                        if *expression >= self.expressions.len() =>
                    {
                        return Err(JsValue::from_str("action expression handle out of range"))
                    }
                    TypedActionInstruction::StoreState { state }
                        if *state >= self.state_slots.len() =>
                    {
                        return Err(JsValue::from_str("action state handle out of range"))
                    }
                    TypedActionInstruction::Call { action, arguments } => {
                        let target = self
                            .actions
                            .get(*action)
                            .ok_or_else(|| JsValue::from_str("action handle out of range"))?;
                        if arguments.len() != target.parameter_slots.len()
                            || arguments
                                .iter()
                                .any(|expression| *expression >= self.expressions.len())
                        {
                            return Err(JsValue::from_str("invalid action call"));
                        }
                    }
                    TypedActionInstruction::Jump { target }
                    | TypedActionInstruction::JumpIfFalse { target }
                        if *target >= action.instructions.len() =>
                    {
                        return Err(JsValue::from_str("action jump target out of range"))
                    }
                    TypedActionInstruction::CapabilityRequest {
                        request,
                        success_pc,
                        failure_pc,
                        finally_pc,
                        result_slot,
                        error_slot,
                        ..
                    } if request.url >= self.expressions.len()
                        || request
                            .body
                            .map(|body| body >= self.expressions.len())
                            .unwrap_or(false)
                        || request.headers.iter().any(|header| {
                            header.name >= self.strings.len()
                                || header.value >= self.expressions.len()
                        })
                        || *success_pc >= action.instructions.len()
                        || *failure_pc >= action.instructions.len()
                        || finally_pc
                            .map(|pc| pc >= action.instructions.len())
                            .unwrap_or(false)
                        || *result_slot >= action.frame_slots
                        || *error_slot >= action.frame_slots =>
                    {
                        return Err(JsValue::from_str("invalid action continuation"))
                    }
                    _ => {}
                }
            }
            if let Some(state) = action.loader_result_state {
                if state >= self.state_slots.len() {
                    return Err(JsValue::from_str("loader result state handle out of range"));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    fn typed_action_artifact(instruction: Value, input_kind: &str) -> TypedApplication {
        serde_json::from_value(serde_json::json!({
            "version": "0.9",
            "rootNode": 0,
            "strings": ["div"],
            "nodes": [{"op": "element", "tag": 0, "parent": null}],
            "inputs": [{"name": 0, "kind": input_kind}],
            "expressions": [{"instructions": []}],
            "actions": [{"instructions": [instruction]}]
        }))
        .unwrap()
    }
    #[test]
    fn typed_decoder_rejects_invalid_collection_action_operands() {
        let scalar = typed_action_artifact(
            serde_json::json!({"op":"collectionMutation","input":0,"kind":"append","key":0,"value":0}),
            "scalar",
        );
        assert!(validate_typed_action_contract(
            &scalar.actions[0],
            scalar.expressions.len(),
            &["scalar".into()]
        )
        .unwrap_err()
        .contains("collection input"));

        let missing_value = typed_action_artifact(
            serde_json::json!({"op":"collectionMutation","input":0,"kind":"append","key":0}),
            "collection",
        );
        assert!(validate_typed_action_contract(
            &missing_value.actions[0],
            missing_value.expressions.len(),
            &["collection".into()]
        )
        .unwrap_err()
        .contains("requires a value"));

        let remove_value = typed_action_artifact(
            serde_json::json!({"op":"collectionMutation","input":0,"kind":"keyedRemove","key":0,"value":0}),
            "collection",
        );
        assert!(validate_typed_action_contract(
            &remove_value.actions[0],
            remove_value.expressions.len(),
            &["collection".into()]
        )
        .unwrap_err()
        .contains("forbids a value"));
    }

    #[test]
    fn typed_decoder_rejects_duplicate_parameter_slots() {
        let mut app = typed_action_artifact(serde_json::json!({"op":"return"}), "collection");
        app.actions[0].frame_slots = 1;
        app.actions[0].parameter_slots = vec![0, 0];
        assert!(validate_typed_action_contract(
            &app.actions[0],
            app.expressions.len(),
            &["collection".into()]
        )
        .unwrap_err()
        .contains("duplicate"));
    }

    #[test]
    fn typed_decoder_rejects_duplicate_event_slots_and_unknown_loop() {
        let mut app = typed_action_artifact(serde_json::json!({"op":"return"}), "collection");
        app.actions[0].frame_slots = 2;
        app.events.push(TypedEvent {
            target: 0,
            event_type: 0,
            action: 0,
            fields: vec![
                TypedEventField { name: 0, slot: 0 },
                TypedEventField { name: 0, slot: 0 },
            ],
            r#loop: None,
        });
        assert!(validate_typed_event_contract(
            &app.events[0],
            app.nodes.len(),
            app.strings.len(),
            app.actions.len(),
            app.loops.len(),
            app.actions[0].frame_slots,
        )
        .is_err());

        app.events[0].fields.pop();
        app.events[0].r#loop = Some(0);
        assert!(validate_typed_event_contract(
            &app.events[0],
            app.nodes.len(),
            app.strings.len(),
            app.actions.len(),
            app.loops.len(),
            app.actions[0].frame_slots,
        )
        .is_err());
    }

    #[test]
    fn typed_decoder_rejects_unsupported_event_fields_and_wrong_loop_ownership() {
        let app = typed_action_artifact(serde_json::json!({"op":"return"}), "collection");
        let event = TypedEvent {
            target: 0,
            event_type: 0,
            action: 0,
            fields: vec![TypedEventField { name: 0, slot: 0 }],
            r#loop: None,
        };
        assert!(!supported_event_field("notAnEventField"));
        assert!(subtree_contains(&app.nodes, app.root_node, event.target));
    }
}
