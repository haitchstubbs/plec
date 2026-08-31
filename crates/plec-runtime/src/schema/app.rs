use serde::Deserialize;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Application {
    pub version: String,
    pub root_element_id: String,
    #[serde(default)]
    pub revision: Option<String>,
    pub elements: Vec<ElementNode>,
    pub texts: Vec<TextNode>,
    #[serde(default)]
    pub bindings: Vec<Binding>,
    #[serde(default)]
    pub expressions: Vec<Expression>,
    #[serde(default)]
    pub prop_programs: Vec<PropProgram>,
    #[serde(default)]
    pub events: Vec<EventBinding>,
    #[serde(default)]
    pub loops: Vec<Loop>,
    #[serde(default)]
    pub conditionals: Vec<Conditional>,
    #[serde(default)]
    pub contexts: Vec<ContextScope>,
    #[serde(default)]
    pub context_definitions: Vec<ContextDefinition>,
    #[serde(default)]
    pub host_element_refs: Vec<HostElementRef>,
    #[serde(default)]
    pub actions: Vec<Action>,
    #[serde(default)]
    pub local_states: Vec<LocalState>,
    #[serde(default)]
    pub layout: LayoutMetadata,
}

#[derive(Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LayoutMetadata {
    pub route_outlets: Vec<RouteOutlet>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteOutlet {
    pub id: String,
    pub element_id: String,
}

#[derive(Clone, Deserialize)]
pub struct Action {
    pub id: String,
    #[serde(default)]
    pub operations: Vec<serde_json::Value>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalState {
    pub id: String,
    pub name: String,
    pub initial_value: String,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ElementNode {
    pub id: String,
    pub tag: String,
    #[serde(default)]
    pub attributes: Vec<Attribute>,
    #[serde(default)]
    pub children: Vec<String>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attribute {
    pub name: String,
    pub static_value: Option<String>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextNode {
    pub id: String,
    pub static_value: Option<String>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Binding {
    #[serde(rename = "id")]
    pub _id: String,
    pub kind: String,
    pub target_id: String,
    pub attribute_name: Option<String>,
    pub expression_id: Option<String>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PropProgram {
    pub target_id: String,
    pub writes: Vec<PropWrite>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PropWrite {
    pub name: String,
    pub static_value: Option<String>,
    pub expression_id: Option<String>,
    pub kind: String,
}

#[derive(Clone, Deserialize)]
pub struct Expression {
    pub id: String,
    pub expression: serde_json::Value,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventBinding {
    #[serde(rename = "type")]
    pub event_type: String,
    pub target_id: String,
    pub action_id: String,
    pub field: Option<String>,
    pub loop_id: Option<String>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Loop {
    #[serde(rename = "id")]
    pub id: String,
    pub parent_id: String,
    pub input_id: Option<String>,
    pub query_id: Option<String>,
    #[serde(default = "default_row_item_name")]
    pub item_name: String,
    pub row_template_root_element_id: Option<String>,
    #[serde(default)]
    pub rows: Vec<LoopRow>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoopRow {
    pub root_element_id: String,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conditional {
    pub id: String,
    pub parent_id: String,
    pub expression_id: String,
    #[serde(default)]
    pub consequent: Vec<String>,
    #[serde(default)]
    pub alternate: Vec<String>,
}

pub fn default_row_item_name() -> String {
    "todo".into()
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextScope {
    pub id: String,
    pub context_id: String,
    pub value_expression_id: Option<String>,
    #[serde(default)]
    pub children: Vec<String>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextDefinition {
    pub id: String,
    pub default_expression_id: String,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostElementRef {
    pub id: String,
    pub target_id: String,
    #[serde(default)]
    pub attachments: Vec<String>,
}
