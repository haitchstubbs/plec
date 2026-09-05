use std::sync::Arc;

use async_graphql::{Context, EmptyMutation, EmptySubscription, Json, Object, Schema};

use plec_ir::{
    Binding, ComponentApplication, ComponentParameter, CookieCapability, DependencyEdge,
    DependencyEndpoint, Event, EventField, ExecutableComponent, HostSlot, Input, Listener, Loop,
    Node, PropProgram, PropWrite, Reaction, RefSlot, RouteOutlet, StateSlot, Text,
};

struct QueryRoot;

#[Object]
impl QueryRoot {
    async fn components(&self, ctx: &Context<'_>) -> Vec<ComponentInfo> {
        let application = ctx.data_unchecked::<Arc<ComponentApplication>>();

        application
            .components
            .iter()
            .map(ComponentInfo::from)
            .collect()
    }
}

type InspectorSchema = Schema<QueryRoot, EmptyMutation, EmptySubscription>;

pub struct Inspector {
    schema: InspectorSchema,
}

impl Inspector {
    pub fn new(application: &ComponentApplication) -> Self {
        let application = Arc::new(application.clone());

        let schema = Schema::build(QueryRoot, EmptyMutation, EmptySubscription)
            .data(application)
            .finish();

        Self { schema }
    }

    pub async fn query(&self, query: &str) -> async_graphql::Response {
        self.schema.execute(query).await
    }
}

fn index(value: usize) -> i32 {
    value as i32
}

#[Object]
impl ComponentInfo {
    async fn id(&self) -> &str {
        &self.id
    }

    async fn root_node(&self) -> i32 {
        index(self.root_node)
    }

    async fn node_count(&self) -> usize {
        self.node_count
    }

    async fn binding_count(&self) -> usize {
        self.binding_count
    }

    async fn strings(&self) -> &Vec<String> {
        &self.strings
    }

    async fn constants(&self) -> &Vec<Json<serde_json::Value>> {
        &self.constants
    }

    async fn nodes(&self) -> &Vec<NodeInfo> {
        &self.nodes
    }

    async fn texts(&self) -> &Vec<TextInfo> {
        &self.texts
    }

    async fn bindings(&self) -> &Vec<BindingInfo> {
        &self.bindings
    }

    async fn prop_programs(&self) -> &Vec<PropProgramInfo> {
        &self.prop_programs
    }

    async fn events(&self) -> &Vec<EventInfo> {
        &self.events
    }

    async fn inputs(&self) -> &Vec<InputInfo> {
        &self.inputs
    }

    async fn host_slots(&self) -> &Vec<HostSlotInfo> {
        &self.host_slots
    }

    async fn capabilities(&self) -> &Vec<CookieCapabilityInfo> {
        &self.capabilities
    }

    async fn state_slots(&self) -> &Vec<StateSlotInfo> {
        &self.state_slots
    }

    async fn ref_slots(&self) -> &Vec<RefSlotInfo> {
        &self.ref_slots
    }

    async fn host_ref_count(&self) -> usize {
        self.host_ref_count
    }

    async fn reactions(&self) -> &Vec<ReactionInfo> {
        &self.reactions
    }

    async fn listeners(&self) -> &Vec<ListenerInfo> {
        &self.listeners
    }

    async fn parameters(&self) -> &Vec<ComponentParameterInfo> {
        &self.parameters
    }

    async fn expressions(&self) -> Vec<Json<serde_json::Value>> {
        self.expressions.clone()
    }

    async fn actions(&self) -> Vec<Json<serde_json::Value>> {
        self.actions.clone()
    }

    async fn loops(&self) -> &Vec<LoopInfo> {
        &self.loops
    }

    async fn dependency_edges(&self) -> &Vec<DependencyEdgeInfo> {
        &self.dependency_edges
    }

    async fn route_outlets(&self) -> &Vec<RouteOutletInfo> {
        &self.route_outlets
    }
}

struct ComponentInfo {
    id: String,
    root_node: usize,
    node_count: usize,
    binding_count: usize,
    strings: Vec<String>,
    constants: Vec<Json<serde_json::Value>>,
    nodes: Vec<NodeInfo>,
    texts: Vec<TextInfo>,
    bindings: Vec<BindingInfo>,
    prop_programs: Vec<PropProgramInfo>,
    events: Vec<EventInfo>,
    inputs: Vec<InputInfo>,
    host_slots: Vec<HostSlotInfo>,
    capabilities: Vec<CookieCapabilityInfo>,
    state_slots: Vec<StateSlotInfo>,
    ref_slots: Vec<RefSlotInfo>,
    host_ref_count: usize,
    reactions: Vec<ReactionInfo>,
    listeners: Vec<ListenerInfo>,
    parameters: Vec<ComponentParameterInfo>,
    expressions: Vec<Json<serde_json::Value>>,
    actions: Vec<Json<serde_json::Value>>,
    loops: Vec<LoopInfo>,
    dependency_edges: Vec<DependencyEdgeInfo>,
    route_outlets: Vec<RouteOutletInfo>,
}

impl From<&ExecutableComponent> for ComponentInfo {
    fn from(component: &ExecutableComponent) -> Self {
        Self {
            id: component.id.clone(),
            root_node: component.root_node,
            node_count: component.nodes.len(),
            binding_count: component.bindings.len(),
            strings: component.strings.clone(),
            constants: component
                .constants
                .iter()
                .map(|value| Json(serde_json::to_value(value).expect("IR values serialize")))
                .collect(),
            nodes: component.nodes.iter().map(NodeInfo::from).collect(),
            texts: component.texts.iter().map(TextInfo::from).collect(),
            bindings: component.bindings.iter().map(BindingInfo::from).collect(),
            prop_programs: component
                .prop_programs
                .iter()
                .map(PropProgramInfo::from)
                .collect(),
            events: component.events.iter().map(EventInfo::from).collect(),
            inputs: component.inputs.iter().map(InputInfo::from).collect(),
            host_slots: component
                .host_slots
                .iter()
                .map(HostSlotInfo::from)
                .collect(),
            capabilities: component
                .capabilities
                .iter()
                .map(CookieCapabilityInfo::from)
                .collect(),
            state_slots: component
                .state_slots
                .iter()
                .map(StateSlotInfo::from)
                .collect(),
            ref_slots: component.ref_slots.iter().map(RefSlotInfo::from).collect(),
            host_ref_count: component.host_refs.len(),
            reactions: component.reactions.iter().map(ReactionInfo::from).collect(),
            listeners: component.listeners.iter().map(ListenerInfo::from).collect(),
            parameters: component
                .parameters
                .iter()
                .map(ComponentParameterInfo::from)
                .collect(),
            expressions: program_json(&component.expressions),
            actions: program_json(&component.actions),
            loops: component.loops.iter().map(LoopInfo::from).collect(),
            dependency_edges: component
                .dependency_edges
                .iter()
                .map(DependencyEdgeInfo::from)
                .collect(),
            route_outlets: component
                .route_outlets
                .iter()
                .map(RouteOutletInfo::from)
                .collect(),
        }
    }
}

fn program_json<I: serde::Serialize>(programs: &[I]) -> Vec<Json<serde_json::Value>> {
    programs
        .iter()
        .map(|program| Json(serde_json::to_value(program).expect("IR programs serialize")))
        .collect()
}

/// The IR `Node` enum flattened: `op` discriminates the variant and the
/// remaining fields are present only when the variant carries them. Field
/// names mirror the serde wire shape of `plec_ir::Node`.
#[Object]
impl NodeInfo {
    async fn op(&self) -> &str {
        &self.op
    }

    async fn tag(&self) -> Option<i32> {
        self.tag.map(index)
    }

    async fn namespace(&self) -> Option<&str> {
        self.namespace.as_deref()
    }

    async fn parent(&self) -> Option<i32> {
        self.parent.map(index)
    }

    async fn children(&self) -> &Vec<i32> {
        &self.children
    }

    async fn host_ref(&self) -> Option<i32> {
        self.host_ref.map(index)
    }

    async fn text(&self) -> Option<i32> {
        self.text.map(index)
    }

    async fn test(&self) -> Option<i32> {
        self.test.map(index)
    }

    async fn consequent(&self) -> Option<i32> {
        self.consequent.map(index)
    }

    async fn alternate(&self) -> Option<i32> {
        self.alternate.map(index)
    }

    async fn r#loop(&self) -> Option<i32> {
        self.loop_.map(index)
    }

    async fn component(&self) -> Option<i32> {
        self.component.map(index)
    }

    async fn props(&self) -> &Vec<ComponentPropInfo> {
        &self.props
    }

    async fn prop(&self) -> Option<i32> {
        self.prop.map(index)
    }
}

struct NodeInfo {
    op: String,
    tag: Option<usize>,
    namespace: Option<String>,
    parent: Option<usize>,
    children: Vec<i32>,
    host_ref: Option<usize>,
    text: Option<usize>,
    test: Option<usize>,
    consequent: Option<usize>,
    alternate: Option<usize>,
    loop_: Option<usize>,
    component: Option<usize>,
    props: Vec<ComponentPropInfo>,
    prop: Option<usize>,
}

impl From<&Node> for NodeInfo {
    fn from(node: &Node) -> Self {
        match node {
            Node::Element {
                tag,
                namespace,
                parent,
                children,
                host_ref,
            } => Self {
                op: "element".into(),
                tag: Some(*tag),
                namespace: Some((*namespace).to_string()),
                parent: *parent,
                children: children.iter().map(|child| index(*child)).collect(),
                host_ref: *host_ref,
                text: None,
                test: None,
                consequent: None,
                alternate: None,
                loop_: None,
                component: None,
                props: Vec::new(),
                prop: None,
            },
            Node::Text { text, parent } => Self {
                op: "text".into(),
                tag: None,
                namespace: None,
                parent: *parent,
                children: Vec::new(),
                host_ref: None,
                text: Some(*text),
                test: None,
                consequent: None,
                alternate: None,
                loop_: None,
                component: None,
                props: Vec::new(),
                prop: None,
            },
            Node::Conditional {
                test,
                parent,
                consequent,
                alternate,
            } => Self {
                op: "conditional".into(),
                tag: None,
                namespace: None,
                parent: *parent,
                children: Vec::new(),
                host_ref: None,
                text: None,
                test: Some(*test),
                consequent: Some(*consequent),
                alternate: *alternate,
                loop_: None,
                component: None,
                props: Vec::new(),
                prop: None,
            },
            Node::Loop { r#loop, parent } => Self {
                op: "loop".into(),
                tag: None,
                namespace: None,
                parent: *parent,
                children: Vec::new(),
                host_ref: None,
                text: None,
                test: None,
                consequent: None,
                alternate: None,
                loop_: Some(*r#loop),
                component: None,
                props: Vec::new(),
                prop: None,
            },
            Node::Component {
                component,
                parent,
                props,
                children,
            } => Self {
                op: "component".into(),
                tag: None,
                namespace: None,
                parent: *parent,
                children: children.iter().map(|child| index(*child)).collect(),
                host_ref: None,
                text: None,
                test: None,
                consequent: None,
                alternate: None,
                loop_: None,
                component: Some(*component),
                props: props.iter().map(ComponentPropInfo::from).collect(),
                prop: None,
            },
            Node::DynamicComponent {
                prop,
                parent,
                props,
                children,
            } => Self {
                op: "dynamicComponent".into(),
                tag: None,
                namespace: None,
                parent: *parent,
                children: children.iter().map(|child| index(*child)).collect(),
                host_ref: None,
                text: None,
                test: None,
                consequent: None,
                alternate: None,
                loop_: None,
                component: None,
                props: props.iter().map(ComponentPropInfo::from).collect(),
                prop: Some(*prop),
            },
            Node::Slot { parent } => Self {
                op: "slot".into(),
                tag: None,
                namespace: None,
                parent: *parent,
                children: Vec::new(),
                host_ref: None,
                text: None,
                test: None,
                consequent: None,
                alternate: None,
                loop_: None,
                component: None,
                props: Vec::new(),
                prop: None,
            },
        }
    }
}

/// The IR `ComponentProp` enum flattened; `kind` discriminates.
#[Object]
impl ComponentPropInfo {
    async fn kind(&self) -> &str {
        &self.kind
    }

    async fn name(&self) -> i32 {
        index(self.name)
    }

    async fn expression(&self) -> Option<i32> {
        self.expression.map(index)
    }

    async fn action(&self) -> Option<i32> {
        self.action.map(index)
    }

    async fn component(&self) -> Option<i32> {
        self.component.map(index)
    }
}

struct ComponentPropInfo {
    kind: String,
    name: usize,
    expression: Option<usize>,
    action: Option<usize>,
    component: Option<usize>,
}

impl From<&plec_ir::ComponentProp> for ComponentPropInfo {
    fn from(prop: &plec_ir::ComponentProp) -> Self {
        match prop {
            plec_ir::ComponentProp::Value { name, expression } => Self {
                kind: "value".into(),
                name: *name,
                expression: Some(*expression),
                action: None,
                component: None,
            },
            plec_ir::ComponentProp::Callable { name, action } => Self {
                kind: "callable".into(),
                name: *name,
                expression: None,
                action: Some(*action),
                component: None,
            },
            plec_ir::ComponentProp::Component { name, component } => Self {
                kind: "component".into(),
                name: *name,
                expression: None,
                action: None,
                component: Some(*component),
            },
        }
    }
}

#[Object]
impl TextInfo {
    async fn value(&self) -> Option<&str> {
        self.0.value.as_deref()
    }

    async fn binding(&self) -> Option<i32> {
        self.0.binding.map(index)
    }
}

struct TextInfo(Text);

impl From<&Text> for TextInfo {
    fn from(text: &Text) -> Self {
        Self(text.clone())
    }
}

#[Object]
impl BindingInfo {
    async fn target(&self) -> i32 {
        index(self.0.target)
    }

    async fn sink(&self) -> &str {
        self.0.sink
    }

    async fn name(&self) -> Option<i32> {
        self.0.name.map(index)
    }

    async fn expression(&self) -> i32 {
        index(self.0.expression)
    }
}

struct BindingInfo(Binding);

impl From<&Binding> for BindingInfo {
    fn from(binding: &Binding) -> Self {
        Self(binding.clone())
    }
}

#[Object]
impl PropProgramInfo {
    async fn target(&self) -> i32 {
        index(self.0.target)
    }

    async fn writes(&self) -> Vec<PropWriteInfo> {
        self.0.writes.iter().map(PropWriteInfo::from).collect()
    }
}

struct PropProgramInfo(PropProgram);

impl From<&PropProgram> for PropProgramInfo {
    fn from(program: &PropProgram) -> Self {
        Self(program.clone())
    }
}

#[Object]
impl PropWriteInfo {
    async fn name(&self) -> Option<i32> {
        self.0.name.map(index)
    }

    async fn kind(&self) -> &str {
        self.0.kind
    }

    async fn constant(&self) -> Option<i32> {
        self.0.constant.map(index)
    }

    async fn expression(&self) -> Option<i32> {
        self.0.expression.map(index)
    }

    async fn spread(&self) -> bool {
        self.0.spread
    }
}

struct PropWriteInfo(PropWrite);

impl From<&PropWrite> for PropWriteInfo {
    fn from(write: &PropWrite) -> Self {
        Self(write.clone())
    }
}

#[Object]
impl EventInfo {
    async fn target(&self) -> i32 {
        index(self.0.target)
    }

    async fn event_type(&self) -> i32 {
        index(self.0.event_type)
    }

    async fn action(&self) -> i32 {
        index(self.0.action)
    }

    async fn r#loop(&self) -> Option<i32> {
        self.0.r#loop.map(index)
    }

    async fn fields(&self) -> Vec<EventFieldInfo> {
        self.0.fields.iter().map(EventFieldInfo::from).collect()
    }
}

struct EventInfo(Event);

impl From<&Event> for EventInfo {
    fn from(event: &Event) -> Self {
        Self(event.clone())
    }
}

#[Object]
impl EventFieldInfo {
    async fn name(&self) -> i32 {
        index(self.0.name)
    }

    async fn slot(&self) -> i32 {
        index(self.0.slot)
    }
}

struct EventFieldInfo(EventField);

impl From<&EventField> for EventFieldInfo {
    fn from(field: &EventField) -> Self {
        Self(field.clone())
    }
}

#[Object]
impl InputInfo {
    async fn name(&self) -> i32 {
        index(self.0.name)
    }

    async fn kind(&self) -> &str {
        self.0.kind
    }
}

struct InputInfo(Input);

impl From<&Input> for InputInfo {
    fn from(input: &Input) -> Self {
        Self(input.clone())
    }
}

#[Object]
impl HostSlotInfo {
    async fn kind(&self) -> &str {
        self.0.kind
    }

    async fn query(&self) -> Option<i32> {
        self.0.query.map(index)
    }

    async fn name(&self) -> Option<i32> {
        self.0.name.map(index)
    }
}

struct HostSlotInfo(HostSlot);

impl From<&HostSlot> for HostSlotInfo {
    fn from(slot: &HostSlot) -> Self {
        Self(slot.clone())
    }
}

#[Object]
impl CookieCapabilityInfo {
    async fn kind(&self) -> &str {
        self.0.kind
    }

    async fn name(&self) -> &str {
        &self.0.name
    }

    async fn operations(&self) -> &Vec<&'static str> {
        &self.0.operations
    }

    async fn path(&self) -> &str {
        &self.0.path
    }

    async fn same_site(&self) -> Option<&str> {
        self.0.same_site.as_deref()
    }

    async fn secure(&self) -> Option<bool> {
        self.0.secure
    }

    async fn expiry_modes(&self) -> &Vec<&'static str> {
        &self.0.expiry_modes
    }
}

struct CookieCapabilityInfo(CookieCapability);

impl From<&CookieCapability> for CookieCapabilityInfo {
    fn from(capability: &CookieCapability) -> Self {
        Self(capability.clone())
    }
}

#[Object]
impl StateSlotInfo {
    async fn initial_expression(&self) -> i32 {
        index(self.0.initial_expression)
    }

    async fn frame_slot(&self) -> i32 {
        index(self.0.frame_slot)
    }
}

struct StateSlotInfo(StateSlot);

impl From<&StateSlot> for StateSlotInfo {
    fn from(slot: &StateSlot) -> Self {
        Self(slot.clone())
    }
}

#[Object]
impl RefSlotInfo {
    async fn initial_expression(&self) -> i32 {
        index(self.0.initial_expression)
    }
}

struct RefSlotInfo(RefSlot);

impl From<&RefSlot> for RefSlotInfo {
    fn from(slot: &RefSlot) -> Self {
        Self(slot.clone())
    }
}

#[Object]
impl ReactionInfo {
    async fn dependencies(&self) -> Vec<i32> {
        self.0.dependencies.iter().map(|dep| index(*dep)).collect()
    }

    async fn action(&self) -> i32 {
        index(self.0.action)
    }

    async fn cleanup_action(&self) -> Option<i32> {
        self.0.cleanup_action.map(index)
    }
}

struct ReactionInfo(Reaction);

impl From<&Reaction> for ReactionInfo {
    fn from(reaction: &Reaction) -> Self {
        Self(reaction.clone())
    }
}

#[Object]
impl ListenerInfo {
    async fn source(&self) -> &str {
        self.0.source
    }

    async fn event(&self) -> i32 {
        index(self.0.event)
    }

    async fn action(&self) -> i32 {
        index(self.0.action)
    }
}

struct ListenerInfo(Listener);

impl From<&Listener> for ListenerInfo {
    fn from(listener: &Listener) -> Self {
        Self(listener.clone())
    }
}

#[Object]
impl ComponentParameterInfo {
    async fn name(&self) -> i32 {
        index(self.0.name)
    }

    async fn callable(&self) -> bool {
        self.0.callable
    }

    async fn component(&self) -> bool {
        self.0.component
    }
}

struct ComponentParameterInfo(ComponentParameter);

impl From<&ComponentParameter> for ComponentParameterInfo {
    fn from(parameter: &ComponentParameter) -> Self {
        Self(parameter.clone())
    }
}

#[Object]
impl LoopInfo {
    async fn source_expression(&self) -> i32 {
        index(self.0.source_expression)
    }

    async fn key_expression(&self) -> i32 {
        index(self.0.key_expression)
    }

    async fn item_slot(&self) -> i32 {
        index(self.0.item_slot)
    }

    async fn row_template(&self) -> i32 {
        index(self.0.row_template)
    }

    async fn dependency_slots(&self) -> Vec<i32> {
        self.0
            .dependency_slots
            .iter()
            .map(|slot| index(*slot))
            .collect()
    }

    async fn input(&self) -> Option<i32> {
        self.0.input.map(index)
    }
}

struct LoopInfo(Loop);

impl From<&Loop> for LoopInfo {
    fn from(r#loop: &Loop) -> Self {
        Self(r#loop.clone())
    }
}

#[Object]
impl DependencyEdgeInfo {
    async fn source(&self) -> DependencyEndpointInfo {
        DependencyEndpointInfo::from(&self.0.source)
    }

    async fn target(&self) -> DependencyEndpointInfo {
        DependencyEndpointInfo::from(&self.0.target)
    }
}

struct DependencyEdgeInfo(DependencyEdge);

impl From<&DependencyEdge> for DependencyEdgeInfo {
    fn from(edge: &DependencyEdge) -> Self {
        Self(edge.clone())
    }
}

#[Object]
impl DependencyEndpointInfo {
    async fn kind(&self) -> &str {
        self.0.kind
    }

    async fn handle(&self) -> i32 {
        index(self.0.handle)
    }

    async fn r#loop(&self) -> Option<i32> {
        self.0.r#loop.map(index)
    }
}

struct DependencyEndpointInfo(DependencyEndpoint);

impl From<&DependencyEndpoint> for DependencyEndpointInfo {
    fn from(endpoint: &DependencyEndpoint) -> Self {
        Self(endpoint.clone())
    }
}

#[Object]
impl RouteOutletInfo {
    async fn id(&self) -> &str {
        &self.0.id
    }

    async fn node(&self) -> i32 {
        index(self.0.node)
    }
}

struct RouteOutletInfo(RouteOutlet);

impl From<&RouteOutlet> for RouteOutletInfo {
    fn from(outlet: &RouteOutlet) -> Self {
        Self(outlet.clone())
    }
}
