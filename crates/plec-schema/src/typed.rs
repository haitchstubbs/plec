use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use wasm_bindgen::JsValue;

use super::delta::RuntimeValue;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypedApplication {
    #[serde(default = "component_version")]
    pub version: String,
    #[serde(default)]
    pub id: String,
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
    pub prop_programs: Vec<TypedPropProgram>,
    #[serde(default)]
    pub events: Vec<TypedEvent>,
    #[serde(default)]
    pub inputs: Vec<TypedInput>,
    #[serde(default)]
    pub state_slots: Vec<TypedStateSlot>,
    #[serde(default)]
    pub ref_slots: Vec<TypedRefSlot>,
    #[serde(default)]
    pub host_refs: Vec<TypedHostRef>,
    #[serde(default)]
    pub reactions: Vec<TypedReaction>,
    #[serde(default)]
    pub listeners: Vec<TypedGlobalListener>,
    #[serde(default)]
    pub parameters: Vec<TypedComponentParameter>,
    pub route_error_state: Option<usize>,
    #[serde(default)]
    pub expressions: Vec<TypedProgram>,
    #[serde(default)]
    pub actions: Vec<TypedAction>,
    #[serde(default)]
    pub loops: Vec<TypedLoop>,
    #[serde(default)]
    pub dependency_edges: Vec<TypedDependencyEdge>,
    #[serde(default)]
    pub route_outlets: Vec<TypedRouteOutlet>,
    #[serde(default)]
    pub host_slots: Vec<TypedHostSlot>,
    #[serde(default)]
    pub capabilities: Vec<TypedCookieCapability>,
    #[serde(skip)]
    pub host_inputs: HashMap<String, RuntimeValue>,
    #[serde(skip)]
    pub runtime_props: Vec<RuntimeValue>,
    #[serde(skip)]
    pub runtime_component_props: Vec<Option<usize>>,
    #[serde(skip)]
    pub runtime_host_component_props: Vec<Option<TypedHostComponentTarget>>,
    #[serde(skip)]
    pub ref_values: Vec<RuntimeValue>,
}

fn component_version() -> String {
    "0.10".into()
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypedComponentApplication {
    pub version: String,
    pub root_component: usize,
    pub components: Vec<TypedApplication>,
}

impl plec_ir::SsrStructureApplication for TypedComponentApplication {
    fn structure_graph(&self, graph_id: &str) -> Option<&dyn plec_ir::SsrStructureGraph> {
        let component = self
            .components
            .iter()
            .find(|component| component.id == graph_id)?;
        Some(component)
    }
}

impl plec_ir::SsrStructureGraph for TypedApplication {
    fn structure_node(&self, handle: usize) -> Option<plec_ir::SsrStructureNode> {
        Some(match self.nodes.get(handle)? {
            TypedNode::Conditional { alternate, .. } => plec_ir::SsrStructureNode::Conditional {
                has_alternate: alternate.is_some(),
            },
            TypedNode::Loop { .. } => plec_ir::SsrStructureNode::Loop,
            _ => plec_ir::SsrStructureNode::Other,
        })
    }
}

impl TypedComponentApplication {
    pub fn validate(&self) -> Result<(), JsValue> {
        self.validate_with_policy(&plec_ir::sink::TagPolicy::default())
    }

    /// Validates every component graph under an explicit element-tag policy.
    pub fn validate_with_policy(&self, policy: &plec_ir::sink::TagPolicy) -> Result<(), JsValue> {
        use crate::limits::{
            MAX_COMPONENT_COUNT, MAX_TOTAL_CONSTANT_NODES, MAX_TOTAL_INSTRUCTIONS,
            MAX_TOTAL_IR_ENTRIES, MAX_TOTAL_STRING_POOL_BYTES,
        };
        if self.version != "0.10"
            || self.root_component >= self.components.len()
            || self.components.len() > MAX_COMPONENT_COUNT
        {
            return Err(JsValue::from_str("invalid component application"));
        }
        for component in &self.components {
            component.validate_with_policy(policy)?;
            if component.id.is_empty() {
                return Err(JsValue::from_str("component graph id is required"));
            }
            let mut parameters = HashSet::new();
            for parameter in &component.parameters {
                let Some(name) = component.strings.get(parameter.name) else {
                    return Err(JsValue::from_str("component parameter handle out of range"));
                };
                if !parameters.insert(name) {
                    return Err(JsValue::from_str("duplicate component parameter"));
                }
            }
            for node in &component.nodes {
                if let TypedNode::Component {
                    component: target,
                    props,
                    children,
                    ..
                } = node
                {
                    let Some(target) = self.components.get(*target) else {
                        return Err(JsValue::from_str("component target out of range"));
                    };
                    validate_component_slot_target(target, children, component.nodes.len())
                        .map_err(JsValue::from_str)?;
                    let required = target
                        .parameters
                        .iter()
                        .filter(|parameter| {
                            target.strings.get(parameter.name).map(String::as_str)
                                != Some("children")
                        })
                        .count();
                    if props.len() != required
                        || children.iter().any(|child| *child >= component.nodes.len())
                    {
                        return Err(JsValue::from_str("missing component prop"));
                    }
                    let mut supplied = HashSet::new();
                    for prop in props {
                        let Some(name) = component.strings.get(prop.name()) else {
                            return Err(JsValue::from_str("component prop name out of range"));
                        };
                        let expected = target.parameters.iter().find(|parameter| {
                            target.strings.get(parameter.name).map(String::as_str)
                                == Some(name.as_str())
                        });
                        if expected.is_none()
                            || expected.is_some_and(|parameter| {
                                parameter.callable != prop.callable()
                                    || parameter.component
                                        != matches!(prop, TypedComponentProp::Component { .. })
                            })
                            || !prop.valid(component)
                            || !supplied.insert(name.clone())
                        {
                            return Err(JsValue::from_str("invalid component prop"));
                        }
                        if matches!(prop, TypedComponentProp::Component { component: target, .. } if *target >= self.components.len())
                        {
                            return Err(JsValue::from_str("component prop target out of range"));
                        }
                    }
                    if target
                        .parameters
                        .iter()
                        .filter(|parameter| {
                            target.strings.get(parameter.name).map(String::as_str)
                                != Some("children")
                        })
                        .any(|parameter| {
                            target
                                .strings
                                .get(parameter.name)
                                .map(String::as_str)
                                .map(|name| !supplied.contains(name))
                                .unwrap_or(true)
                        })
                    {
                        return Err(JsValue::from_str("missing component prop"));
                    }
                } else if let TypedNode::DynamicComponent {
                    prop,
                    props,
                    children,
                    ..
                } = node
                {
                    if !component
                        .parameters
                        .get(*prop)
                        .is_some_and(|parameter| parameter.component)
                        || children.iter().any(|child| *child >= component.nodes.len())
                        || props.iter().any(|prop| !prop.valid(component))
                    {
                        return Err(JsValue::from_str("invalid dynamic component"));
                    }
                }
            }
        }
        validate_component_call_graph_acyclic(self.components.iter().map(component_call_targets))?;
        let mut total_entries = 0usize;
        let mut total_instructions = 0usize;
        let mut total_string_bytes = 0usize;
        let mut total_constant_nodes = 0usize;
        for component in &self.components {
            total_entries = total_entries
                .checked_add(component_ir_entries(component))
                .ok_or_else(|| JsValue::from_str("application IR entry count overflow"))?;
            total_instructions = total_instructions
                .checked_add(
                    component
                        .expressions
                        .iter()
                        .map(|program| program.instructions.len())
                        .sum::<usize>(),
                )
                .and_then(|total| {
                    total.checked_add(
                        component
                            .actions
                            .iter()
                            .map(|action| action.instructions.len())
                            .sum::<usize>(),
                    )
                })
                .ok_or_else(|| JsValue::from_str("application instruction count overflow"))?;
            total_string_bytes = total_string_bytes
                .checked_add(component.strings.iter().map(String::len).sum::<usize>())
                .ok_or_else(|| JsValue::from_str("application string-pool bytes overflow"))?;
            total_constant_nodes = total_constant_nodes
                .checked_add(
                    component
                        .constants
                        .iter()
                        .map(runtime_value_nodes)
                        .sum::<usize>(),
                )
                .ok_or_else(|| JsValue::from_str("application constant node count overflow"))?;
        }
        if total_entries > MAX_TOTAL_IR_ENTRIES {
            return Err(JsValue::from_str("application IR entries exceed limit"));
        }
        if total_instructions > MAX_TOTAL_INSTRUCTIONS {
            return Err(JsValue::from_str("application instructions exceed limit"));
        }
        if total_string_bytes > MAX_TOTAL_STRING_POOL_BYTES {
            return Err(JsValue::from_str(
                "application string-pool bytes exceed limit",
            ));
        }
        if total_constant_nodes > MAX_TOTAL_CONSTANT_NODES {
            return Err(JsValue::from_str("application constant nodes exceed limit"));
        }
        Ok(())
    }
}

fn component_ir_entries(component: &TypedApplication) -> usize {
    component.strings.len()
        + component.constants.len()
        + component.nodes.len()
        + component.texts.len()
        + component.bindings.len()
        + component.prop_programs.len()
        + component.events.len()
        + component.inputs.len()
        + component.state_slots.len()
        + component.ref_slots.len()
        + component.host_refs.len()
        + component.reactions.len()
        + component.listeners.len()
        + component.parameters.len()
        + component.expressions.len()
        + component.actions.len()
        + component.loops.len()
        + component.dependency_edges.len()
        + component.route_outlets.len()
        + component.host_slots.len()
        + component.capabilities.len()
}

fn runtime_value_nodes(value: &RuntimeValue) -> usize {
    let mut nodes = 0usize;
    let mut stack = vec![value];
    while let Some(value) = stack.pop() {
        nodes += 1;
        match value {
            RuntimeValue::Array(values) => stack.extend(values),
            RuntimeValue::Record(values) => stack.extend(values.values()),
            RuntimeValue::Null
            | RuntimeValue::Bool(_)
            | RuntimeValue::Number(_)
            | RuntimeValue::String(_) => {}
        }
    }
    nodes
}

fn validate_component_slot_target(
    target: &TypedApplication,
    children: &[usize],
    caller_node_count: usize,
) -> Result<(), &'static str> {
    let slots = target
        .nodes
        .iter()
        .filter(|node| matches!(node, TypedNode::Slot { .. }))
        .count();
    if slots > 1 || (!children.is_empty() && slots != 1) {
        return Err("invalid component slot");
    }
    if children.iter().any(|child| *child >= caller_node_count) {
        return Err("component child out of range");
    }
    Ok(())
}

/// Every static component-call edge leaving one component graph: direct
/// `Component` nodes plus component-valued props. Host-resolved component
/// props (`host` target) resolve outside the component table at runtime and
/// carry no static graph edge — the compiler excludes them from its
/// recursion check the same way. Dynamic component targets resolve from
/// runtime prop values, so no static edge exists to validate for them;
/// their instantiation is bounded by the mounted-region budget.
fn component_call_targets(component: &TypedApplication) -> Vec<usize> {
    let mut targets = Vec::new();
    for node in &component.nodes {
        if let TypedNode::Component {
            component: target,
            props,
            ..
        } = node
        {
            targets.push(*target);
            for prop in props {
                if let TypedComponentProp::Component {
                    component: target,
                    host: None,
                    ..
                } = prop
                {
                    targets.push(*target);
                }
            }
        }
    }
    targets
}

/// The component-call graph must be acyclic: a reference cycle would mount
/// fresh component instances forever (the runtime only stops at the
/// mounted-region budget, long after wasted work), and every recursive
/// traversal over the component table assumes this shape. Kahn's peeling
/// keeps the check iterative — the graph may hold up to `MAX_COMPONENT_COUNT`
/// nodes, too deep for native recursion.
fn validate_component_call_graph_acyclic<I>(targets_per_component: I) -> Result<(), &'static str>
where
    I: IntoIterator<Item = Vec<usize>>,
{
    let edges: Vec<Vec<usize>> = targets_per_component.into_iter().collect();
    let mut in_degree = vec![0usize; edges.len()];
    for targets in &edges {
        for target in targets {
            in_degree[*target] += 1;
        }
    }
    let mut queue: Vec<usize> = (0..edges.len())
        .filter(|index| in_degree[*index] == 0)
        .collect();
    let mut cursor = 0usize;
    while let Some(index) = queue.get(cursor).copied() {
        cursor += 1;
        for target in &edges[index] {
            in_degree[*target] -= 1;
            if in_degree[*target] == 0 {
                queue.push(*target);
            }
        }
    }
    if cursor < edges.len() {
        return Err("component call graph contains a cycle");
    }
    Ok(())
}

#[derive(Clone, Deserialize)]
pub struct TypedComponentParameter {
    pub name: usize,
    pub callable: bool,
    #[serde(default)]
    pub component: bool,
}

#[derive(Clone)]
pub enum TypedComponentProp {
    Value {
        name: usize,
        expression: usize,
    },
    Callable {
        name: usize,
        action: usize,
    },
    Component {
        name: usize,
        component: usize,
        host: Option<TypedHostComponentTarget>,
    },
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypedHostComponentTarget {
    pub provider: String,
    pub component: String,
}

impl<'de> Deserialize<'de> for TypedComponentProp {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            kind: String,
            name: usize,
            expression: Option<usize>,
            action: Option<usize>,
            component: Option<usize>,
            host: Option<TypedHostComponentTarget>,
        }
        let raw = Raw::deserialize(deserializer)?;
        match (
            raw.kind.as_str(),
            raw.expression,
            raw.action,
            raw.component,
            raw.host,
        ) {
            ("value", Some(expression), None, None, None) => Ok(Self::Value {
                name: raw.name,
                expression,
            }),
            ("callable", None, Some(action), None, None) => Ok(Self::Callable {
                name: raw.name,
                action,
            }),
            ("component", None, None, Some(component), host) => Ok(Self::Component {
                name: raw.name,
                component,
                host,
            }),
            _ => Err(serde::de::Error::custom("invalid component prop")),
        }
    }
}

impl TypedComponentProp {
    pub fn name(&self) -> usize {
        match self {
            Self::Value { name, .. }
            | Self::Callable { name, .. }
            | Self::Component { name, .. } => *name,
        }
    }
    pub fn callable(&self) -> bool {
        matches!(self, Self::Callable { .. })
    }
    fn valid(&self, app: &TypedApplication) -> bool {
        match self {
            Self::Value { expression, .. } => *expression < app.expressions.len(),
            Self::Callable { action, .. } => *action < app.actions.len(),
            Self::Component {
                component, host, ..
            } => {
                host.as_ref()
                    .is_some_and(|host| !host.provider.is_empty() && !host.component.is_empty())
                    || *component < usize::MAX
            }
        }
    }
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypedHostSlot {
    pub kind: String,
    pub query: Option<usize>,
    pub name: Option<usize>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypedCookieCapability {
    pub kind: String,
    pub name: String,
    pub operations: Vec<String>,
    #[serde(default = "default_cookie_path")]
    pub path: String,
    pub same_site: Option<String>,
    pub secure: Option<bool>,
    pub expiry_modes: Vec<String>,
}
fn default_cookie_path() -> String {
    "/".into()
}

#[derive(Clone, Deserialize)]
pub struct TypedRouteOutlet {
    pub id: String,
    pub node: usize,
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

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypedAction {
    pub instructions: Vec<TypedActionInstruction>,
    #[serde(default)]
    pub frame_slots: usize,
    #[serde(default)]
    pub parameter_slots: Vec<usize>,
    pub loader_result_state: Option<usize>,
    #[serde(default)]
    pub route_loader: bool,
    #[serde(default)]
    pub route_retry: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum TypedActionInstruction {
    Evaluate {
        expression: usize,
    },
    StoreState {
        state: usize,
    },
    MutationStart {
        generation: usize,
        pending: usize,
        error: usize,
    },
    MutationPublish {
        generation: usize,
        pending: usize,
        error: usize,
        data: usize,
        #[serde(rename = "invocationSlot")]
        invocation_slot: usize,
        #[serde(rename = "valueSlot")]
        value_slot: usize,
        success: bool,
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
    RouteReload,
    CallProp {
        prop: usize,
        #[serde(default)]
        arguments: Vec<usize>,
    },
    CallPropOptional {
        prop: usize,
        #[serde(default)]
        arguments: Vec<usize>,
    },
    CollectionMutation {
        input: usize,
        kind: String,
        key: usize,
        value: Option<usize>,
    },
    StoreHostRef {
        r#ref: usize,
    },
    Call {
        action: usize,
        #[serde(default)]
        arguments: Vec<usize>,
        #[serde(rename = "successPc")]
        success_pc: Option<usize>,
        #[serde(rename = "failurePc")]
        failure_pc: Option<usize>,
        #[serde(rename = "resultSlot")]
        result_slot: Option<usize>,
        #[serde(rename = "errorSlot")]
        error_slot: Option<usize>,
    },
    CallFrame {
        parameter: usize,
        #[serde(default)]
        arguments: Vec<usize>,
        #[serde(rename = "successPc")]
        success_pc: Option<usize>,
        #[serde(rename = "failurePc")]
        failure_pc: Option<usize>,
        #[serde(rename = "resultSlot")]
        result_slot: Option<usize>,
        #[serde(rename = "errorSlot")]
        error_slot: Option<usize>,
    },
    Jump {
        target: usize,
    },
    JumpIfFalse {
        target: usize,
    },
    CapabilityRequest {
        #[serde(flatten)]
        request: TypedCapabilityRequest,
        #[serde(rename = "successPc")]
        success_pc: usize,
        #[serde(rename = "failurePc")]
        failure_pc: usize,
        #[serde(rename = "finallyPc")]
        finally_pc: Option<usize>,
        #[serde(rename = "resultSlot")]
        result_slot: usize,
        #[serde(rename = "errorSlot")]
        error_slot: usize,
    },
    Return {
        #[serde(default)]
        outcome: TypedReturnOutcome,
        value: Option<usize>,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum TypedReturnOutcome {
    Success,
    Failure,
}
impl Default for TypedReturnOutcome {
    fn default() -> Self {
        Self::Success
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "capability", content = "request", rename_all = "camelCase")]
pub enum TypedCapabilityRequest {
    Fetch(TypedFetchRequest),
    Cookie(TypedCookieRequest),
}

#[derive(Clone, Debug, Deserialize)]
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

#[derive(Clone, Debug, Deserialize)]
pub struct TypedFetchHeader {
    pub name: usize,
    pub value: usize,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypedCookieRequest {
    pub operation: String,
    pub name: usize,
    pub value: Option<usize>,
    #[serde(default = "default_cookie_path")]
    pub path: String,
    pub same_site: Option<String>,
    pub secure: Option<bool>,
    #[serde(default = "default_cookie_expiry")]
    pub expiry: String,
    pub max_age: Option<i64>,
}
fn default_cookie_expiry() -> String {
    "session".into()
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum TypedNode {
    Element {
        tag: usize,
        #[serde(default = "html_namespace")]
        namespace: String,
        parent: Option<usize>,
        #[serde(default)]
        children: Vec<usize>,
        #[serde(default)]
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
        #[serde(default)]
        props: Vec<TypedComponentProp>,
        #[serde(default)]
        children: Vec<usize>,
    },
    DynamicComponent {
        prop: usize,
        parent: Option<usize>,
        #[serde(default)]
        props: Vec<TypedComponentProp>,
        #[serde(default)]
        children: Vec<usize>,
    },
    HostComponent {
        provider: String,
        component: String,
        parent: Option<usize>,
        #[serde(default)]
        props: Vec<TypedComponentProp>,
    },
    Slot {
        parent: Option<usize>,
    },
}
fn html_namespace() -> String {
    "html".into()
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
pub struct TypedPropProgram {
    pub target: usize,
    pub writes: Vec<TypedPropWrite>,
}

#[derive(Clone, Deserialize)]
pub struct TypedPropWrite {
    pub name: Option<usize>,
    pub kind: String,
    pub constant: Option<usize>,
    pub expression: Option<usize>,
    #[serde(default)]
    pub spread: bool,
}

#[derive(Clone, Deserialize)]
pub struct TypedInput {
    pub name: usize,
    pub kind: String,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypedStateSlot {
    pub name: Option<usize>,
    pub initial_expression: usize,
    pub frame_slot: usize,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypedRefSlot {
    pub initial_expression: usize,
}
#[derive(Clone, Deserialize)]
pub struct TypedHostRef {}
#[derive(Clone, Deserialize)]
pub struct TypedReaction {
    pub dependencies: Vec<usize>,
    pub action: usize,
    #[serde(default)]
    pub cleanup_action: Option<usize>,
}
#[derive(Clone, Deserialize)]
pub struct TypedGlobalListener {
    pub source: String,
    pub event: usize,
    pub action: usize,
}

#[derive(Clone, Deserialize)]
pub struct TypedProgram {
    pub instructions: Vec<TypedExpressionInstruction>,
}

#[derive(Clone, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum TypedExpressionInstruction {
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
    LoadRowRecord,
    LoadRowField {
        field: usize,
    },
    LoadEventField {
        field: usize,
    },
    LoadFrame {
        slot: usize,
    },
    LoadHost {
        host: usize,
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
        #[serde(default)]
        spreads: Vec<bool>,
    },
    MakeRecord {
        fields: Vec<usize>,
        #[serde(default)]
        spreads: Vec<bool>,
    },
    OmitFields {
        fields: Vec<usize>,
    },
    Filter {
        predicate: usize,
        #[serde(rename = "item_slot", alias = "itemSlot")]
        item_slot: usize,
        #[serde(rename = "index_slot", alias = "indexSlot")]
        index_slot: Option<usize>,
    },
    Map {
        mapper: usize,
        #[serde(rename = "item_slot", alias = "itemSlot")]
        item_slot: usize,
        #[serde(rename = "index_slot", alias = "indexSlot")]
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
    // The frame is allocated as `vec![Null; frame_slots]` before any
    // instruction runs (and cloned per stacked continuation), so an
    // untrusted `frameSlots` value must be capped before slot-index checks
    // would otherwise accept it.
    if action.frame_slots > crate::limits::MAX_FRAME_SLOTS {
        return Err("action frame slots exceed limit");
    }
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
        "event"
            | "type"
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
    // Iterative and visited-guarded on both walks: this helper runs during
    // validation, before topology has proven the graph acyclic, so neither
    // the child descent nor the parent walk may recurse unbounded.
    if root == target {
        return true;
    }
    let mut descendants = Vec::new();
    let mut seen = HashSet::new();
    seen.insert(root);
    descendants.push(root);
    while let Some(index) = descendants.pop() {
        match nodes.get(index) {
            Some(TypedNode::Element { children, .. })
            | Some(TypedNode::Component { children, .. }) => {
                for child in children {
                    if *child == target {
                        return true;
                    }
                    if seen.insert(*child) {
                        descendants.push(*child);
                    }
                }
            }
            Some(TypedNode::Conditional {
                consequent,
                alternate,
                ..
            }) => {
                for branch in [Some(*consequent), *alternate] {
                    let Some(branch) = branch else { continue };
                    if branch == target {
                        return true;
                    }
                    if seen.insert(branch) {
                        descendants.push(branch);
                    }
                }
            }
            _ => {}
        }
    }
    let mut ancestors = Vec::new();
    let mut seen_ancestors = HashSet::new();
    seen_ancestors.insert(target);
    ancestors.push(target);
    while let Some(index) = ancestors.pop() {
        let parent = match nodes.get(index) {
            Some(
                TypedNode::Element {
                    parent: Some(parent),
                    ..
                }
                | TypedNode::Text {
                    parent: Some(parent),
                    ..
                }
                | TypedNode::Conditional {
                    parent: Some(parent),
                    ..
                }
                | TypedNode::Loop {
                    parent: Some(parent),
                    ..
                }
                | TypedNode::Component {
                    parent: Some(parent),
                    ..
                }
                | TypedNode::Slot {
                    parent: Some(parent),
                },
            ) => *parent,
            _ => continue,
        };
        if parent == root {
            return true;
        }
        if seen_ancestors.insert(parent) {
            ancestors.push(parent);
        }
    }
    false
}

/// A revisit while walking ownership edges is either a cycle or two parents
/// claiming one node; both break mount/adoption ownership invariants.
fn mark_owned_node(
    owned: &mut [bool],
    stack: &mut Vec<(usize, usize)>,
    index: usize,
    depth: usize,
) -> Result<(), &'static str> {
    if owned[index] {
        return Err("node ownership is cyclic or shared");
    }
    owned[index] = true;
    stack.push((index, depth));
    Ok(())
}

impl TypedApplication {
    /// Validates untrusted executable IR before it reaches the typed runtime.
    ///
    /// Strict element-tag policy: standard HTML/SVG elements only. Hosts with
    /// an explicit trusted custom-element configuration must use
    /// [`TypedApplication::validate_with_policy`].
    pub fn validate(&self) -> Result<(), JsValue> {
        self.validate_with_policy(&plec_ir::sink::TagPolicy::default())
    }

    /// Validates untrusted executable IR under an explicit element-tag
    /// policy. The policy only ever widens the standard-element allowlist
    /// with configured custom elements; forbidden tags stay rejected.
    pub fn validate_with_policy(&self, policy: &plec_ir::sink::TagPolicy) -> Result<(), JsValue> {
        self.validate_contract_with_policy(policy)
            .map_err(JsValue::from_str)
    }

    /// Node-ownership contract for untrusted graphs: every structural handle
    /// is in range, ownership edges form a forest rooted at the graph root
    /// and loop row templates (acyclic, no node claimed twice, and no tree
    /// deeper than `MAX_NODE_GRAPH_DEPTH`), and every node is reachable.
    /// Mount recursion, region ownership, and SSR adoption all assume this
    /// shape, so hostile graphs are rejected here before any execution or
    /// traversal can follow them.
    fn validate_topology(&self, policy: &plec_ir::sink::TagPolicy) -> Result<(), &'static str> {
        use crate::limits::MAX_NODE_GRAPH_DEPTH;
        let node_count = self.nodes.len();
        let parent_in_range =
            |parent: &Option<usize>| parent.map(|parent| parent < node_count).unwrap_or(true);
        for node in &self.nodes {
            match node {
                TypedNode::Element {
                    tag,
                    namespace,
                    parent,
                    children,
                    ..
                } => {
                    if *tag >= self.strings.len() {
                        return Err("element tag handle out of range");
                    }
                    let tag_name = &self.strings[*tag];
                    // DOM-sink policy (plec_ir::sink): a tag string is
                    // interpolated verbatim by every serializer, so anything
                    // outside the strict HTML/SVG grammar is a markup
                    // injection channel and fails the graph closed.
                    if !plec_ir::sink::is_safe_tag_name(tag_name) {
                        return Err("unsafe element tag");
                    }
                    // Element instantiation is namespace-aware: only the
                    // HTML and SVG namespaces exist in executable IR, and
                    // the tag must belong to its namespace's allowlist.
                    if namespace != "html" && namespace != "svg" {
                        return Err("invalid element namespace");
                    }
                    if !plec_ir::sink::is_allowed_element_tag_with_policy(
                        tag_name, namespace, policy,
                    ) {
                        if plec_ir::sink::is_forbidden_element_tag(tag_name) {
                            return Err("forbidden element tag");
                        }
                        if namespace == "html" && tag_name.contains('-') {
                            return Err("custom element tag not permitted");
                        }
                        return Err("unsupported element tag");
                    }
                    if children.iter().any(|child| *child >= node_count) {
                        return Err("element child handle out of range");
                    }
                    if !parent_in_range(parent) {
                        return Err("node parent handle out of range");
                    }
                }
                TypedNode::Text { text, parent } => {
                    if *text >= self.texts.len() {
                        return Err("text handle out of range");
                    }
                    if !parent_in_range(parent) {
                        return Err("node parent handle out of range");
                    }
                }
                TypedNode::Conditional {
                    test,
                    parent,
                    consequent,
                    alternate,
                } => {
                    if *test >= self.expressions.len() {
                        return Err("conditional test expression handle out of range");
                    }
                    if *consequent >= node_count
                        || alternate
                            .map(|branch| branch >= node_count)
                            .unwrap_or(false)
                    {
                        return Err("conditional branch handle out of range");
                    }
                    if !parent_in_range(parent) {
                        return Err("node parent handle out of range");
                    }
                }
                TypedNode::Loop { r#loop, parent } => {
                    if *r#loop >= self.loops.len() {
                        return Err("loop handle out of range");
                    }
                    if !parent_in_range(parent) {
                        return Err("node parent handle out of range");
                    }
                }
                TypedNode::Component {
                    parent, children, ..
                }
                | TypedNode::DynamicComponent {
                    parent, children, ..
                } => {
                    if children.iter().any(|child| *child >= node_count) {
                        return Err("component child handle out of range");
                    }
                    if !parent_in_range(parent) {
                        return Err("node parent handle out of range");
                    }
                }
                TypedNode::Slot { parent } => {
                    if !parent_in_range(parent) {
                        return Err("node parent handle out of range");
                    }
                }
                TypedNode::HostComponent {
                    provider,
                    component,
                    parent,
                    props,
                } => {
                    if provider.is_empty()
                        || component.is_empty()
                        || !parent_in_range(parent)
                        || props.iter().any(|prop| !prop.valid(self))
                    {
                        return Err("invalid host component");
                    }
                }
            }
        }
        for loop_def in &self.loops {
            if loop_def.row_template >= node_count {
                return Err("loop row template handle out of range");
            }
        }
        let mut owned = vec![false; node_count];
        let mut stack = Vec::new();
        // Depth is tracked alongside ownership so recursion-based consumers
        // (mount, adoption, SSR rendering) only ever receive graphs within
        // MAX_NODE_GRAPH_DEPTH; deeper chains must fail at the boundary
        // instead of inside a traversal.
        mark_owned_node(&mut owned, &mut stack, self.root_node, 1)?;
        for loop_def in &self.loops {
            mark_owned_node(&mut owned, &mut stack, loop_def.row_template, 1)?;
        }
        while let Some((index, depth)) = stack.pop() {
            if depth > MAX_NODE_GRAPH_DEPTH {
                return Err("node graph depth exceeds limit");
            }
            let child_depth = depth + 1;
            match &self.nodes[index] {
                TypedNode::Element { children, .. }
                | TypedNode::Component { children, .. }
                | TypedNode::DynamicComponent { children, .. } => {
                    for child in children {
                        mark_owned_node(&mut owned, &mut stack, *child, child_depth)?;
                    }
                }
                TypedNode::Conditional {
                    consequent,
                    alternate,
                    ..
                } => {
                    mark_owned_node(&mut owned, &mut stack, *consequent, child_depth)?;
                    if let Some(alternate) = alternate {
                        mark_owned_node(&mut owned, &mut stack, *alternate, child_depth)?;
                    }
                }
                TypedNode::Text { .. }
                | TypedNode::Loop { .. }
                | TypedNode::Slot { .. }
                | TypedNode::HostComponent { .. } => {}
            }
        }
        if owned.iter().any(|owned| !owned) {
            return Err("node graph contains unrooted nodes");
        }
        Ok(())
    }

    #[allow(dead_code)] // This function may not be used in all contexts
    fn validate_contract(&self) -> Result<(), &'static str> {
        self.validate_contract_with_policy(&plec_ir::sink::TagPolicy::default())
    }

    fn validate_contract_with_policy(
        &self,
        policy: &plec_ir::sink::TagPolicy,
    ) -> Result<(), &'static str> {
        use crate::limits::{
            MAX_ACTION_INSTRUCTIONS, MAX_COMPONENT_COLLECTION_LEN, MAX_COMPONENT_STRING_BYTES,
            MAX_EXPRESSION_INSTRUCTIONS,
        };
        fn collection_ok(len: usize) -> Result<(), &'static str> {
            if len > MAX_COMPONENT_COLLECTION_LEN {
                return Err("component collection exceeds limit");
            }
            Ok(())
        }
        if self.version != "0.10" {
            return Err("unsupported executable application version");
        }
        collection_ok(self.strings.len())?;
        collection_ok(self.constants.len())?;
        collection_ok(self.nodes.len())?;
        collection_ok(self.texts.len())?;
        collection_ok(self.bindings.len())?;
        collection_ok(self.prop_programs.len())?;
        collection_ok(self.events.len())?;
        collection_ok(self.inputs.len())?;
        collection_ok(self.state_slots.len())?;
        collection_ok(self.ref_slots.len())?;
        collection_ok(self.host_refs.len())?;
        collection_ok(self.reactions.len())?;
        collection_ok(self.listeners.len())?;
        collection_ok(self.parameters.len())?;
        collection_ok(self.expressions.len())?;
        collection_ok(self.actions.len())?;
        collection_ok(self.loops.len())?;
        collection_ok(self.dependency_edges.len())?;
        collection_ok(self.route_outlets.len())?;
        collection_ok(self.host_slots.len())?;
        collection_ok(self.capabilities.len())?;
        for string in &self.strings {
            if string.len() > MAX_COMPONENT_STRING_BYTES {
                return Err("component string pool entry exceeds limit");
            }
        }
        for constant in &self.constants {
            constant.check_limits()?;
        }
        if self
            .expressions
            .iter()
            .any(|program| program.instructions.len() > MAX_EXPRESSION_INSTRUCTIONS)
        {
            return Err("expression program exceeds instruction limit");
        }
        if self
            .actions
            .iter()
            .any(|action| action.instructions.len() > MAX_ACTION_INSTRUCTIONS)
        {
            return Err("action program exceeds instruction limit");
        }
        if self.root_node >= self.nodes.len() {
            return Err("root node handle out of range");
        }
        self.validate_topology(policy)?;
        let mut outlets = HashSet::new();
        for outlet in &self.route_outlets {
            if !outlets.insert(&outlet.id)
                || !matches!(self.nodes.get(outlet.node), Some(TypedNode::Element { .. }))
            {
                return Err("invalid typed route outlet");
            }
        }
        for state in &self.state_slots {
            if state.initial_expression >= self.expressions.len() {
                return Err("state expression handle out of range");
            }
            if state
                .name
                .map(|name| name >= self.strings.len())
                .unwrap_or(false)
            {
                return Err("state name handle out of range");
            }
        }
        for reference in &self.ref_slots {
            if reference.initial_expression >= self.expressions.len() {
                return Err("ref expression handle out of range");
            }
        }
        for reaction in &self.reactions {
            if reaction.dependencies.is_empty()
                || reaction
                    .dependencies
                    .iter()
                    .any(|value| *value >= self.expressions.len())
                || reaction.action >= self.actions.len()
                || reaction
                    .cleanup_action
                    .is_some_and(|action| action >= self.actions.len())
            {
                return Err("invalid reaction");
            }
        }
        for listener in &self.listeners {
            if !matches!(listener.source.as_str(), "window" | "document")
                || listener.event >= self.strings.len()
                || listener.action >= self.actions.len()
            {
                return Err("invalid global listener");
            }
        }
        for node in &self.nodes {
            if matches!(node, TypedNode::Element { host_ref: Some(reference), .. } if *reference >= self.host_refs.len())
            {
                return Err("host ref handle out of range");
            }
        }
        if self
            .route_error_state
            .map(|state| state >= self.state_slots.len())
            .unwrap_or(false)
        {
            return Err("route error state handle out of range");
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
            )?;
            if !matches!(
                self.nodes.get(event.target),
                Some(TypedNode::Element { .. })
            ) {
                return Err("event target must be an element");
            }
            if event.fields.iter().any(|field| {
                self.strings
                    .get(field.name)
                    .map(String::as_str)
                    .map(|name| !supported_event_field(name))
                    .unwrap_or(true)
            }) {
                return Err("unsupported typed event field");
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
                    return Err("event target is outside its loop template")
                }
                None if in_any_loop => return Err("loop event is missing loop ownership"),
                _ => {}
            }
        }
        for action in &self.actions {
            #[cfg(not(feature = "fetch"))]
            if action.instructions.iter().any(|instruction| {
                matches!(
                    instruction,
                    TypedActionInstruction::CapabilityRequest {
                        request: TypedCapabilityRequest::Fetch(_),
                        ..
                    }
                )
            }) {
                return Err("fetch capability is disabled");
            }
            let input_kinds = self
                .inputs
                .iter()
                .map(|input| input.kind.clone())
                .collect::<Vec<_>>();
            validate_typed_action_contract(action, self.expressions.len(), &input_kinds)?;
            for instruction in &action.instructions {
                match instruction {
                    TypedActionInstruction::Evaluate { expression }
                        if *expression >= self.expressions.len() =>
                    {
                        return Err("action expression handle out of range")
                    }
                    TypedActionInstruction::StoreState { state }
                        if *state >= self.state_slots.len() =>
                    {
                        return Err("action state handle out of range")
                    }
                    TypedActionInstruction::MutationStart {
                        generation,
                        pending,
                        error,
                    } if [generation, pending, error]
                        .iter()
                        .any(|state| **state >= self.state_slots.len()) =>
                    {
                        return Err("mutation state handle out of range")
                    }
                    TypedActionInstruction::MutationPublish {
                        generation,
                        pending,
                        error,
                        data,
                        invocation_slot,
                        value_slot,
                        ..
                    } if [generation, pending, error, data]
                        .iter()
                        .any(|state| **state >= self.state_slots.len())
                        || *invocation_slot >= action.frame_slots
                        || *value_slot >= action.frame_slots =>
                    {
                        return Err("invalid mutation publication")
                    }
                    TypedActionInstruction::StoreRef { reference }
                        if *reference >= self.ref_slots.len() =>
                    {
                        return Err("action ref handle out of range")
                    }
                    TypedActionInstruction::CaptureActiveElement { reference }
                    | TypedActionInstruction::FocusRef { reference }
                        if *reference >= self.ref_slots.len() =>
                    {
                        return Err("action ref handle out of range")
                    }
                    TypedActionInstruction::FocusHostRef { reference }
                        if *reference >= self.host_refs.len() =>
                    {
                        return Err("action host ref handle out of range")
                    }
                    TypedActionInstruction::CallProp { prop, arguments }
                        if self
                            .parameters
                            .get(*prop)
                            .map(|parameter| parameter.callable)
                            != Some(true)
                            || arguments
                                .iter()
                                .any(|expression| *expression >= self.expressions.len()) =>
                    {
                        return Err("action callable prop out of range")
                    }
                    TypedActionInstruction::Call {
                        action: callee,
                        arguments,
                        success_pc,
                        failure_pc,
                        result_slot,
                        error_slot,
                    } => {
                        let target = self
                            .actions
                            .get(*callee)
                            .ok_or("action handle out of range")?;
                        let continuation_fields = [
                            success_pc.is_some(),
                            failure_pc.is_some(),
                            result_slot.is_some(),
                            error_slot.is_some(),
                        ];
                        if continuation_fields.iter().any(|present| *present)
                            && continuation_fields.iter().any(|present| !*present)
                            || arguments.len() != target.parameter_slots.len()
                            || arguments
                                .iter()
                                .any(|expression| *expression >= self.expressions.len())
                            || success_pc
                                .map(|pc| pc >= action.instructions.len())
                                .unwrap_or(false)
                            || failure_pc
                                .map(|pc| pc >= action.instructions.len())
                                .unwrap_or(false)
                            || result_slot
                                .map(|slot| slot >= action.frame_slots)
                                .unwrap_or(false)
                            || error_slot
                                .map(|slot| slot >= action.frame_slots)
                                .unwrap_or(false)
                        {
                            return Err("invalid action call");
                        }
                    }
                    TypedActionInstruction::CallFrame {
                        parameter,
                        arguments,
                        success_pc,
                        failure_pc,
                        result_slot,
                        error_slot,
                    } => {
                        let continuation_fields = [
                            success_pc.is_some(),
                            failure_pc.is_some(),
                            result_slot.is_some(),
                            error_slot.is_some(),
                        ];
                        if continuation_fields.iter().any(|present| *present)
                            && continuation_fields.iter().any(|present| !*present)
                            || *parameter >= action.frame_slots
                            || arguments
                                .iter()
                                .any(|expression| *expression >= self.expressions.len())
                            || success_pc
                                .map(|pc| pc >= action.instructions.len())
                                .unwrap_or(false)
                            || failure_pc
                                .map(|pc| pc >= action.instructions.len())
                                .unwrap_or(false)
                            || result_slot
                                .map(|slot| slot >= action.frame_slots)
                                .unwrap_or(false)
                            || error_slot
                                .map(|slot| slot >= action.frame_slots)
                                .unwrap_or(false)
                        {
                            return Err("invalid action callFrame");
                        }
                    }
                    TypedActionInstruction::Jump { target }
                    | TypedActionInstruction::JumpIfFalse { target }
                        if *target >= action.instructions.len() =>
                    {
                        return Err("action jump target out of range")
                    }
                    TypedActionInstruction::CapabilityRequest {
                        request,
                        success_pc,
                        failure_pc,
                        finally_pc,
                        result_slot,
                        error_slot,
                        ..
                    } if matches!(request, TypedCapabilityRequest::Fetch(request) if request.url >= self.expressions.len() || request.body.map(|body| body >= self.expressions.len()).unwrap_or(false) || request.headers.iter().any(|header| header.name >= self.strings.len() || header.value >= self.expressions.len()))
                        || matches!(request, TypedCapabilityRequest::Cookie(request) if request.name >= self.strings.len() || request.value.map(|value| value >= self.expressions.len()).unwrap_or(false) || !["get", "set", "delete"].contains(&request.operation.as_str()) || (request.operation == "set" && request.value.is_none()) || !["session", "maxAge"].contains(&request.expiry.as_str()) || (request.expiry == "maxAge" && request.max_age.is_none()) || (request.expiry == "session" && request.max_age.is_some()) || request.same_site.as_deref().is_some_and(|same_site| !["lax", "strict", "none"].contains(&same_site)))
                        || *success_pc >= action.instructions.len()
                        || *failure_pc >= action.instructions.len()
                        || finally_pc
                            .map(|pc| pc >= action.instructions.len())
                            .unwrap_or(false)
                        || *result_slot >= action.frame_slots
                        || *error_slot >= action.frame_slots =>
                    {
                        return Err("invalid action continuation")
                    }
                    TypedActionInstruction::StoreHostRef { r#ref }
                        if *r#ref >= self.strings.len() =>
                    {
                        return Err("host ref string handle out of range")
                    }
                    TypedActionInstruction::Return {
                        value: Some(value), ..
                    } if *value >= self.expressions.len() => {
                        return Err("action return expression handle out of range")
                    }
                    _ => {}
                }
            }
            if let Some(state) = action.loader_result_state {
                if state >= self.state_slots.len() {
                    return Err("loader result state handle out of range");
                }
            }
            if action.route_loader
                && (action.loader_result_state.is_none()
                    || action.instructions.iter().any(|instruction| {
                        matches!(instruction, TypedActionInstruction::StoreState { .. })
                    }))
            {
                return Err("route loader must write only its loader result state".into());
            }
            if action.route_loader
                && !action.parameter_slots.is_empty()
                && action.parameter_slots != [0, 1]
            {
                return Err("route loader context slots must be [0, 1]".into());
            }
        }
        for program in &self.expressions {
            for instruction in &program.instructions {
                match instruction {
                    TypedExpressionInstruction::Jump { target }
                    | TypedExpressionInstruction::JumpIfFalse { target }
                    | TypedExpressionInstruction::JumpIfTrue { target }
                        if *target >= program.instructions.len() =>
                    {
                        return Err("expression jump target out of range");
                    }
                    TypedExpressionInstruction::Filter { predicate, .. }
                        if *predicate >= self.expressions.len() =>
                    {
                        return Err("expression program handle out of range");
                    }
                    TypedExpressionInstruction::Map { mapper, .. }
                        if *mapper >= self.expressions.len() =>
                    {
                        return Err("expression program handle out of range");
                    }
                    _ => {}
                }
            }
            if program.instructions.iter().any(|instruction| matches!(instruction, TypedExpressionInstruction::LoadHost { host } if *host >= self.host_slots.len())) {
                return Err("host input handle out of range");
            }
            if program.instructions.iter().any(|instruction| matches!(instruction, TypedExpressionInstruction::LoadRef { reference } if *reference >= self.ref_slots.len())) {
                return Err("ref input handle out of range");
            }
        }
        // DOM-sink policy (plec_ir::sink): bindings and prop programs are the
        // only element mutation channels, so their sinks are validated here
        // before any substituted artifact reaches the runtime applier.
        for binding in &self.bindings {
            if binding.target >= self.nodes.len() {
                return Err("binding target handle out of range");
            }
            match binding.sink.as_str() {
                "text" => {}
                "attribute" | "property" => {
                    let name = binding
                        .name
                        .and_then(|handle| self.strings.get(handle))
                        .ok_or("binding name handle out of range")?;
                    let safe = if binding.sink == "attribute" {
                        plec_ir::sink::is_safe_attribute_name(name)
                    } else {
                        plec_ir::sink::is_safe_property_name(name)
                    };
                    if !safe {
                        return Err("unsafe typed binding sink");
                    }
                }
                _ => return Err("unsupported typed binding sink"),
            }
        }
        for program in &self.prop_programs {
            if program.target >= self.nodes.len() {
                return Err("prop program target handle out of range");
            }
            for write in &program.writes {
                match write.kind.as_str() {
                    "attribute" | "property" => {}
                    _ => return Err("unsupported typed prop write kind"),
                }
                if write.spread {
                    if write.name.is_some() {
                        return Err("spread prop write cannot carry a name handle");
                    }
                    continue;
                }
                let name = write
                    .name
                    .and_then(|handle| self.strings.get(handle))
                    .ok_or("prop write name handle out of range")?;
                let safe = if write.kind == "attribute" {
                    plec_ir::sink::is_safe_attribute_name(name)
                } else {
                    plec_ir::sink::is_safe_property_name(name)
                };
                if !safe {
                    return Err("unsafe typed prop write sink");
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
            "version": "0.10",
            "rootNode": 0,
            "strings": ["div"],
            "nodes": [{"op": "element", "tag": 0, "parent": null}],
            "inputs": [{"name": 0, "kind": input_kind}],
            "expressions": [{"instructions": []}],
            "actions": [{"instructions": [instruction]}]
        }))
        .unwrap()
    }

    fn event_application(event: Value, nodes: Value, loops: Value) -> TypedApplication {
        serde_json::from_value(serde_json::json!({
            "version": "0.10",
            "rootNode": 0,
            "strings": ["div", "click", "value", "unsupported"],
            "nodes": nodes,
            "expressions": [{"instructions": []}],
            "actions": [{"frameSlots": 1, "instructions": [{"op": "return"}]}],
            "loops": loops,
            "events": [event]
        }))
        .unwrap()
    }

    fn component_slot_application(frame_nodes: Value) -> TypedComponentApplication {
        serde_json::from_value(serde_json::json!({
            "version":"0.10", "rootComponent":0,
            "components":[
                {"id":"App","rootNode":0,"strings":["p"],"constants":[],
                 "nodes":[
                    {"op":"component","component":1,"parent":null,"props":[],"children":[1]},
                    {"op":"element","tag":0,"parent":null,"children":[]}
                 ],"texts":[],"bindings":[],"propPrograms":[],"events":[],"inputs":[],"stateSlots":[],"parameters":[],"expressions":[],"actions":[],"loops":[],"dependencyEdges":[]},
                {"id":"Frame","rootNode":0,"strings":["section"],"constants":[],
                 "nodes":frame_nodes,"texts":[],"bindings":[],"propPrograms":[],"events":[],"inputs":[],"stateSlots":[],"parameters":[],"expressions":[],"actions":[],"loops":[],"dependencyEdges":[]}
            ]
        }))
        .unwrap()
    }

    #[test]
    fn typed_component_decoder_rejects_invalid_slot_targets() {
        let slotless = component_slot_application(serde_json::json!(
            [{"op":"element","tag":0,"parent":null,"children":[]}]
        ));
        assert_eq!(
            validate_component_slot_target(
                &slotless.components[1],
                &[1],
                slotless.components[0].nodes.len()
            ),
            Err("invalid component slot")
        );

        let multiple = component_slot_application(serde_json::json!([
            {"op":"element","tag":0,"parent":null,"children":[1,2]},
            {"op":"slot","parent":0},
            {"op":"slot","parent":0}
        ]));
        assert_eq!(
            validate_component_slot_target(
                &multiple.components[1],
                &[1],
                multiple.components[0].nodes.len()
            ),
            Err("invalid component slot")
        );
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

    #[cfg(not(feature = "fetch"))]
    #[test]
    fn core_rejects_typed_fetch_capabilities() {
        let app = typed_action_artifact(
            serde_json::json!({"op":"capabilityRequest","capability":"fetch","request":{"url":0,"method":"GET","decode":"empty"},"successPc":0,"failurePc":0,"resultSlot":0,"errorSlot":0}),
            "scalar",
        );
        assert_eq!(app.validate_contract(), Err("fetch capability is disabled"));
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
    fn typed_decoder_rejects_partial_call_continuations() {
        let mut app = typed_action_artifact(
            serde_json::json!({"op":"call","action":0,"arguments":[]}),
            "collection",
        );
        assert!(app.validate_contract().is_ok());
        app.actions[0].instructions[0] = serde_json::from_value(
            serde_json::json!({"op":"call","action":0,"arguments":[],"successPc":0}),
        )
        .unwrap();
        assert_eq!(app.validate_contract(), Err("invalid action call"));
    }

    #[test]
    fn typed_decoder_validates_call_frame_parameter_as_an_action_slot() {
        let mut app = typed_action_artifact(
            serde_json::json!({
                "op":"callFrame", "parameter":1, "arguments":[],
                "successPc":1, "failurePc":1, "resultSlot":2, "errorSlot":3
            }),
            "collection",
        );
        app.actions[0].frame_slots = 4;
        app.actions[0].parameter_slots = vec![0, 1];
        app.actions[0]
            .instructions
            .push(serde_json::from_value(serde_json::json!({"op":"return"})).unwrap());
        app.actions.push(TypedAction {
            frame_slots: 0,
            parameter_slots: vec![],
            loader_result_state: None,
            route_loader: false,
            route_retry: false,
            instructions: vec![serde_json::from_value(serde_json::json!({"op":"return"})).unwrap()],
        });

        assert!(app.validate_contract().is_ok());

        if let TypedActionInstruction::CallFrame { parameter, .. } =
            &mut app.actions[0].instructions[0]
        {
            *parameter = 4;
        } else {
            panic!("expected callFrame");
        }
        assert_eq!(app.validate_contract(), Err("invalid action callFrame"));
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
    fn typed_application_validation_rejects_bad_event_handles_and_fields() {
        let nodes = serde_json::json!([{"op":"element","tag":0,"children":[]}]);
        for event in [
            serde_json::json!({"target":1,"type":1,"action":0,"fields":[]}),
            serde_json::json!({"target":0,"type":1,"action":1,"fields":[]}),
            serde_json::json!({"target":0,"type":1,"action":0,"loop":0,"fields":[]}),
            serde_json::json!({"target":0,"type":1,"action":0,"fields":[{"name":3,"slot":0}]}),
            serde_json::json!({"target":0,"type":1,"action":0,"fields":[{"name":2,"slot":0},{"name":2,"slot":0}]}),
            serde_json::json!({"target":0,"type":1,"action":0,"fields":[{"name":2,"slot":1}]}),
        ] {
            assert!(
                event_application(event, nodes.clone(), serde_json::json!([]))
                    .validate_contract()
                    .is_err()
            );
        }
    }

    #[test]
    fn typed_application_validation_rejects_non_element_and_invalid_loop_ownership() {
        let non_element = event_application(
            serde_json::json!({"target":1,"type":1,"action":0,"fields":[]}),
            serde_json::json!([{"op":"element","tag":0,"children":[1]},{"op":"text","text":0,"parent":0}]),
            serde_json::json!([]),
        );
        assert!(non_element.validate_contract().is_err());

        let loop_def = serde_json::json!([{
            "sourceExpression":0,"keyExpression":0,"itemSlot":0,"rowTemplate":2,"input":null
        }]);
        let nodes = serde_json::json!([
            {"op":"element","tag":0,"children":[1,2]},
            {"op":"element","tag":0,"parent":0,"children":[]},
            {"op":"element","tag":0,"parent":0,"children":[]}
        ]);
        let outside_template = event_application(
            serde_json::json!({"target":1,"type":1,"action":0,"loop":0,"fields":[]}),
            nodes.clone(),
            loop_def.clone(),
        );
        assert!(outside_template.validate_contract().is_err());
        let missing_loop = event_application(
            serde_json::json!({"target":2,"type":1,"action":0,"fields":[]}),
            nodes,
            loop_def,
        );
        assert!(missing_loop.validate_contract().is_err());
    }

    fn sink_application(bindings: Value, prop_programs: Value, strings: Value) -> TypedApplication {
        serde_json::from_value(serde_json::json!({
            "version": "0.10",
            "rootNode": 0,
            "strings": strings,
            "nodes": [{"op": "element", "tag": 0, "children": []}],
            "expressions": [{"instructions": []}],
            "bindings": bindings,
            "propPrograms": prop_programs
        }))
        .unwrap()
    }

    #[test]
    fn typed_application_validation_rejects_hostile_binding_sinks() {
        let strings = serde_json::json!([
            "div",
            "innerHTML",
            "ONCLICK",
            "srcdoc",
            "data-plec-node",
            "value",
            "href"
        ]);
        // Property sink is allowlisted: innerHTML must never be a sink.
        assert_eq!(
            sink_application(
                serde_json::json!([{"target":0,"sink":"property","name":1,"expression":0}]),
                serde_json::json!([]),
                strings
            )
            .validate_contract(),
            Err("unsafe typed binding sink")
        );
        let strings = serde_json::json!([
            "div",
            "innerHTML",
            "ONCLICK",
            "srcdoc",
            "data-plec-node",
            "value",
            "href"
        ]);
        // Attribute sink rejects event-handler casing, srcdoc, and the
        // reserved runtime namespace.
        for name in [2, 3, 4] {
            assert_eq!(
                sink_application(
                    serde_json::json!([{"target":0,"sink":"attribute","name":name,"expression":0}]),
                    serde_json::json!([]),
                    strings.clone()
                )
                .validate_contract(),
                Err("unsafe typed binding sink")
            );
        }
        // Unknown sinks and dangling name handles fail closed.
        assert_eq!(
            sink_application(
                serde_json::json!([{"target":0,"sink":"innerHTML","name":1,"expression":0}]),
                serde_json::json!([]),
                strings.clone()
            )
            .validate_contract(),
            Err("unsupported typed binding sink")
        );
        assert_eq!(
            sink_application(
                serde_json::json!([{"target":0,"sink":"attribute","name":99,"expression":0}]),
                serde_json::json!([]),
                strings
            )
            .validate_contract(),
            Err("binding name handle out of range")
        );
    }

    #[test]
    fn typed_application_validation_accepts_supported_binding_sinks() {
        let app = sink_application(
            serde_json::json!([
                {"target":0,"sink":"text","expression":0},
                {"target":0,"sink":"attribute","name":0,"expression":0},
                {"target":0,"sink":"property","name":1,"expression":0},
                {"target":0,"sink":"attribute","name":2,"expression":0}
            ]),
            serde_json::json!([{"target":0,"writes":[
                {"name":0,"kind":"attribute","constant":0},
                {"kind":"attribute","expression":0,"spread":true},
                {"name":1,"kind":"property","expression":0}
            ]}]),
            serde_json::json!(["div", "value", "href"]),
        );
        assert!(app.validate_contract().is_ok());
    }

    #[test]
    fn typed_application_validation_rejects_hostile_prop_write_sinks() {
        let strings = serde_json::json!(["div", "innerHTML", "ONCLICK", "checked"]);
        // Crafted property-sink IR selecting innerHTML must be rejected
        // before it reaches the runtime applier.
        assert_eq!(
            sink_application(
                serde_json::json!([]),
                serde_json::json!([{"target":0,"writes":[{"name":1,"kind":"property","expression":0}]}]),
                strings.clone()
            )
            .validate_contract(),
            Err("unsafe typed prop write sink")
        );
        assert_eq!(
            sink_application(
                serde_json::json!([]),
                serde_json::json!([{"target":0,"writes":[{"name":2,"kind":"attribute","expression":0}]}]),
                strings.clone()
            )
            .validate_contract(),
            Err("unsafe typed prop write sink")
        );
        assert_eq!(
            sink_application(
                serde_json::json!([]),
                serde_json::json!([{"target":0,"writes":[{"name":3,"kind":"style","expression":0}]}]),
                strings.clone()
            )
            .validate_contract(),
            Err("unsupported typed prop write kind")
        );
        // Spread writes stay anonymous; a named spread is malformed.
        assert_eq!(
            sink_application(
                serde_json::json!([]),
                serde_json::json!([{"target":0,"writes":[{"name":3,"kind":"attribute","expression":0,"spread":true}]}]),
                strings
            )
            .validate_contract(),
            Err("spread prop write cannot carry a name handle")
        );
    }

    #[test]
    fn typed_application_validation_rejects_oversized_collections() {
        let app: TypedApplication = serde_json::from_value(serde_json::json!({
            "version": "0.10",
            "rootNode": 0,
            "strings": ["div"],
            "nodes": [{"op": "element", "tag": 0, "parent": null}],
            "expressions": [],
            "actions": []
        }))
        .unwrap();
        let mut bloated = app.clone();
        bloated.nodes = (0..=crate::limits::MAX_COMPONENT_COLLECTION_LEN)
            .map(|_| app.nodes[0].clone())
            .collect();
        assert_eq!(
            bloated.validate_contract(),
            Err("component collection exceeds limit")
        );
    }

    #[test]
    fn typed_application_validation_rejects_deep_constants() {
        fn nested(depth: usize) -> Value {
            let mut value = serde_json::json!(null);
            for _ in 0..depth {
                value = serde_json::json!([value]);
            }
            value
        }
        let app: TypedApplication = serde_json::from_value(serde_json::json!({
            "version": "0.10",
            "rootNode": 0,
            "strings": ["div"],
            "nodes": [{"op": "element", "tag": 0, "parent": null}],
            "constants": [nested(crate::limits::MAX_VALUE_DEPTH + 8)],
            "expressions": [],
            "actions": []
        }))
        .unwrap();
        assert_eq!(
            app.validate_contract(),
            Err("runtime value nesting exceeds limit")
        );
    }

    #[test]
    fn typed_application_validation_rejects_oversized_expression_programs() {
        let instructions = vec![
            serde_json::json!({"op": "jump", "target": 0});
            crate::limits::MAX_EXPRESSION_INSTRUCTIONS + 1
        ];
        let app: TypedApplication = serde_json::from_value(serde_json::json!({
            "version": "0.10",
            "rootNode": 0,
            "strings": ["div"],
            "nodes": [{"op": "element", "tag": 0, "parent": null}],
            "expressions": [{"instructions": instructions}],
            "actions": []
        }))
        .unwrap();
        assert_eq!(
            app.validate_contract(),
            Err("expression program exceeds instruction limit")
        );
    }

    #[test]
    fn typed_application_validation_rejects_oversized_string_pool_entries() {
        let app: TypedApplication = serde_json::from_value(serde_json::json!({
            "version": "0.10",
            "rootNode": 0,
            "strings": ["x".repeat(crate::limits::MAX_COMPONENT_STRING_BYTES + 1)],
            "nodes": [{"op": "element", "tag": 0, "parent": null}],
            "expressions": [],
            "actions": []
        }))
        .unwrap();
        assert_eq!(
            app.validate_contract(),
            Err("component string pool entry exceeds limit")
        );
    }

    #[test]
    fn component_application_accepts_component_shape_guard_limit() {
        let app: TypedApplication = serde_json::from_value(serde_json::json!({
            "version": "0.10",
            "id": "tiny",
            "rootNode": 0,
            "strings": ["div"],
            "nodes": [{"op": "element", "tag": 0, "parent": null}],
            "expressions": [],
            "actions": []
        }))
        .unwrap();
        let components = (0..crate::limits::MAX_COMPONENT_COUNT)
            .map(|index| {
                let mut component = app.clone();
                component.id = format!("tiny-{index}");
                component
            })
            .collect();
        assert!(TypedComponentApplication {
            version: "0.10".into(),
            root_component: 0,
            components,
        }
        .validate()
        .is_ok());
    }

    fn topology_application(nodes: Value, loops: Value) -> TypedApplication {
        serde_json::from_value(serde_json::json!({
            "version": "0.10",
            "rootNode": 0,
            "strings": ["div", "ul", "li"],
            "nodes": nodes,
            "texts": [{"value": "Done"}],
            "expressions": [{"instructions": []}],
            "actions": [{"instructions": [{"op": "return"}]}],
            "loops": loops
        }))
        .unwrap()
    }

    fn rooted_loop_application() -> TypedApplication {
        // Same shape as compiled output: a static root holding a loop node,
        // and a row-template subtree rooted separately via the loop def.
        topology_application(
            serde_json::json!([
                {"op": "element", "tag": 0, "parent": null, "children": [3]},
                {"op": "element", "tag": 1, "parent": null, "children": [2]},
                {"op": "element", "tag": 2, "parent": 1, "children": []},
                {"op": "loop", "loop": 0, "parent": 0}
            ]),
            serde_json::json!([{
                "sourceExpression": 0, "keyExpression": 0, "itemSlot": 0,
                "rowTemplate": 1, "input": null
            }]),
        )
    }

    #[test]
    fn topology_validation_accepts_rooted_loop_forest() {
        assert!(rooted_loop_application().validate_contract().is_ok());
    }

    #[test]
    fn topology_validation_rejects_self_child_graph() {
        let app = topology_application(
            serde_json::json!([
                {"op": "element", "tag": 0, "parent": null, "children": [0]}
            ]),
            serde_json::json!([]),
        );
        assert_eq!(
            app.validate_contract(),
            Err("node ownership is cyclic or shared")
        );
    }

    #[test]
    fn topology_validation_rejects_two_node_cycle() {
        let app = topology_application(
            serde_json::json!([
                {"op": "element", "tag": 0, "parent": null, "children": [1]},
                {"op": "element", "tag": 0, "parent": 0, "children": [0]}
            ]),
            serde_json::json!([]),
        );
        assert_eq!(
            app.validate_contract(),
            Err("node ownership is cyclic or shared")
        );
    }

    #[test]
    fn topology_validation_rejects_shared_child() {
        let app = topology_application(
            serde_json::json!([
                {"op": "element", "tag": 0, "parent": null, "children": [1, 2]},
                {"op": "element", "tag": 0, "parent": 0, "children": [2]},
                {"op": "element", "tag": 0, "parent": 0, "children": []}
            ]),
            serde_json::json!([]),
        );
        assert_eq!(
            app.validate_contract(),
            Err("node ownership is cyclic or shared")
        );
    }

    #[test]
    fn topology_validation_rejects_unrooted_nodes() {
        let app = topology_application(
            serde_json::json!([
                {"op": "element", "tag": 0, "parent": null, "children": []},
                {"op": "element", "tag": 0, "parent": null, "children": []}
            ]),
            serde_json::json!([]),
        );
        assert_eq!(
            app.validate_contract(),
            Err("node graph contains unrooted nodes")
        );
    }

    fn linear_chain_nodes(depth: usize) -> Value {
        let nodes: Vec<Value> = (0..depth)
            .map(|index| {
                serde_json::json!({
                    "op": "element", "tag": 0,
                    "parent": index.checked_sub(1),
                    "children": if index + 1 < depth { vec![index + 1] } else { vec![] },
                })
            })
            .collect();
        Value::Array(nodes)
    }

    #[test]
    fn topology_validation_rejects_deeper_than_depth_limit() {
        // Mount, adoption, and SSR rendering recurse over this forest, so a
        // graph deeper than MAX_NODE_GRAPH_DEPTH must fail at validation
        // instead of inside a traversal.
        let app = topology_application(
            linear_chain_nodes(crate::limits::MAX_NODE_GRAPH_DEPTH + 1),
            serde_json::json!([]),
        );
        assert_eq!(
            app.validate_contract(),
            Err("node graph depth exceeds limit")
        );
    }

    #[test]
    fn topology_validation_accepts_chain_at_depth_limit() {
        // The ceiling must reject only pathological shapes: a chain exactly
        // at MAX_NODE_GRAPH_DEPTH stays legitimate output.
        let app = topology_application(
            linear_chain_nodes(crate::limits::MAX_NODE_GRAPH_DEPTH),
            serde_json::json!([]),
        );
        assert!(app.validate_contract().is_ok());
    }

    #[test]
    fn component_call_graph_cycle_is_rejected() {
        // Direct over the graph checker: TypedComponentApplication::validate
        // surfaces errors as JsValue, which host test binaries cannot touch.
        // The artifact-level wiring is covered by the wasm suite
        // (load_application_rejects_component_call_cycle).
        assert_eq!(
            validate_component_call_graph_acyclic(vec![vec![1], vec![0]]),
            Err("component call graph contains a cycle")
        );
        assert_eq!(
            validate_component_call_graph_acyclic(vec![vec![0]]),
            Err("component call graph contains a cycle")
        );
        assert!(validate_component_call_graph_acyclic(vec![vec![1], vec![2], vec![]]).is_ok());
        // Host-resolved component props carry no static edge (the compiler
        // excludes them from its recursion check the same way).
        let app: TypedApplication = serde_json::from_value(serde_json::json!({
            "version": "0.10", "rootNode": 0, "strings": ["Icon", "div"],
            "nodes": [
                {"op": "component", "component": 1, "parent": null, "props": [
                    {"kind": "component", "name": 0, "component": 0,
                     "host": {"provider": "lucide", "component": "House"}}
                ], "children": []},
                {"op": "element", "tag": 1, "parent": null}
            ],
            "parameters": [],
            "expressions": [], "actions": []
        }))
        .unwrap();
        let targets = component_call_targets(&app);
        assert_eq!(targets, vec![1]);
        assert!(validate_component_call_graph_acyclic(vec![targets, Vec::new()]).is_ok());
    }

    #[test]
    fn action_frame_slots_beyond_limit_are_rejected() {
        let action: TypedAction = serde_json::from_value(serde_json::json!({
            "instructions": [],
            "frameSlots": crate::limits::MAX_FRAME_SLOTS + 1,
        }))
        .unwrap();
        assert_eq!(
            validate_typed_action_contract(&action, 0, &[]),
            Err("action frame slots exceed limit")
        );
    }

    #[test]
    fn topology_validation_rejects_forbidden_and_unsupported_element_tags() {
        let tag_application = |tag: &str, namespace: &str| -> TypedApplication {
            serde_json::from_value(serde_json::json!({
                "version": "0.10",
                "rootNode": 0,
                "strings": ["div", tag],
                "nodes": [{
                    "op": "element",
                    "tag": 1,
                    "namespace": namespace,
                    "parent": null,
                    "children": []
                }],
                "expressions": [{"instructions": []}],
                "actions": [{"instructions": [{"op": "return"}]}]
            }))
            .unwrap()
        };
        for tag in [
            "script", "SCRIPT", "base", "object", "embed", "iframe", "link", "meta", "style",
        ] {
            assert_eq!(
                tag_application(tag, "html").validate_contract(),
                Err("forbidden element tag"),
                "{tag} must be forbidden"
            );
        }
        // SVG namespace does not rescue a forbidden HTML-namespace tag:
        // matching is by tag identity, not namespace.
        assert_eq!(
            tag_application("script", "svg").validate_contract(),
            Err("forbidden element tag")
        );
        assert_eq!(
            tag_application("foo", "html").validate_contract(),
            Err("unsupported element tag")
        );
        assert_eq!(
            tag_application("clipPath", "html").validate_contract(),
            Err("unsupported element tag")
        );
        assert_eq!(
            tag_application("div", "svg").validate_contract(),
            Err("unsupported element tag")
        );
        assert!(tag_application("div", "html").validate_contract().is_ok());
        assert!(tag_application("circle", "svg").validate_contract().is_ok());
    }

    #[test]
    fn topology_validation_rejects_custom_elements_without_policy() {
        let custom_application = |tag: &str| -> TypedApplication {
            serde_json::from_value(serde_json::json!({
                "version": "0.10",
                "rootNode": 0,
                "strings": ["div", tag],
                "nodes": [{"op": "element", "tag": 1, "parent": null, "children": []}],
                "expressions": [{"instructions": []}],
                "actions": [{"instructions": [{"op": "return"}]}]
            }))
            .unwrap()
        };
        assert_eq!(
            custom_application("my-widget").validate_contract(),
            Err("custom element tag not permitted")
        );
    }

    #[test]
    fn validate_with_policy_allows_only_configured_custom_elements() {
        let policy_application = |tag: &str| -> TypedApplication {
            serde_json::from_value(serde_json::json!({
                "version": "0.10",
                "rootNode": 0,
                "strings": ["div", tag],
                "nodes": [{"op": "element", "tag": 1, "parent": null, "children": []}],
                "expressions": [{"instructions": []}],
                "actions": [{"instructions": [{"op": "return"}]}]
            }))
            .unwrap()
        };
        let policy = plec_ir::sink::TagPolicy {
            custom_elements: std::collections::BTreeSet::from([String::from("my-widget")]),
        };
        assert!(policy_application("my-widget")
            .validate_with_policy(&policy)
            .is_ok());
        // The policy cannot rehabilitate forbidden tags: strict validation
        // still rejects `script` with the same identity rule.
        assert_eq!(
            policy_application("script").validate_contract(),
            Err("forbidden element tag")
        );
    }

    #[test]
    fn topology_validation_rejects_unknown_element_namespaces() {
        let namespace_application = |namespace: &str| -> TypedApplication {
            serde_json::from_value(serde_json::json!({
                "version": "0.10",
                "rootNode": 0,
                "strings": ["div"],
                "nodes": [{
                    "op": "element",
                    "tag": 0,
                    "namespace": namespace,
                    "parent": null,
                    "children": []
                }],
                "expressions": [{"instructions": []}],
                "actions": [{"instructions": [{"op": "return"}]}]
            }))
            .unwrap()
        };
        for namespace in ["math", "", "HTML", "xhtml", "SVG"] {
            assert_eq!(
                namespace_application(namespace).validate_contract(),
                Err("invalid element namespace"),
                "namespace {namespace:?} must be rejected"
            );
        }
    }

    #[test]
    fn topology_validation_rejects_out_of_range_structural_handles() {
        let element_child = topology_application(
            serde_json::json!([
                {"op": "element", "tag": 0, "parent": null, "children": [1]}
            ]),
            serde_json::json!([]),
        );
        assert_eq!(
            element_child.validate_contract(),
            Err("element child handle out of range")
        );

        let tag = topology_application(
            serde_json::json!([
                {"op": "element", "tag": 9, "parent": null, "children": []}
            ]),
            serde_json::json!([]),
        );
        assert_eq!(
            tag.validate_contract(),
            Err("element tag handle out of range")
        );

        let tag_application = |tag: &str, namespace: &str| -> TypedApplication {
            serde_json::from_value(serde_json::json!({
                "version": "0.10",
                "rootNode": 0,
                "strings": ["div", tag],
                "nodes": [{
                    "op": "element",
                    "tag": 1,
                    "namespace": namespace,
                    "parent": null,
                    "children": []
                }],
                "expressions": [{"instructions": []}],
                "actions": [{"instructions": [{"op": "return"}]}]
            }))
            .unwrap()
        };
        assert_eq!(
            tag_application("img src=x onerror=alert(1)", "html").validate_contract(),
            Err("unsafe element tag")
        );
        assert_eq!(
            tag_application("svg:script", "html").validate_contract(),
            Err("unsafe element tag")
        );
        assert!(tag_application("clipPath", "svg")
            .validate_contract()
            .is_ok());

        let text = topology_application(
            serde_json::json!([
                {"op": "text", "text": 5, "parent": null}
            ]),
            serde_json::json!([]),
        );
        assert_eq!(text.validate_contract(), Err("text handle out of range"));

        let loop_node = topology_application(
            serde_json::json!([
                {"op": "loop", "loop": 7, "parent": null}
            ]),
            serde_json::json!([]),
        );
        assert_eq!(
            loop_node.validate_contract(),
            Err("loop handle out of range")
        );

        let parent = topology_application(
            serde_json::json!([
                {"op": "element", "tag": 0, "parent": 9, "children": []}
            ]),
            serde_json::json!([]),
        );
        assert_eq!(
            parent.validate_contract(),
            Err("node parent handle out of range")
        );

        let branch = topology_application(
            serde_json::json!([
                {"op": "conditional", "test": 0, "parent": null, "consequent": 9, "alternate": null}
            ]),
            serde_json::json!([]),
        );
        assert_eq!(
            branch.validate_contract(),
            Err("conditional branch handle out of range")
        );

        let test_expression = topology_application(
            serde_json::json!([
                {"op": "conditional", "test": 9, "parent": null, "consequent": 0, "alternate": null}
            ]),
            serde_json::json!([]),
        );
        assert_eq!(
            test_expression.validate_contract(),
            Err("conditional test expression handle out of range")
        );

        let row_template: TypedApplication = serde_json::from_value(serde_json::json!({
            "version": "0.10",
            "rootNode": 0,
            "strings": ["div"],
            "nodes": [{"op": "element", "tag": 0, "parent": null, "children": []}],
            "expressions": [{"instructions": []}],
            "actions": [],
            "loops": [{
                "sourceExpression": 0, "keyExpression": 0, "itemSlot": 0,
                "rowTemplate": 9, "input": null
            }]
        }))
        .unwrap();
        assert_eq!(
            row_template.validate_contract(),
            Err("loop row template handle out of range")
        );
    }

    fn expression_application(instructions: Value) -> TypedApplication {
        serde_json::from_value(serde_json::json!({
            "version": "0.10",
            "rootNode": 0,
            "strings": ["div"],
            "nodes": [{"op": "element", "tag": 0, "parent": null, "children": []}],
            "expressions": [{"instructions": instructions}],
            "actions": []
        }))
        .unwrap()
    }

    #[test]
    fn expression_validation_rejects_out_of_range_jump_targets() {
        for op in ["jump", "jumpIfFalse", "jumpIfTrue"] {
            let app = expression_application(serde_json::json!([
                {"op": op, "target": 5},
                {"op": "return"}
            ]));
            assert_eq!(
                app.validate_contract(),
                Err("expression jump target out of range"),
                "{op}"
            );
        }
        // In-range forward jumps stay supported.
        let app = expression_application(serde_json::json!([
            {"op": "jumpIfFalse", "target": 1},
            {"op": "return"}
        ]));
        assert!(app.validate_contract().is_ok());
    }

    #[test]
    fn expression_validation_rejects_out_of_range_predicate_and_mapper_handles() {
        let filter = expression_application(serde_json::json!([
            {"op": "filter", "predicate": 9, "itemSlot": 0}
        ]));
        assert_eq!(
            filter.validate_contract(),
            Err("expression program handle out of range")
        );
        let map = expression_application(serde_json::json!([
            {"op": "map", "mapper": 9, "itemSlot": 0}
        ]));
        assert_eq!(
            map.validate_contract(),
            Err("expression program handle out of range")
        );
    }
}
