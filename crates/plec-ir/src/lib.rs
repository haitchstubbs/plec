use serde::{Deserialize, Serialize};

pub const VERSION: &str = "0.10";
// The component graph schema is still in its 0.10 development window.  Keep
// additions in this contract until it is deliberately released.
pub const COMPONENT_VERSION: &str = "0.10";

/// Execution ownership and public exposure are intentionally separate. A
/// value may be serializable while still being server-only (for example a
/// session identifier); only an explicit PublicExport may cross the boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExecutionOwner { Shared, Server, Client }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicExport {
    pub name: String,
    pub source_owner: ExecutionOwner,
    pub value_is_serializable: bool,
    pub explicitly_public: bool,
}

pub fn validate_public_export(export: &PublicExport) -> Result<(), &'static str> {
    if export.source_owner == ExecutionOwner::Client { return Err("client values cannot be server exports"); }
    if !export.value_is_serializable { return Err("public export must be serializable"); }
    if !export.explicitly_public { return Err("server value requires an explicit public export boundary"); }
    Ok(())
}

/// A separately-versioned component application.  Component definitions keep
/// their local node/state handles; call nodes connect those local programs.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentApplication {
    pub version: &'static str,
    pub root_component: usize,
    pub components: Vec<ExecutableComponent>,
}

/// The router is deliberately a separate artifact: graphs keep local runtime
/// handles while this manifest owns the links between route instances.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteManifest {
    pub version: u32,
    #[serde(default)]
    pub revision: String,
    pub root_graph_id: String,
    pub routes: Vec<RouteManifestEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteManifestEntry {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub path: String,
    pub graph_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_graph_id: Option<String>,
    #[serde(
        skip_serializing_if = "is_replace_pending_mode",
        default = "default_pending_mode"
    )]
    pub pending_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_graph_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loader_action: Option<usize>,
    pub outlet_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<RouteMetadata>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RouteMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl RouteManifest {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 3 {
            return Err("unsupported route manifest version".into());
        }
        if self.root_graph_id.is_empty() {
            return Err("route manifest root graph id is required".into());
        }
        Ok(())
    }
}

fn is_replace_pending_mode(value: &String) -> bool {
    value == "replace"
}

fn default_pending_mode() -> String {
    "replace".into()
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutableComponent {
    pub id: String,
    pub root_node: usize,
    pub strings: Vec<String>,
    pub constants: Vec<Value>,
    pub nodes: Vec<Node>,
    pub texts: Vec<Text>,
    pub bindings: Vec<Binding>,
    pub prop_programs: Vec<PropProgram>,
    pub events: Vec<Event>,
    pub inputs: Vec<Input>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub host_slots: Vec<HostSlot>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub capabilities: Vec<CookieCapability>,
    pub state_slots: Vec<StateSlot>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub ref_slots: Vec<RefSlot>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub host_refs: Vec<HostRef>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub reactions: Vec<Reaction>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub listeners: Vec<Listener>,
    pub parameters: Vec<ComponentParameter>,
    pub expressions: Vec<ExpressionProgram>,
    pub actions: Vec<ActionProgram>,
    pub loops: Vec<Loop>,
    pub dependency_edges: Vec<DependencyEdge>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub route_outlets: Vec<RouteOutlet>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ComponentParameter {
    pub name: usize,
    pub callable: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub component: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ComponentProp {
    Value { name: usize, expression: usize },
    Callable { name: usize, action: usize },
    Component { name: usize, component: usize },
}

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
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub host_slots: Vec<HostSlot>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub capabilities: Vec<CookieCapability>,
    pub state_slots: Vec<StateSlot>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub ref_slots: Vec<RefSlot>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub host_refs: Vec<HostRef>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub reactions: Vec<Reaction>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub listeners: Vec<Listener>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub parameters: Vec<ComponentParameter>,
    pub expressions: Vec<ExpressionProgram>,
    pub actions: Vec<ActionProgram>,
    pub loops: Vec<Loop>,
    pub dependency_edges: Vec<DependencyEdge>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub route_outlets: Vec<RouteOutlet>,
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
            host_slots: vec![],
            capabilities: vec![],
            state_slots: vec![],
            ref_slots: vec![],
            host_refs: vec![],
            reactions: vec![],
            listeners: vec![],
            parameters: vec![],
            expressions: vec![],
            actions: vec![],
            loops: vec![],
            dependency_edges: vec![],
            route_outlets: vec![],
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
        #[serde(skip_serializing_if = "is_html_namespace")]
        namespace: &'static str,
        parent: Option<usize>,
        children: Vec<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        host_ref: Option<usize>,
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
    Component {
        component: usize,
        parent: Option<usize>,
        props: Vec<ComponentProp>,
        #[serde(skip_serializing_if = "Vec::is_empty", default)]
        children: Vec<usize>,
    },
    /// A component whose graph definition comes from a component-valued prop.
    DynamicComponent {
        prop: usize,
        parent: Option<usize>,
        props: Vec<ComponentProp>,
        #[serde(skip_serializing_if = "Vec::is_empty", default)]
        children: Vec<usize>,
    },
    /// Insertion range for the implicit `children` prop. The content belongs
    /// to the caller, not the component definition which declares this node.
    Slot {
        parent: Option<usize>,
    },
}

fn is_html_namespace(value: &&'static str) -> bool {
    *value == "html"
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<usize>,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub constant: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expression: Option<usize>,
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub spread: bool,
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
#[serde(rename_all = "camelCase")]
pub struct HostSlot {
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<usize>,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CookieCapability {
    pub kind: &'static str,
    pub name: String,
    pub operations: Vec<&'static str>,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub same_site: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secure: Option<bool>,
    pub expiry_modes: Vec<&'static str>,
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
#[serde(rename_all = "camelCase")]
pub struct RefSlot {
    pub initial_expression: usize,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HostRef {}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Reaction {
    pub dependencies: Vec<usize>,
    pub action: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cleanup_action: Option<usize>,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listener {
    pub source: &'static str,
    pub event: usize,
    pub action: usize,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExpressionProgram {
    pub instructions: Vec<ExpressionInstruction>,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum ExpressionInstruction {
    Constant {
        constant: usize,
    },
    LoadState {
        state: usize,
    },
    LoadRef {
        reference: usize,
    },
    LoadProp {
        prop: usize,
    },
    LoadFrame {
        slot: usize,
    },
    LoadHost {
        host: usize,
    },
    /// The whole serializable loop row. This is distinct from a field read so
    /// computed row access and row spreads retain their normal value-graph
    /// semantics.
    LoadRowRecord,
    LoadRowField {
        field: usize,
    },
    Field {
        field: usize,
    },
    Index,
    Unary {
        kind: &'static str,
    },
    Binary {
        kind: &'static str,
    },
    String {
        kind: &'static str,
        count: usize,
    },
    MakeArray {
        count: usize,
        #[serde(skip_serializing_if = "Vec::is_empty", default)]
        spreads: Vec<bool>,
    },
    MakeRecord {
        fields: Vec<usize>,
        #[serde(skip_serializing_if = "Vec::is_empty", default)]
        spreads: Vec<bool>,
    },
    OmitFields {
        fields: Vec<usize>,
    },
    Map {
        mapper: usize,
        item_slot: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        index_slot: Option<usize>,
    },
    Filter {
        predicate: usize,
        item_slot: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        index_slot: Option<usize>,
    },
    Jump {
        target: usize,
    },
    JumpIfFalse {
        target: usize,
    },
    Return,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionProgram {
    #[serde(skip_serializing_if = "is_zero", default)]
    pub frame_slots: usize,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub parameter_slots: Vec<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loader_result_state: Option<usize>,
    #[serde(skip_serializing_if = "is_false", default)]
    pub route_loader: bool,
    pub instructions: Vec<ActionInstruction>,
}

fn is_zero(value: &usize) -> bool {
    *value == 0
}

fn is_false(value: &bool) -> bool {
    !*value
}
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum ActionInstruction {
    Evaluate {
        expression: usize,
    },
    StoreState {
        state: usize,
    },
    StoreFrame {
        slot: usize,
    },
    StoreRef {
        reference: usize,
    },
    CaptureActiveElement {
        reference: usize,
    },
    FocusHostRef {
        reference: usize,
    },
    FocusRef {
        reference: usize,
    },
    PreventDefault,
    CallProp {
        prop: usize,
        #[serde(skip_serializing_if = "Vec::is_empty", default)]
        arguments: Vec<usize>,
    },
    CallPropOptional {
        prop: usize,
        #[serde(skip_serializing_if = "Vec::is_empty", default)]
        arguments: Vec<usize>,
    },
    CollectionMutation {
        input: usize,
        kind: &'static str,
        key: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        value: Option<usize>,
    },
    CapabilityRequest {
        #[serde(flatten)]
        request: CapabilityRequest,
        #[serde(rename = "successPc")]
        success_pc: usize,
        #[serde(rename = "failurePc")]
        failure_pc: usize,
        #[serde(rename = "finallyPc", skip_serializing_if = "Option::is_none")]
        finally_pc: Option<usize>,
        #[serde(rename = "resultSlot")]
        result_slot: usize,
        #[serde(rename = "errorSlot")]
        error_slot: usize,
    },
    Call {
        action: usize,
        #[serde(skip_serializing_if = "Vec::is_empty", default)]
        arguments: Vec<usize>,
        #[serde(rename = "successPc", skip_serializing_if = "Option::is_none")]
        success_pc: Option<usize>,
        #[serde(rename = "failurePc", skip_serializing_if = "Option::is_none")]
        failure_pc: Option<usize>,
        #[serde(rename = "resultSlot", skip_serializing_if = "Option::is_none")]
        result_slot: Option<usize>,
        #[serde(rename = "errorSlot", skip_serializing_if = "Option::is_none")]
        error_slot: Option<usize>,
    },
    CallFrame {
        parameter: usize,
        #[serde(skip_serializing_if = "Vec::is_empty", default)]
        arguments: Vec<usize>,
        #[serde(rename = "successPc", skip_serializing_if = "Option::is_none")]
        success_pc: Option<usize>,
        #[serde(rename = "failurePc", skip_serializing_if = "Option::is_none")]
        failure_pc: Option<usize>,
        #[serde(rename = "resultSlot", skip_serializing_if = "Option::is_none")]
        result_slot: Option<usize>,
        #[serde(rename = "errorSlot", skip_serializing_if = "Option::is_none")]
        error_slot: Option<usize>,
    },
    Jump {
        target: usize,
    },
    JumpIfFalse {
        target: usize,
    },
    Return {
        #[serde(skip_serializing_if = "is_success", default)]
        outcome: ReturnOutcome,
        #[serde(skip_serializing_if = "Option::is_none")]
        value: Option<usize>,
    },
}

fn is_success(value: &ReturnOutcome) -> bool {
    matches!(value, ReturnOutcome::Success)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ReturnOutcome {
    Success,
    Failure,
}

impl Default for ReturnOutcome {
    fn default() -> Self {
        Self::Success
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "capability", content = "request", rename_all = "camelCase")]
pub enum CapabilityRequest {
    Fetch {
        url: usize,
        method: &'static str,
        #[serde(skip_serializing_if = "Vec::is_empty", default)]
        headers: Vec<FetchHeader>,
        #[serde(skip_serializing_if = "Option::is_none")]
        body: Option<usize>,
        decode: &'static str,
        #[serde(rename = "requireOk")]
        require_ok: bool,
    },
    Cookie {
        operation: &'static str,
        name: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        value: Option<usize>,
        path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        same_site: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        secure: Option<bool>,
        expiry: &'static str,
        #[serde(rename = "maxAge", skip_serializing_if = "Option::is_none")]
        max_age: Option<i64>,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FetchHeader {
    pub name: usize,
    pub value: usize,
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

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RouteOutlet {
    pub id: String,
    pub node: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_manifest_round_trips_the_active_transport_schema() {
        let manifest = RouteManifest {
            version: 3,
            revision: "test".into(),
            root_graph_id: "root".into(),
            routes: vec![RouteManifestEntry {
                id: "todos".into(),
                parent_id: None,
                path: "/todos".into(),
                graph_id: "todos-graph".into(),
                pending_graph_id: None,
                pending_mode: "replace".into(),
                error_graph_id: None,
                loader_action: None,
                outlet_id: "main".into(),
                meta: None,
            }],
        };
        let json = serde_json::to_value(&manifest).unwrap();
        assert_eq!(
            serde_json::from_value::<RouteManifest>(json).unwrap(),
            manifest
        );
    }

    #[test]
    fn serializable_server_values_still_require_explicit_public_exposure() {
        let hidden = PublicExport { name: "session".into(), source_owner: ExecutionOwner::Server, value_is_serializable: true, explicitly_public: false };
        assert_eq!(validate_public_export(&hidden), Err("server value requires an explicit public export boundary"));
        let public = PublicExport { explicitly_public: true, ..hidden };
        assert!(validate_public_export(&public).is_ok());
    }

    #[test]
    fn route_manifest_rejects_a_non_v3_version() {
        let value =
            serde_json::json!({"version":2,"revision":"x","rootGraphId":"root","routes":[]});
        assert!(serde_json::from_value::<RouteManifest>(value)
            .unwrap()
            .validate()
            .is_err());
    }
}
