use serde::Serialize;

pub const VERSION: &str = "0.9";

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutableApplication {
    pub version: &'static str,
    pub root_node: usize,
    pub strings: Vec<String>,
    pub constants: Vec<Value>,
    pub nodes: Vec<Node>,
    pub texts: Vec<Text>,
    pub bindings: Vec<Binding>,
    pub prop_programs: Vec<PropProgram>,
    pub events: Vec<Event>,
    pub inputs: Vec<Input>,
    pub state_slots: Vec<StateSlot>,
    pub expressions: Vec<ExpressionProgram>,
    pub actions: Vec<ActionProgram>,
    pub loops: Vec<Loop>,
    pub dependency_edges: Vec<DependencyEdge>,
}

impl Default for ExecutableApplication {
    fn default() -> Self {
        Self {
            version: VERSION,
            root_node: 0,
            strings: vec![],
            constants: vec![],
            nodes: vec![],
            texts: vec![],
            bindings: vec![],
            prop_programs: vec![],
            events: vec![],
            inputs: vec![],
            state_slots: vec![],
            expressions: vec![],
            actions: vec![],
            loops: vec![],
            dependency_edges: vec![],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    Record(std::collections::BTreeMap<String, Value>),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum Node {
    Element {
        tag: usize,
        parent: Option<usize>,
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
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Text {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binding: Option<usize>,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Binding {
    pub target: usize,
    pub sink: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<usize>,
    pub expression: usize,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PropProgram {
    pub target: usize,
    pub writes: Vec<PropWrite>,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PropWrite {
    pub name: usize,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub constant: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expression: Option<usize>,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Event {
    pub target: usize,
    #[serde(rename = "type")]
    pub event_type: usize,
    pub action: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#loop: Option<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub fields: Vec<EventField>,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Input {
    pub name: usize,
    pub kind: &'static str,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EventField {
    pub name: usize,
    pub slot: usize,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateSlot {
    pub initial_expression: usize,
    pub frame_slot: usize,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExpressionProgram {
    pub instructions: Vec<ExpressionInstruction>,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum ExpressionInstruction {
    Constant { constant: usize },
    LoadState { state: usize },
    LoadRowField { field: usize },
    Field { field: usize },
    Unary { kind: &'static str },
    Binary { kind: &'static str },
    String { kind: &'static str, count: usize },
    MakeArray { count: usize },
    MakeRecord { fields: Vec<usize> },
    Jump { target: usize },
    JumpIfFalse { target: usize },
    Return,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ActionProgram {
    pub instructions: Vec<ActionInstruction>,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum ActionInstruction {
    Evaluate { expression: usize },
    StoreState { state: usize },
    Jump { target: usize },
    JumpIfFalse { target: usize },
    Return,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Loop {
    pub source_expression: usize,
    pub key_expression: usize,
    pub item_slot: usize,
    pub row_template: usize,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub dependency_slots: Vec<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<usize>,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DependencyEdge {
    pub source: DependencyEndpoint,
    pub target: DependencyEndpoint,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DependencyEndpoint {
    pub kind: &'static str,
    pub handle: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#loop: Option<usize>,
}
