use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};
use wasm_bindgen::{closure::Closure, prelude::*, JsCast};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{
    AbortController, Comment, Document, Element, Event, EventTarget, HtmlInputElement,
    KeyboardEvent, MouseEvent, Node, Request, RequestInit, Response,
};

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Application {
    version: String,
    root_element_id: String,
    #[serde(default)]
    revision: Option<String>,
    elements: Vec<ElementNode>,
    texts: Vec<TextNode>,
    #[serde(default)]
    bindings: Vec<Binding>,
    #[serde(default)]
    expressions: Vec<Expression>,
    #[serde(default)]
    prop_programs: Vec<PropProgram>,
    #[serde(default)]
    events: Vec<EventBinding>,
    #[serde(default)]
    loops: Vec<Loop>,
    #[serde(default)]
    conditionals: Vec<Conditional>,
    #[serde(default)]
    contexts: Vec<ContextScope>,
    #[serde(default)]
    context_definitions: Vec<ContextDefinition>,
    #[serde(default)]
    host_element_refs: Vec<HostElementRef>,
    #[serde(default)]
    actions: Vec<Action>,
    #[serde(default)]
    local_states: Vec<LocalState>,
    #[serde(default)]
    layout: LayoutMetadata,
}

/* 0.9 is intentionally a separate decoded shape while route compatibility
 * still keeps the historical 0.8 executor below.  This avoids translating
 * executable tables back into demo-shaped string IDs. */
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TypedApplication {
    version: String,
    root_node: usize,
    strings: Vec<String>,
    #[serde(default)]
    constants: Vec<Value>,
    nodes: Vec<TypedNode>,
    #[serde(default)]
    texts: Vec<TypedText>,
    #[serde(default)]
    bindings: Vec<TypedBinding>,
    #[serde(default)]
    events: Vec<TypedEvent>,
    #[serde(default)]
    inputs: Vec<TypedInput>,
    #[serde(default)]
    state_slots: Vec<TypedStateSlot>,
    #[serde(default)]
    expressions: Vec<TypedProgram>,
    #[serde(default)]
    actions: Vec<TypedAction>,
    #[serde(default)]
    loops: Vec<TypedLoop>,
    #[serde(default)]
    dependency_edges: Vec<TypedDependencyEdge>,
}
#[derive(Clone, Deserialize)]
struct TypedEvent { target: usize, #[serde(rename = "type")] event_type: usize, action: usize, #[serde(default)] fields: Vec<usize>, r#loop: Option<usize> }
#[derive(Clone, Deserialize)]
struct TypedAction { instructions: Vec<TypedActionInstruction>, #[serde(default)] frame_slots: usize, #[serde(default)] parameter_slots: Vec<usize>, loader_result_state: Option<usize>, #[serde(default)] route_loader: bool }
#[derive(Clone, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
enum TypedActionInstruction {
    Evaluate { expression: usize }, StoreState { state: usize },
    CollectionMutation { input: usize, kind: String, key: usize, value: Option<usize> }, PreventDefault,
    Call { action: usize, #[serde(default)] arguments: Vec<usize> },
    Jump { target: usize }, JumpIfFalse { target: usize },
    CapabilityRequest { capability: String, request: TypedFetchRequest, success_pc: usize, failure_pc: usize, finally_pc: Option<usize>, result_slot: usize, error_slot: usize },
    Return,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TypedFetchRequest { url: usize, method: String, #[serde(default)] headers: Vec<TypedFetchHeader>, body: Option<usize>, decode: String, #[serde(default = "default_true")] require_ok: bool }
#[derive(Clone, Deserialize)]
struct TypedFetchHeader { name: usize, value: usize }
fn default_true() -> bool { true }

/** Rules shared by the WASM trust boundary and host-native unit tests. */
fn validate_typed_action_contract(
    action: &TypedAction,
    expression_count: usize,
    input_kinds: &[String],
) -> Result<(), &'static str> {
    let mut parameter_slots = HashSet::new();
    for slot in &action.parameter_slots {
        if *slot >= action.frame_slots { return Err("action parameter frame slot out of range"); }
        if !parameter_slots.insert(*slot) { return Err("duplicate action parameter frame slot"); }
    }
    for instruction in &action.instructions {
        if let TypedActionInstruction::CollectionMutation { input, key, value, kind } = instruction {
            if *input >= input_kinds.len() { return Err("action input handle out of range"); }
            if input_kinds[*input] != "collection" { return Err("collection mutation requires a collection input"); }
            if *key >= expression_count { return Err("collection mutation key expression handle out of range"); }
            match (kind.as_str(), value) {
                ("append" | "keyedReplace", Some(value)) if *value < expression_count => {},
                ("append" | "keyedReplace", Some(_)) => return Err("collection mutation value expression handle out of range"),
                ("append" | "keyedReplace", None) => return Err("collection mutation requires a value expression"),
                ("keyedRemove", None) => {},
                ("keyedRemove", Some(_)) => return Err("collection remove forbids a value expression"),
                _ => return Err("unknown collection mutation kind"),
            }
        }
    }
    Ok(())
}
#[derive(Clone, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
enum TypedNode {
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
struct TypedText {
    value: Option<String>,
    binding: Option<usize>,
}
#[derive(Clone, Deserialize)]
struct TypedBinding {
    target: usize,
    sink: String,
    name: Option<usize>,
    expression: usize,
}
#[derive(Clone, Deserialize)]
struct TypedInput {
    name: usize,
    kind: String,
}
#[derive(Clone, Deserialize)]
struct TypedStateSlot {
    initial_expression: usize,
    frame_slot: usize,
}
#[derive(Clone, Deserialize)]
struct TypedProgram {
    instructions: Vec<Value>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TypedLoop {
    source_expression: usize,
    key_expression: usize,
    item_slot: usize,
    index_slot: Option<usize>,
    row_template: usize,
    #[serde(default)]
    dependency_slots: Vec<usize>,
    input: Option<usize>,
}
#[derive(Clone, Deserialize)]
struct TypedDependencyEdge {
    source: TypedDependencyEndpoint,
    target: TypedDependencyEndpoint,
}
#[derive(Clone, Deserialize)]
struct TypedDependencyEndpoint {
    kind: String,
    handle: usize,
    r#loop: Option<usize>,
}
struct TypedRow {
    root: Node,
    values: serde_json::Map<String, Value>,
    nodes: HashMap<usize, Node>,
}
#[derive(Default)]
struct TypedLoopRows {
    order: Vec<String>,
    rows: HashMap<String, TypedRow>,
}
struct TypedRuntime {
    app: TypedApplication,
    root: Option<Element>,
    nodes: HashMap<usize, Node>,
    states: Vec<Value>,
    loops: HashMap<usize, TypedLoopRows>,
    listeners: Vec<Listener>,
    pending_fetches: Vec<TypedPendingFetch>,
}
#[derive(Clone)]
struct TypedPendingFetch { action: usize, success_pc: usize, failure_pc: usize, finally_pc: Option<usize>, result_slot: usize, error_slot: usize, frame: Vec<Value>, event: Vec<Value>, row: Option<serde_json::Map<String, Value>>, url: String, method: String, headers: Vec<(String, String)>, body: Option<String>, decode: String, require_ok: bool }
#[derive(Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct LayoutMetadata {
    route_outlets: Vec<RouteOutlet>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RouteOutlet {
    id: String,
    element_id: String,
}
#[derive(Clone, Deserialize)]
struct Action {
    id: String,
    #[serde(default)]
    operations: Vec<Value>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LocalState {
    id: String,
    name: String,
    initial_value: String,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ElementNode {
    id: String,
    tag: String,
    #[serde(default)]
    attributes: Vec<Attribute>,
    #[serde(default)]
    children: Vec<String>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Attribute {
    name: String,
    static_value: Option<String>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TextNode {
    id: String,
    static_value: Option<String>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Binding {
    #[serde(rename = "id")]
    _id: String,
    kind: String,
    target_id: String,
    attribute_name: Option<String>,
    expression_id: Option<String>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PropProgram {
    target_id: String,
    writes: Vec<PropWrite>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PropWrite {
    name: String,
    static_value: Option<String>,
    expression_id: Option<String>,
    kind: String,
}
#[derive(Clone, Deserialize)]
struct Expression {
    id: String,
    expression: Value,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EventBinding {
    #[serde(rename = "type")]
    event_type: String,
    target_id: String,
    action_id: String,
    field: Option<String>,
    loop_id: Option<String>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Loop {
    #[serde(rename = "id")]
    id: String,
    parent_id: String,
    input_id: Option<String>,
    query_id: Option<String>,
    #[serde(default = "default_row_item_name")]
    item_name: String,
    row_template_root_element_id: Option<String>,
    #[serde(default)]
    rows: Vec<LoopRow>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoopRow {
    root_element_id: String,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Conditional {
    id: String,
    parent_id: String,
    expression_id: String,
    #[serde(default)]
    consequent: Vec<String>,
    #[serde(default)]
    alternate: Vec<String>,
}
fn default_row_item_name() -> String {
    "todo".into()
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContextScope {
    id: String,
    context_id: String,
    value_expression_id: Option<String>,
    #[serde(default)]
    children: Vec<String>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContextDefinition {
    id: String,
    default_expression_id: String,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HostElementRef {
    id: String,
    target_id: String,
    #[serde(default)]
    attachments: Vec<String>,
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Delta {
    Update {
        input_id: String,
        row_key: String,
        changes: HashMap<String, Value>,
    },
    Insert {
        input_id: String,
        row_key: String,
        row: HashMap<String, Value>,
        before_row_key: Option<String>,
    },
    Remove {
        input_id: String,
        row_key: String,
    },
    Move {
        input_id: String,
        row_key: String,
        before_row_key: Option<String>,
    },
}
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateMetrics {
    dom_operations: u32,
    nodes_touched: u32,
    bindings_touched: u32,
    wasm_dom_us: f64,
}
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct MountMetrics {
    decode_us: f64,
    static_mount_us: f64,
    row_program_execute_us: f64,
    row_state_registration_us: f64,
    fragment_append_us: f64,
    program_compile_us: f64,
    program_revision: Option<String>,
    instruction_count: u32,
    compiled_expression_count: u32,
    field_slot_count: u32,
    binding_program_count: u32,
    average_row_program_execute_us: f64,
    row_count: u32,
    created_elements: u32,
    created_texts: u32,
    bindings: u32,
    dom_operations: u32,
}
struct Row {
    root: Node,
    values: HashMap<String, Value>,
    nodes: HashMap<String, Node>,
}

/** Runtime-owned identity. ComponentGraph ids are compiler artifacts; an
 * instance id is stable for its mount position, outlet, and optional key. */
struct GraphInstance {
    parent_id: Option<String>,
    outlet_id: String,
    key: Option<String>,
    graph_id: Option<String>,
    // These resources are deliberately owned by the topology instance rather
    // than the application definition. They are the boundary required for a
    // persistent layout with independently replaced outlet children.
    local_state: HashMap<String, Value>,
    dom_nodes: HashMap<String, Node>,
    host_refs: HashMap<String, Node>,
    rows: HashMap<String, HashMap<String, Row>>,
    root: Option<Node>,
    listeners: Vec<Listener>,
    continuation_epoch: u64,
    next_request_id: u64,
    abort_controllers: HashMap<u64, AbortController>,
    active_effects: usize,
    active_listeners: usize,
}
struct Listener {
    element: Element,
    event_type: String,
    callback: Closure<dyn FnMut(Event)>,
}
struct RouterListener {
    target: EventTarget,
    event_type: String,
    callback: Closure<dyn FnMut(Event)>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RouteManifest {
    #[serde(default)]
    version: Option<u32>,
    root_graph_id: String,
    routes: Vec<RouteManifestEntry>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RouteManifestEntry {
    path: String,
    graph_id: String,
    outlet_id: String,
    #[serde(default)]
    loader: Option<Action>,
    #[serde(default)]
    loader_state_slot_id: Option<String>,
    #[serde(default)]
    loader_action: Option<usize>,
}
#[derive(Clone)]
struct RouterState {
    manifest: RouteManifest,
    root_instance_id: String,
}

#[wasm_bindgen]
pub struct PlecRuntime {
    /** Immutable compiled definitions. Mount state stays in instances; loading
     * a second graph must not overwrite the definition of the first. */
    registry: Rc<RefCell<HashMap<String, Application>>>,
    instances: Rc<RefCell<HashMap<String, GraphInstance>>>,
    router: Rc<RefCell<Option<RouterState>>>,
    router_listeners: Rc<RefCell<Vec<RouterListener>>>,
    typed: Rc<RefCell<Option<TypedRuntime>>>,
    typed_registry: Rc<RefCell<HashMap<String, TypedApplication>>>,
    typed_manifest: Rc<RefCell<Option<RouteManifest>>>,
}
impl Clone for PlecRuntime {
    fn clone(&self) -> Self {
        Self {
            registry: Rc::clone(&self.registry),
            instances: Rc::clone(&self.instances),
            router: Rc::clone(&self.router),
            router_listeners: Rc::clone(&self.router_listeners),
            typed: Rc::clone(&self.typed),
            typed_registry: Rc::clone(&self.typed_registry),
            typed_manifest: Rc::clone(&self.typed_manifest),
        }
    }
}
#[wasm_bindgen]
impl PlecRuntime {
    #[wasm_bindgen(constructor)]
    pub fn new() -> PlecRuntime {
        PlecRuntime {
            registry: Rc::new(RefCell::new(HashMap::new())),
            instances: Rc::new(RefCell::new(HashMap::new())),
            router: Rc::new(RefCell::new(None)),
            router_listeners: Rc::new(RefCell::new(Vec::new())),
            typed: Rc::new(RefCell::new(None)),
            typed_registry: Rc::new(RefCell::new(HashMap::new())),
            typed_manifest: Rc::new(RefCell::new(None)),
        }
    }
    pub fn load_application(&self, ir: JsValue) -> Result<(), JsValue> {
        let value: Value = serde_wasm_bindgen::from_value(ir.clone()).map_err(error)?;
        if value.get("version").and_then(Value::as_str) == Some("0.9") {
            let app: TypedApplication = serde_json::from_value(value).map_err(error)?;
            *self.typed.borrow_mut() = Some(TypedRuntime::new(app)?);
            return Ok(());
        }
        let application: Application = serde_wasm_bindgen::from_value(ir).map_err(error)?;
        if application.version != "0.8" {
            return Err(JsValue::from_str("unsupported application IR version"));
        }
        // The historical single-graph API remains a thin adapter for existing
        // callers. It is a registry entry, never runtime-global execution state.
        self.registry
            .borrow_mut()
            .insert("__legacy__".into(), application);
        Ok(())
    }
    /** Registering is definition-only: it creates no DOM, listeners, or
     * runtime identity. A route/outlet mount chooses an instance later. */
    pub fn register_graph(&self, graph_id: String, ir: JsValue) -> Result<(), JsValue> {
        let value: Value = serde_wasm_bindgen::from_value(ir.clone()).map_err(error)?;
        if value.get("version").and_then(Value::as_str) == Some("0.9") {
            let app: TypedApplication = serde_json::from_value(value).map_err(error)?;
            app.validate()?;
            self.typed_registry.borrow_mut().insert(graph_id, app);
            return Ok(());
        }
        let application: Application = serde_wasm_bindgen::from_value(ir).map_err(error)?;
        if application.version != "0.8" {
            return Err(JsValue::from_str("unsupported application IR version"));
        }
        self.registry.borrow_mut().insert(graph_id, application);
        Ok(())
    }
    pub fn registered_graph_count(&self) -> u32 {
        self.registry.borrow().len() as u32
    }
    /** Runtime-owned application entrypoint. Graph definitions are registered
     * by the thin browser transport before start; route matching, history and
     * outlet replacement never return to TypeScript. */
    pub fn start(&self, root: Element, manifest: JsValue) -> Result<(), JsValue> {
        let manifest: RouteManifest = serde_wasm_bindgen::from_value(manifest).map_err(error)?;
        if manifest.version == Some(3) {
            if !self.typed_registry.borrow().contains_key(&manifest.root_graph_id) {
                return Err(JsValue::from_str("typed root graph is not registered"));
            }
            *self.typed_manifest.borrow_mut() = Some(manifest);
            return self.navigate_typed_route(&window()?.location().pathname().unwrap_or_else(|_| "/".into()), root);
        }
        self.dispose_router_listeners();
        let root_instance_id = graph_instance_id(None, "main", None);
        if self.instances.borrow().contains_key(&root_instance_id) {
            self.dispose_graph_instance(root_instance_id.clone())?;
        }
        self.create_instance(
            root_instance_id.clone(),
            None,
            "main".into(),
            None,
            manifest.root_graph_id.clone(),
        )?;
        self.mount_instance(&root_instance_id, root, true)?;
        *self.router.borrow_mut() = Some(RouterState {
            manifest,
            root_instance_id,
        });
        self.install_router_listeners()?;
        let pathname = window()?
            .location()
            .pathname()
            .unwrap_or_else(|_| "/".into());
        self.navigate_internal(&pathname, true)
    }
    pub fn navigate(&self, href: String, replace: bool) -> Result<(), JsValue> {
        if self.typed_manifest.borrow().is_some() {
            let root = self.typed.borrow().as_ref().and_then(|typed| typed.root.clone())
                .ok_or_else(|| JsValue::from_str("typed router has not started"))?;
            return self.navigate_typed_route(&href, root);
        }
        self.navigate_internal(&href, replace)
    }
    pub fn mount(&self, root: Element) -> Result<JsValue, JsValue> {
        if self.typed.borrow().is_some() {
            return self.mount_typed(root);
        }
        let instance_id = graph_instance_id(None, "main", None);
        if self.instances.borrow().contains_key(&instance_id) {
            self.dispose_graph_instance(instance_id.clone())?;
        }
        self.create_instance(
            instance_id.clone(),
            None,
            "main".into(),
            None,
            "__legacy__".into(),
        )?;
        self.mount_instance(&instance_id, root, true)
    }
    fn mount_instance(
        &self,
        instance_id: &str,
        root: Element,
        replace_root: bool,
    ) -> Result<JsValue, JsValue> {
        let app = self.app_for_instance(instance_id)?;
        let doc = document()?;
        let elements = index_elements(&app);
        let texts = index_texts(&app);
        let mut nodes = HashMap::new();
        let contexts = index_contexts(&app);
        let loops = index_loops(&app);
        let conditionals = index_conditionals(&app);
        let environment = context_defaults(&app);
        let root_node = instantiate(
            &doc,
            &app.root_element_id,
            &elements,
            &texts,
            &contexts,
            &loops,
            &conditionals,
            &app.bindings,
            &app.prop_programs,
            &app.expressions,
            &app.events,
            &self.state_scope(instance_id, &app)?,
            &environment,
            &mut nodes,
        )?;
        if replace_root {
            root.set_inner_html("");
        }
        root.append_child(&root_node)?;
        let mut handles = HashMap::new();
        for reference in &app.host_element_refs {
            if let Some(node) = nodes.get(&reference.target_id) {
                for attachment in &reference.attachments {
                    handles.insert(attachment.clone(), node.clone());
                }
                handles.insert(reference.id.clone(), node.clone());
            }
        }
        for binding in &app.bindings {
            if let Some(node) = nodes.get(&binding.target_id) {
                apply_binding_host(
                    node,
                    binding,
                    &app.expressions,
                    &self.state_scope(instance_id, &app)?,
                    &handles,
                )?;
            }
        }
        {
            let mut instances = self.instances.borrow_mut();
            let instance = instances
                .get_mut(instance_id)
                .ok_or_else(|| JsValue::from_str("unknown graph instance"))?;
            instance.dom_nodes = nodes;
            instance.host_refs = handles;
            instance.rows.clear();
            instance.root = Some(root_node);
        }
        self.install_event_listeners(instance_id, &app)?;
        finish(MountMetrics {
            program_revision: app.revision.clone(),
            created_elements: app.elements.len() as u32,
            created_texts: app.texts.len() as u32,
            bindings: app.bindings.len() as u32,
            dom_operations: 1,
            ..Default::default()
        })
    }
    pub fn adopt(&self, root: Element) -> Result<JsValue, JsValue> {
        let instance_id = graph_instance_id(None, "main", None);
        if self.instances.borrow().contains_key(&instance_id) {
            self.dispose_graph_instance(instance_id.clone())?;
        }
        self.create_instance(
            instance_id.clone(),
            None,
            "main".into(),
            None,
            "__legacy__".into(),
        )?;
        let app = self.app_for_instance(&instance_id)?;
        let root_selector = format!("[data-runtime-node=\"{}\"]", app.root_element_id);
        let Some(runtime_root) = root.query_selector(&root_selector)? else {
            return serde_wasm_bindgen::to_value(&Option::<MountMetrics>::None).map_err(error);
        };
        let mut nodes = HashMap::new();
        nodes.insert(app.root_element_id.clone(), runtime_root.clone().into());
        let descendants = runtime_root.query_selector_all("[data-runtime-node]")?;
        for index in 0..descendants.length() {
            let Some(node) = descendants.item(index) else {
                continue;
            };
            let Ok(element) = node.clone().dyn_into::<Element>() else {
                continue;
            };
            let Some(id) = element.get_attribute("data-runtime-node") else {
                continue;
            };
            nodes.insert(id, node);
        }
        let mut handles = HashMap::new();
        for reference in &app.host_element_refs {
            if let Some(node) = nodes.get(&reference.target_id) {
                for attachment in &reference.attachments {
                    handles.insert(attachment.clone(), node.clone());
                }
                handles.insert(reference.id.clone(), node.clone());
            }
        }
        for binding in &app.bindings {
            if let Some(node) = nodes.get(&binding.target_id) {
                apply_binding_host(
                    node,
                    binding,
                    &app.expressions,
                    &self.state_scope(&instance_id, &app)?,
                    &handles,
                )?;
            }
        }
        {
            let mut instances = self.instances.borrow_mut();
            let instance = instances
                .get_mut(&instance_id)
                .ok_or_else(|| JsValue::from_str("unknown graph instance"))?;
            instance.dom_nodes = nodes;
            instance.host_refs = handles;
            instance.rows.clear();
            instance.root = Some(runtime_root.into());
        }
        self.install_event_listeners(&instance_id, &app)?;
        serde_wasm_bindgen::to_value(&Some(MountMetrics {
            program_revision: app.revision.clone(),
            dom_operations: 0,
            ..Default::default()
        }))
        .map_err(error)
    }
    pub fn dispose(&self) -> Result<(), JsValue> {
        if let Some(mut typed) = self.typed.borrow_mut().take() {
            typed.clear_listeners();
            if let Some(root) = typed.root {
                root.set_inner_html("");
            }
        }
        self.dispose_router_listeners();
        *self.router.borrow_mut() = None;
        *self.typed_manifest.borrow_mut() = None;
        self.typed_registry.borrow_mut().clear();
        let instance_ids = self.instances.borrow().keys().cloned().collect::<Vec<_>>();
        for instance_id in instance_ids {
            if self.instances.borrow().contains_key(&instance_id) {
                self.dispose_graph_instance(instance_id)?;
            }
        }
        self.registry.borrow_mut().clear();
        Ok(())
    }
    /** Reserve a graph mount position before DOM instantiation. The caller
     * supplies no opaque id: identity follows the compiled topology. */
    pub fn mount_graph_instance(
        &self,
        parent_instance_id: Option<String>,
        outlet_id: String,
        key: Option<String>,
    ) -> Result<String, JsValue> {
        let instance_id =
            graph_instance_id(parent_instance_id.as_deref(), &outlet_id, key.as_deref());
        let mut instances = self.instances.borrow_mut();
        if instances.contains_key(&instance_id) {
            return Err(JsValue::from_str("graph instance mount collision"));
        }
        instances.insert(
            instance_id.clone(),
            GraphInstance {
                parent_id: parent_instance_id,
                outlet_id,
                key,
                graph_id: None,
                local_state: HashMap::new(),
                dom_nodes: HashMap::new(),
                host_refs: HashMap::new(),
                rows: HashMap::new(),
                root: None,
                listeners: Vec::new(),
                continuation_epoch: 0,
                next_request_id: 0,
                abort_controllers: HashMap::new(),
                active_effects: 0,
                active_listeners: 0,
            },
        );
        Ok(instance_id)
    }
    pub fn dispose_graph_instance(&self, instance_id: String) -> Result<(), JsValue> {
        let mut instances = self.instances.borrow_mut();
        if !instances.contains_key(&instance_id) {
            return Err(JsValue::from_str("unknown graph instance"));
        }
        let descendants = instances
            .iter()
            .filter_map(|(id, instance)| {
                (instance.parent_id.as_deref() == Some(instance_id.as_str())).then(|| id.clone())
            })
            .collect::<Vec<_>>();
        for child in descendants {
            drop(instances);
            self.dispose_graph_instance(child)?;
            instances = self.instances.borrow_mut();
        }
        let mut instance = instances
            .remove(&instance_id)
            .expect("instance checked above");
        drop(instances);
        instance.continuation_epoch = instance.continuation_epoch.saturating_add(1);
        for (_, controller) in instance.abort_controllers.drain() {
            controller.abort();
        }
        for listener in instance.listeners {
            let _ = listener.element.remove_event_listener_with_callback(
                &listener.event_type,
                listener.callback.as_ref().unchecked_ref(),
            );
        }
        if let Some(root) = instance.root {
            if let Some(parent) = root.parent_node() {
                parent.remove_child(&root)?;
            }
        }
        Ok(())
    }
    /** Replace exactly one outlet port. The parent instance is intentionally
     * retained; descendants are disposed before their owning child. DOM
     * mounting is layered on this ownership operation. */
    pub fn replace_outlet_instance(
        &self,
        parent_instance_id: String,
        outlet_id: String,
        graph_id: String,
    ) -> Result<String, JsValue> {
        if !self.instances.borrow().contains_key(&parent_instance_id) {
            return Err(JsValue::from_str("unknown parent graph instance"));
        }
        let existing = self
            .instances
            .borrow()
            .iter()
            .filter_map(|(id, instance)| {
                (instance.parent_id.as_deref() == Some(parent_instance_id.as_str())
                    && instance.outlet_id == outlet_id)
                    .then(|| id.clone())
            })
            .collect::<Vec<_>>();
        for id in existing {
            self.dispose_graph_instance(id)?;
        }
        if !self.registry.borrow().contains_key(&graph_id) {
            return Err(JsValue::from_str("unregistered graph"));
        }
        let outlet = self.outlet_element(&parent_instance_id, &outlet_id)?;
        let id = graph_instance_id(Some(&parent_instance_id), &outlet_id, None);
        self.create_instance(
            id.clone(),
            Some(parent_instance_id),
            outlet_id,
            None,
            graph_id,
        )?;
        self.mount_instance(&id, outlet, false)?;
        Ok(id)
    }
    pub fn initialize_input(&self, input_id: String, rows: JsValue) -> Result<JsValue, JsValue> {
        if self.typed.borrow().is_some() {
            return self.initialize_typed_input(&input_id, rows);
        }
        let instance_id = self.legacy_instance_id()?;
        self.initialize_input_for(&instance_id, input_id, rows)
    }
    /** Instance-addressed input delivery is the normal multi-graph API.
     * `initialize_input` above exists only for the pre-router single-root
     * browser adapter. */
    pub fn initialize_instance_input(
        &self,
        instance_id: String,
        input_id: String,
        rows: JsValue,
    ) -> Result<JsValue, JsValue> {
        self.initialize_input_for(&instance_id, input_id, rows)
    }
    fn initialize_input_for(
        &self,
        instance_id: &str,
        input_id: String,
        rows: JsValue,
    ) -> Result<JsValue, JsValue> {
        let values: Vec<HashMap<String, Value>> =
            serde_wasm_bindgen::from_value(rows).map_err(error)?;
        let app = self.app_for_instance(instance_id)?;
        let loop_node = find_loop(&app, &input_id)?;
        let parent = self
            .instances
            .borrow()
            .get(instance_id)
            .ok_or_else(|| JsValue::from_str("unknown graph instance"))?
            .dom_nodes
            .get(&loop_node.parent_id)
            .cloned()
            .ok_or_else(|| JsValue::from_str("loop parent missing"))?;
        let parent: Element = parent
            .dyn_into()
            .map_err(|_| JsValue::from_str("loop parent"))?;
        let root_id = loop_node
            .row_template_root_element_id
            .ok_or_else(|| JsValue::from_str("row template missing"))?;
        let doc = document()?;
        let elements = index_elements(&app);
        let texts = index_texts(&app);
        let fragment = doc.create_document_fragment();
        let mut collected = HashMap::new();
        let contexts = index_contexts(&app);
        let loops = index_loops(&app);
        let conditionals = index_conditionals(&app);
        let environment = context_defaults(&app);
        for value in values {
            let key = value_string(value.get("id"));
            let scope = row_scope(&loop_node.item_name, &value);
            let mut nodes = HashMap::new();
            let row_root = instantiate(
                &doc,
                &root_id,
                &elements,
                &texts,
                &contexts,
                &loops,
                &conditionals,
                &app.bindings,
                &app.prop_programs,
                &app.expressions,
                &app.events,
                &scope,
                &environment,
                &mut nodes,
            )?;
            if let Ok(element) = row_root.clone().dyn_into::<Element>() {
                element.set_attribute("data-runtime-row-key", &key)?;
            }
            self.install_event_listeners_for_nodes(instance_id, &app, &nodes)?;
            fragment.append_child(&row_root)?;
            collected.insert(
                key,
                Row {
                    root: row_root,
                    values: value,
                    nodes,
                },
            );
        }
        parent.append_child(&fragment)?;
        let count = collected.len() as u32;
        self.instances
            .borrow_mut()
            .get_mut(instance_id)
            .ok_or_else(|| JsValue::from_str("unknown graph instance"))?
            .rows
            .insert(input_id, collected);
        finish(MountMetrics {
            row_count: count,
            dom_operations: 1,
            ..Default::default()
        })
    }
    pub fn apply_delta(&self, delta: JsValue) -> Result<JsValue, JsValue> {
        if self.typed.borrow().is_some() {
            return self.apply_typed_delta(delta);
        }
        let instance_id = self.legacy_instance_id()?;
        self.apply_delta_for(&instance_id, delta)
    }
    pub fn apply_instance_delta(
        &self,
        instance_id: String,
        delta: JsValue,
    ) -> Result<JsValue, JsValue> {
        self.apply_delta_for(&instance_id, delta)
    }
    fn apply_delta_for(&self, instance_id: &str, delta: JsValue) -> Result<JsValue, JsValue> {
        let start = now();
        let delta: Delta = serde_wasm_bindgen::from_value(delta).map_err(error)?;
        let mut metrics = UpdateMetrics::default();
        match delta {
            Delta::Update {
                input_id,
                row_key,
                changes,
            } => self.update(instance_id, &input_id, &row_key, changes, &mut metrics)?,
            Delta::Insert {
                input_id,
                row_key,
                row,
                before_row_key,
            } => self.insert(
                instance_id,
                &input_id,
                row_key,
                row,
                before_row_key,
                &mut metrics,
            )?,
            Delta::Remove { input_id, row_key } => {
                self.remove(instance_id, &input_id, &row_key, &mut metrics)?
            }
            Delta::Move {
                input_id,
                row_key,
                before_row_key,
            } => self.move_row(
                instance_id,
                &input_id,
                &row_key,
                before_row_key,
                &mut metrics,
            )?,
        };
        metrics.wasm_dom_us = (now() - start) * 1000.0;
        serde_wasm_bindgen::to_value(&metrics).map_err(error)
    }
    pub fn apply_deltas(&self, deltas: JsValue) -> Result<JsValue, JsValue> {
        let values: Vec<Value> = serde_wasm_bindgen::from_value(deltas).map_err(error)?;
        let mut total = UpdateMetrics::default();
        for value in values {
            let result: UpdateMetrics = serde_wasm_bindgen::from_value(
                self.apply_delta(serde_wasm_bindgen::to_value(&value).map_err(error)?)?,
            )
            .map_err(error)?;
            total.dom_operations += result.dom_operations;
            total.nodes_touched += result.nodes_touched;
            total.bindings_touched += result.bindings_touched;
            total.wasm_dom_us += result.wasm_dom_us;
        }
        serde_wasm_bindgen::to_value(&total).map_err(error)
    }
    /** Programmatic dispatch is intentionally value-only. Normal DOM events
     * never cross the JS/wasm boundary: Rust installs and extracts them. */
    pub fn apply_action(&self, action_id: String, event: JsValue) -> Result<JsValue, JsValue> {
        let instance_id = self.legacy_instance_id()?;
        self.apply_action_for(&instance_id, action_id, event)
    }
    pub fn apply_instance_action(
        &self,
        instance_id: String,
        action_id: String,
        event: JsValue,
    ) -> Result<JsValue, JsValue> {
        self.apply_action_for(&instance_id, action_id, event)
    }
    fn apply_action_for(
        &self,
        instance_id: &str,
        action_id: String,
        event: JsValue,
    ) -> Result<JsValue, JsValue> {
        let app = self.app_for_instance(&instance_id)?;
        let action = app
            .actions
            .iter()
            .find(|action| action.id == action_id)
            .ok_or_else(|| JsValue::from_str("unknown action"))?;
        let mut metrics = UpdateMetrics::default();
        let event = serde_wasm_bindgen::from_value(event).unwrap_or_default();
        self.execute_action_operations(
            &instance_id,
            &app,
            &action.operations,
            &event,
            None,
            &mut metrics,
            None,
        )?;
        self.refresh_state_bindings(&instance_id, &app, &mut metrics)?;
        serde_wasm_bindgen::to_value(&metrics).map_err(error)
    }
}
impl PlecRuntime {
    fn navigate_internal(&self, href: &str, replace: bool) -> Result<(), JsValue> {
        let state = self
            .router
            .borrow()
            .clone()
            .ok_or_else(|| JsValue::from_str("runtime has not been started"))?;
        let pathname = href.split(['?', '#']).next().unwrap_or(href);
        let route_path = pathname.trim_start_matches('/');
        let route = state
            .manifest
            .routes
            .iter()
            .find(|route| route.path == route_path)
            .or_else(|| state.manifest.routes.iter().find(|route| route.path == "*"))
            .cloned()
            .ok_or_else(|| JsValue::from_str("no compiled route for pathname"))?;
        let history = window()?.history()?;
        if replace {
            history.replace_state_with_url(&JsValue::NULL, "", Some(pathname))?;
        } else {
            history.push_state_with_url(&JsValue::NULL, "", Some(pathname))?;
        }
        self.refresh_navigation_state(pathname)?;
        let instance_id =
            self.replace_outlet_instance(state.root_instance_id, route.outlet_id, route.graph_id)?;
        if let Some(mut loader) = route.loader {
            if let Some(slot) = route.loader_state_slot_id {
                for operation in &mut loader.operations {
                    if operation.get("kind").and_then(Value::as_str) != Some("capability-request") {
                        continue;
                    }
                    let result = operation
                        .get("successResultName")
                        .and_then(Value::as_str)
                        .unwrap_or("result")
                        .to_string();
                    let success = operation
                        .as_object_mut()
                        .and_then(|operation| operation.get_mut("success"))
                        .and_then(Value::as_array_mut);
                    if let Some(success) = success {
                        success.push(serde_json::json!({
                            "kind": "set-state", "stateSlotId": slot,
                            "value": { "kind": "identifier", "name": result },
                        }));
                    }
                }
            }
            let app = self.app_for_instance(&instance_id)?;
            self.execute_action_operations(
                &instance_id,
                &app,
                &loader.operations,
                &HashMap::new(),
                None,
                &mut UpdateMetrics::default(),
                None,
            )?;
        }
        Ok(())
    }
    fn refresh_navigation_state(&self, pathname: &str) -> Result<(), JsValue> {
        let links = document()?.query_selector_all("a[href]")?;
        for index in 0..links.length() {
            let Some(node) = links.item(index) else {
                continue;
            };
            let Ok(link) = node.dyn_into::<Element>() else {
                continue;
            };
            let href = link.get_attribute("href").unwrap_or_default();
            if href == pathname {
                link.set_attribute("aria-current", "page")?;
            } else {
                link.remove_attribute("aria-current")?;
            }
        }
        Ok(())
    }
    fn install_router_listeners(&self) -> Result<(), JsValue> {
        let document = document()?;
        let document_target: EventTarget = document.clone().into();
        let runtime: *const PlecRuntime = self;
        let click = Closure::wrap(Box::new(move |event: Event| {
            let Ok(mouse) = event.clone().dyn_into::<MouseEvent>() else {
                return;
            };
            if event.default_prevented()
                || mouse.button() != 0
                || mouse.meta_key()
                || mouse.ctrl_key()
                || mouse.shift_key()
                || mouse.alt_key()
            {
                return;
            }
            let Some(target) = event
                .target()
                .and_then(|target| target.dyn_into::<Element>().ok())
            else {
                return;
            };
            let Ok(Some(anchor)) = target.closest("a[href]") else {
                return;
            };
            if anchor.get_attribute("target").is_some() || anchor.has_attribute("download") {
                return;
            }
            let Some(href) = anchor.get_attribute("href") else {
                return;
            };
            if !href.starts_with('/') || href.starts_with("//") {
                return;
            }
            event.prevent_default();
            unsafe {
                if let Some(runtime) = runtime.as_ref() {
                    let _ = runtime.navigate_internal(&href, false);
                }
            }
        }) as Box<dyn FnMut(Event)>);
        document_target
            .add_event_listener_with_callback("click", click.as_ref().unchecked_ref())?;
        self.router_listeners.borrow_mut().push(RouterListener {
            target: document_target,
            event_type: "click".into(),
            callback: click,
        });
        let window_target: EventTarget = window()?.into();
        let runtime: *const PlecRuntime = self;
        let popstate = Closure::wrap(Box::new(move |_event: Event| unsafe {
            if let Some(runtime) = runtime.as_ref() {
                if let Ok(location) =
                    window().and_then(|window| window.location().pathname().map_err(|error| error))
                {
                    let _ = runtime.navigate_internal(&location, true);
                }
            }
        }) as Box<dyn FnMut(Event)>);
        window_target
            .add_event_listener_with_callback("popstate", popstate.as_ref().unchecked_ref())?;
        self.router_listeners.borrow_mut().push(RouterListener {
            target: window_target,
            event_type: "popstate".into(),
            callback: popstate,
        });
        Ok(())
    }
    fn dispose_router_listeners(&self) {
        for listener in self.router_listeners.borrow_mut().drain(..) {
            let _ = listener.target.remove_event_listener_with_callback(
                &listener.event_type,
                listener.callback.as_ref().unchecked_ref(),
            );
        }
    }
    fn legacy_instance_id(&self) -> Result<String, JsValue> {
        let id = graph_instance_id(None, "main", None);
        self.instances
            .borrow()
            .contains_key(&id)
            .then_some(id)
            .ok_or_else(|| JsValue::from_str("mount must be called first"))
    }
    fn app_for_instance(&self, instance_id: &str) -> Result<Application, JsValue> {
        let graph_id = self
            .instances
            .borrow()
            .get(instance_id)
            .and_then(|instance| instance.graph_id.clone())
            .ok_or_else(|| JsValue::from_str("graph instance has no graph"))?;
        self.registry
            .borrow()
            .get(&graph_id)
            .cloned()
            .ok_or_else(|| JsValue::from_str("graph definition is not registered"))
    }
    fn state_scope(
        &self,
        instance_id: &str,
        app: &Application,
    ) -> Result<HashMap<String, Value>, JsValue> {
        let instances = self.instances.borrow();
        let values = &instances
            .get(instance_id)
            .ok_or_else(|| JsValue::from_str("unknown graph instance"))?
            .local_state;
        Ok(app
            .local_states
            .iter()
            .filter_map(|slot| {
                values
                    .get(&slot.id)
                    .map(|value| (slot.name.clone(), value.clone()))
            })
            .collect())
    }
    fn create_instance(
        &self,
        id: String,
        parent_id: Option<String>,
        outlet_id: String,
        key: Option<String>,
        graph_id: String,
    ) -> Result<(), JsValue> {
        let app = self
            .registry
            .borrow()
            .get(&graph_id)
            .cloned()
            .ok_or_else(|| JsValue::from_str("unregistered graph"))?;
        let values = app
            .local_states
            .iter()
            .map(|slot| (slot.id.clone(), parse_initial_state(&slot.initial_value)))
            .collect();
        let mut instances = self.instances.borrow_mut();
        if instances.contains_key(&id) {
            return Err(JsValue::from_str("graph instance mount collision"));
        }
        instances.insert(
            id,
            GraphInstance {
                parent_id,
                outlet_id,
                key,
                graph_id: Some(graph_id),
                local_state: values,
                dom_nodes: HashMap::new(),
                host_refs: HashMap::new(),
                rows: HashMap::new(),
                root: None,
                listeners: Vec::new(),
                continuation_epoch: 1,
                next_request_id: 0,
                abort_controllers: HashMap::new(),
                active_effects: 0,
                active_listeners: 0,
            },
        );
        Ok(())
    }
    fn outlet_element(&self, parent_id: &str, outlet_id: &str) -> Result<Element, JsValue> {
        let app = self.app_for_instance(parent_id)?;
        let element_id = app
            .layout
            .route_outlets
            .iter()
            .find(|outlet| outlet.id == outlet_id)
            .map(|outlet| outlet.element_id.as_str())
            .unwrap_or(outlet_id);
        self.instances
            .borrow()
            .get(parent_id)
            .and_then(|instance| instance.dom_nodes.get(element_id))
            .cloned()
            .ok_or_else(|| JsValue::from_str("route outlet missing"))?
            .dyn_into()
            .map_err(|_| JsValue::from_str("route outlet is not an element"))
    }
    fn execute_action_operations(
        &self,
        instance_id: &str,
        app: &Application,
        operations: &[Value],
        event: &HashMap<String, Value>,
        native_event: Option<&Event>,
        metrics: &mut UpdateMetrics,
        continuation_scope: Option<&HashMap<String, Value>>,
    ) -> Result<(), JsValue> {
        for operation in operations {
            match operation.get("kind").and_then(Value::as_str) {
                Some("set-state") => {
                    let Some(slot) = operation.get("stateSlotId").and_then(Value::as_str) else {
                        continue;
                    };
                    let scope = self.action_scope(instance_id, app, event, continuation_scope)?;
                    let value = operation
                        .get("value")
                        .map(|expression| evaluate(expression, &scope))
                        .unwrap_or(Value::Null);
                    self.instances
                        .borrow_mut()
                        .get_mut(instance_id)
                        .ok_or_else(|| JsValue::from_str("unknown graph instance"))?
                        .local_state
                        .insert(slot.into(), value);
                }
                Some("if") => {
                    let test = operation
                        .get("test")
                        .map(|expression| {
                            self.action_scope(instance_id, app, event, continuation_scope)
                                .map(|scope| evaluate(expression, &scope))
                        })
                        .transpose()?
                        .unwrap_or(Value::Bool(false));
                    let branch = if truthy(&test) {
                        "consequent"
                    } else {
                        "alternate"
                    };
                    let nested = operation
                        .get(branch)
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    self.execute_action_operations(
                        instance_id,
                        app,
                        &nested,
                        event,
                        native_event,
                        metrics,
                        continuation_scope,
                    )?;
                }
                Some("prevent-default") => {
                    if let Some(event) = native_event {
                        event.prevent_default();
                    }
                }
                Some("return") => return Ok(()),
                Some("capability-request") => {
                    if operation.get("capability").and_then(Value::as_str) != Some("network.fetch")
                    {
                        return Err(JsValue::from_str("unsupported capability"));
                    }
                    self.start_fetch(
                        instance_id,
                        app.clone(),
                        operation.clone(),
                        event.clone(),
                        continuation_scope.cloned().unwrap_or_default(),
                    )?;
                }
                Some("invoke-action-ref") => {
                    let id = operation
                        .get("actionId")
                        .and_then(Value::as_str)
                        .ok_or_else(|| JsValue::from_str("action reference missing id"))?;
                    let referenced = app
                        .actions
                        .iter()
                        .find(|action| action.id == id)
                        .ok_or_else(|| JsValue::from_str("unknown action reference"))?;
                    self.execute_action_operations(
                        instance_id,
                        app,
                        &referenced.operations,
                        event,
                        native_event,
                        metrics,
                        continuation_scope,
                    )?;
                }
                Some(other) => {
                    return Err(JsValue::from_str(&format!(
                        "unsupported action operation: {other}"
                    )))
                }
                None => return Err(JsValue::from_str("invalid action operation")),
            }
        }
        Ok(())
    }
    /** Start a browser fetch without handing any continuation work to
     * JavaScript. The future retains only runtime-owned state and validates its
     * instance epoch before it can affect the DOM. */
    fn start_fetch(
        &self,
        instance_id: &str,
        app: Application,
        operation: Value,
        event: HashMap<String, Value>,
        inherited_scope: HashMap<String, Value>,
    ) -> Result<(), JsValue> {
        let scope = self.action_scope(instance_id, &app, &event, Some(&inherited_scope))?;
        let request = operation
            .get("request")
            .ok_or_else(|| JsValue::from_str("fetch request missing"))?;
        let url = evaluate(request.get("url").unwrap_or(&Value::Null), &scope)
            .as_str()
            .unwrap_or_default()
            .to_string();
        if url.is_empty() {
            return Err(JsValue::from_str("fetch URL is empty"));
        }
        let controller = AbortController::new()?;
        let mut init = RequestInit::new();
        init.set_method(
            request
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or("GET"),
        );
        init.set_signal(Some(&controller.signal()));
        let headers = web_sys::Headers::new()?;
        if let Some(values) = request.get("headers").and_then(Value::as_object) {
            for (name, expression) in values {
                let value = evaluate(expression, &scope);
                headers.set(name, value.as_str().unwrap_or_default())?;
            }
        }
        init.set_headers(&headers);
        if let Some(body) = request.get("jsonBody") {
            let body = serde_json::to_string(&evaluate(body, &scope)).map_err(error)?;
            init.set_body(&JsValue::from_str(&body));
        }
        let fetch_request = Request::new_with_str_and_init(&url, &init)?;
        let (epoch, request_id) = {
            let mut instances = self.instances.borrow_mut();
            let instance = instances
                .get_mut(instance_id)
                .ok_or_else(|| JsValue::from_str("unknown graph instance"))?;
            instance.next_request_id += 1;
            let request_id = instance.next_request_id;
            instance.abort_controllers.insert(request_id, controller);
            (instance.continuation_epoch, request_id)
        };
        let runtime = self.clone();
        let instance_id = instance_id.to_string();
        spawn_local(async move {
            let response = async {
                let window =
                    web_sys::window().ok_or_else(|| JsValue::from_str("window unavailable"))?;
                let response: Response = JsFuture::from(window.fetch_with_request(&fetch_request))
                    .await?
                    .dyn_into()?;
                let status = response.status();
                if operation
                    .get("request")
                    .and_then(|r| r.get("requireOk"))
                    .and_then(Value::as_bool)
                    .unwrap_or(true)
                    && !response.ok()
                {
                    return Err(JsValue::from_str(&format!("request failed ({status})")));
                }
                let decode = operation
                    .get("request")
                    .and_then(|r| r.get("decode"))
                    .and_then(Value::as_str)
                    .unwrap_or("json");
                let result = match decode {
                    "empty" => Value::Null,
                    "text" => Value::String(
                        JsFuture::from(response.text()?)
                            .await?
                            .as_string()
                            .unwrap_or_default(),
                    ),
                    _ => serde_wasm_bindgen::from_value(JsFuture::from(response.json()?).await?)
                        .map_err(error)?,
                };
                Ok::<_, JsValue>((result, status))
            }
            .await;
            let _ = runtime.complete_fetch(
                &instance_id,
                epoch,
                request_id,
                app,
                operation,
                event,
                inherited_scope,
                response,
            );
        });
        Ok(())
    }
    fn complete_fetch(
        &self,
        instance_id: &str,
        epoch: u64,
        request_id: u64,
        app: Application,
        operation: Value,
        event: HashMap<String, Value>,
        inherited_scope: HashMap<String, Value>,
        response: Result<(Value, u16), JsValue>,
    ) -> Result<(), JsValue> {
        {
            let mut instances = self.instances.borrow_mut();
            let Some(instance) = instances.get_mut(instance_id) else {
                return Ok(());
            };
            if instance.continuation_epoch != epoch {
                return Ok(());
            }
            instance.abort_controllers.remove(&request_id);
        }
        let mut metrics = UpdateMetrics::default();
        let mut scope = inherited_scope;
        let (branch, name, value) = match response {
            Ok((result, _)) => (
                "success",
                operation
                    .get("successResultName")
                    .and_then(Value::as_str)
                    .unwrap_or("result"),
                result,
            ),
            Err(reason) => (
                "failure",
                operation
                    .get("failureErrorName")
                    .and_then(Value::as_str)
                    .unwrap_or("error"),
                serde_json::json!({"message": reason.as_string().unwrap_or_else(|| "network request failed".into())}),
            ),
        };
        scope.insert(name.into(), value);
        let operations = operation
            .get(branch)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        self.execute_action_operations(
            instance_id,
            &app,
            &operations,
            &event,
            None,
            &mut metrics,
            Some(&scope),
        )?;
        let finally = operation
            .get("finally")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        self.execute_action_operations(
            instance_id,
            &app,
            &finally,
            &event,
            None,
            &mut metrics,
            Some(&scope),
        )?;
        self.refresh_state_bindings(instance_id, &app, &mut metrics)
    }
    fn action_scope(
        &self,
        instance_id: &str,
        app: &Application,
        event: &HashMap<String, Value>,
        continuation_scope: Option<&HashMap<String, Value>>,
    ) -> Result<HashMap<String, Value>, JsValue> {
        let mut scope = self.state_scope(instance_id, app)?;
        scope.insert(
            "event".into(),
            Value::Object(event.clone().into_iter().collect()),
        );
        if let Some(values) = continuation_scope {
            scope.extend(values.clone());
        }
        Ok(scope)
    }
    /** Reconstruct the lexical map item from the event's keyed DOM row. This
     * keeps row callbacks executable without serializing closures to JS. */
    fn row_scope_for_event(
        &self,
        instance_id: &str,
        app: &Application,
        loop_id: &str,
        key: Option<&Value>,
    ) -> Option<HashMap<String, Value>> {
        let loop_node = app.loops.iter().find(|entry| entry.id == loop_id)?;
        let input_id = loop_node.input_id.as_ref()?;
        let key = key?.as_str()?;
        let instances = self.instances.borrow();
        let row = instances.get(instance_id)?.rows.get(input_id)?.get(key)?;
        Some(row_scope(&loop_node.item_name, &row.values))
    }
    fn install_event_listeners(&self, instance_id: &str, app: &Application) -> Result<(), JsValue> {
        self.remove_event_listeners(instance_id)?;
        let nodes = self
            .instances
            .borrow()
            .get(instance_id)
            .ok_or_else(|| JsValue::from_str("unknown graph instance"))?
            .dom_nodes
            .clone();
        self.install_event_listeners_for_nodes(instance_id, app, &nodes)
    }
    fn install_event_listeners_for_nodes(
        &self,
        instance_id: &str,
        app: &Application,
        nodes: &HashMap<String, Node>,
    ) -> Result<(), JsValue> {
        let mut new_listeners = Vec::new();
        for binding in &app.events {
            let Some(node) = nodes.get(&binding.target_id) else {
                continue;
            };
            let Ok(element) = node.clone().dyn_into::<Element>() else {
                continue;
            };
            let action_id = binding.action_id.clone();
            let loop_id = binding.loop_id.clone();
            let instance_id = instance_id.to_owned();
            let runtime: *const PlecRuntime = self;
            let callback = Closure::wrap(Box::new(move |event: Event| {
                // The runtime instance outlives registered listeners; dispose removes each
                // listener before its instance can be released.
                unsafe {
                    if let Some(runtime) = runtime.as_ref() {
                        let _ = runtime.dispatch_dom_event(
                            &instance_id,
                            action_id.clone(),
                            loop_id.clone(),
                            event,
                        );
                    }
                }
            }) as Box<dyn FnMut(Event)>);
            element.add_event_listener_with_callback(
                &binding.event_type,
                callback.as_ref().unchecked_ref(),
            )?;
            new_listeners.push(Listener {
                element,
                event_type: binding.event_type.clone(),
                callback,
            });
        }
        let mut instances = self.instances.borrow_mut();
        let instance = instances
            .get_mut(instance_id)
            .ok_or_else(|| JsValue::from_str("unknown graph instance"))?;
        instance.active_listeners += new_listeners.len();
        instance.listeners.extend(new_listeners);
        Ok(())
    }
    fn remove_event_listeners(&self, instance_id: &str) -> Result<(), JsValue> {
        let listeners = std::mem::take(
            &mut self
                .instances
                .borrow_mut()
                .get_mut(instance_id)
                .ok_or_else(|| JsValue::from_str("unknown graph instance"))?
                .listeners,
        );
        for listener in listeners {
            let _ = listener.element.remove_event_listener_with_callback(
                &listener.event_type,
                listener.callback.as_ref().unchecked_ref(),
            );
        }
        Ok(())
    }
    fn rebuild_event_listeners(&self, instance_id: &str, app: &Application) -> Result<(), JsValue> {
        self.remove_event_listeners(instance_id)?;
        let mounted = self
            .instances
            .borrow()
            .get(instance_id)
            .ok_or_else(|| JsValue::from_str("unknown graph instance"))?
            .dom_nodes
            .clone();
        self.install_event_listeners_for_nodes(instance_id, app, &mounted)?;
        let row_nodes = self
            .instances
            .borrow()
            .get(instance_id)
            .ok_or_else(|| JsValue::from_str("unknown graph instance"))?
            .rows
            .values()
            .flat_map(|rows| rows.values().map(|row| row.nodes.clone()))
            .collect::<Vec<_>>();
        for nodes in row_nodes {
            self.install_event_listeners_for_nodes(instance_id, app, &nodes)?;
        }
        Ok(())
    }
    fn dispatch_dom_event(
        &self,
        instance_id: &str,
        action_id: String,
        loop_id: Option<String>,
        event: Event,
    ) -> Result<(), JsValue> {
        let mut value = HashMap::new();
        value.insert("type".into(), Value::String(event.type_()));
        let target = event
            .current_target()
            .or_else(|| event.target())
            .and_then(|target| target.dyn_into::<Element>().ok());
        if let Some(target) = target {
            if let Ok(input) = target.clone().dyn_into::<HtmlInputElement>() {
                value.insert("value".into(), Value::String(input.value()));
                value.insert("checked".into(), Value::Bool(input.checked()));
            }
            if let Ok(Some(row)) = target.closest("[data-runtime-row-key]") {
                if let Some(key) = row.get_attribute("data-runtime-row-key") {
                    value.insert("rowKey".into(), Value::String(key));
                }
            }
        }
        if let Ok(keyboard) = event.clone().dyn_into::<KeyboardEvent>() {
            value.insert("key".into(), Value::String(keyboard.key()));
            value.insert("metaKey".into(), Value::Bool(keyboard.meta_key()));
            value.insert("ctrlKey".into(), Value::Bool(keyboard.ctrl_key()));
            value.insert("shiftKey".into(), Value::Bool(keyboard.shift_key()));
            value.insert("altKey".into(), Value::Bool(keyboard.alt_key()));
        } else if let Ok(mouse) = event.clone().dyn_into::<MouseEvent>() {
            value.insert("button".into(), Value::Number(mouse.button().into()));
            value.insert("metaKey".into(), Value::Bool(mouse.meta_key()));
            value.insert("ctrlKey".into(), Value::Bool(mouse.ctrl_key()));
            value.insert("shiftKey".into(), Value::Bool(mouse.shift_key()));
            value.insert("altKey".into(), Value::Bool(mouse.alt_key()));
        }
        // Action programs use the finite event record from the IR, not a DOM
        // Event. Preserve the common React shape so `event.currentTarget.value`
        // and `.checked` lower without a JavaScript compatibility callback.
        value.insert(
            "currentTarget".into(),
            Value::Object(value.clone().into_iter().collect()),
        );
        let app = self.app_for_instance(instance_id)?;
        let action = app
            .actions
            .iter()
            .find(|action| action.id == action_id)
            .ok_or_else(|| JsValue::from_str("unknown action"))?;
        let mut metrics = UpdateMetrics::default();
        let row_scope = loop_id
            .as_deref()
            .and_then(|loop_id| {
                self.row_scope_for_event(instance_id, &app, loop_id, value.get("rowKey"))
            })
            .unwrap_or_default();
        self.execute_action_operations(
            instance_id,
            &app,
            &action.operations,
            &value,
            Some(&event),
            &mut metrics,
            Some(&row_scope),
        )?;
        self.refresh_state_bindings(instance_id, &app, &mut metrics)
    }
    fn refresh_state_bindings(
        &self,
        instance_id: &str,
        app: &Application,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        self.sync_todo_state_loops(instance_id, app, metrics)?;
        self.refresh_row_conditionals_and_bindings(instance_id, app, metrics)?;
        let scope = self.state_scope(instance_id, app)?;
        let instances = self.instances.borrow();
        let instance = instances
            .get(instance_id)
            .ok_or_else(|| JsValue::from_str("unknown graph instance"))?;
        let nodes = &instance.dom_nodes;
        let handles = &instance.host_refs;
        for binding in &app.bindings {
            let Some(node) = nodes.get(&binding.target_id) else {
                continue;
            };
            apply_binding_host(node, binding, &app.expressions, &scope, &handles)?;
            metrics.dom_operations += 1;
            metrics.nodes_touched += 1;
            metrics.bindings_touched += 1;
        }
        for program in &app.prop_programs {
            let Some(node) = nodes.get(&program.target_id) else {
                continue;
            };
            apply_prop_program(node, program, &app.expressions, &scope)?;
            metrics.dom_operations += 1;
            metrics.nodes_touched += 1;
        }
        Ok(())
    }
    /** Row conditionals own anchored DOM regions. State changes swap only a
     * selected branch; the keyed row root and sibling rows remain mounted. */
    fn refresh_row_conditionals_and_bindings(
        &self,
        instance_id: &str,
        app: &Application,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let loops = app
            .loops
            .iter()
            .filter(|entry| entry.input_id.is_some())
            .cloned()
            .collect::<Vec<_>>();
        let mut changed = false;
        for loop_node in &loops {
            let Some(input_id) = loop_node.input_id.as_deref() else {
                continue;
            };
            let keys = self
                .instances
                .borrow()
                .get(instance_id)
                .and_then(|instance| instance.rows.get(input_id))
                .map(|rows| rows.keys().cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            for key in keys {
                if self.refresh_row_conditionals(
                    instance_id,
                    app,
                    loop_node,
                    input_id,
                    &key,
                    metrics,
                )? {
                    changed = true;
                }
            }
        }
        if changed {
            self.rebuild_event_listeners(instance_id, app)?;
        }
        for loop_node in &loops {
            let Some(input_id) = loop_node.input_id.as_deref() else {
                continue;
            };
            let rows = self
                .instances
                .borrow()
                .get(instance_id)
                .and_then(|instance| instance.rows.get(input_id))
                .map(|rows| {
                    rows.values()
                        .map(|row| (row.values.clone(), row.nodes.clone()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            for (values, nodes) in rows {
                let mut scope = self.state_scope(instance_id, app)?;
                scope.extend(row_scope(&loop_node.item_name, &values));
                for binding in &app.bindings {
                    if let Some(node) = nodes.get(&binding.target_id) {
                        apply_binding(node, binding, &app.expressions, &scope)?;
                        metrics.dom_operations += 1;
                        metrics.nodes_touched += 1;
                        metrics.bindings_touched += 1;
                    }
                }
                for program in &app.prop_programs {
                    if let Some(node) = nodes.get(&program.target_id) {
                        apply_prop_program(node, program, &app.expressions, &scope)?;
                        metrics.dom_operations += 1;
                        metrics.nodes_touched += 1;
                    }
                }
            }
        }
        Ok(())
    }
    fn refresh_row_conditionals(
        &self,
        instance_id: &str,
        app: &Application,
        loop_node: &Loop,
        input_id: &str,
        key: &str,
        metrics: &mut UpdateMetrics,
    ) -> Result<bool, JsValue> {
        let snapshot = self
            .instances
            .borrow()
            .get(instance_id)
            .and_then(|instance| instance.rows.get(input_id))
            .and_then(|rows| rows.get(key))
            .map(|row| (row.values.clone(), row.nodes.clone()));
        let Some((values, nodes)) = snapshot else {
            return Ok(false);
        };
        let mut scope = self.state_scope(instance_id, app)?;
        scope.extend(row_scope(&loop_node.item_name, &values));
        let elements = index_elements(app);
        let texts = index_texts(app);
        let contexts = index_contexts(app);
        let loops = index_loops(app);
        let conditionals = index_conditionals(app);
        let environment = context_defaults(app);
        let mut changed = false;
        for conditional in &app.conditionals {
            let Some(start) = nodes.get(&conditional.id).cloned() else {
                continue;
            };
            let Ok(comment) = start.clone().dyn_into::<Comment>() else {
                continue;
            };
            let desired = app
                .expressions
                .iter()
                .find(|entry| entry.id == conditional.expression_id)
                .map(|entry| {
                    truthy(&evaluate_with_context(
                        &entry.expression,
                        &scope,
                        &environment,
                    ))
                })
                .unwrap_or(false);
            let marker = format!(
                "plec:conditional:{}:{}",
                conditional.id,
                if desired { 1 } else { 0 }
            );
            if comment.data() == marker {
                continue;
            }
            let end_marker = format!("plec:conditional-end:{}", conditional.id);
            let mut cursor = start.next_sibling();
            let mut end = None;
            while let Some(node) = cursor {
                if node
                    .clone()
                    .dyn_into::<Comment>()
                    .ok()
                    .map(|entry| entry.data() == end_marker)
                    .unwrap_or(false)
                {
                    end = Some(node);
                    break;
                }
                cursor = node.next_sibling();
            }
            let end = end.ok_or_else(|| JsValue::from_str("conditional end marker missing"))?;
            while let Some(node) = start.next_sibling() {
                if node.is_same_node(Some(&end)) {
                    break;
                }
                node.parent_node()
                    .ok_or_else(|| JsValue::from_str("conditional parent missing"))?
                    .remove_child(&node)?;
                metrics.dom_operations += 1;
            }
            let stale = conditional_branch_node_ids(app, conditional);
            let mut fresh = HashMap::new();
            let selected = if desired {
                &conditional.consequent
            } else {
                &conditional.alternate
            };
            for child_id in selected {
                let child = instantiate(
                    &document()?,
                    child_id,
                    &elements,
                    &texts,
                    &contexts,
                    &loops,
                    &conditionals,
                    &app.bindings,
                    &app.prop_programs,
                    &app.expressions,
                    &app.events,
                    &scope,
                    &environment,
                    &mut fresh,
                )?;
                end.parent_node()
                    .ok_or_else(|| JsValue::from_str("conditional parent missing"))?
                    .insert_before(&child, Some(&end))?;
                metrics.dom_operations += 1;
            }
            comment.set_data(&marker);
            let mut instances = self.instances.borrow_mut();
            let row = instances
                .get_mut(instance_id)
                .and_then(|instance| instance.rows.get_mut(input_id))
                .and_then(|rows| rows.get_mut(key))
                .ok_or_else(|| JsValue::from_str("row missing"))?;
            for id in &stale {
                row.nodes.remove(id);
            }
            row.nodes.extend(fresh);
            changed = true;
        }
        Ok(changed)
    }
    /** The initial demo has one compiler-inferred list input sourced from the
     * `todos` local state created from loader data. Reconcile it by key rather
     * than remounting the list, so action continuations can add one row. */
    fn sync_todo_state_loops(
        &self,
        instance_id: &str,
        app: &Application,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let Some(slot) = app.local_states.iter().find(|slot| slot.name == "todos") else {
            return Ok(());
        };
        let values = self
            .instances
            .borrow()
            .get(instance_id)
            .and_then(|instance| instance.local_state.get(&slot.id).cloned())
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default();
        let search_slot = app
            .local_states
            .iter()
            .find(|slot| slot.name == "search")
            .map(|slot| slot.id.clone());
        let search = search_slot
            .as_deref()
            .and_then(|slot| {
                self.instances
                    .borrow()
                    .get(instance_id)
                    .and_then(|instance| instance.local_state.get(slot))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_default()
            .trim()
            .to_lowercase();
        let input_ids = app
            .loops
            .iter()
            .filter_map(|loop_node| loop_node.input_id.clone())
            .collect::<HashSet<_>>();
        for input_id in input_ids {
            let next = values
                .iter()
                .filter_map(|value| {
                    let row = value
                        .as_object()?
                        .clone()
                        .into_iter()
                        .collect::<HashMap<_, _>>();
                    if !search.is_empty()
                        && !row
                            .get("title")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_lowercase()
                            .contains(&search)
                    {
                        return None;
                    }
                    let key = row.get("id")?.as_str()?.to_string();
                    Some((key, row))
                })
                .collect::<Vec<_>>();
            let existing = self
                .instances
                .borrow()
                .get(instance_id)
                .and_then(|instance| instance.rows.get(&input_id))
                .map(|rows| {
                    rows.iter()
                        .map(|(key, row)| (key.clone(), row.values.clone()))
                        .collect::<HashMap<_, _>>()
                })
                .unwrap_or_default();
            let next_keys = next
                .iter()
                .map(|(key, _)| key.clone())
                .collect::<HashSet<_>>();
            for key in existing
                .keys()
                .filter(|key| !next_keys.contains(*key))
                .cloned()
                .collect::<Vec<_>>()
            {
                self.remove(instance_id, &input_id, &key, metrics)?;
            }
            for (key, value) in next {
                if let Some(previous) = existing.get(&key) {
                    let changes = value
                        .iter()
                        .filter_map(|(name, value)| {
                            (previous.get(name) != Some(value))
                                .then(|| (name.clone(), value.clone()))
                        })
                        .collect::<HashMap<_, _>>();
                    if !changes.is_empty() {
                        self.update(instance_id, &input_id, &key, changes, metrics)?;
                    }
                } else {
                    self.insert(instance_id, &input_id, key, value, None, metrics)?;
                }
            }
        }
        Ok(())
    }
    fn update(
        &self,
        instance_id: &str,
        input: &str,
        key: &str,
        changes: HashMap<String, Value>,
        m: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let app = self.app_for_instance(instance_id)?;
        let changed = changes.keys().cloned().collect::<HashSet<_>>();
        let expressions = app
            .expressions
            .iter()
            .map(|entry| (entry.id.as_str(), &entry.expression))
            .collect::<HashMap<_, _>>();
        let mut instances = self.instances.borrow_mut();
        let rows = &mut instances
            .get_mut(instance_id)
            .ok_or_else(|| JsValue::from_str("unknown graph instance"))?
            .rows;
        let row = rows
            .get_mut(input)
            .and_then(|items| items.get_mut(key))
            .ok_or_else(|| JsValue::from_str("row missing"))?;
        row.values.extend(changes);
        for binding in &app.bindings {
            if !binding_dependencies(binding, &expressions)
                .iter()
                .any(|field| changed.contains(field))
            {
                continue;
            }
            if let Some(node) = row.nodes.get(&binding.target_id) {
                apply_binding(node, binding, &app.expressions, &row.values)?;
                m.bindings_touched += 1;
                m.nodes_touched += 1;
                m.dom_operations += 1;
            }
        }
        for program in &app.prop_programs {
            if !program_dependencies(program, &expressions)
                .iter()
                .any(|field| changed.contains(field))
            {
                continue;
            }
            if let Some(node) = row.nodes.get(&program.target_id) {
                apply_prop_program(node, program, &app.expressions, &row.values)?;
                m.nodes_touched += 1;
                m.dom_operations += 1;
            }
        }
        Ok(())
    }
    fn insert(
        &self,
        instance_id: &str,
        input: &str,
        key: String,
        value: HashMap<String, Value>,
        before: Option<String>,
        m: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let app = self.app_for_instance(instance_id)?;
        let loop_node = find_loop(&app, input)?;
        let parent = self
            .instances
            .borrow()
            .get(instance_id)
            .ok_or_else(|| JsValue::from_str("unknown graph instance"))?
            .dom_nodes
            .get(&loop_node.parent_id)
            .cloned()
            .ok_or_else(|| JsValue::from_str("loop parent missing"))?;
        let parent: Element = parent
            .dyn_into()
            .map_err(|_| JsValue::from_str("loop parent"))?;
        let doc = document()?;
        let mut nodes = HashMap::new();
        let elements = index_elements(&app);
        let texts = index_texts(&app);
        let contexts = index_contexts(&app);
        let loops = index_loops(&app);
        let conditionals = index_conditionals(&app);
        let environment = context_defaults(&app);
        let scope = row_scope(&loop_node.item_name, &value);
        let root = instantiate(
            &doc,
            &loop_node
                .row_template_root_element_id
                .ok_or_else(|| JsValue::from_str("row template missing"))?,
            &elements,
            &texts,
            &contexts,
            &loops,
            &conditionals,
            &app.bindings,
            &app.prop_programs,
            &app.expressions,
            &app.events,
            &scope,
            &environment,
            &mut nodes,
        )?;
        if let Ok(element) = root.clone().dyn_into::<Element>() {
            element.set_attribute("data-runtime-row-key", &key)?;
        }
        self.install_event_listeners_for_nodes(instance_id, &app, &nodes)?;
        let anchor = before.and_then(|id| {
            self.instances
                .borrow()
                .get(instance_id)
                .and_then(|instance| instance.rows.get(input))
                .and_then(|rows| rows.get(&id))
                .map(|row| row.root.clone())
        });
        parent.insert_before(&root, anchor.as_ref())?;
        self.instances
            .borrow_mut()
            .get_mut(instance_id)
            .ok_or_else(|| JsValue::from_str("unknown graph instance"))?
            .rows
            .entry(input.into())
            .or_default()
            .insert(
                key,
                Row {
                    root,
                    values: value,
                    nodes,
                },
            );
        m.dom_operations += 1;
        Ok(())
    }
    fn remove(
        &self,
        instance_id: &str,
        input: &str,
        key: &str,
        m: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let removed = if let Some(row) = self
            .instances
            .borrow_mut()
            .get_mut(instance_id)
            .ok_or_else(|| JsValue::from_str("unknown graph instance"))?
            .rows
            .get_mut(input)
            .and_then(|rows| rows.remove(key))
        {
            row.root
                .parent_node()
                .map(|parent| parent.remove_child(&row.root))
                .transpose()?;
            m.dom_operations += 1;
            true
        } else {
            false
        };
        if removed {
            self.rebuild_event_listeners(instance_id, &self.app_for_instance(instance_id)?)?;
        }
        Ok(())
    }
    fn move_row(
        &self,
        instance_id: &str,
        input: &str,
        key: &str,
        before: Option<String>,
        m: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let app = self.app_for_instance(instance_id)?;
        let parent: Element = self
            .instances
            .borrow()
            .get(instance_id)
            .ok_or_else(|| JsValue::from_str("unknown graph instance"))?
            .dom_nodes
            .get(&find_loop(&app, input)?.parent_id)
            .cloned()
            .ok_or_else(|| JsValue::from_str("loop parent missing"))?
            .dyn_into()
            .map_err(|_| JsValue::from_str("loop parent"))?;
        let row = self
            .instances
            .borrow()
            .get(instance_id)
            .ok_or_else(|| JsValue::from_str("unknown graph instance"))?
            .rows
            .get(input)
            .and_then(|rows| rows.get(key))
            .ok_or_else(|| JsValue::from_str("row missing"))?
            .root
            .clone();
        let anchor = before.and_then(|id| {
            self.instances
                .borrow()
                .get(instance_id)
                .and_then(|instance| instance.rows.get(input))
                .and_then(|rows| rows.get(&id))
                .map(|row| row.root.clone())
        });
        parent.insert_before(&row, anchor.as_ref())?;
        m.dom_operations += 1;
        Ok(())
    }
    /** Version-3 artifacts never enter the string-id router. Until nested
     * typed outlets gain their own instance table, navigation replaces the
     * typed mount root as one owned instance. */
    fn navigate_typed_route(&self, href: &str, root: Element) -> Result<(), JsValue> {
        let manifest = self.typed_manifest.borrow().clone()
            .ok_or_else(|| JsValue::from_str("typed router manifest missing"))?;
        let pathname = href.split('?').next().unwrap_or(href);
        let route = manifest.routes.iter().find(|route| route.path == pathname)
            .or_else(|| manifest.routes.iter().find(|route| route.path == "*"));
        let (graph_id, loader_action) = match route {
            Some(route) => (&route.graph_id, route.loader_action),
            None => (&manifest.root_graph_id, None),
        };
        let app = self.typed_registry.borrow().get(graph_id).cloned()
            .ok_or_else(|| JsValue::from_str("typed route graph is not registered"))?;
        let typed = TypedRuntime::new(app)?;
        *self.typed.borrow_mut() = Some(typed);
        self.mount_typed(root)?;
        if let Some(action) = loader_action {
            #[cfg(not(feature = "fetch"))]
            return Err(JsValue::from_str("fetch capability is disabled"));
            #[cfg(feature = "fetch")]
            {
                let pending = {
                    let mut typed = self.typed.borrow_mut();
                    let typed = typed.as_mut().expect("typed runtime installed");
                    let mut metrics = UpdateMetrics::default();
                    typed.execute_action(action, &[], None, None, &mut metrics)?;
                    typed.take_pending_fetches()
                };
                for request in pending { self.start_typed_fetch(request)?; }
            }
        }
        Ok(())
    }
    fn mount_typed(&self, root: Element) -> Result<JsValue, JsValue> {
        let metrics = {
            let mut typed = self.typed.borrow_mut();
            typed
                .as_mut()
                .ok_or_else(|| JsValue::from_str("typed application missing"))?
                .mount(root)?
        };
        self.install_typed_event_listeners()?;
        serde_wasm_bindgen::to_value(&metrics).map_err(error)
    }
    fn install_typed_event_listeners(&self) -> Result<(), JsValue> {
        let events = {
            let mut typed = self.typed.borrow_mut();
            let typed = typed.as_mut().ok_or_else(|| JsValue::from_str("typed application missing"))?;
            typed.clear_listeners();
            typed.app.events.clone()
        };
        for binding in events {
            let (element, event_type) = {
                let typed = self.typed.borrow();
                let typed = typed.as_ref().unwrap();
                let node = typed.nodes.get(&binding.target).cloned().ok_or_else(|| JsValue::from_str("event target is not mounted"))?;
                (node.dyn_into::<Element>().map_err(|_| JsValue::from_str("event target is not an element"))?, typed.app.strings.get(binding.event_type).cloned().ok_or_else(|| JsValue::from_str("event type handle out of range"))?)
            };
            let runtime = self.clone();
            let fields = binding.fields.clone();
            let action = binding.action;
            let loop_index = binding.r#loop;
            let callback = Closure::wrap(Box::new(move |event: Event| {
                let _ = runtime.dispatch_typed_event(action, loop_index, &fields, event);
            }) as Box<dyn FnMut(Event)>);
            element.add_event_listener_with_callback(&event_type, callback.as_ref().unchecked_ref())?;
            self.typed.borrow_mut().as_mut().unwrap().listeners.push(Listener { element, event_type, callback });
        }
        Ok(())
    }
    fn dispatch_typed_event(&self, action: usize, loop_index: Option<usize>, fields: &[usize], event: Event) -> Result<(), JsValue> {
        let target = event.current_target().or_else(|| event.target()).and_then(|value| value.dyn_into::<Element>().ok());
        let values = {
            let typed = self.typed.borrow();
            let app = &typed.as_ref().ok_or_else(|| JsValue::from_str("typed application missing"))?.app;
            fields.iter().map(|field| typed_event_field(app.strings.get(*field).map(String::as_str).unwrap_or(""), &event, target.as_ref())).collect::<Vec<_>>()
        };
        let row = if let (Some(loop_index), Some(target)) = (loop_index, target.as_ref()) {
            target.closest("[data-runtime-row-key]")?.and_then(|element| element.get_attribute("data-runtime-row-key")).and_then(|key| self.typed.borrow().as_ref()?.loops.get(&loop_index)?.rows.get(&key).map(|row| row.values.clone()))
        } else { None };
        let pending = {
            let mut typed = self.typed.borrow_mut();
            let mut metrics = UpdateMetrics::default();
            typed.as_mut().unwrap().execute_action(action, &values, row, Some(&event), &mut metrics)?;
            typed.as_mut().unwrap().take_pending_fetches()
        };
        #[cfg(feature = "fetch")]
        for request in pending { self.start_typed_fetch(request)?; }
        #[cfg(not(feature = "fetch"))]
        if !pending.is_empty() { return Err(JsValue::from_str("fetch capability is disabled")); }
        Ok(())
    }
    #[cfg(feature = "fetch")]
    fn start_typed_fetch(&self, pending: TypedPendingFetch) -> Result<(), JsValue> {
        let mut init = RequestInit::new();
        init.set_method(&pending.method);
        let headers = web_sys::Headers::new()?;
        for (name, value) in &pending.headers { headers.set(name, value)?; }
        init.set_headers(&headers);
        if let Some(body) = &pending.body { init.set_body(&JsValue::from_str(body)); }
        let request = Request::new_with_str_and_init(&pending.url, &init)?;
        let runtime = self.clone();
        spawn_local(async move {
            let result = async {
                let response: Response = JsFuture::from(window()?.fetch_with_request(&request)).await?.dyn_into()?;
                if pending.require_ok && !response.ok() { return Err(JsValue::from_str(&format!("request failed ({})", response.status()))); }
                match pending.decode.as_str() {
                    "empty" => Ok(Value::Null),
                    "text" => Ok(Value::String(JsFuture::from(response.text()?).await?.as_string().unwrap_or_default())),
                    _ => serde_wasm_bindgen::from_value(JsFuture::from(response.json()?).await?).map_err(error),
                }
            }.await;
            let _ = runtime.complete_typed_fetch(pending, result);
        });
        Ok(())
    }
    #[cfg(feature = "fetch")]
    fn complete_typed_fetch(&self, pending: TypedPendingFetch, result: Result<Value, JsValue>) -> Result<(), JsValue> {
        let more = {
            let mut typed = self.typed.borrow_mut();
            let Some(typed) = typed.as_mut() else { return Ok(()); };
            if typed.root.is_none() { return Ok(()); }
            let mut frame = pending.frame;
            let pc = match result {
                Ok(value) => {
                    if pending.result_slot >= frame.len() { return Err(JsValue::from_str("result frame slot out of range")); }
                    frame[pending.result_slot] = value.clone();
                    if let Some(state) = typed.app.actions.get(pending.action).and_then(|action| action.route_loader.then_some(action.loader_result_state).flatten()) {
                        if state >= typed.states.len() { return Err(JsValue::from_str("loader state handle out of range")); }
                        typed.states[state] = value;
                        let mut loader_metrics = UpdateMetrics::default();
                        typed.refresh_state(state, &mut loader_metrics)?;
                    }
                    pending.success_pc
                }
                Err(error) => { if pending.error_slot >= frame.len() { return Err(JsValue::from_str("error frame slot out of range")); } frame[pending.error_slot] = serde_json::json!({"message": error.as_string().unwrap_or_else(|| "network request failed".into())}); pending.failure_pc }
            };
            let mut metrics = UpdateMetrics::default();
            typed.execute_action_at(pending.action, pc, frame, &pending.event, pending.row, None, &mut metrics)?;
            typed.take_pending_fetches()
        };
        for request in more { self.start_typed_fetch(request)?; }
        Ok(())
    }
    fn initialize_typed_input(&self, input_id: &str, rows: JsValue) -> Result<JsValue, JsValue> {
        let rows: Vec<Value> = serde_wasm_bindgen::from_value(rows).map_err(error)?;
        let mut typed = self.typed.borrow_mut();
        let typed = typed
            .as_mut()
            .ok_or_else(|| JsValue::from_str("typed application missing"))?;
        let mut metrics = UpdateMetrics::default();
        typed.reconcile_input(input_id, rows, &mut metrics)?;
        serde_wasm_bindgen::to_value(&metrics).map_err(error)
    }
    fn apply_typed_delta(&self, delta: JsValue) -> Result<JsValue, JsValue> {
        let delta: Delta = serde_wasm_bindgen::from_value(delta).map_err(error)?;
        let mut typed = self.typed.borrow_mut();
        let typed = typed
            .as_mut()
            .ok_or_else(|| JsValue::from_str("typed application missing"))?;
        let mut metrics = UpdateMetrics::default();
        typed.apply_delta(delta, &mut metrics)?;
        serde_wasm_bindgen::to_value(&metrics).map_err(error)
    }
}

fn graph_instance_id(parent: Option<&str>, outlet: &str, key: Option<&str>) -> String {
    let segment = |value: &str| value.replace('%', "%25").replace('/', "%2F");
    let parent = parent.map(segment).unwrap_or_else(|| "root".into());
    let key = key
        .map(|value| format!("/key:{}", segment(value)))
        .unwrap_or_default();
    format!("{parent}/outlet:{}{key}", segment(outlet))
}

impl TypedRuntime {
    fn new(app: TypedApplication) -> Result<Self, JsValue> {
        app.validate()?;
        let mut states = Vec::new();
        for slot in &app.state_slots {
            states.push(typed_eval(&app, slot.initial_expression, &[], None, 0)?);
        }
        Ok(Self {
            app,
            root: None,
            nodes: HashMap::new(),
            states,
            loops: HashMap::new(),
            listeners: Vec::new(),
            pending_fetches: Vec::new(),
        })
    }
    fn mount(&mut self, root: Element) -> Result<MountMetrics, JsValue> {
        root.set_inner_html("");
        self.nodes.clear();
        self.loops.clear();
        let doc = document()?;
        let node =
            self.instantiate_node(&doc, self.app.root_node, None, None, 0, &mut HashMap::new())?;
        root.append_child(&node)?;
        self.root = Some(root);
        self.apply_static_bindings()?;
        Ok(MountMetrics {
            created_elements: self
                .app
                .nodes
                .iter()
                .filter(|n| matches!(n, TypedNode::Element { .. }))
                .count() as u32,
            created_texts: self.app.texts.len() as u32,
            bindings: self.app.bindings.len() as u32,
            dom_operations: 1,
            ..Default::default()
        })
    }
    fn clear_listeners(&mut self) {
        for listener in self.listeners.drain(..) {
            let _ = listener.element.remove_event_listener_with_callback(&listener.event_type, listener.callback.as_ref().unchecked_ref());
        }
    }
    fn execute_action(&mut self, action: usize, event: &[Value], row: Option<serde_json::Map<String, Value>>, native_event: Option<&Event>, metrics: &mut UpdateMetrics) -> Result<(), JsValue> {
        let program = self.app.actions.get(action).cloned().ok_or_else(|| JsValue::from_str("action handle out of range"))?;
        let mut frame = vec![Value::Null; program.frame_slots];
        self.execute_action_at(action, 0, frame, event, row, native_event, metrics)
    }
    fn execute_action_at(&mut self, action: usize, mut pc: usize, mut frame: Vec<Value>, event: &[Value], row: Option<serde_json::Map<String, Value>>, native_event: Option<&Event>, metrics: &mut UpdateMetrics) -> Result<(), JsValue> {
        let program = self.app.actions.get(action).cloned().ok_or_else(|| JsValue::from_str("action handle out of range"))?;
        let mut stack = Vec::new();
        while let Some(instruction) = program.instructions.get(pc).cloned() {
            match instruction {
                TypedActionInstruction::Evaluate { expression } => stack.push(typed_eval_frame(&self.app, expression, &self.states, row.as_ref(), 0, &frame, event)?),
                TypedActionInstruction::StoreState { state } => {
                    let value = stack.pop().unwrap_or(Value::Null);
                    if state >= self.states.len() { return Err(JsValue::from_str("state handle out of range")); }
                    self.states[state] = value;
                    self.refresh_state(state, metrics)?;
                }
                TypedActionInstruction::PreventDefault => if let Some(event) = native_event { event.prevent_default(); },
                TypedActionInstruction::Jump { target } => { pc = target; continue; }
                TypedActionInstruction::JumpIfFalse { target } => if !typed_truthy(stack.last().unwrap_or(&Value::Null)) { pc = target; continue; },
                TypedActionInstruction::Call { action: target, arguments } => {
                    let target_program = self.app.actions.get(target).cloned().ok_or_else(|| JsValue::from_str("action handle out of range"))?;
                    let mut child = vec![Value::Null; target_program.frame_slots];
                    for (index, expression) in arguments.into_iter().enumerate() {
                        if let Some(slot) = target_program.parameter_slots.get(index) { child[*slot] = typed_eval_frame(&self.app, expression, &self.states, row.as_ref(), 0, &frame, event)?; }
                    }
                    self.execute_action_at(target, 0, child, event, row.clone(), native_event, metrics)?;
                }
                TypedActionInstruction::CollectionMutation { .. } => return Err(JsValue::from_str("collection mutations require an explicit lowering")),
                TypedActionInstruction::CapabilityRequest { capability, request, success_pc, failure_pc, finally_pc, result_slot, error_slot } => {
                    if capability != "fetch" { return Err(JsValue::from_str("unsupported typed capability")); }
                    #[cfg(not(feature = "fetch"))]
                    return Err(JsValue::from_str("fetch capability is disabled"));
                    #[cfg(feature = "fetch")]
                    {
                        let url = typed_value_string(&typed_eval_frame(&self.app, request.url, &self.states, row.as_ref(), 0, &frame, event)?);
                        if url.is_empty() { return Err(JsValue::from_str("fetch URL is empty")); }
                        let headers = request.headers.iter().map(|header| Ok((self.app.strings.get(header.name).cloned().ok_or_else(|| JsValue::from_str("header name handle out of range"))?, typed_value_string(&typed_eval_frame(&self.app, header.value, &self.states, row.as_ref(), 0, &frame, event)?)))).collect::<Result<Vec<_>, JsValue>>()?;
                        let body = request.body.map(|expression| typed_eval_frame(&self.app, expression, &self.states, row.as_ref(), 0, &frame, event).map(|value| value.to_string())).transpose()?;
                        self.pending_fetches.push(TypedPendingFetch { action, success_pc, failure_pc, finally_pc, result_slot, error_slot, frame, event: event.to_vec(), row, url, method: request.method, headers, body, decode: request.decode, require_ok: request.require_ok });
                        return Ok(());
                    }
                },
                TypedActionInstruction::Return => return Ok(()),
            }
            pc += 1;
        }
        Ok(())
    }
    fn take_pending_fetches(&mut self) -> Vec<TypedPendingFetch> { std::mem::take(&mut self.pending_fetches) }
    fn refresh_state(&mut self, state: usize, metrics: &mut UpdateMetrics) -> Result<(), JsValue> {
        let targets = self.app.dependency_edges.iter().filter_map(|edge| (edge.source.kind == "state" && edge.source.handle == state).then(|| (edge.target.kind.clone(), edge.target.handle))).collect::<Vec<_>>();
        for (kind, handle) in targets {
            if kind == "binding" {
                if let (Some(binding), Some(node)) = (self.app.bindings.get(handle).cloned(), self.nodes.get(&self.app.bindings[handle].target).cloned()) {
                    typed_apply_binding(&self.app, &binding, &node, &self.states, None, 0)?;
                    metrics.dom_operations += 1; metrics.bindings_touched += 1;
                }
            } else if kind == "loop" { let parent = self.parent_for_loop(handle)?; self.render_loop(handle, &parent)?; }
        }
        Ok(())
    }
    fn instantiate_node(
        &mut self,
        doc: &Document,
        index: usize,
        parent: Option<&Node>,
        row: Option<&serde_json::Map<String, Value>>,
        row_index: usize,
        local: &mut HashMap<usize, Node>,
    ) -> Result<Node, JsValue> {
        match self
            .app
            .nodes
            .get(index)
            .ok_or_else(|| JsValue::from_str("node handle out of range"))?
            .clone()
        {
            TypedNode::Element { tag, children, .. } => {
                let element = doc.create_element(
                    self.app
                        .strings
                        .get(tag)
                        .ok_or_else(|| JsValue::from_str("tag handle out of range"))?,
                )?;
                element.set_attribute("data-runtime-node", &index.to_string())?;
                let node: Node = element.into();
                if let Some(parent) = parent {
                    parent.append_child(&node)?;
                }
                for child in children {
                    self.instantiate_node(doc, child, Some(&node), row, row_index, local)?;
                }
                if row.is_some() {
                    local.insert(index, node.clone());
                } else {
                    self.nodes.insert(index, node.clone());
                }
                Ok(node)
            }
            TypedNode::Text { text, .. } => {
                let descriptor = self
                    .app
                    .texts
                    .get(text)
                    .ok_or_else(|| JsValue::from_str("text handle out of range"))?;
                let value = descriptor.value.clone().unwrap_or_default();
                let node: Node = doc.create_text_node(&value).into();
                if let Some(parent) = parent {
                    parent.append_child(&node)?;
                }
                if row.is_some() {
                    local.insert(index, node.clone());
                } else {
                    self.nodes.insert(index, node.clone());
                }
                Ok(node)
            }
            TypedNode::Loop { r#loop, .. } => {
                let marker: Node = doc.create_comment(&format!("plec:loop:{}", r#loop)).into();
                if let Some(parent) = parent {
                    parent.append_child(&marker)?;
                }
                self.render_loop(
                    r#loop,
                    parent.ok_or_else(|| JsValue::from_str("loop parent missing"))?,
                )?;
                Ok(marker)
            }
            TypedNode::Conditional {
                test,
                consequent,
                alternate,
                ..
            } => {
                let selected =
                    if typed_truthy(&typed_eval(&self.app, test, &self.states, row, row_index)?) {
                        consequent
                    } else {
                        alternate.unwrap_or(consequent)
                    };
                self.instantiate_node(doc, selected, parent, row, row_index, local)
            }
        }
    }
    fn render_loop(&mut self, loop_index: usize, parent: &Node) -> Result<(), JsValue> {
        let loop_def = self
            .app
            .loops
            .get(loop_index)
            .ok_or_else(|| JsValue::from_str("loop handle out of range"))?
            .clone();
        let values = typed_eval(&self.app, loop_def.source_expression, &self.states, None, 0)?;
        let rows = values
            .as_array()
            .ok_or_else(|| JsValue::from_str("LOOP_SOURCE_NOT_ARRAY"))?
            .clone();
        let mut projection = Vec::new();
        for (index, value) in rows.into_iter().enumerate() {
            let row = value
                .as_object()
                .cloned()
                .ok_or_else(|| JsValue::from_str("LOOP_ROW_NOT_OBJECT"))?;
            let key = typed_value_string(&typed_eval(
                &self.app,
                loop_def.key_expression,
                &self.states,
                Some(&row),
                index,
            )?);
            if projection
                .iter()
                .any(|(existing, _): &(String, serde_json::Map<String, Value>)| existing == &key)
            {
                return Err(JsValue::from_str("DUPLICATE_LOOP_KEY"));
            }
            projection.push((key, row));
        }
        self.reconcile_loop(
            loop_index,
            parent,
            projection,
            &mut UpdateMetrics::default(),
        )
    }
    fn reconcile_input(
        &mut self,
        input: &str,
        values: Vec<Value>,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let input_index = self
            .app
            .inputs
            .iter()
            .position(|entry| self.app.strings.get(entry.name).map(String::as_str) == Some(input))
            .ok_or_else(|| JsValue::from_str("unknown input"))?;
        let targets = self
            .app
            .loops
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| (entry.input == Some(input_index)).then_some(index))
            .collect::<Vec<_>>();
        for loop_index in targets {
            let loop_def = self.app.loops[loop_index].clone();
            let parent = self.parent_for_loop(loop_index)?;
            let mut projection = Vec::new();
            for (index, value) in values.iter().enumerate() {
                let row = value
                    .as_object()
                    .cloned()
                    .ok_or_else(|| JsValue::from_str("LOOP_ROW_NOT_OBJECT"))?;
                projection.push((
                    typed_value_string(&typed_eval(
                        &self.app,
                        loop_def.key_expression,
                        &self.states,
                        Some(&row),
                        index,
                    )?),
                    row,
                ));
            }
            self.reconcile_loop(loop_index, &parent, projection, metrics)?;
        }
        Ok(())
    }
    fn parent_for_loop(&self, loop_index: usize) -> Result<Node, JsValue> {
        let node_index = self
            .app
            .nodes
            .iter()
            .position(
                |node| matches!(node, TypedNode::Loop { r#loop, .. } if *r#loop == loop_index),
            )
            .ok_or_else(|| JsValue::from_str("loop node missing"))?;
        let parent = match self.app.nodes.get(node_index) {
            Some(TypedNode::Loop { parent, .. }) => *parent,
            _ => None,
        }
        .ok_or_else(|| JsValue::from_str("loop parent missing"))?;
        self.nodes
            .get(&parent)
            .cloned()
            .ok_or_else(|| JsValue::from_str("loop parent not mounted"))
    }
    fn reconcile_loop(
        &mut self,
        loop_index: usize,
        parent: &Node,
        projection: Vec<(String, serde_json::Map<String, Value>)>,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let desired = projection
            .iter()
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        let stale = self
            .loops
            .get(&loop_index)
            .map(|rows| {
                rows.order
                    .iter()
                    .filter(|key| !desired.contains(key))
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for key in stale {
            if let Some(row) = self
                .loops
                .get_mut(&loop_index)
                .and_then(|rows| rows.rows.remove(&key))
            {
                if let Some(parent) = row.root.parent_node() {
                    parent.remove_child(&row.root)?;
                    metrics.dom_operations += 1;
                }
            }
        }
        for (position, (key, values)) in projection.into_iter().enumerate() {
            let existing = self
                .loops
                .get(&loop_index)
                .and_then(|rows| rows.rows.get(&key))
                .map(|row| row.values.clone());
            if let Some(previous) = existing {
                if previous != values {
                    self.update_typed_row(loop_index, &key, values, metrics)?;
                }
            } else {
                self.insert_typed_row(loop_index, parent, key.clone(), values, position, metrics)?;
            }
        }
        let roots = self
            .loops
            .get(&loop_index)
            .map(|rows| {
                desired
                    .iter()
                    .filter_map(|key| rows.rows.get(key).map(|row| row.root.clone()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for root in roots {
            parent.append_child(&root)?;
        }
        self.loops.entry(loop_index).or_default().order = desired;
        Ok(())
    }
    fn insert_typed_row(
        &mut self,
        loop_index: usize,
        parent: &Node,
        key: String,
        values: serde_json::Map<String, Value>,
        index: usize,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let template = self.app.loops[loop_index].row_template;
        let mut nodes = HashMap::new();
        let doc = document()?;
        let root = self.instantiate_node(&doc, template, None, Some(&values), index, &mut nodes)?;
        if let Ok(element) = root.clone().dyn_into::<Element>() {
            element.set_attribute("data-runtime-row-key", &key)?;
        }
        parent.append_child(&root)?;
        self.loops.entry(loop_index).or_default().rows.insert(
            key,
            TypedRow {
                root,
                values,
                nodes,
            },
        );
        metrics.dom_operations += 1;
        Ok(())
    }
    fn update_typed_row(
        &mut self,
        loop_index: usize,
        key: &str,
        values: serde_json::Map<String, Value>,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let row = self
            .loops
            .get_mut(&loop_index)
            .and_then(|rows| rows.rows.get_mut(key))
            .ok_or_else(|| JsValue::from_str("row missing"))?;
        row.values = values;
        let bindings = self.app.bindings.clone();
        for binding in bindings {
            if let Some(node) = row.nodes.get(&binding.target) {
                typed_apply_binding(
                    &self.app,
                    &binding,
                    node,
                    &self.states,
                    Some(&row.values),
                    0,
                )?;
                metrics.dom_operations += 1;
                metrics.bindings_touched += 1;
            }
        }
        Ok(())
    }
    fn apply_static_bindings(&mut self) -> Result<(), JsValue> {
        for binding in self.app.bindings.clone() {
            if let Some(node) = self.nodes.get(&binding.target) {
                typed_apply_binding(&self.app, &binding, node, &self.states, None, 0)?;
            }
        }
        Ok(())
    }
    fn apply_delta(&mut self, delta: Delta, metrics: &mut UpdateMetrics) -> Result<(), JsValue> {
        let input = match &delta {
            Delta::Update { input_id, .. }
            | Delta::Insert { input_id, .. }
            | Delta::Remove { input_id, .. }
            | Delta::Move { input_id, .. } => input_id,
        };
        let input_index = self
            .app
            .inputs
            .iter()
            .position(|entry| {
                self.app.strings.get(entry.name).map(String::as_str) == Some(input.as_str())
            })
            .ok_or_else(|| JsValue::from_str("unknown input"))?;
        let targets = self
            .app
            .loops
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| (entry.input == Some(input_index)).then_some(index))
            .collect::<Vec<_>>();
        for loop_index in targets {
            let mut keys = self
                .loops
                .get(&loop_index)
                .map(|rows| rows.order.clone())
                .unwrap_or_default();
            let mut values = self
                .loops
                .get(&loop_index)
                .map(|rows| {
                    rows.rows
                        .iter()
                        .map(|(key, row)| (key.clone(), row.values.clone()))
                        .collect::<HashMap<_, _>>()
                })
                .unwrap_or_default();
            match &delta {
                Delta::Update {
                    row_key, changes, ..
                } => {
                    let row = values
                        .get_mut(row_key)
                        .ok_or_else(|| JsValue::from_str("row missing"))?;
                    row.extend(changes.clone());
                }
                Delta::Insert {
                    row_key,
                    row,
                    before_row_key,
                    ..
                } => {
                    values.insert(row_key.clone(), row.clone().into_iter().collect());
                    keys.retain(|key| key != row_key);
                    let position = before_row_key
                        .as_ref()
                        .and_then(|before| keys.iter().position(|key| key == before))
                        .unwrap_or(keys.len());
                    keys.insert(position, row_key.clone());
                }
                Delta::Remove { row_key, .. } => {
                    values.remove(row_key);
                    keys.retain(|key| key != row_key);
                }
                Delta::Move {
                    row_key,
                    before_row_key,
                    ..
                } => {
                    keys.retain(|key| key != row_key);
                    let position = before_row_key
                        .as_ref()
                        .and_then(|before| keys.iter().position(|key| key == before))
                        .unwrap_or(keys.len());
                    keys.insert(position, row_key.clone());
                }
            }
            let projection = keys
                .into_iter()
                .filter_map(|key| values.remove(&key).map(|value| (key, value)))
                .collect::<Vec<_>>();
            let parent = self.parent_for_loop(loop_index)?;
            self.reconcile_loop(loop_index, &parent, projection, metrics)?;
        }
        Ok(())
    }
}

impl TypedApplication {
    /** Cheap decoder validation mirrors the TypeScript schema at the WASM
     * trust boundary, before an instruction can reach the VM. */
    fn validate(&self) -> Result<(), JsValue> {
        if self.version != "0.9" { return Err(JsValue::from_str("unsupported executable application version")); }
        if self.root_node >= self.nodes.len() { return Err(JsValue::from_str("root node handle out of range")); }
        for state in &self.state_slots {
            if state.initial_expression >= self.expressions.len() { return Err(JsValue::from_str("state expression handle out of range")); }
        }
        for event in &self.events {
            if event.target >= self.nodes.len() || event.event_type >= self.strings.len() || event.action >= self.actions.len() || event.fields.iter().any(|field| *field >= self.strings.len()) {
                return Err(JsValue::from_str("event handle out of range"));
            }
        }
        for action in &self.actions {
            let input_kinds = self.inputs.iter().map(|input| input.kind.clone()).collect::<Vec<_>>();
            validate_typed_action_contract(action, self.expressions.len(), &input_kinds).map_err(JsValue::from_str)?;
            for instruction in &action.instructions {
                match instruction {
                    TypedActionInstruction::Evaluate { expression } if *expression >= self.expressions.len() => return Err(JsValue::from_str("action expression handle out of range")),
                    TypedActionInstruction::StoreState { state } if *state >= self.state_slots.len() => return Err(JsValue::from_str("action state handle out of range")),
                    TypedActionInstruction::CollectionMutation { .. } => {}
                    TypedActionInstruction::Call { action, .. } if *action >= self.actions.len() => return Err(JsValue::from_str("action handle out of range")),
                    TypedActionInstruction::Jump { target } | TypedActionInstruction::JumpIfFalse { target } if *target >= action.instructions.len() => return Err(JsValue::from_str("action jump target out of range")),
                    TypedActionInstruction::CapabilityRequest { request, success_pc, failure_pc, finally_pc, result_slot, error_slot, .. } => {
                        if request.url >= self.expressions.len() || request.body.map(|body| body >= self.expressions.len()).unwrap_or(false) || request.headers.iter().any(|header| header.name >= self.strings.len() || header.value >= self.expressions.len()) || *success_pc >= action.instructions.len() || *failure_pc >= action.instructions.len() || finally_pc.map(|pc| pc >= action.instructions.len()).unwrap_or(false) || *result_slot >= action.frame_slots || *error_slot >= action.frame_slots { return Err(JsValue::from_str("invalid action continuation")); }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }
}

fn typed_eval(
    app: &TypedApplication,
    program: usize,
    states: &[Value],
    row: Option<&serde_json::Map<String, Value>>,
    row_index: usize,
) -> Result<Value, JsValue> {
    typed_eval_frame(app, program, states, row, row_index, &[], &[])
}
fn typed_eval_frame(
    app: &TypedApplication,
    program: usize,
    states: &[Value],
    row: Option<&serde_json::Map<String, Value>>,
    row_index: usize,
    frame: &[Value],
    event: &[Value],
) -> Result<Value, JsValue> {
    let instructions = &app
        .expressions
        .get(program)
        .ok_or_else(|| JsValue::from_str("expression handle out of range"))?
        .instructions;
    let mut stack = Vec::<Value>::new();
    let mut pc = 0usize;
    while pc < instructions.len() {
        let instruction = &instructions[pc];
        let op = instruction.get("op").and_then(Value::as_str).unwrap_or("");
        match op {
            "constant" => stack.push(
                app.constants
                    .get(
                        instruction
                            .get("constant")
                            .and_then(Value::as_u64)
                            .unwrap_or(0) as usize,
                    )
                    .cloned()
                    .unwrap_or(Value::Null),
            ),
            "loadState" => stack.push(
                states
                    .get(
                        instruction
                            .get("state")
                            .and_then(Value::as_u64)
                            .unwrap_or(0) as usize,
                    )
                    .cloned()
                    .unwrap_or(Value::Null),
            ),
            "loadRowField" => {
                let field = app
                    .strings
                    .get(
                        instruction
                            .get("field")
                            .and_then(Value::as_u64)
                            .unwrap_or(0) as usize,
                    )
                    .map(String::as_str)
                    .unwrap_or("");
                stack.push(if field.is_empty() {
                    Value::Object(row.cloned().unwrap_or_default())
                } else {
                    row.and_then(|value| value.get(field))
                        .cloned()
                        .unwrap_or(Value::Null)
                });
            }
            "loadEventField" => stack.push(event.get(instruction.get("field").and_then(Value::as_u64).unwrap_or(0) as usize).cloned().unwrap_or(Value::Null)),
            "loadFrame" => stack.push(frame.get(instruction.get("slot").and_then(Value::as_u64).unwrap_or(0) as usize).cloned().unwrap_or(Value::Null)),
            "field" => {
                let object = stack.pop().unwrap_or(Value::Null);
                let field = app
                    .strings
                    .get(
                        instruction
                            .get("field")
                            .and_then(Value::as_u64)
                            .unwrap_or(0) as usize,
                    )
                    .map(String::as_str)
                    .unwrap_or("");
                stack.push(object.get(field).cloned().unwrap_or(Value::Null));
            }
            "filter" | "map" => {
                let source = stack.pop().unwrap_or(Value::Null);
                let callback = instruction
                    .get(if op == "filter" {
                        "predicate"
                    } else {
                        "mapper"
                    })
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as usize;
                let mut output = Vec::new();
                for (index, value) in source
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .enumerate()
                {
                    let object = value.as_object().cloned().unwrap_or_default();
                    let value = typed_eval(app, callback, states, Some(&object), index)?;
                    if op == "map" {
                        output.push(value);
                    } else if typed_truthy(&value) {
                        output.push(Value::Object(object));
                    }
                }
                stack.push(Value::Array(output));
            }
            "string" => {
                let count = instruction
                    .get("count")
                    .and_then(Value::as_u64)
                    .unwrap_or(1) as usize;
                let mut parts = (0..count).filter_map(|_| stack.pop()).collect::<Vec<_>>();
                parts.reverse();
                let kind = instruction
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or("concat");
                let result = match kind {
                    "trim" => typed_value_string(parts.first().unwrap_or(&Value::Null))
                        .trim()
                        .to_owned(),
                    "lower" => {
                        typed_value_string(parts.first().unwrap_or(&Value::Null)).to_lowercase()
                    }
                    "upper" => {
                        typed_value_string(parts.first().unwrap_or(&Value::Null)).to_uppercase()
                    }
                    "includes" => typed_value_string(parts.first().unwrap_or(&Value::Null))
                        .contains(&typed_value_string(parts.get(1).unwrap_or(&Value::Null)))
                        .to_string(),
                    _ => parts.iter().map(typed_value_string).collect::<String>(),
                };
                stack.push(Value::String(result));
            }
            "binary" => {
                let right = stack.pop().unwrap_or(Value::Null);
                let left = stack.pop().unwrap_or(Value::Null);
                let result = match instruction
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                {
                    "equal" => Value::Bool(left == right),
                    "notEqual" => Value::Bool(left != right),
                    "and" => {
                        if typed_truthy(&left) {
                            right
                        } else {
                            left
                        }
                    }
                    "or" => {
                        if typed_truthy(&left) {
                            left
                        } else {
                            right
                        }
                    }
                    "coalesce" => {
                        if left.is_null() {
                            right
                        } else {
                            left
                        }
                    }
                    "add" => Value::String(format!(
                        "{}{}",
                        typed_value_string(&left),
                        typed_value_string(&right)
                    )),
                    _ => Value::Null,
                };
                stack.push(result);
            }
            "unary" => {
                let value = stack.pop().unwrap_or(Value::Null);
                stack.push(match instruction.get("kind").and_then(Value::as_str) {
                    Some("not") => Value::Bool(!typed_truthy(&value)),
                    Some("minus") => Value::from(-value.as_f64().unwrap_or(0.0)),
                    _ => value,
                });
            }
            "jumpIfFalse" => {
                if !typed_truthy(stack.last().unwrap_or(&Value::Null)) {
                    pc = instruction
                        .get("target")
                        .and_then(Value::as_u64)
                        .unwrap_or(pc as u64) as usize;
                    continue;
                }
            }
            "jump" => {
                pc = instruction
                    .get("target")
                    .and_then(Value::as_u64)
                    .unwrap_or(pc as u64) as usize;
                continue;
            }
            "return" => return Ok(stack.pop().unwrap_or(Value::Null)),
            _ => {}
        };
        pc += 1;
    }
    Ok(stack.pop().unwrap_or(Value::Null))
}
fn typed_apply_binding(
    app: &TypedApplication,
    binding: &TypedBinding,
    node: &Node,
    states: &[Value],
    row: Option<&serde_json::Map<String, Value>>,
    index: usize,
) -> Result<(), JsValue> {
    let value = typed_eval(app, binding.expression, states, row, index)?;
    if binding.sink == "text" {
        node.set_text_content(Some(&typed_value_string(&value)));
        return Ok(());
    }
    let element: Element = node
        .clone()
        .dyn_into()
        .map_err(|_| JsValue::from_str("binding target is not element"))?;
    let name = binding
        .name
        .and_then(|handle| app.strings.get(handle))
        .map(String::as_str)
        .unwrap_or("");
    if binding.sink == "property" {
        js_sys::Reflect::set(
            &element,
            &JsValue::from_str(name),
            &JsValue::from_str(&typed_value_string(&value)),
        )
        .map_err(|_| JsValue::from_str("property write failed"))?;
    } else {
        element.set_attribute(
            if name == "className" { "class" } else { name },
            &typed_value_string(&value),
        )?;
    }
    Ok(())
}
fn typed_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64().unwrap_or(0.0) != 0.0,
        Value::String(value) => !value.is_empty(),
        Value::Array(value) => !value.is_empty(),
        Value::Object(_) => true,
    }
}
fn typed_value_string(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Null => String::new(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        _ => value.to_string(),
    }
}
fn typed_event_field(name: &str, event: &Event, target: Option<&Element>) -> Value {
    match name {
        "type" => Value::String(event.type_()),
        "value" => target.and_then(|element| element.clone().dyn_into::<HtmlInputElement>().ok()).map(|input| Value::String(input.value())).unwrap_or(Value::Null),
        "checked" => target.and_then(|element| element.clone().dyn_into::<HtmlInputElement>().ok()).map(|input| Value::Bool(input.checked())).unwrap_or(Value::Null),
        "rowKey" => target.and_then(|element| element.closest("[data-runtime-row-key]").ok().flatten()).and_then(|row| row.get_attribute("data-runtime-row-key")).map(Value::String).unwrap_or(Value::Null),
        "key" => event.clone().dyn_into::<KeyboardEvent>().map(|key| Value::String(key.key())).unwrap_or(Value::Null),
        "button" => event.clone().dyn_into::<MouseEvent>().map(|mouse| Value::from(mouse.button())).unwrap_or(Value::Null),
        "metaKey" => event.clone().dyn_into::<KeyboardEvent>().map(|key| Value::Bool(key.meta_key())).or_else(|event| event.dyn_into::<MouseEvent>().map(|mouse| Value::Bool(mouse.meta_key()))).unwrap_or(Value::Bool(false)),
        "ctrlKey" => event.clone().dyn_into::<KeyboardEvent>().map(|key| Value::Bool(key.ctrl_key())).or_else(|event| event.dyn_into::<MouseEvent>().map(|mouse| Value::Bool(mouse.ctrl_key()))).unwrap_or(Value::Bool(false)),
        "shiftKey" => event.clone().dyn_into::<KeyboardEvent>().map(|key| Value::Bool(key.shift_key())).or_else(|event| event.dyn_into::<MouseEvent>().map(|mouse| Value::Bool(mouse.shift_key()))).unwrap_or(Value::Bool(false)),
        "altKey" => event.clone().dyn_into::<KeyboardEvent>().map(|key| Value::Bool(key.alt_key())).or_else(|event| event.dyn_into::<MouseEvent>().map(|mouse| Value::Bool(mouse.alt_key()))).unwrap_or(Value::Bool(false)),
        _ => Value::Null,
    }
}
fn instantiate(
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
fn apply_binding_with_context(
    node: &Node,
    binding: &Binding,
    expressions: &[Expression],
    scope: &HashMap<String, Value>,
    environment: &HashMap<String, Value>,
) -> Result<(), JsValue> {
    let value = binding
        .expression_id
        .as_ref()
        .and_then(|id| expressions.iter().find(|entry| entry.id == *id))
        .map(|entry| evaluate_with_context(&entry.expression, scope, environment))
        .unwrap_or(Value::Null);
    if binding.kind == "text" {
        node.clone()
            .dyn_into::<web_sys::Text>()
            .map_err(|_| JsValue::from_str("text binding target"))?
            .set_data(&value_string(Some(&value)));
    } else {
        let element: Element = node
            .clone()
            .dyn_into()
            .map_err(|_| JsValue::from_str("element binding target"))?;
        set_value(
            &element,
            binding.attribute_name.as_deref().unwrap_or_default(),
            &value,
        )?;
    }
    Ok(())
}
fn apply_binding_host(
    node: &Node,
    binding: &Binding,
    expressions: &[Expression],
    scope: &HashMap<String, Value>,
    handles: &HashMap<String, Node>,
) -> Result<(), JsValue> {
    let value = binding
        .expression_id
        .as_ref()
        .and_then(|id| expressions.iter().find(|entry| entry.id == *id))
        .map(|entry| evaluate(&resolve_host_reads(&entry.expression, handles), scope))
        .unwrap_or(Value::Null);
    if binding.kind == "text" {
        node.clone()
            .dyn_into::<web_sys::Text>()
            .map_err(|_| JsValue::from_str("text binding target"))?
            .set_data(&value_string(Some(&value)));
    } else {
        set_value(
            &node
                .clone()
                .dyn_into::<Element>()
                .map_err(|_| JsValue::from_str("element binding target"))?,
            binding.attribute_name.as_deref().unwrap_or_default(),
            &value,
        )?;
    }
    Ok(())
}
fn resolve_host_reads(value: &Value, handles: &HashMap<String, Node>) -> Value {
    if value.get("kind").and_then(Value::as_str) == Some("host-element-read") {
        let reference = value
            .get("refId")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let Some(element) = handles
            .get(reference)
            .and_then(|node| node.clone().dyn_into::<Element>().ok())
        else {
            return serde_json::json!({"kind":"literal","value":null});
        };
        let capability = value.get("capability").unwrap_or(&Value::Null);
        let result = match capability.get("kind").and_then(Value::as_str) {
            Some("property") => match capability.get("name").and_then(Value::as_str) {
                Some("value") => element
                    .clone()
                    .dyn_into::<web_sys::HtmlInputElement>()
                    .ok()
                    .map(|input| Value::String(input.value()))
                    .unwrap_or(Value::Null),
                Some("tagName") => Value::String(element.tag_name()),
                Some(name) => element
                    .get_attribute(name)
                    .map(Value::String)
                    .unwrap_or(Value::Bool(false)),
                _ => Value::Null,
            },
            Some("closest") => Value::Bool(
                element
                    .closest(
                        capability
                            .get("selector")
                            .and_then(Value::as_str)
                            .unwrap_or_default(),
                    )
                    .ok()
                    .flatten()
                    .is_some(),
            ),
            Some("is-active") => Value::Bool(
                element
                    .owner_document()
                    .and_then(|document| document.active_element())
                    .map(|active| active == element)
                    .unwrap_or(false),
            ),
            _ => Value::Null,
        };
        return serde_json::json!({"kind":"literal","value":result});
    }
    match value {
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|item| resolve_host_reads(item, handles))
                .collect(),
        ),
        Value::Object(values) => Value::Object(
            values
                .iter()
                .map(|(key, item)| (key.clone(), resolve_host_reads(item, handles)))
                .collect(),
        ),
        _ => value.clone(),
    }
}
fn expression_value_with_context(
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
fn apply_binding(
    node: &Node,
    binding: &Binding,
    expressions: &[Expression],
    scope: &HashMap<String, Value>,
) -> Result<(), JsValue> {
    let value = expressions
        .iter()
        .find(|entry| Some(&entry.id) == binding.expression_id.as_ref())
        .map(|entry| evaluate(&entry.expression, scope))
        .unwrap_or(Value::Null);
    if binding.kind == "text" {
        node.clone()
            .dyn_into::<web_sys::Text>()
            .map_err(|_| JsValue::from_str("text binding target"))?
            .set_data(&value_string(Some(&value)));
    } else {
        let element: Element = node
            .clone()
            .dyn_into()
            .map_err(|_| JsValue::from_str("element binding target"))?;
        set_value(
            &element,
            binding.attribute_name.as_deref().unwrap_or_default(),
            &value,
        )?;
    }
    Ok(())
}
fn expression_value(
    id: &Option<String>,
    expressions: &[Expression],
    scope: &HashMap<String, Value>,
) -> Value {
    id.as_ref()
        .and_then(|id| expressions.iter().find(|entry| entry.id == *id))
        .map(|entry| evaluate(&entry.expression, scope))
        .unwrap_or(Value::Null)
}
fn apply_prop_program(
    node: &Node,
    program: &PropProgram,
    expressions: &[Expression],
    scope: &HashMap<String, Value>,
) -> Result<(), JsValue> {
    let element: Element = node
        .clone()
        .dyn_into()
        .map_err(|_| JsValue::from_str("prop program target"))?;
    for write in &program.writes {
        if write.kind == "event" || write.kind == "ref" {
            continue;
        }
        if write.kind == "spread" {
            let value = expression_value(&write.expression_id, expressions, scope);
            let Some(record) = value.as_object() else {
                continue;
            };
            for (name, value) in record {
                if name == "children" || name.starts_with("on") {
                    continue;
                }
                set_value(&element, name, value)?;
            }
            continue;
        }
        let value = write
            .static_value
            .as_ref()
            .map(|value| Value::String(value.clone()))
            .unwrap_or_else(|| expression_value(&write.expression_id, expressions, scope));
        set_value(&element, &write.name, &value)?;
    }
    Ok(())
}
fn program_dependencies(
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
fn set_value(element: &Element, name: &str, value: &Value) -> Result<(), JsValue> {
    let name = if name == "className" { "class" } else { name };
    if name == "checked" {
        if let Ok(input) = element.clone().dyn_into::<web_sys::HtmlInputElement>() {
            input.set_checked(value.as_bool().unwrap_or(false));
            return Ok(());
        }
    }
    if name == "indeterminate" {
        if let Ok(input) = element.clone().dyn_into::<web_sys::HtmlInputElement>() {
            input.set_indeterminate(value.as_bool().unwrap_or(false));
            return Ok(());
        }
    }
    if name == "value" {
        if let Ok(input) = element.clone().dyn_into::<web_sys::HtmlInputElement>() {
            input.set_value(&value_string(Some(value)));
            return Ok(());
        }
    }
    if value.as_bool() == Some(false) || value.is_null() {
        element.remove_attribute(name)?;
    } else {
        element.set_attribute(name, &value_string(Some(value)))?;
    }
    Ok(())
}
fn evaluate(expression: &Value, scope: &HashMap<String, Value>) -> Value {
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
fn evaluate_with_context(
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
fn number(value: f64) -> Value {
    serde_json::Number::from_f64(value)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}
fn binding_dependencies(binding: &Binding, expressions: &HashMap<&str, &Value>) -> HashSet<String> {
    let mut fields = HashSet::new();
    if let Some(expression_id) = binding.expression_id.as_deref() {
        if let Some(expression) = expressions.get(expression_id) {
            collect_dependencies(expression, &mut fields);
        }
    }
    fields
}
fn collect_dependencies(expression: &Value, fields: &mut HashSet<String>) {
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
fn index_elements(app: &Application) -> HashMap<String, ElementNode> {
    app.elements
        .iter()
        .cloned()
        .map(|node| (node.id.clone(), node))
        .collect()
}
fn index_texts(app: &Application) -> HashMap<String, TextNode> {
    app.texts
        .iter()
        .cloned()
        .map(|node| (node.id.clone(), node))
        .collect()
}
fn index_contexts(app: &Application) -> HashMap<String, ContextScope> {
    app.contexts
        .iter()
        .cloned()
        .map(|node| (node.id.clone(), node))
        .collect()
}
fn index_loops(app: &Application) -> HashMap<String, Loop> {
    app.loops
        .iter()
        .cloned()
        .map(|node| (node.id.clone(), node))
        .collect()
}
fn index_conditionals(app: &Application) -> HashMap<String, Conditional> {
    app.conditionals
        .iter()
        .cloned()
        .map(|node| (node.id.clone(), node))
        .collect()
}
/** Node ids owned by a conditional branch. Removing these registrations before
 * mounting the opposite branch prevents listeners/bindings from retaining
 * detached DOM targets. */
fn conditional_branch_node_ids(app: &Application, conditional: &Conditional) -> HashSet<String> {
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
fn context_defaults(app: &Application) -> HashMap<String, Value> {
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
fn find_loop(app: &Application, input: &str) -> Result<Loop, JsValue> {
    app.loops
        .iter()
        .find(|entry| {
            entry.input_id.as_deref() == Some(input) || entry.query_id.as_deref() == Some(input)
        })
        .cloned()
        .ok_or_else(|| JsValue::from_str("loop missing"))
}
fn row_scope(item_name: &str, row: &HashMap<String, Value>) -> HashMap<String, Value> {
    let value = Value::Object(
        row.iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    );
    HashMap::from([(item_name.into(), value)])
}
fn value_string(value: Option<&Value>) -> String {
    match value.unwrap_or(&Value::Null) {
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Null => String::new(),
        value => value.to_string(),
    }
}
fn parse_initial_state(value: &str) -> Value {
    serde_json::from_str(value).unwrap_or_else(|_| match value {
        "undefined" => Value::Null,
        other => Value::String(other.into()),
    })
}
fn truthy(value: &Value) -> bool {
    !value.is_null() && value.as_bool() != Some(false) && value.as_str() != Some("")
}
fn document() -> Result<Document, JsValue> {
    web_sys::window()
        .ok_or_else(|| JsValue::from_str("window unavailable"))?
        .document()
        .ok_or_else(|| JsValue::from_str("document unavailable"))
}
fn window() -> Result<web_sys::Window, JsValue> {
    web_sys::window().ok_or_else(|| JsValue::from_str("window unavailable"))
}
fn now() -> f64 {
    web_sys::window()
        .and_then(|window| window.performance())
        .map(|performance| performance.now())
        .unwrap_or(0.0)
}
fn error(error: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&error.to_string())
}
fn finish(metrics: MountMetrics) -> Result<JsValue, JsValue> {
    serde_wasm_bindgen::to_value(&metrics).map_err(error)
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

    #[test]
    fn graph_instance_identity_comes_from_mount_topology_and_keys() {
        assert_eq!(graph_instance_id(None, "main", None), "root/outlet:main");
        assert_eq!(
            graph_instance_id(Some("root/outlet:main"), "rows", Some("todo/1")),
            "root%2Foutlet:main/outlet:rows/key:todo%2F1"
        );
    }

    fn typed_action_artifact(instruction: Value, input_kind: &str) -> TypedApplication {
        serde_json::from_value(serde_json::json!({
            "version": "0.9",
            "rootNode": 0,
            "strings": ["div"],
            "nodes": [{"op": "element", "tag": 0, "parent": null}],
            "inputs": [{"name": 0, "kind": input_kind}],
            "expressions": [{"instructions": []}],
            "actions": [{"instructions": [instruction]}]
        })).unwrap()
    }

    #[test]
    fn typed_decoder_rejects_invalid_collection_action_operands() {
        let scalar = typed_action_artifact(
            serde_json::json!({"op":"collectionMutation","input":0,"kind":"append","key":0,"value":0}),
            "scalar",
        );
        assert!(validate_typed_action_contract(&scalar.actions[0], scalar.expressions.len(), &["scalar".into()]).unwrap_err().contains("collection input"));

        let missing_value = typed_action_artifact(
            serde_json::json!({"op":"collectionMutation","input":0,"kind":"append","key":0}),
            "collection",
        );
        assert!(validate_typed_action_contract(&missing_value.actions[0], missing_value.expressions.len(), &["collection".into()]).unwrap_err().contains("requires a value"));

        let remove_value = typed_action_artifact(
            serde_json::json!({"op":"collectionMutation","input":0,"kind":"keyedRemove","key":0,"value":0}),
            "collection",
        );
        assert!(validate_typed_action_contract(&remove_value.actions[0], remove_value.expressions.len(), &["collection".into()]).unwrap_err().contains("forbids a value"));
    }

    #[test]
    fn typed_decoder_rejects_duplicate_parameter_slots() {
        let mut app = typed_action_artifact(serde_json::json!({"op":"return"}), "collection");
        app.actions[0].frame_slots = 1;
        app.actions[0].parameter_slots = vec![0, 0];
        assert!(validate_typed_action_contract(&app.actions[0], app.expressions.len(), &["collection".into()]).unwrap_err().contains("duplicate"));
    }
}
