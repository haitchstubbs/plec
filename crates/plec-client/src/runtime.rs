use crate::bindings::*;
use crate::cookie::*;
use crate::events::*;
#[cfg(feature = "fetch")]
use crate::fetch::*;
use crate::prelude::*;
use crate::reorder::keyed_reorder_plan;
use plec_dom::cookie::CookiePolicy;
use plec_dom::platform::*;
use plec_eval::eval::*;
use plec_schema::typed::TypedComponentProp;
use std::cell::Cell;
use std::collections::{HashMap, HashSet};

thread_local! {
    /// Test instrumentation for the adoption index: DOM nodes visited while
    /// building ownership indexes. Exposed through `PlecRuntime` counters so
    /// wasm integration tests can prove nested adoptions walk only their own
    /// scope instead of re-walking the whole adoption root per component
    /// level.
    static ADOPTION_INDEX_WALKS: Cell<u32> = const { Cell::new(0) };
    /// Divergence policy observation: server-rendered text nodes whose
    /// recomputed static binding differs after a snapshot-backed adoption.
    /// Divergence is allowed (recompute-consequences semantics) and only ever
    /// reported, never treated as an adoption failure. Exposed through
    /// `PlecRuntime::ssr_text_divergences` for the dev diagnostic channel.
    static SSR_TEXT_DIVERGENCES: Cell<u32> = const { Cell::new(0) };
}

/// Converts a host-supplied payload into a JSON-plain JS value. The
/// serde-wasm-bindgen serializer emits serde maps as JS `Map` instances,
/// which `JSON.stringify` silently renders as `{}` — every map, array, and
/// plain object is rebuilt as a plain object/array first. Recursion is
/// depth- and width-capped so a hostile JS graph fails predictably instead of
/// overflowing the WASM stack or exhausting memory.
pub fn normalize_json_value(value: &JsValue, depth: usize) -> Result<JsValue, JsValue> {
    use plec_ir::limits::MAX_DECODE_JS_NODES;
    let mut remaining_nodes = MAX_DECODE_JS_NODES;
    normalize_json_value_bounded(value, depth, &mut remaining_nodes)
}

fn normalize_json_value_bounded(
    value: &JsValue,
    depth: usize,
    remaining_nodes: &mut usize,
) -> Result<JsValue, JsValue> {
    use plec_ir::limits::MAX_DECODE_JS_DEPTH;
    if depth > MAX_DECODE_JS_DEPTH {
        return Err(JsValue::from_str(
            "payload nesting exceeds the decode depth limit",
        ));
    }
    if *remaining_nodes == 0 {
        return Err(JsValue::from_str(
            "payload property or element count exceeds the decode width limit",
        ));
    }
    *remaining_nodes -= 1;

    if value.is_undefined() {
        // serde-wasm-bindgen represents Option::None as undefined. Preserve
        // the old decoder's JSON-compatible null semantics before stringify.
        return Ok(JsValue::NULL);
    }
    if value.is_instance_of::<js_sys::Map>() {
        let map = value.unchecked_ref::<js_sys::Map>();
        if map.size() as usize > *remaining_nodes {
            return Err(JsValue::from_str(
                "payload property or element count exceeds the decode width limit",
            ));
        }
        let out = js_sys::Object::new();
        let entries = map.entries();
        loop {
            let next = entries.next()?;
            if next.done() {
                break;
            }
            let pair = js_sys::Array::from(&next.value());
            let key = pair
                .get(0)
                .as_string()
                .ok_or_else(|| JsValue::from_str("map keys must be strings"))?;
            let normalized =
                normalize_json_value_bounded(&pair.get(1), depth + 1, remaining_nodes)?;
            js_sys::Reflect::set(&out, &key.into(), &normalized)?;
        }
        Ok(out.into())
    } else if value.is_instance_of::<js_sys::Array>() {
        let array = js_sys::Array::from(value);
        if array.length() as usize > *remaining_nodes {
            return Err(JsValue::from_str(
                "payload property or element count exceeds the decode width limit",
            ));
        }
        let out = js_sys::Array::new();
        for item in array.iter() {
            out.push(&normalize_json_value_bounded(
                &item,
                depth + 1,
                remaining_nodes,
            )?);
        }
        Ok(out.into())
    } else if value.is_object() {
        let object = value.unchecked_ref::<js_sys::Object>();
        let keys = js_sys::Object::keys(object);
        if keys.length() as usize > *remaining_nodes {
            return Err(JsValue::from_str(
                "payload property or element count exceeds the decode width limit",
            ));
        }
        let out = js_sys::Object::new();
        for key in keys.iter() {
            let field = js_sys::Reflect::get(object, &key)?;
            let normalized = normalize_json_value_bounded(&field, depth + 1, remaining_nodes)?;
            js_sys::Reflect::set(&out, &key, &normalized)?;
        }
        Ok(out.into())
    } else {
        Ok(value.clone())
    }
}

/// Decodes a host-supplied JSON payload with a byte ceiling before
/// recursion: the payload round-trips through the JS engine's own
/// `JSON.stringify` (native stack) and `serde_json`'s depth-guarded
/// parser, so hostile shapes fail predictably instead of overflowing the
/// WASM stack during `serde_wasm_bindgen`'s recursive walk.
pub(crate) fn decode_bounded_json<T: serde::de::DeserializeOwned>(
    value: &JsValue,
    max_bytes: usize,
    label: &str,
) -> Result<T, JsValue> {
    if value.is_undefined() || value.is_null() {
        return Err(JsValue::from_str(&format!("{label} is not valid JSON")));
    }
    let normalized = normalize_json_value(value, 0)?;
    let text = js_sys::JSON::stringify(&normalized)
        .map_err(|_| JsValue::from_str(&format!("{label} is not valid JSON")))?;
    let text: String = text.into();
    if text.len() > max_bytes {
        return Err(JsValue::from_str(&format!("{label} exceeds byte limit")));
    }
    serde_json::from_str(&text)
        .map_err(|error| JsValue::from_str(&format!("{label} is not valid JSON: {error}")))
}

/// Recorded execution state for one nested component instance
/// (`PlecSsrSnapshot.structure.nested`), keyed by the component's marker
/// path. Branch records are the ownership cause for the component's own
/// conditionals; loop records are the claimed row identity and order. Row
/// values recompute from imported runtime state and are never transferred.
#[derive(Clone, Default)]
pub struct SsrNestedComponentRecords {
    pub branches: HashMap<usize, plec_ir::SsrSelectedBranch>,
    pub loops: HashMap<usize, Vec<String>>,
}

/// Nested component records reachable below one runtime, keyed by marker
/// path. Each adopted component passes its subtree's records down to the
/// child runtime so arbitrary nesting depth resolves without re-walking.
pub type SsrNestedRecords = HashMap<String, SsrNestedComponentRecords>;

#[derive(Default)]
pub struct TypedLoopRows {
    pub order: Vec<String>,
    pub rows: HashMap<String, TypedRow>,
    /// The canonical address prefix every row of this loop extends:
    /// `{parent}/loop:{node}:` — recorded at instantiation/adoption so delta
    /// inserts emit the same `plec:loop:{rowPath}` grammar the server did.
    pub path_base: Option<String>,
}

pub struct TypedRow {
    pub root: Node,
    pub end: Option<Node>,
    pub values: HashMap<String, RuntimeValue>,
    pub nodes: HashMap<usize, Node>,
    /// Concrete, row-owned conditional regions. A branch change must never
    /// replace the keyed row which contains it.
    pub conditionals: HashMap<usize, TypedConditionalRegion>,
    pub generation: u64,
    pub region_slot: RegionSlot,
}

/// Mutable ownership for a static conditional branch. The graph node table is
/// immutable; this region owns the concrete nodes that exist for one branch.
pub struct TypedConditionalRegion {
    pub start: Node,
    pub end: Node,
    pub selected: Option<usize>,
    pub nodes: HashMap<usize, Node>,
    pub generation: u64,
    pub region_slot: RegionSlot,
}

/// A listener is requested as part of creating a concrete node/region. The
/// public wrapper drains this list once mutable DOM work has released its
/// borrow; it is intentionally not a recovery scan of the whole graph.
pub struct TypedListenerRequest {
    pub target: usize,
    pub node: Node,
    pub owner: TypedListenerOwner,
}

pub struct TypedComponentRequest {
    pub call: usize,
    pub component: usize,
    pub props: HashMap<String, RuntimeValue>,
    pub callbacks: HashMap<String, TypedCallbackSpec>,
    pub component_props: HashMap<String, TypedComponentTarget>,
    pub children: Vec<usize>,
    pub row_context: Option<TypedRowContext>,
    pub start: Node,
    pub end: Node,
    pub key: String,
    /// The child instance's canonical structural address
    /// (`{parent}/component:{call}`), shared by fresh mounts and adoption so
    /// CSR-created component DOM carries the server-rendered grammar.
    pub path: String,
    /// SSR already owns this range. Component work must bind it rather than
    /// inserting a second copy before the end marker.
    pub adoption: Option<TypedAdoptionRequest>,
    /// This child instance's own branch/loop records, extracted from the
    /// parent's nested map by marker path. Consumed by the child `adopt`.
    pub ssr_branches: HashMap<usize, plec_ir::SsrSelectedBranch>,
    pub ssr_loops: HashMap<usize, Vec<String>>,
    /// Deeper nested records below this child, passed down for grandchild
    /// adoption.
    pub ssr_nested: SsrNestedRecords,
}

#[derive(Clone)]
pub enum TypedComponentTarget {
    Native(usize),
    Host(TypedHostComponentTarget),
}

pub struct TypedAdoptionRequest {
    pub root: Element,
}

#[derive(Clone)]
pub struct TypedRowContext {
    pub loop_index: usize,
    pub row_key: String,
}

pub struct TypedSlotRequest {
    pub start: Node,
    pub end: Node,
}

#[derive(Clone)]
pub struct TypedCallback {
    pub parent_id: String,
    pub action: usize,
    pub row: Option<HashMap<String, RuntimeValue>>,
    pub arguments: Vec<RuntimeValue>,
}

#[derive(Clone)]
pub struct TypedCallbackSpec {
    pub action: usize,
    pub row: Option<HashMap<String, RuntimeValue>>,
}

pub struct TypedComponentRefresh {
    pub call: usize,
    pub start: Node,
    pub props: HashMap<String, RuntimeValue>,
    pub component_props: HashMap<String, TypedComponentTarget>,
}

/// Component-valued props are resolved only when their concrete target is
/// mounted. A library component using a direct `(props)` parameter therefore
/// needs the complete named value-prop record rather than a lookup by the
/// synthetic `__plec_props` name.
fn component_runtime_props(
    app: &TypedApplication,
    props: &HashMap<String, RuntimeValue>,
) -> Result<Vec<RuntimeValue>, JsValue> {
    app.parameters
        .iter()
        .map(|parameter| {
            let name = app
                .strings
                .get(parameter.name)
                .ok_or_else(|| JsValue::from_str("component parameter handle out of range"))?;
            if name == "__plec_props" {
                Ok(props
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| RuntimeValue::Record(props.clone())))
            } else {
                Ok(props.get(name).cloned().unwrap_or(RuntimeValue::Null))
            }
        })
        .collect()
}

pub struct TypedRuntime {
    pub app: TypedApplication,
    /// This instance's canonical structural address (`root`, or
    /// `{parent}/outlet:{id}` / `{parent}/component:{call}` below it). Every
    /// emitted DOM address derives from this prefix plus graph topology —
    /// never from allocation counters or DOM position.
    pub path: String,
    pub root: Option<Element>,
    /// The mounted graph itself is one live region. Rows and conditional
    /// regions acquire their own slots below it.
    pub region_slot: RegionSlot,
    pub region_tracker: Rc<RegionTracker>,
    pub reconcile_budget: Rc<RefCell<Option<ReconcileBudget>>>,
    pub nodes: HashMap<usize, Node>,
    pub states: Vec<RuntimeValue>,
    pub host_ref_nodes: Vec<Option<Node>>,
    /// DOM objects live only in these opaque focus slots, never RuntimeValue.
    pub focus_refs: Vec<Option<Element>>,
    pub pending_reactions: Vec<usize>,
    /// Current nested reaction-drain depth (see
    /// `limits::MAX_REACTION_DRAIN_DEPTH`): reaction actions re-enter
    /// refresh_state and drain again, so depth must be tracked across
    /// nested drains to keep recursion bounded.
    pub reaction_drain_depth: usize,
    /// Current nested mount-instantiation depth (see
    /// `limits::MAX_MOUNT_DEPTH`): a validated acyclic chain may still be
    /// deep enough to overflow the WASM stack, so mount recursion is bounded.
    pub mount_depth: usize,
    /// Frame address recorded when the outermost `instantiate_node` entered
    /// (see `limits::MAX_MOUNT_STACK_BYTES`). Zero outside a mount chain.
    /// The logical depth budget cannot guarantee native stack safety on its
    /// own because per-frame cost varies with node kind and build profile.
    pub mount_stack_base: usize,
    /// Current nested SSR row-adoption depth (see `limits::MAX_MOUNT_DEPTH`):
    /// `adopt_row_node` recurses over element children and conditional
    /// branches just like mount instantiation, so adoption shares the same
    /// depth ceiling.
    pub adopt_depth: usize,
    /// Frame address recorded when the outermost `adopt_row_node` entered
    /// (see `limits::MAX_MOUNT_STACK_BYTES`). Zero outside an adoption walk.
    pub adopt_stack_base: usize,
    pub reaction_cleanups: Vec<Option<usize>>,
    pub collections: HashMap<usize, TypedCollection>,
    pub loops: HashMap<usize, TypedLoopRows>,
    pub conditionals: HashMap<usize, TypedConditionalRegion>,
    pub listeners: Vec<TypedListener>,
    pub global_listeners: Vec<TypedGlobalListenerHandle>,
    pub listener_requests: Vec<TypedListenerRequest>,
    pub component_requests: Vec<TypedComponentRequest>,
    pub slot_requests: Vec<TypedSlotRequest>,
    pub component_refreshes: Vec<TypedComponentRefresh>,
    pub callback_requests: Vec<TypedCallback>,
    pub callbacks: Vec<Option<TypedCallback>>,
    pub next_generation: u64,
    pub graph_generation: u64,
    pub host_inputs: HashMap<String, RuntimeValue>,
    pub host_refs: HashMap<String, Node>,
    pub host_instances: Vec<HostInstance>,
    /// Host-owned cookie capability policy shared with the owning
    /// `RuntimeState`. Synchronous `getSync` reads evaluate against this
    /// per-runtime store, so separately embedded runtimes never observe each
    /// other's grants.
    pub cookie_policy: Rc<RefCell<Option<HashMap<String, CookiePolicy>>>>,
    /// Element-tag policy this instance validates and instantiates under.
    /// Defaults to strict; hosts widen it only through the explicit
    /// `RuntimeState::set_tag_policy` capability channel.
    pub tag_policy: plec_ir::sink::TagPolicy,
    /// Component indices in a 0.10 node are local to its graph artifact.
    pub component_definitions: Option<Vec<TypedApplication>>,
    pub pending_cookies: Vec<TypedPendingCookie>,
    pub next_cookie_id: u64,
    pub next_component_instance: u64,
    #[cfg(feature = "fetch")]
    pub pending_fetches: Vec<TypedPendingFetch>,
    #[cfg(feature = "fetch")]
    pub next_fetch_id: u64,
    #[cfg(feature = "fetch")]
    pub abort_controllers: HashMap<u64, AbortController>,
    /// True when this instance adopted server DOM on behalf of an imported
    /// SSR snapshot. Gates the binding-divergence observation in
    /// `apply_static_bindings`: recomputed static values may legally differ
    /// from the server-rendered text, and that divergence is counted instead
    /// of being treated as an adoption failure.
    pub ssr_imported: bool,
    /// Nested component records this runtime may hand to adopted children,
    /// scoped to the subtree below this runtime's path. Empty outside a
    /// snapshot-backed adoption.
    pub ssr_nested: SsrNestedRecords,
}

pub struct HostInstance {
    pub boundary: Element,
    pub handle: JsValue,
    pub call: Option<usize>,
    pub provider: String,
    pub component: String,
}

/** Mutable typed graph ownership. Definitions live in `typed_component_registry`; this
 * record exists only for one mounted route position. */
pub struct TypedGraphInstance {
    pub parent_id: Option<String>,
    pub outlet_id: String,
    pub graph_id: String,
    pub route_id: Option<String>,
    pub match_key: Option<String>,
    pub route_state: Option<TypedRouteState>,
    /// The normal graph remains alive only while its route loader is pending.
    pub loader_runtime: Option<TypedRuntime>,
    pub component_call: Option<usize>,
    pub component_start: Option<Node>,
    pub runtime: TypedRuntime,
}

impl TypedGraphInstance {
    pub fn runtime_for_generation(&self, generation: u64) -> Option<&TypedRuntime> {
        if self.runtime.graph_generation == generation {
            Some(&self.runtime)
        } else {
            self.loader_runtime
                .as_ref()
                .filter(|runtime| runtime.graph_generation == generation)
        }
    }

    pub fn runtime_for_generation_mut(&mut self, generation: u64) -> Option<&mut TypedRuntime> {
        if self.runtime.graph_generation == generation {
            Some(&mut self.runtime)
        } else {
            self.loader_runtime
                .as_mut()
                .filter(|runtime| runtime.graph_generation == generation)
        }
    }
}

pub struct TypedRouteState {
    pub normal_graph_id: String,
    pub pending_graph_id: Option<String>,
    pub pending_mode: String,
    pub error_graph_id: Option<String>,
    pub loader_action: Option<usize>,
    pub params: HashMap<String, String>,
    pub location: (String, String, String),
    pub phase: TypedRoutePhase,
}

#[derive(PartialEq, Eq)]
pub enum TypedRoutePhase {
    Normal,
    Loading,
    Error,
}

impl RuntimeState {
    pub fn mount(&self, root: Element) -> Result<JsValue, JsValue> {
        self.mount_typed(root)
    }
}

impl TypedRuntime {
    fn clear_host_ref_for_node(&mut self, index: usize, node: &Node) {
        let Some(TypedNode::Element {
            host_ref: Some(reference),
            ..
        }) = self.app.nodes.get(index)
        else {
            return;
        };
        if self
            .host_ref_nodes
            .get(*reference)
            .and_then(Option::as_ref)
            .is_some_and(|active| active.is_same_node(Some(node)))
        {
            self.host_ref_nodes[*reference] = None;
        }
    }
    pub fn set_component_definitions(&mut self, definitions: Vec<TypedApplication>) {
        self.component_definitions = Some(definitions);
    }
}

impl RuntimeState {
    pub fn apply_delta(&self, delta: JsValue) -> Result<JsValue, JsValue> {
        self.apply_typed_delta(delta)
    }

    /// Keyed-delta entry point for routed and standalone host inputs. The
    /// legacy per-instance fallback (removed with the IR 0.8 graph scheme,
    /// wasm-runtime-ixk.7) used to dispatch here only when a typed instance
    /// forest was live; typed instances are now the only execution state.
    pub fn apply_deltas(&self, deltas: JsValue) -> Result<JsValue, JsValue> {
        let deltas: Vec<Delta> = decode_bounded_json(
            &deltas,
            plec_schema::limits::MAX_HOST_INPUT_JSON_BYTES,
            "deltas",
        )?;
        let deltas = coalesce_deltas(deltas);
        self.apply_typed_deltas(deltas)
    }

    pub fn initialize_input(&self, input_id: String, rows: JsValue) -> Result<JsValue, JsValue> {
        self.initialize_typed_input(&input_id, rows)
    }

    pub fn list_input_instances(&self, input_id: String) -> Result<JsValue, JsValue> {
        let ids = self
            .typed
            .borrow()
            .iter()
            .filter_map(|(id, instance)| {
                instance
                    .runtime
                    .app
                    .inputs
                    .iter()
                    .any(|input| {
                        instance
                            .runtime
                            .app
                            .strings
                            .get(input.name)
                            .map(String::as_str)
                            == Some(input_id.as_str())
                    })
                    .then(|| id.clone())
            })
            .collect::<Vec<_>>();
        serde_wasm_bindgen::to_value(&ids).map_err(error)
    }
}

impl RuntimeState {
    pub fn mount_typed(&self, root: Element) -> Result<JsValue, JsValue> {
        let id = graph_instance_id(None, "main", None);
        let metrics = {
            let mut typed = self.typed.borrow_mut();
            typed
                .get_mut(&id)
                .ok_or_else(|| JsValue::from_str("typed application missing"))?
                .runtime
                .mount(root)?
        };
        self.flush_component_work()?;
        self.install_typed_event_listeners()?;
        self.install_typed_global_listeners()?;
        serde_wasm_bindgen::to_value(&metrics).map_err(error)
    }
}

impl RuntimeState {
    /// The live instance forest must never overwrite an entry. The monotonic
    /// component counter and the router's dispose-before-mount discipline
    /// make id collisions unreachable for well-formed callers, so a
    /// collision is a broken invariant: it must fail loudly instead of
    /// silently replacing a live instance and stranding its DOM.
    pub fn ensure_typed_instance_absent(&self, id: &str, origin: &str) -> Result<(), JsValue> {
        let existing = self
            .typed
            .borrow()
            .get(id)
            .map(|instance| (instance.graph_id.clone(), instance.outlet_id.clone()));
        if let Some((graph_id, outlet_id)) = existing {
            return Err(error(format!(
                "typed graph instance id collision: {origin} would overwrite live instance {id} (existing graph {graph_id}, outlet {outlet_id})"
            )));
        }
        Ok(())
    }

    pub fn flush_component_work(&self) -> Result<(), JsValue> {
        loop {
            self.mount_component_requests()?;
            let refreshes = self
                .typed
                .borrow_mut()
                .iter_mut()
                .flat_map(|(parent, instance)| {
                    instance
                        .runtime
                        .component_refreshes
                        .drain(..)
                        .map(|refresh| (parent.clone(), refresh))
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            if refreshes.is_empty() {
                self.dispose_orphan_components();
                // The outer reconcile transaction includes every deferred
                // child mount/refresh above; release its unused reservation
                // only once this work queue reaches quiescence.
                self.reconcile_budget.borrow_mut().take();
                return Ok(());
            }
            for (parent, refresh) in refreshes {
                let host_call = self.typed.borrow().get(&parent).and_then(|instance| {
                    instance
                        .runtime
                        .host_instances
                        .iter()
                        .any(|host| host.boundary.is_same_node(Some(&refresh.start)))
                        .then_some(())
                });
                if host_call.is_some() {
                    let mut typed = self.typed.borrow_mut();
                    typed
                        .get_mut(&parent)
                        .expect("host component parent exists")
                        .runtime
                        .refresh_host_boundary(&refresh.start, &refresh.props)?;
                    continue;
                }
                let children = self
                    .typed
                    .borrow()
                    .iter()
                    .filter_map(|(id, instance)| {
                        (instance.parent_id.as_deref() == Some(parent.as_str())
                            && instance.component_call == Some(refresh.call)
                            && instance
                                .component_start
                                .as_ref()
                                .is_some_and(|start| start.is_same_node(Some(&refresh.start))))
                        .then(|| id.clone())
                    })
                    .collect::<Vec<_>>();
                for child in children {
                    let mut typed = self.typed.borrow_mut();
                    let instance = typed.get_mut(&child).expect("component instance exists");
                    let values = component_runtime_props(&instance.runtime.app, &refresh.props)?;
                    let changed = (0..instance.runtime.app.parameters.len())
                        .filter(|prop| {
                            instance.runtime.app.runtime_props.get(*prop) != values.get(*prop)
                        })
                        .collect::<Vec<_>>();
                    instance.runtime.app.runtime_props = values;
                    if !changed.is_empty() {
                        let mut metrics = UpdateMetrics::default();
                        for prop in changed {
                            instance.runtime.refresh_prop(prop, &mut metrics)?;
                        }
                    }
                }
            }
        }
    }

    fn dispose_orphan_components(&self) {
        loop {
            let stale = self
                .typed
                .borrow()
                .iter()
                .filter_map(|(id, instance)| {
                    instance.parent_id.as_ref().and_then(|_| {
                        instance
                            .runtime
                            .nodes
                            .get(&instance.runtime.app.root_node)
                            .filter(|node| node.parent_node().is_none())
                            .map(|_| id.clone())
                    })
                })
                .collect::<Vec<_>>();
            if stale.is_empty() {
                return;
            }
            let mut typed = self.typed.borrow_mut();
            for id in stale {
                if let Some(mut instance) = typed.remove(&id) {
                    instance.runtime.invalidate_fetches();
                    instance.runtime.clear_listeners();
                    instance.runtime.dispose_host_components();
                }
            }
        }
    }

    pub fn mount_component_requests(&self) -> Result<(), JsValue> {
        loop {
            let requests = self
                .typed
                .borrow_mut()
                .iter_mut()
                .flat_map(|(id, instance)| {
                    instance
                        .runtime
                        .component_requests
                        .drain(..)
                        .map(|request| (id.clone(), request))
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            if requests.is_empty() {
                return Ok(());
            }
            for (parent_id, request) in requests {
                let id = format!("{parent_id}/component:{}:{}", request.call, request.key);
                self.ensure_typed_instance_absent(
                    &id,
                    &format!(
                        "component call {} (key {}, graph component {})",
                        request.call, request.key, request.component
                    ),
                )?;
                let definitions = self
                    .typed
                    .borrow()
                    .get(&parent_id)
                    .and_then(|instance| instance.runtime.component_definitions.clone())
                    .ok_or_else(|| JsValue::from_str("component graph definitions missing"))?;
                let mut app = definitions
                    .get(request.component)
                    .ok_or_else(|| JsValue::from_str("component target out of range"))?
                    .clone();
                let direct_regions = 1 + app
                    .nodes
                    .iter()
                    .filter(|node| matches!(node, TypedNode::Conditional { .. }))
                    .count();
                self.reserve_deferred_reconcile(app.nodes.len(), direct_regions, app.nodes.len())?;
                app.runtime_props = component_runtime_props(&app, &request.props)?;
                app.runtime_component_props = app
                    .parameters
                    .iter()
                    .map(|parameter| {
                        let name = app.strings.get(parameter.name).ok_or_else(|| {
                            JsValue::from_str("component parameter handle out of range")
                        })?;
                        Ok(request
                            .component_props
                            .get(name)
                            .and_then(|target| match target {
                                TypedComponentTarget::Native(component) => Some(*component),
                                TypedComponentTarget::Host(_) => None,
                            }))
                    })
                    .collect::<Result<Vec<_>, JsValue>>()?;
                app.runtime_host_component_props = app
                    .parameters
                    .iter()
                    .map(|parameter| {
                        let name = app.strings.get(parameter.name).ok_or_else(|| {
                            JsValue::from_str("component parameter handle out of range")
                        })?;
                        Ok(request
                            .component_props
                            .get(name)
                            .and_then(|target| match target {
                                TypedComponentTarget::Host(target) => Some(target.clone()),
                                TypedComponentTarget::Native(_) => None,
                            }))
                    })
                    .collect::<Result<Vec<_>, JsValue>>()?;
                let mut runtime = TypedRuntime::new_with_tag_policy(
                    app,
                    self.region_tracker.clone(),
                    self.reconcile_budget.clone(),
                    self.cookie_policy.clone(),
                    self.effective_tag_policy(),
                )?;
                runtime.set_component_definitions(definitions);
                runtime.path = request.path.clone();
                runtime.callbacks = runtime
                    .app
                    .parameters
                    .iter()
                    .map(|parameter| {
                        let name = runtime.app.strings.get(parameter.name).ok_or_else(|| {
                            JsValue::from_str("component parameter handle out of range")
                        })?;
                        Ok(request.callbacks.get(name).map(|callback| TypedCallback {
                            parent_id: parent_id.clone(),
                            action: callback.action,
                            row: callback.row.clone(),
                            arguments: vec![],
                        }))
                    })
                    .collect::<Result<Vec<_>, JsValue>>()?;
                runtime.set_host_inputs(self.typed_host_inputs.borrow().clone())?;
                runtime.graph_generation = self.next_typed_generation();
                if let Some(adoption) = request.adoption.as_ref() {
                    runtime.adopt(
                        adoption.root.clone(),
                        TypedAdoptionScope::Range {
                            start: request.start.clone(),
                            end: request.end.clone(),
                        },
                        &request.path,
                        &request.ssr_branches,
                        &request.ssr_loops,
                        &request.ssr_nested,
                    )?;
                } else {
                    runtime.mount_before(&request.end)?;
                }
                if let Some(row) = request.row_context.as_ref() {
                    if let Some(element) = runtime
                        .nodes
                        .get(&runtime.app.root_node)
                        .and_then(|node| node.dyn_ref::<Element>())
                    {
                        element.set_attribute("data-runtime-row-key", &row.row_key)?;
                    }
                }
                if !request.children.is_empty()
                    && !runtime.slot_requests.is_empty()
                    && request.adoption.is_none()
                {
                    let slot = runtime.slot_requests.pop().expect("slot exists");
                    if !runtime.slot_requests.is_empty() {
                        return Err(JsValue::from_str(
                            "component declares multiple children slots",
                        ));
                    }
                    let mut typed = self.typed.borrow_mut();
                    typed
                        .get_mut(&parent_id)
                        .ok_or_else(|| JsValue::from_str("component parent missing"))?
                        .runtime
                        .mount_slot_children(
                            &request.children,
                            &slot.start,
                            &slot.end,
                            request.row_context.as_ref(),
                        )?;
                }
                self.typed.borrow_mut().insert(
                    id.clone(),
                    TypedGraphInstance {
                        parent_id: Some(parent_id),
                        outlet_id: format!("component:{}", request.call),
                        graph_id: format!("component:{}", request.component),
                        route_id: None,
                        match_key: None,
                        route_state: None,
                        loader_runtime: None,
                        component_call: Some(request.call),
                        component_start: Some(request.start),
                        runtime,
                    },
                );
            }
        }
    }
}

impl RuntimeState {
    pub fn initialize_typed_input(
        &self,
        input_id: &str,
        rows: JsValue,
    ) -> Result<JsValue, JsValue> {
        if self.typed_components.borrow().is_some() {
            let rows: Vec<Value> = decode_bounded_json(
                &rows,
                plec_schema::limits::MAX_HOST_INPUT_JSON_BYTES,
                "input rows",
            )?;
            let ids = self
                .typed
                .borrow()
                .iter()
                .filter_map(|(id, instance)| {
                    instance
                        .runtime
                        .app
                        .inputs
                        .iter()
                        .any(|input| {
                            instance
                                .runtime
                                .app
                                .strings
                                .get(input.name)
                                .map(String::as_str)
                                == Some(input_id)
                        })
                        .then(|| id.clone())
                })
                .collect::<Vec<_>>();
            let mut metrics = UpdateMetrics::default();
            for id in ids {
                let mut typed = self.typed.borrow_mut();
                typed
                    .get_mut(&id)
                    .expect("live typed instance")
                    .runtime
                    .reconcile_input(input_id, rows.clone(), &mut metrics)?;
            }
            self.flush_component_work()?;
            self.install_typed_event_listeners()?;
            return serde_wasm_bindgen::to_value(&metrics).map_err(error);
        }
        let id = graph_instance_id(None, "main", None);
        self.initialize_typed_input_for(&id, input_id, rows)
    }
}

impl RuntimeState {
    pub fn initialize_typed_input_for(
        &self,
        id: &str,
        input_id: &str,
        rows: JsValue,
    ) -> Result<JsValue, JsValue> {
        let rows: Vec<Value> = decode_bounded_json(
            &rows,
            plec_schema::limits::MAX_HOST_INPUT_JSON_BYTES,
            "input rows",
        )?;
        let metrics = {
            let mut typed = self.typed.borrow_mut();
            let typed = typed
                .get_mut(id)
                .ok_or_else(|| JsValue::from_str("typed application missing"))?;
            let mut metrics = UpdateMetrics::default();
            typed
                .runtime
                .reconcile_input(input_id, rows, &mut metrics)?;
            metrics
        };
        self.flush_component_work()?;
        self.install_typed_event_listeners()?;
        serde_wasm_bindgen::to_value(&metrics).map_err(error)
    }
}

impl RuntimeState {
    pub fn apply_typed_delta(&self, delta: JsValue) -> Result<JsValue, JsValue> {
        let delta: Delta = decode_bounded_json(
            &delta,
            plec_schema::limits::MAX_HOST_INPUT_JSON_BYTES,
            "delta",
        )?;
        self.apply_typed_deltas(vec![delta])
    }

    /// Preserve batch order while avoiding one JS/WASM round trip per delta.
    pub fn apply_typed_deltas(&self, deltas: Vec<Delta>) -> Result<JsValue, JsValue> {
        let started = now();
        let mut metrics = UpdateMetrics::default();
        for delta in deltas {
            let ids = match (
                self.typed_components.borrow().is_some(),
                delta.instance_id(),
            ) {
                (true, Some(id)) => vec![id.to_owned()],
                (true, None) => self
                    .typed
                    .borrow()
                    .iter()
                    .filter_map(|(id, instance)| {
                        instance
                            .runtime
                            .app
                            .inputs
                            .iter()
                            .any(|input| {
                                instance
                                    .runtime
                                    .app
                                    .strings
                                    .get(input.name)
                                    .map(String::as_str)
                                    == Some(delta.input_id())
                            })
                            .then(|| id.clone())
                    })
                    .collect(),
                (false, _) => vec![graph_instance_id(None, "main", None)],
            };
            for id in ids {
                self.typed
                    .borrow_mut()
                    .get_mut(&id)
                    .ok_or_else(|| JsValue::from_str("typed application missing"))?
                    .runtime
                    .apply_delta(delta.clone(), &mut metrics)?;
            }
        }
        self.flush_component_work()?;
        self.install_typed_event_listeners()?;
        metrics.wasm_dom_us = (now() - started) * 1000.0;
        serde_wasm_bindgen::to_value(&metrics).map_err(error)
    }
}

impl TypedRuntime {
    pub fn set_route_error(&mut self, error: RuntimeValue) -> Result<(), JsValue> {
        // Error components may render a fixed fallback and not consume the
        // route error value. In that case there is no state slot to populate.
        if let Some(state) = self.app.route_error_state {
            self.states[state] = error;
        }
        Ok(())
    }

    pub fn set_host_inputs(
        &mut self,
        inputs: HashMap<String, RuntimeValue>,
    ) -> Result<(), JsValue> {
        self.host_inputs = inputs;
        self.app.host_inputs = self.host_inputs.clone();
        self.states = self
            .app
            .state_slots
            .iter()
            .map(|slot| {
                typed_eval(
                    &self.app,
                    self.cookie_policy.borrow().as_ref(),
                    slot.initial_expression,
                    &[],
                    None,
                    0,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(())
    }
    pub fn new(
        app: TypedApplication,
        cookie_policy: Rc<RefCell<Option<HashMap<String, CookiePolicy>>>>,
    ) -> Result<Self, JsValue> {
        let tracker = Rc::new(RegionTracker::new());
        Self::new_with_runtime_limits(app, tracker, Rc::new(RefCell::new(None)), cookie_policy)
    }

    pub fn new_with_runtime_limits(
        app: TypedApplication,
        region_tracker: Rc<RegionTracker>,
        reconcile_budget: Rc<RefCell<Option<ReconcileBudget>>>,
        cookie_policy: Rc<RefCell<Option<HashMap<String, CookiePolicy>>>>,
    ) -> Result<Self, JsValue> {
        Self::new_with_tag_policy(
            app,
            region_tracker,
            reconcile_budget,
            cookie_policy,
            plec_ir::sink::TagPolicy::default(),
        )
    }

    /// Like [`TypedRuntime::new_with_runtime_limits`], but validates and
    /// instantiates elements under an explicit trusted tag policy. The
    /// policy can only add configured custom elements to the standard
    /// allowlists; forbidden tags stay rejected at every layer.
    pub fn new_with_tag_policy(
        app: TypedApplication,
        region_tracker: Rc<RegionTracker>,
        reconcile_budget: Rc<RefCell<Option<ReconcileBudget>>>,
        cookie_policy: Rc<RefCell<Option<HashMap<String, CookiePolicy>>>>,
        tag_policy: plec_ir::sink::TagPolicy,
    ) -> Result<Self, JsValue> {
        app.validate_with_policy(&tag_policy)?;
        let mut app = app;
        let mut states = Vec::new();
        for slot in &app.state_slots {
            states.push(typed_eval(
                &app,
                cookie_policy.borrow().as_ref(),
                slot.initial_expression,
                &[],
                None,
                0,
            )?);
        }
        app.ref_values = app
            .ref_slots
            .iter()
            .map(|slot| {
                typed_eval(
                    &app,
                    cookie_policy.borrow().as_ref(),
                    slot.initial_expression,
                    &[],
                    None,
                    0,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let host_ref_nodes = vec![None; app.host_refs.len()];
        let focus_refs = vec![None; app.ref_slots.len()];
        let reaction_cleanups = vec![None; app.reactions.len()];
        Ok(Self {
            app,
            path: "root".into(),
            root: None,
            region_slot: RegionSlot::acquire(region_tracker.clone())?,
            region_tracker,
            reconcile_budget,
            nodes: HashMap::new(),
            states,
            host_ref_nodes,
            focus_refs,
            pending_reactions: Vec::new(),
            reaction_drain_depth: 0,
            mount_depth: 0,
            mount_stack_base: 0,
            adopt_depth: 0,
            adopt_stack_base: 0,
            reaction_cleanups,
            loops: HashMap::new(),
            conditionals: HashMap::new(),
            collections: HashMap::new(),
            listeners: Vec::new(),
            global_listeners: Vec::new(),
            listener_requests: Vec::new(),
            component_requests: Vec::new(),
            slot_requests: Vec::new(),
            component_refreshes: Vec::new(),
            callback_requests: Vec::new(),
            callbacks: Vec::new(),
            next_generation: 1,
            graph_generation: 1,
            host_inputs: HashMap::new(),
            host_refs: HashMap::new(),
            host_instances: Vec::new(),
            component_definitions: None,
            pending_cookies: Vec::new(),
            next_cookie_id: 0,
            next_component_instance: 0,
            cookie_policy,
            tag_policy,
            #[cfg(feature = "fetch")]
            pending_fetches: Vec::new(),
            #[cfg(feature = "fetch")]
            next_fetch_id: 0,
            #[cfg(feature = "fetch")]
            abort_controllers: HashMap::new(),
            ssr_imported: false,
            ssr_nested: HashMap::new(),
        })
    }
}

fn host_registry() -> Result<JsValue, JsValue> {
    let registry = js_sys::Reflect::get(
        &js_sys::global(),
        &JsValue::from_str("__plec_host_components"),
    )?;
    if registry.is_undefined() || registry.is_null() {
        return Err(JsValue::from_str(
            "host component registry is not installed",
        ));
    }
    Ok(registry)
}

fn host_call(method: &str, args: &[JsValue]) -> Result<JsValue, JsValue> {
    let registry = host_registry()?;
    let function = js_sys::Reflect::get(&registry, &JsValue::from_str(method))?
        .dyn_into::<js_sys::Function>()
        .map_err(|_| JsValue::from_str("host component registry method is not callable"))?;
    let arguments = js_sys::Array::new();
    for argument in args {
        arguments.push(argument);
    }
    function.apply(&registry, &arguments).map_err(|error| error)
}

fn evaluate_host_props(
    app: &TypedApplication,
    cookie_policy: Option<&plec_dom::cookie::CookiePolicyMap>,
    props: &[TypedComponentProp],
    states: &[RuntimeValue],
    row: Option<&HashMap<String, RuntimeValue>>,
    row_index: usize,
) -> Result<JsValue, JsValue> {
    let mut values = HashMap::new();
    for prop in props {
        let TypedComponentProp::Value { name, expression } = prop else {
            continue;
        };
        let name = app
            .strings
            .get(*name)
            .ok_or_else(|| JsValue::from_str("host component prop name out of range"))?;
        let value = typed_eval(app, cookie_policy, *expression, states, row, row_index)?;
        if name == "__plec_props" {
            if let RuntimeValue::Record(record) = value {
                values = record;
            } else {
                return Err(JsValue::from_str("host component props must be a record"));
            }
        } else {
            values.insert(name.clone(), value);
        }
    }
    host_value_to_js(&RuntimeValue::Record(values))
}

/// Serializes a runtime value crossing into a host provider as a JSON-plain
/// JS value. The default `serde_wasm_bindgen` serializer emits Rust maps as
/// ES6 `Map` instances, and providers read props with object spread /
/// `Object.entries`, so a `Map` props bag would silently drop every prop
/// (className, href, aria-*, ...) at the provider boundary. Nested records
/// inside props must stay plain objects for the same reason.
fn host_value_to_js(value: &RuntimeValue) -> Result<JsValue, JsValue> {
    use serde::Serialize as _;
    value
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(error)
}

fn host_identity(node: &TypedNode) -> Result<(String, String), JsValue> {
    match node {
        TypedNode::HostComponent {
            provider,
            component,
            ..
        } => Ok((provider.clone(), component.clone())),
        _ => Err(JsValue::from_str("host component node expected")),
    }
}

fn validate_host_boundary(
    boundary: &Element,
    provider: &str,
    component: &str,
    marker: &str,
) -> Result<(), JsValue> {
    if boundary.tag_name().to_ascii_lowercase() != "span"
        || boundary.get_attribute("data-plec-host").as_deref()
            != Some(&format!("{provider}:{component}"))
    {
        return Err(JsValue::from_str(&format!("mismatch:ssr-host:{marker}")));
    }
    Ok(())
}

impl TypedRuntime {
    fn refresh_host_boundary(
        &mut self,
        boundary: &Node,
        props: &HashMap<String, RuntimeValue>,
    ) -> Result<(), JsValue> {
        let index = self
            .host_instances
            .iter()
            .position(|instance| instance.boundary.is_same_node(Some(boundary)))
            .ok_or_else(|| JsValue::from_str("host component instance missing"))?;
        let values = props
            .get("__plec_props")
            .cloned()
            .unwrap_or_else(|| RuntimeValue::Record(props.clone()));
        let props = host_value_to_js(&values)?;
        self.update_host(&self.host_instances[index].boundary.clone(), props)
    }

    fn mount_host(
        &mut self,
        boundary: Element,
        provider: &str,
        component: &str,
        props: JsValue,
        call: Option<usize>,
    ) -> Result<(), JsValue> {
        let handle = host_call(
            "mount",
            &[
                JsValue::from_str(provider),
                JsValue::from_str(component),
                boundary.clone().into(),
                props,
            ],
        )?;
        self.host_instances.push(HostInstance {
            boundary,
            handle,
            call,
            provider: provider.into(),
            component: component.into(),
        });
        Ok(())
    }

    fn update_host(&mut self, boundary: &Element, props: JsValue) -> Result<(), JsValue> {
        let instance = self
            .host_instances
            .iter()
            .find(|instance| instance.boundary.is_same_node(Some(boundary)))
            .ok_or_else(|| JsValue::from_str("host component instance missing"))?;
        host_call("update", &[instance.handle.clone(), props]).map(|_| ())
    }

    fn dispose_host_boundary(&mut self, node: &Node) -> Result<(), JsValue> {
        if let Some(index) = self
            .host_instances
            .iter()
            .position(|instance| instance.boundary.is_same_node(node.dyn_ref()))
        {
            let instance = self.host_instances.swap_remove(index);
            host_call("dispose", &[instance.handle])?;
        }
        Ok(())
    }

    pub fn dispose_host_components(&mut self) {
        for instance in self.host_instances.drain(..) {
            let _ = host_call("dispose", &[instance.handle]);
        }
    }
}

#[cfg(not(feature = "fetch"))]
impl TypedRuntime {
    pub fn take_pending_fetches(&mut self) -> Vec<()> {
        Vec::new()
    }
}

impl TypedRuntime {
    fn queue_component_refreshes(
        &mut self,
        nodes: Vec<(usize, Node)>,
        row: Option<&HashMap<String, RuntimeValue>>,
        row_index: usize,
    ) -> Result<(), JsValue> {
        for (call, start) in nodes {
            let Some(props) = self
                .app
                .nodes
                .get(call)
                .cloned()
                .and_then(|node| match node {
                    TypedNode::Component { props, .. }
                    | TypedNode::DynamicComponent { props, .. }
                    | TypedNode::HostComponent { props, .. } => Some(props),
                    _ => None,
                })
            else {
                continue;
            };
            let props = props
                .into_iter()
                .filter_map(|prop| match prop {
                    TypedComponentProp::Value { name, expression } => Some((name, expression)),
                    TypedComponentProp::Callable { .. } | TypedComponentProp::Component { .. } => {
                        None
                    }
                })
                .map(|(name, expression)| {
                    let name = self
                        .app
                        .strings
                        .get(name)
                        .ok_or_else(|| JsValue::from_str("component prop name out of range"))?
                        .clone();
                    let value = typed_eval(
                        &self.app,
                        self.cookie_policy.borrow().as_ref(),
                        expression,
                        &self.states,
                        row,
                        row_index,
                    )?;
                    Ok((name, value))
                })
                .collect::<Result<HashMap<_, _>, JsValue>>()?;
            let component_props = self
                .app
                .nodes
                .get(call)
                .and_then(|node| match node {
                    TypedNode::Component { props, .. }
                    | TypedNode::DynamicComponent { props, .. } => Some(props),
                    _ => None,
                })
                .into_iter()
                .flatten()
                .filter_map(|prop| match prop {
                    TypedComponentProp::Component {
                        name,
                        component,
                        host,
                        ..
                    } => Some((
                        self.app.strings.get(*name)?.clone(),
                        host.clone()
                            .map(TypedComponentTarget::Host)
                            .unwrap_or(TypedComponentTarget::Native(*component)),
                    )),
                    _ => None,
                })
                .collect();
            self.component_refreshes.push(TypedComponentRefresh {
                call,
                start,
                props,
                component_props,
            });
        }
        Ok(())
    }

    pub fn queue_static_component_refreshes(&mut self) -> Result<(), JsValue> {
        self.queue_component_refreshes(
            self.nodes
                .iter()
                .map(|(call, node)| (*call, node.clone()))
                .collect(),
            None,
            0,
        )
    }

    fn queue_row_component_refreshes(
        &mut self,
        nodes: HashMap<usize, Node>,
        values: &HashMap<String, RuntimeValue>,
        row_index: usize,
    ) -> Result<(), JsValue> {
        self.queue_component_refreshes(nodes.into_iter().collect(), Some(values), row_index)
    }

    pub fn mount(&mut self, root: Element) -> Result<MountMetrics, JsValue> {
        self.invalidate_fetches();
        self.clear_listeners();
        self.dispose_host_components();
        root.set_inner_html("");
        self.nodes.clear();
        self.loops.clear();
        self.conditionals.clear();
        self.listener_requests.clear();
        // A failed earlier mount can unwind mid-chain; the fresh mount must
        // start with clean recursion accounting, not stale depth/watermark.
        self.mount_depth = 0;
        self.mount_stack_base = 0;
        let doc = document()?;
        let root_node: Node = root.clone().into();
        self.instantiate_node(
            &doc,
            self.app.root_node,
            Some(&root_node),
            None,
            0,
            None,
            &mut HashMap::new(),
            &mut HashMap::new(),
        )?;
        self.root = Some(root);
        self.apply_static_bindings()?;
        self.queue_static_listeners();
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

    /// Rebuild the runtime-side ownership of SSR DOM without changing that
    /// DOM.  The server emits paths from the same graph-instance/component
    /// traversal used here, so this is deliberately a strict contract: an
    /// absent or stale marker is an adoption failure, never an excuse to
    /// silently attach a listener to a guessed node.
    ///
    /// Every claim resolves through one `TypedAdoptionIndex` built for the
    /// adoption scope; no claim re-scans the DOM.
    pub fn adopt(
        &mut self,
        root: Element,
        scope: TypedAdoptionScope,
        path: &str,
        branches: &HashMap<usize, plec_ir::SsrSelectedBranch>,
        loops: &HashMap<usize, Vec<String>>,
        nested: &SsrNestedRecords,
    ) -> Result<MountMetrics, JsValue> {
        self.invalidate_fetches();
        self.clear_listeners();
        self.dispose_host_components();
        self.nodes.clear();
        self.loops.clear();
        self.conditionals.clear();
        self.listener_requests.clear();
        self.component_requests.clear();
        self.slot_requests.clear();
        // Nested component records below this runtime's path: adopted
        // children extract their own entry (and their subtree's) at queue
        // time, so arbitrary nesting depth consumes recorded state instead
        // of inferring it from DOM shape.
        self.ssr_nested = nested.clone();

        let markers = TypedAdoptionIndex::build(scope)?;
        // Handles below an unselected conditional branch have no
        // server-rendered DOM: the walk must not try to claim them.
        let mut skipped: HashSet<usize> = HashSet::new();
        // A row template is stored in the same flat node table as its loop
        // anchor and may precede that anchor. Precompute all template handles
        // so the outer graph walk never claims one with an unscoped path.
        for loop_def in &self.app.loops {
            self.collect_branch_handles(loop_def.row_template, &mut skipped);
        }
        for (index, node) in self.app.nodes.clone().into_iter().enumerate() {
            if skipped.contains(&index) {
                continue;
            }
            match node {
                TypedNode::Element { tag, host_ref, .. } => {
                    let marker = format!("{path}/node:{index}");
                    let element = markers.element(&marker)?;
                    let expected_tag = self
                        .app
                        .strings
                        .get(tag)
                        .ok_or_else(|| JsValue::from_str("adoption tag handle out of range"))?;
                    if element.tag_name().to_ascii_lowercase() != expected_tag.to_ascii_lowercase()
                    {
                        return Err(JsValue::from_str(&format!("mismatch:ssr-tag:{marker}")));
                    }
                    let dom_node: Node = element.into();
                    if let Some(reference) = host_ref {
                        let slot = self.host_ref_nodes.get_mut(reference).ok_or_else(|| {
                            JsValue::from_str("adoption host ref handle out of range")
                        })?;
                        *slot = Some(dom_node.clone());
                    }
                    self.nodes.insert(index, dom_node);
                }
                TypedNode::HostComponent { props, .. } => {
                    let marker = format!("{path}/node:{index}");
                    let boundary = markers.element(&marker)?;
                    let (provider, component) = host_identity(&self.app.nodes[index])?;
                    validate_host_boundary(&boundary, &provider, &component, &marker)?;
                    let dom_node: Node = boundary.clone().into();
                    let values = evaluate_host_props(
                        &self.app,
                        self.cookie_policy.borrow().as_ref(),
                        &props,
                        &self.states,
                        None,
                        0,
                    )?;
                    self.mount_host(boundary, &provider, &component, values, Some(index))?;
                    self.nodes.insert(index, dom_node);
                }
                TypedNode::Text { .. } => {
                    let marker = format!("plec:text:{path}:{index}");
                    let marker_node = markers.get(&marker).ok_or_else(|| {
                        JsValue::from_str(&format!("missing:ssr-text:{path}:{index}"))
                    })?;
                    let text = match marker_node.next_sibling() {
                        // The value itself. Injected bare whitespace merges
                        // into this node at parse time; the snapshot recompute
                        // rewrites the merged data with the true value.
                        Some(sibling) if sibling.node_type() == Node::TEXT_NODE => sibling,
                        // An empty server value serializes as the empty-comment
                        // sentinel instead of a text node (and legacy documents
                        // may have another plec boundary marker there): the
                        // marker still proves the exact position, so synthesize
                        // the text node the binding will own.
                        Some(sibling) if sibling.node_type() == Node::COMMENT_NODE => {
                            let parent = marker_node.parent_node().ok_or_else(|| {
                                JsValue::from_str(&format!("detached:ssr-text:{path}:{index}"))
                            })?;
                            let text = document()?.create_text_node("");
                            parent.insert_before(&text, Some(&sibling))?;
                            text.into()
                        }
                        // Anything else sits between the marker and the text
                        // it anchors: the adjacency contract is broken, so the
                        // claim must fail closed instead of anchoring the
                        // binding at a guessed position.
                        Some(_) => {
                            return Err(JsValue::from_str(&format!(
                                "adjacency:ssr-text:{path}:{index}"
                            )));
                        }
                        None => {
                            let parent = marker_node.parent_node().ok_or_else(|| {
                                JsValue::from_str(&format!("detached:ssr-text:{path}:{index}"))
                            })?;
                            let text = document()?.create_text_node("");
                            parent.append_child(&text)?;
                            text.into()
                        }
                    };
                    self.nodes.insert(index, text);
                }
                TypedNode::Component {
                    component,
                    props,
                    children,
                    ..
                } => {
                    self.queue_adopted_component(
                        index, component, props, children, &root, &markers, path, None, 0, None,
                    )?;
                }
                TypedNode::Slot { .. } => {
                    let start = markers
                        .get(&format!("plec:slot:{path}:{index}"))
                        .cloned()
                        .ok_or_else(|| {
                            JsValue::from_str(&format!("missing:ssr-slot:{path}:{index}"))
                        })?;
                    let end = markers
                        .get(&format!("plec:slot-end:{path}:{index}"))
                        .cloned()
                        .ok_or_else(|| {
                            JsValue::from_str(&format!("missing:ssr-slot-end:{path}:{index}"))
                        })?;
                    self.nodes.insert(index, start.clone());
                    self.slot_requests.push(TypedSlotRequest { start, end });
                }
                TypedNode::Loop { r#loop, .. } => {
                    let keys = loops.get(&index).ok_or_else(|| {
                        JsValue::from_str(&format!("missing:ssr-loop:{path}:{index}"))
                    })?;
                    self.adopt_loop_rows(index, r#loop, path, keys, &root, &markers, &mut skipped)?;
                }
                TypedNode::Conditional { .. } => {
                    self.adopt_conditional_region(index, path, branches, &markers, &mut skipped)?;
                }
                TypedNode::DynamicComponent {
                    prop,
                    props,
                    children,
                    ..
                } => {
                    if let Some(Some(target)) =
                        self.app.runtime_host_component_props.get(prop).cloned()
                    {
                        let marker = format!("{path}/node:{index}");
                        let boundary = markers.element(&marker)?;
                        validate_host_boundary(
                            &boundary,
                            &target.provider,
                            &target.component,
                            &marker,
                        )?;
                        let values = evaluate_host_props(
                            &self.app,
                            self.cookie_policy.borrow().as_ref(),
                            &props,
                            &self.states,
                            None,
                            0,
                        )?;
                        self.mount_host(
                            boundary.clone(),
                            &target.provider,
                            &target.component,
                            values,
                            Some(index),
                        )?;
                        self.nodes.insert(index, boundary.into());
                        continue;
                    }
                    let component = self
                        .app
                        .runtime_component_props
                        .get(prop)
                        .and_then(|value| *value)
                        .ok_or_else(|| JsValue::from_str("missing:ssr-dynamic-component"))?;
                    self.queue_adopted_component(
                        index, component, props, children, &root, &markers, path, None, 0, None,
                    )?;
                }
            }
        }
        self.root = Some(root);
        // Adopted regions own the branch DOM the walk just claimed. Branch
        // node handles resolve their markers like any other node, so the
        // region's node map is collected after the walk completes, then
        // listeners attach under the conditional owner exactly as a fresh
        // reconcile would.
        let adopted_regions = self
            .conditionals
            .iter()
            .filter(|(_, region)| region.nodes.is_empty())
            .map(|(conditional, region)| (*conditional, region.selected, region.generation))
            .collect::<Vec<_>>();
        for (conditional, selected, generation) in adopted_regions {
            let mut nodes = HashMap::new();
            if let Some(handle) = selected {
                self.collect_branch_nodes(handle, &mut nodes);
            }
            if let Some(region) = self.conditionals.get_mut(&conditional) {
                region.nodes = nodes;
            }
            let owner = TypedListenerOwner::Conditional {
                conditional,
                generation,
                row: None,
            };
            let claimed = self
                .conditionals
                .get(&conditional)
                .map(|region| {
                    region
                        .nodes
                        .iter()
                        .map(|(target, node)| (*target, node.clone()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            for (target, node) in claimed {
                self.queue_listener(target, node, owner.clone());
            }
        }
        self.apply_static_bindings()?;
        self.queue_static_listeners();
        Ok(MountMetrics {
            bindings: self.app.bindings.len() as u32,
            row_count: self
                .loops
                .values()
                .map(|rows| rows.order.len() as u32)
                .sum(),
            ..Default::default()
        })
    }

    fn ssr_row_path(path: &str, loop_index: usize, key: &str) -> String {
        format!(
            "{path}/loop:{loop_index}/key:{}",
            key.replace('%', "%25").replace('/', "%2F")
        )
    }

    fn adopt_loop_rows(
        &mut self,
        loop_node: usize,
        loop_index: usize,
        path: &str,
        expected: &[String],
        root: &Element,
        markers: &TypedAdoptionIndex,
        skipped: &mut HashSet<usize>,
    ) -> Result<usize, JsValue> {
        let loop_def = self
            .app
            .loops
            .get(loop_index)
            .ok_or_else(|| JsValue::from_str("loop handle out of range"))?
            .clone();
        let values = typed_eval(
            &self.app,
            self.cookie_policy.borrow().as_ref(),
            loop_def.source_expression,
            &self.states,
            None,
            0,
        )?;
        let rows = values
            .array()
            .ok_or_else(|| JsValue::from_str("LOOP_SOURCE_NOT_ARRAY"))?;
        if rows.len() > plec_ir::limits::MAX_LOOP_ROWS {
            return Err(JsValue::from_str("LOOP_ROW_LIMIT_EXCEEDED"));
        }
        let mut projection = Vec::new();
        let mut computed = Vec::new();
        for (index, value) in rows.iter().cloned().enumerate() {
            let row = value
                .record()
                .cloned()
                .ok_or_else(|| JsValue::from_str("LOOP_ROW_NOT_OBJECT"))?;
            let key = typed_value_string(&typed_eval(
                &self.app,
                self.cookie_policy.borrow().as_ref(),
                loop_def.key_expression,
                &self.states,
                Some(&row),
                index,
            )?);
            if computed.iter().any(|existing| existing == &key) {
                return Err(JsValue::from_str(&format!("duplicate:ssr-row-key:{key}")));
            }
            computed.push(key.clone());
            projection.push((key, row));
        }
        for key in expected {
            if !computed.iter().any(|candidate| candidate == key) {
                return Err(JsValue::from_str(&format!("missing:ssr-row:{key}")));
            }
        }
        for key in &computed {
            if !expected.iter().any(|candidate| candidate == key) {
                return Err(JsValue::from_str(&format!("extra:ssr-row:{key}")));
            }
        }
        if computed != expected {
            return Err(JsValue::from_str(&format!(
                "mismatch:ssr-row-order:{path}:{loop_node}"
            )));
        }
        // Adopted loops still grow on the client: record the canonical row
        // address prefix so delta inserts emit the server's row grammar.
        self.loops.entry(loop_index).or_default().path_base =
            Some(format!("{path}/loop:{loop_node}/key:"));

        // Template handles are not top-level mounted nodes. Keep the outer
        // adoption walk from trying to claim them with the unscoped path.
        self.collect_branch_handles(loop_def.row_template, skipped);
        let parent_handle = match self.app.nodes.get(loop_node) {
            Some(TypedNode::Loop { parent, .. }) => {
                parent.ok_or_else(|| JsValue::from_str("loop parent missing"))?
            }
            _ => return Err(JsValue::from_str("loop node expected")),
        };
        let parent = self
            .nodes
            .get(&parent_handle)
            .cloned()
            .ok_or_else(|| JsValue::from_str("loop parent not adopted"))?;
        let template = loop_def.row_template;
        let root_kind = self
            .app
            .nodes
            .get(template)
            .cloned()
            .ok_or_else(|| JsValue::from_str("row template handle out of range"))?;
        let actual_keys = self.row_root_keys(path, loop_node, template, &root_kind, markers);
        let mut seen_dom = HashSet::new();
        for encoded in actual_keys {
            let key = Self::decode_ssr_segment(&encoded);
            if !seen_dom.insert(key.clone()) {
                return Err(JsValue::from_str(&format!("duplicate:ssr-row-key:{key}")));
            }
            if !expected.iter().any(|candidate| candidate == &key) {
                return Err(JsValue::from_str(&format!("extra:ssr-row:{key}")));
            }
        }
        let mut claimed_roots = Vec::new();
        for (row_index, (key, values)) in projection.into_iter().enumerate() {
            let row_path = Self::ssr_row_path(path, loop_node, &key);
            let mut nodes = HashMap::new();
            let mut conditionals = HashMap::new();
            let (root, end) = self.adopt_row_node(
                template,
                &row_path,
                &values,
                row_index,
                &TypedRowContext {
                    loop_index,
                    row_key: key.clone(),
                },
                root,
                markers,
                &mut nodes,
                &mut conditionals,
                skipped,
            )?;
            if !root
                .parent_node()
                .is_some_and(|owner| owner.is_same_node(Some(&parent)))
            {
                return Err(JsValue::from_str(&format!("mismatch:ssr-row:{key}")));
            }
            self.apply_bindings_to_nodes(
                &nodes,
                Some(&values),
                row_index,
                &mut UpdateMetrics::default(),
            )?;
            let generation = self.next_generation;
            self.next_generation += 1;
            self.loops.entry(loop_index).or_default().rows.insert(
                key.clone(),
                TypedRow {
                    root: root.clone(),
                    end,
                    values,
                    nodes,
                    conditionals,
                    generation,
                    region_slot: RegionSlot::acquire(self.region_tracker.clone())?,
                },
            );
            self.queue_row_listeners(loop_index, &key);
            let row = self.loops.get(&loop_index).unwrap().rows.get(&key).unwrap();
            let row_owner = TypedListenerOwner::Row {
                loop_index,
                row_key: key.clone(),
                generation: row.generation,
            };
            let requests = row
                .conditionals
                .iter()
                .flat_map(|(conditional, region)| {
                    let owner = TypedListenerOwner::Conditional {
                        conditional: *conditional,
                        generation: region.generation,
                        row: Some(Box::new(row_owner.clone())),
                    };
                    region
                        .nodes
                        .iter()
                        .map(move |(target, node)| (*target, node.clone(), owner.clone()))
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            for (target, node, owner) in requests {
                self.queue_listener(target, node, owner);
            }
            claimed_roots.push(root);
        }
        let actual_order = claimed_roots
            .iter()
            .filter_map(|root| {
                root.parent_node().and_then(|owner| {
                    let children = owner.child_nodes();
                    (0..children.length()).find_map(|position| {
                        children
                            .item(position)
                            .filter(|candidate| candidate.is_same_node(Some(root)))
                            .map(|_| position as usize)
                    })
                })
            })
            .collect::<Vec<_>>();
        if actual_order.windows(2).any(|pair| pair[0] > pair[1]) {
            return Err(JsValue::from_str(&format!(
                "mismatch:ssr-row-order:{path}:{loop_node}"
            )));
        }
        self.loops.entry(loop_index).or_default().order = expected.to_vec();
        Ok(expected.len())
    }

    fn decode_ssr_segment(value: &str) -> String {
        value.replace("%2F", "/").replace("%25", "%")
    }

    fn row_root_keys(
        &self,
        path: &str,
        loop_index: usize,
        template: usize,
        node: &TypedNode,
        markers: &TypedAdoptionIndex,
    ) -> Vec<String> {
        let base = format!("{path}/loop:{loop_index}/key:");
        let (prefix, suffix) = match node {
            TypedNode::Element { .. } => (base, format!("/node:{template}")),
            TypedNode::Text { .. } => (format!("plec:text:{base}"), format!(":{template}")),
            TypedNode::Component { .. } => {
                (format!("plec:component:{base}"), format!(":{template}"))
            }
            TypedNode::DynamicComponent { .. } => {
                (format!("plec:component:{base}"), format!(":{template}"))
            }
            TypedNode::HostComponent { .. } => (base, format!("/node:{template}")),
            TypedNode::Conditional { .. } => {
                (format!("plec:conditional:{base}"), format!(":{template}"))
            }
            TypedNode::Slot { .. } => (format!("plec:slot:{base}"), format!(":{template}")),
            TypedNode::Loop { .. } => return vec![],
        };
        markers
            .markers_for_prefix(&prefix)
            .into_iter()
            .filter_map(|marker| {
                marker
                    .strip_prefix(&prefix)
                    .and_then(|value| value.strip_suffix(&suffix))
                    .map(str::to_owned)
            })
            .collect()
    }

    fn adopt_row_node(
        &mut self,
        index: usize,
        path: &str,
        row: &HashMap<String, RuntimeValue>,
        row_index: usize,
        row_context: &TypedRowContext,
        adoption_root: &Element,
        markers: &TypedAdoptionIndex,
        local: &mut HashMap<usize, Node>,
        regions: &mut HashMap<usize, TypedConditionalRegion>,
        skipped: &mut HashSet<usize>,
    ) -> Result<(Node, Option<Node>), JsValue> {
        // Every recursive adoption path (element children and conditional
        // branches) flows back through this wrapper, so adoption shares the
        // mount contract: a validated graph is capped at
        // `MAX_NODE_GRAPH_DEPTH`, and the wrapper fails closed on depth or
        // native stack budget (`MAX_MOUNT_STACK_BYTES`) instead of
        // overflowing the WASM stack.
        let stack_pointer = Self::mount_stack_pointer();
        if self.adopt_depth == 0 {
            self.adopt_stack_base = stack_pointer;
        }
        if self.adopt_depth >= plec_ir::limits::MAX_MOUNT_DEPTH {
            return Err(JsValue::from_str("adopt depth exceeds limit"));
        }
        if self.adopt_stack_base.saturating_sub(stack_pointer)
            > plec_ir::limits::MAX_MOUNT_STACK_BYTES
        {
            return Err(JsValue::from_str("adopt stack budget exceeded"));
        }
        self.adopt_depth += 1;
        let result = self.adopt_row_node_bounded(
            index,
            path,
            row,
            row_index,
            row_context,
            adoption_root,
            markers,
            local,
            regions,
            skipped,
        );
        self.adopt_depth -= 1;
        if self.adopt_depth == 0 {
            self.adopt_stack_base = 0;
        }
        result
    }

    fn adopt_row_node_bounded(
        &mut self,
        index: usize,
        path: &str,
        row: &HashMap<String, RuntimeValue>,
        row_index: usize,
        row_context: &TypedRowContext,
        adoption_root: &Element,
        markers: &TypedAdoptionIndex,
        local: &mut HashMap<usize, Node>,
        regions: &mut HashMap<usize, TypedConditionalRegion>,
        skipped: &mut HashSet<usize>,
    ) -> Result<(Node, Option<Node>), JsValue> {
        let node = self
            .app
            .nodes
            .get(index)
            .cloned()
            .ok_or_else(|| JsValue::from_str("row node handle out of range"))?;
        match node {
            TypedNode::Element {
                tag,
                children,
                host_ref,
                ..
            } => {
                let marker = format!("{path}/node:{index}");
                let element = markers.element(&marker)?;
                let expected_tag = self
                    .app
                    .strings
                    .get(tag)
                    .ok_or_else(|| JsValue::from_str("adoption tag handle out of range"))?;
                if element.tag_name().to_ascii_lowercase() != expected_tag.to_ascii_lowercase() {
                    return Err(JsValue::from_str(&format!("mismatch:ssr-tag:{marker}")));
                }
                let dom_node: Node = element.into();
                if let Some(reference) = host_ref {
                    *self.host_ref_nodes.get_mut(reference).ok_or_else(|| {
                        JsValue::from_str("adoption host ref handle out of range")
                    })? = Some(dom_node.clone());
                }
                local.insert(index, dom_node.clone());
                for child in children {
                    self.adopt_row_node(
                        child,
                        path,
                        row,
                        row_index,
                        row_context,
                        adoption_root,
                        markers,
                        local,
                        regions,
                        skipped,
                    )?;
                }
                Ok((dom_node, None))
            }
            TypedNode::HostComponent { props, .. } => {
                let marker = format!("{path}/node:{index}");
                let boundary = markers.element(&marker)?;
                let (provider, component) = host_identity(&self.app.nodes[index])?;
                validate_host_boundary(&boundary, &provider, &component, &marker)?;
                let dom_node: Node = boundary.clone().into();
                let values = evaluate_host_props(
                    &self.app,
                    self.cookie_policy.borrow().as_ref(),
                    &props,
                    &self.states,
                    Some(row),
                    row_index,
                )?;
                self.mount_host(boundary, &provider, &component, values, Some(index))?;
                local.insert(index, dom_node.clone());
                Ok((dom_node, None))
            }
            TypedNode::Text { .. } => {
                let marker = format!("plec:text:{path}:{index}");
                let marker_node = markers.get(&marker).ok_or_else(|| {
                    JsValue::from_str(&format!("missing:ssr-row-text:{path}:{index}"))
                })?;
                // Same adjacency contract as the graph-level text claim: a
                // text node is the value (merged injected whitespace heals
                // through the recompute), a comment is the empty-value
                // sentinel, anything else is broken adjacency and fails
                // closed.
                let text = match marker_node.next_sibling() {
                    Some(sibling) if sibling.node_type() == Node::TEXT_NODE => sibling,
                    Some(sibling) if sibling.node_type() == Node::COMMENT_NODE => {
                        let parent = marker_node.parent_node().ok_or_else(|| {
                            JsValue::from_str(&format!("detached:ssr-row-text:{path}:{index}"))
                        })?;
                        let text = document()?.create_text_node("");
                        parent.insert_before(&text, Some(&sibling))?;
                        text.into()
                    }
                    Some(_) => {
                        return Err(JsValue::from_str(&format!(
                            "adjacency:ssr-row-text:{path}:{index}"
                        )));
                    }
                    None => {
                        let parent = marker_node.parent_node().ok_or_else(|| {
                            JsValue::from_str(&format!("detached:ssr-row-text:{path}:{index}"))
                        })?;
                        let text = document()?.create_text_node("");
                        parent.append_child(&text)?;
                        text.into()
                    }
                };
                local.insert(index, text.clone());
                Ok((text, None))
            }
            TypedNode::Conditional {
                test,
                consequent,
                alternate,
                ..
            } => {
                let start = markers
                    .get(&format!("plec:conditional:{path}:{index}"))
                    .cloned()
                    .ok_or_else(|| {
                        JsValue::from_str(&format!("missing:ssr-row-branch:{path}:{index}"))
                    })?;
                let end = markers
                    .get(&format!("plec:conditional-end:{path}:{index}"))
                    .cloned()
                    .ok_or_else(|| {
                        JsValue::from_str(&format!("missing:ssr-row-branch-end:{path}:{index}"))
                    })?;
                let selected = if typed_truthy(&typed_eval(
                    &self.app,
                    self.cookie_policy.borrow().as_ref(),
                    test,
                    &self.states,
                    Some(row),
                    row_index,
                )?) {
                    Some(consequent)
                } else {
                    alternate
                };
                if let Some(branch) = selected {
                    let expected = self.branch_ownership_marker(path, branch)?;
                    if !start.next_sibling().is_some_and(|node| {
                        ownership_marker(&node).is_some_and(|marker| marker == expected)
                    }) {
                        return Err(JsValue::from_str(&format!(
                            "mismatch:ssr-row-branch:{path}:{index}"
                        )));
                    }
                } else if !start
                    .next_sibling()
                    .is_some_and(|node| node.is_same_node(Some(&end)))
                {
                    return Err(JsValue::from_str(&format!(
                        "mismatch:ssr-row-branch:{path}:{index}"
                    )));
                }
                let generation = self.next_generation;
                self.next_generation += 1;
                local.insert(index, start.clone());
                if let Some(branch) = selected {
                    self.adopt_row_node(
                        branch,
                        path,
                        row,
                        row_index,
                        row_context,
                        adoption_root,
                        markers,
                        local,
                        regions,
                        skipped,
                    )?;
                } else {
                    self.collect_branch_handles(consequent, skipped);
                    if let Some(alternate) = alternate {
                        self.collect_branch_handles(alternate, skipped);
                    }
                }
                let mut nodes = HashMap::new();
                if let Some(branch) = selected {
                    self.collect_instantiated_branch_nodes(branch, local, &mut nodes);
                }
                regions.insert(
                    index,
                    TypedConditionalRegion {
                        start: start.clone(),
                        end: end.clone(),
                        selected,
                        nodes,
                        generation,
                        region_slot: RegionSlot::acquire(self.region_tracker.clone())?,
                    },
                );
                Ok((start, Some(end)))
            }
            TypedNode::Component {
                component,
                props,
                children,
                ..
            } => {
                let start = markers
                    .get(&format!("plec:component:{path}:{index}"))
                    .cloned()
                    .ok_or_else(|| {
                        JsValue::from_str(&format!("missing:ssr-row-component:{path}:{index}"))
                    })?;
                let end = markers
                    .get(&format!("plec:component-end:{path}:{index}"))
                    .cloned()
                    .ok_or_else(|| {
                        JsValue::from_str(&format!("missing:ssr-row-component-end:{path}:{index}"))
                    })?;
                self.queue_adopted_component(
                    index,
                    component,
                    props,
                    children,
                    adoption_root,
                    markers,
                    path,
                    Some(row),
                    row_index,
                    Some(row_context.clone()),
                )?;
                local.insert(index, start.clone());
                Ok((start, Some(end)))
            }
            TypedNode::DynamicComponent {
                prop,
                props,
                children,
                ..
            } => {
                if let Some(target) = self
                    .app
                    .runtime_host_component_props
                    .get(prop)
                    .cloned()
                    .flatten()
                {
                    let marker = format!("{path}/node:{index}");
                    let boundary = markers.element(&marker)?;
                    validate_host_boundary(
                        &boundary,
                        &target.provider,
                        &target.component,
                        &marker,
                    )?;
                    let dom_node: Node = boundary.clone().into();
                    let values = evaluate_host_props(
                        &self.app,
                        self.cookie_policy.borrow().as_ref(),
                        &props,
                        &self.states,
                        Some(row),
                        row_index,
                    )?;
                    self.mount_host(
                        boundary,
                        &target.provider,
                        &target.component,
                        values,
                        Some(index),
                    )?;
                    local.insert(index, dom_node.clone());
                    return Ok((dom_node, None));
                }
                let component = self
                    .app
                    .runtime_component_props
                    .get(prop)
                    .and_then(|value| *value)
                    .ok_or_else(|| JsValue::from_str("missing:ssr-dynamic-component"))?;
                let start = markers
                    .get(&format!("plec:component:{path}:{index}"))
                    .cloned()
                    .ok_or_else(|| {
                        JsValue::from_str(&format!("missing:ssr-row-component:{path}:{index}"))
                    })?;
                let end = markers
                    .get(&format!("plec:component-end:{path}:{index}"))
                    .cloned()
                    .ok_or_else(|| {
                        JsValue::from_str(&format!("missing:ssr-row-component-end:{path}:{index}"))
                    })?;
                self.queue_adopted_component(
                    index,
                    component,
                    props,
                    children,
                    adoption_root,
                    markers,
                    path,
                    Some(row),
                    row_index,
                    Some(row_context.clone()),
                )?;
                local.insert(index, start.clone());
                Ok((start, Some(end)))
            }
            TypedNode::Slot { .. } => {
                let start = markers
                    .get(&format!("plec:slot:{path}:{index}"))
                    .cloned()
                    .ok_or_else(|| {
                        JsValue::from_str(&format!("missing:ssr-row-slot:{path}:{index}"))
                    })?;
                let end = markers
                    .get(&format!("plec:slot-end:{path}:{index}"))
                    .cloned()
                    .ok_or_else(|| {
                        JsValue::from_str(&format!("missing:ssr-row-slot-end:{path}:{index}"))
                    })?;
                local.insert(index, start.clone());
                Ok((start, Some(end)))
            }
            TypedNode::Loop { .. } => Err(JsValue::from_str("unsupported:ssr-nested-loop")),
        }
    }

    /// Claims one server-rendered conditional region. The snapshot's branch
    /// record is the ownership cause; the markers prove where the region is.
    /// A conditional without a record fails closed — the record, not DOM
    /// shape, defines what the server instantiated.
    fn adopt_conditional_region(
        &mut self,
        index: usize,
        path: &str,
        branches: &HashMap<usize, plec_ir::SsrSelectedBranch>,
        markers: &TypedAdoptionIndex,
        skipped: &mut HashSet<usize>,
    ) -> Result<(), JsValue> {
        let selected = branches.get(&index).copied();
        let start = markers
            .get(&format!("plec:conditional:{path}:{index}"))
            .cloned()
            .ok_or_else(|| JsValue::from_str(&format!("missing:ssr-branch:{path}:{index}")))?;
        let end = markers
            .get(&format!("plec:conditional-end:{path}:{index}"))
            .cloned()
            .ok_or_else(|| JsValue::from_str(&format!("missing:ssr-branch-end:{path}:{index}")))?;
        let TypedNode::Conditional {
            consequent,
            alternate,
            ..
        } = self
            .app
            .nodes
            .get(index)
            .ok_or_else(|| JsValue::from_str("conditional handle out of range"))?
            .clone()
        else {
            return Err(JsValue::from_str("conditional node expected"));
        };
        let selected = match selected {
            Some(selected) => selected,
            None => {
                return Err(JsValue::from_str(&format!(
                    "missing:ssr-branch:{path}:{index}"
                )));
            }
        };
        let selected_node = match selected {
            plec_ir::SsrSelectedBranch::Consequent => Some(consequent),
            // Snapshot validation gates an alternate selection on the node
            // declaring one, so the mismatch arm is a defensive closed door.
            plec_ir::SsrSelectedBranch::Alternate => match alternate {
                Some(handle) => Some(handle),
                None => {
                    return Err(JsValue::from_str(&format!(
                        "mismatch:ssr-branch:{path}:{index}"
                    )));
                }
            },
            plec_ir::SsrSelectedBranch::None => None,
        };
        let inner = start.next_sibling();
        match selected_node {
            // A `none` region owns no DOM: the markers must enclose exactly
            // nothing, otherwise the record contradicts the markup.
            None => {
                let empty = match inner {
                    Some(node) => node.is_same_node(Some(&end)),
                    None => true,
                };
                if !empty {
                    return Err(JsValue::from_str(&format!(
                        "mismatch:ssr-branch:{path}:{index}"
                    )));
                }
            }
            Some(handle) => {
                let expected = self.branch_ownership_marker(path, handle)?;
                let agrees = inner.is_some_and(|node| {
                    ownership_marker(&node).is_some_and(|marker| marker == expected)
                });
                if !agrees {
                    return Err(JsValue::from_str(&format!(
                        "mismatch:ssr-branch:{path}:{index}"
                    )));
                }
            }
        }
        let generation = self.next_generation;
        self.next_generation += 1;
        self.conditionals.insert(
            index,
            TypedConditionalRegion {
                start,
                end,
                selected: selected_node,
                nodes: HashMap::new(),
                generation,
                region_slot: RegionSlot::acquire(self.region_tracker.clone())?,
            },
        );
        // The unrendered sides' whole subtrees are absent from the markup.
        match selected_node {
            Some(handle) => {
                let unselected = if handle == consequent {
                    alternate
                } else {
                    Some(consequent)
                };
                if let Some(side) = unselected {
                    self.collect_branch_handles(side, skipped);
                }
            }
            None => {
                self.collect_branch_handles(consequent, skipped);
                if let Some(alternate) = alternate {
                    self.collect_branch_handles(alternate, skipped);
                }
            }
        }
        Ok(())
    }

    /// Every graph handle that lives below `index`, mirroring the branch
    /// recursion used for claiming so a skipped side covers exactly the
    /// nodes a fresh instantiation of that side would have created. The
    /// walk is iterative: it runs during adoption, where graph depth is
    /// validated but native stack safety must not depend on that alone.
    fn collect_branch_handles(&self, index: usize, output: &mut HashSet<usize>) {
        let mut pending = vec![index];
        while let Some(index) = pending.pop() {
            if !output.insert(index) {
                continue;
            }
            match self.app.nodes.get(index) {
                Some(TypedNode::Element { children, .. })
                | Some(TypedNode::Component { children, .. })
                | Some(TypedNode::DynamicComponent { children, .. }) => {
                    pending.extend(children.iter().copied());
                }
                Some(TypedNode::Conditional {
                    consequent,
                    alternate,
                    ..
                }) => {
                    pending.push(*consequent);
                    if let Some(alternate) = alternate {
                        pending.push(*alternate);
                    }
                }
                _ => {}
            }
        }
    }

    /// The ownership marker the first node inside a claimed branch region
    /// must carry for the adoption to trust the recorded side.
    fn branch_ownership_marker(&self, path: &str, handle: usize) -> Result<String, JsValue> {
        let node = self
            .app
            .nodes
            .get(handle)
            .ok_or_else(|| JsValue::from_str("branch node handle out of range"))?;
        Ok(match node {
            TypedNode::Element { .. } => format!("{path}/node:{handle}"),
            TypedNode::HostComponent { .. } => format!("{path}/node:{handle}"),
            TypedNode::Text { .. } => format!("plec:text:{path}:{handle}"),
            TypedNode::Component { .. } | TypedNode::DynamicComponent { .. } => {
                format!("plec:component:{path}:{handle}")
            }
            TypedNode::Conditional { .. } => format!("plec:conditional:{path}:{handle}"),
            TypedNode::Slot { .. } => format!("plec:slot:{path}:{handle}"),
            // The server renders nothing for a loop branch, so a recorded
            // selection can never legitimately point at one.
            TypedNode::Loop { .. } => {
                return Err(JsValue::from_str(&format!(
                    "mismatch:ssr-branch:{path}:{handle}"
                )));
            }
        })
    }

    fn queue_adopted_component(
        &mut self,
        index: usize,
        component: usize,
        props: Vec<TypedComponentProp>,
        children: Vec<usize>,
        root: &Element,
        markers: &TypedAdoptionIndex,
        path: &str,
        row: Option<&HashMap<String, RuntimeValue>>,
        row_index: usize,
        row_context: Option<TypedRowContext>,
    ) -> Result<(), JsValue> {
        let start = markers
            .get(&format!("plec:component:{path}:{index}"))
            .cloned()
            .ok_or_else(|| JsValue::from_str(&format!("missing:ssr-component:{path}:{index}")))?;
        let end = markers
            .get(&format!("plec:component-end:{path}:{index}"))
            .cloned()
            .ok_or_else(|| {
                JsValue::from_str(&format!("missing:ssr-component-end:{path}:{index}"))
            })?;
        let mut values = HashMap::new();
        let mut callbacks = HashMap::new();
        let mut component_props = HashMap::new();
        for prop in props {
            let name = self
                .app
                .strings
                .get(prop.name())
                .ok_or_else(|| JsValue::from_str("adoption component prop name out of range"))?
                .clone();
            match prop {
                TypedComponentProp::Value { expression, .. } => {
                    values.insert(
                        name,
                        typed_eval(
                            &self.app,
                            self.cookie_policy.borrow().as_ref(),
                            expression,
                            &self.states,
                            row,
                            row_index,
                        )?,
                    );
                }
                TypedComponentProp::Callable { action, .. } => {
                    callbacks.insert(
                        name,
                        TypedCallbackSpec {
                            action,
                            row: row.cloned(),
                        },
                    );
                }
                TypedComponentProp::Component {
                    component, host, ..
                } => {
                    component_props.insert(
                        name,
                        host.map(TypedComponentTarget::Host)
                            .unwrap_or(TypedComponentTarget::Native(component)),
                    );
                }
            }
        }
        let key = format!("{index}:{}", self.next_component_instance);
        self.next_component_instance += 1;
        let component_path = format!("{path}/component:{index}");
        // This instance's recorded execution state, keyed by exactly the
        // marker path the server renderer derived for the same call.
        let record = self.ssr_nested.get(&component_path);
        let ssr_branches = record
            .map(|record| record.branches.clone())
            .unwrap_or_default();
        let ssr_loops = record
            .map(|record| record.loops.clone())
            .unwrap_or_default();
        let nested_prefix = format!("{component_path}/");
        let ssr_nested = self
            .ssr_nested
            .iter()
            .filter(|(nested_path, _)| nested_path.starts_with(&nested_prefix))
            .map(|(nested_path, value)| (nested_path.clone(), value.clone()))
            .collect();
        // A row-scoped component call is owned by its row (`TypedRow.nodes`),
        // never by the graph: a graph-level entry would make static refresh
        // sweeps re-evaluate its props without the row, and a record prop
        // like `todo` recomputes empty (flipping prop-driven row branches).
        if row.is_none() {
            self.nodes.insert(index, start.clone());
        }
        self.component_requests.push(TypedComponentRequest {
            call: index,
            component,
            props: values,
            callbacks,
            component_props,
            children,
            row_context,
            start,
            end,
            key,
            // Identical grammar for adopted and fresh component DOM: the
            // structural address depends only on the call position.
            path: component_path,
            adoption: Some(TypedAdoptionRequest { root: root.clone() }),
            ssr_branches,
            ssr_loops,
            ssr_nested,
        });
        Ok(())
    }
}

/// The DOM scope one adoption owns: a whole element subtree, or the sibling
/// range between two boundary markers. Route and root adoptions use the
/// element scope; adopted component calls use the range between their
/// `plec:component`/`plec:component-end` markers, so nested adoptions never
/// re-walk the adoption root. Row and branch claims reuse the same shape.
pub enum TypedAdoptionScope {
    Element(Element),
    Range { start: Node, end: Node },
}

/// Marker string -> DOM node for one adoption scope, built in a single tree
/// walk. Element markers are `data-plec-node` attribute values; every other
/// marker is an ownership comment. A repeated marker fails the build: a
/// last-wins map would silently bind a duplicated stale marker and corrupt
/// claims invisibly.
pub struct TypedAdoptionIndex {
    markers: HashMap<String, Node>,
}

impl TypedAdoptionIndex {
    fn build(scope: TypedAdoptionScope) -> Result<Self, JsValue> {
        let mut markers = HashMap::new();
        match scope {
            TypedAdoptionScope::Element(root) => Self::visit(&root.into(), &mut markers)?,
            TypedAdoptionScope::Range { start, end } => {
                let mut cursor = start.next_sibling();
                while let Some(node) = cursor {
                    if node.is_same_node(Some(&end)) {
                        break;
                    }
                    Self::visit(&node, &mut markers)?;
                    cursor = node.next_sibling();
                }
            }
        }
        Ok(Self { markers })
    }

    fn visit(node: &Node, markers: &mut HashMap<String, Node>) -> Result<(), JsValue> {
        ADOPTION_INDEX_WALKS.with(|walks| walks.set(walks.get().saturating_add(1)));
        match node.node_type() {
            Node::COMMENT_NODE => {
                if let Some(marker) = node.node_value() {
                    Self::register(markers, marker, node)?;
                }
            }
            Node::ELEMENT_NODE => {
                let element = node.dyn_ref::<Element>();
                if let Some(marker) =
                    element.and_then(|element| element.get_attribute("data-plec-node"))
                {
                    Self::register(markers, marker, node)?;
                }
                if element.is_some_and(|element| element.has_attribute("data-plec-host")) {
                    return Ok(());
                }
            }
            _ => {}
        }
        let children = node.child_nodes();
        for position in 0..children.length() {
            if let Some(child) = children.item(position) {
                Self::visit(&child, markers)?;
            }
        }
        Ok(())
    }

    fn register(
        markers: &mut HashMap<String, Node>,
        marker: String,
        node: &Node,
    ) -> Result<(), JsValue> {
        if markers.insert(marker.clone(), node.clone()).is_some() {
            return Err(JsValue::from_str(&format!("duplicate:ssr-marker:{marker}")));
        }
        Ok(())
    }

    fn get(&self, marker: &str) -> Option<&Node> {
        self.markers.get(marker)
    }

    fn markers_for_prefix(&self, prefix: &str) -> Vec<String> {
        self.markers
            .keys()
            .filter(|marker| marker.starts_with(prefix))
            .cloned()
            .collect()
    }

    /// Element claims only ever resolve markers this index registered from
    /// `data-plec-node` attributes, so the cast failure below is
    /// unreachable; it maps to the nearest existing diagnostic.
    fn element(&self, marker: &str) -> Result<Element, JsValue> {
        match self.markers.get(marker) {
            None => Err(JsValue::from_str(&format!("missing:ssr-node:{marker}"))),
            Some(node) => node
                .clone()
                .dyn_into::<Element>()
                .map_err(|_| JsValue::from_str(&format!("mismatch:ssr-tag:{marker}"))),
        }
    }
}

/// The ownership marker a concrete DOM node carries, if any: comment nodes
/// own their text; elements own their `data-plec-node` attribute value.
fn ownership_marker(node: &Node) -> Option<String> {
    match node.node_type() {
        Node::COMMENT_NODE => node.node_value(),
        Node::ELEMENT_NODE => node
            .dyn_ref::<Element>()
            .and_then(|element| element.get_attribute("data-plec-node")),
        _ => None,
    }
}

impl RuntimeState {
    /// Test instrumentation for the adoption index: DOM nodes visited by
    /// index builds since the last reset. Scoped adoptions visit each owned
    /// node once per owning scope; a regression to full-root walks per
    /// component level would multiply this by the nesting depth.
    pub fn adoption_index_walks(&self) -> u32 {
        ADOPTION_INDEX_WALKS.with(Cell::get)
    }

    pub fn reset_adoption_index_walks(&self) {
        ADOPTION_INDEX_WALKS.with(|walks| walks.set(0));
    }

    /// Server-rendered text values replaced by the deterministic recompute
    /// during the most recent snapshot-backed adoption. Divergence is allowed
    /// and reported only; see `SSR_TEXT_DIVERGENCES`.
    pub fn ssr_text_divergences(&self) -> u32 {
        SSR_TEXT_DIVERGENCES.with(Cell::get)
    }

    pub fn reset_ssr_text_divergences(&self) {
        SSR_TEXT_DIVERGENCES.with(|count| count.set(0));
    }
}

impl TypedRuntime {
    pub fn mount_before(&mut self, end: &Node) -> Result<(), JsValue> {
        // A component request can be produced while caller-owned slot content
        // is being assembled in a DocumentFragment. Inserting that fragment
        // empties it, so its original parent is no longer the parent of the
        // component end marker by the time this request is mounted. The marker
        // is the durable insertion contract: resolve its current parent here.
        let parent = end
            .parent_node()
            .ok_or_else(|| JsValue::from_str("component end marker is detached"))?;
        let doc = document()?;
        let node = self.instantiate_node(
            &doc,
            self.app.root_node,
            Some(&parent),
            None,
            0,
            None,
            &mut HashMap::new(),
            &mut HashMap::new(),
        )?;
        if matches!(
            self.app.nodes.get(self.app.root_node),
            Some(TypedNode::Component { .. } | TypedNode::DynamicComponent { .. })
        ) {
            let component_end = node
                .next_sibling()
                .ok_or_else(|| JsValue::from_str("root component end missing"))?;
            parent.insert_before(&node, Some(end))?;
            parent.insert_before(&component_end, Some(end))?;
        } else {
            parent.insert_before(&node, Some(end))?;
        }
        self.apply_static_bindings()?;
        self.queue_static_listeners();
        Ok(())
    }

    /// Slot templates execute in the caller runtime, so their state, rows,
    /// listeners, and nested component calls retain caller ownership.
    pub fn mount_slot_children(
        &mut self,
        children: &[usize],
        start: &Node,
        end: &Node,
        row_context: Option<&TypedRowContext>,
    ) -> Result<(), JsValue> {
        let parent = end
            .parent_node()
            .ok_or_else(|| JsValue::from_str("slot parent missing"))?;
        let fragment: Node = document()?.create_document_fragment().into();
        let (row, row_index) = match row_context {
            Some(context) => {
                let rows = self
                    .loops
                    .get(&context.loop_index)
                    .ok_or_else(|| JsValue::from_str("slot row missing"))?;
                let row = rows
                    .rows
                    .get(&context.row_key)
                    .ok_or_else(|| JsValue::from_str("slot row missing"))?;
                let row_index = rows
                    .order
                    .iter()
                    .position(|key| key == &context.row_key)
                    .unwrap_or(0);
                (Some(row.values.clone()), row_index)
            }
            None => (None, 0),
        };
        let mut local = HashMap::new();
        let mut regions = HashMap::new();
        for child in children {
            self.instantiate_node(
                &document()?,
                *child,
                Some(&fragment),
                row.as_ref(),
                row_index,
                row_context,
                &mut local,
                &mut regions,
            )?;
        }
        parent.insert_before(&fragment, Some(end))?;
        if let Some(context) = row_context {
            self.apply_bindings_to_nodes(
                &local,
                row.as_ref(),
                row_index,
                &mut UpdateMetrics::default(),
            )?;
            let generation = self
                .loops
                .get(&context.loop_index)
                .and_then(|rows| rows.rows.get(&context.row_key))
                .map(|row| row.generation)
                .ok_or_else(|| JsValue::from_str("slot row missing"))?;
            let owner = TypedListenerOwner::Row {
                loop_index: context.loop_index,
                row_key: context.row_key.clone(),
                generation,
            };
            for (target, node) in &local {
                self.queue_listener(*target, node.clone(), owner.clone());
            }
            for (conditional, region) in &regions {
                let conditional_owner = TypedListenerOwner::Conditional {
                    conditional: *conditional,
                    generation: region.generation,
                    row: Some(Box::new(owner.clone())),
                };
                for (target, node) in &region.nodes {
                    self.queue_listener(*target, node.clone(), conditional_owner.clone());
                }
            }
            let row = self
                .loops
                .get_mut(&context.loop_index)
                .and_then(|rows| rows.rows.get_mut(&context.row_key))
                .ok_or_else(|| JsValue::from_str("slot row missing"))?;
            row.nodes.extend(local);
            row.conditionals.extend(regions);
        } else {
            self.apply_static_bindings()?;
            self.queue_static_listeners();
        }
        let _ = start;
        Ok(())
    }
}

impl TypedRuntime {
    /// Invalidation happens before aborting so graph disposal is silent: a
    /// rejected browser promise can never resume stale action code.
    pub fn invalidate_fetches(&mut self) {
        self.graph_generation = self.graph_generation.saturating_add(1);
        #[cfg(feature = "fetch")]
        {
            self.pending_fetches.clear();
            for (_, controller) in self.abort_controllers.drain() {
                controller.abort();
                self.region_tracker.release_fetch();
            }
        }
    }
}

impl TypedRuntime {
    pub fn clear_listeners(&mut self) {
        for listener in self.listeners.drain(..) {
            let listener = listener.listener;
            let _ = listener.element.remove_event_listener_with_callback(
                &listener.event_type,
                listener.callback.as_ref().unchecked_ref(),
            );
        }
        for listener in self.global_listeners.drain(..) {
            let _ = listener.target.remove_event_listener_with_callback(
                &listener.event_type,
                listener.callback.as_ref().unchecked_ref(),
            );
        }
        self.listener_requests.clear();
    }
}

impl TypedRuntime {
    pub fn dispose_region_listeners(&mut self, loop_index: usize, key: &str, generation: u64) {
        let mut keep = Vec::new();
        for entry in self.listeners.drain(..) {
            if entry.owner.belongs_to_row(loop_index, key, generation) {
                let listener = entry.listener;
                let _ = listener.element.remove_event_listener_with_callback(
                    &listener.event_type,
                    listener.callback.as_ref().unchecked_ref(),
                );
            } else {
                keep.push(entry);
            }
        }
        self.listeners = keep;
    }

    pub fn dispose_owner_listeners(&mut self, owner: &TypedListenerOwner) {
        let mut keep = Vec::new();
        for entry in self.listeners.drain(..) {
            if entry.owner.matches_disposal(owner) {
                let listener = entry.listener;
                let _ = listener.element.remove_event_listener_with_callback(
                    &listener.event_type,
                    listener.callback.as_ref().unchecked_ref(),
                );
            } else {
                keep.push(entry);
            }
        }
        self.listeners = keep;
    }

    pub fn queue_listener(&mut self, target: usize, node: Node, owner: TypedListenerOwner) {
        if self.app.events.iter().any(|event| event.target == target)
            && !self
                .listeners
                .iter()
                .any(|listener| listener.target == target && listener.owner == owner)
            && !self
                .listener_requests
                .iter()
                .any(|request| request.target == target && request.owner == owner)
        {
            self.listener_requests.push(TypedListenerRequest {
                target,
                node,
                owner,
            });
        }
    }

    fn queue_static_listeners(&mut self) {
        // Nodes owned by a conditional region already receive their listener
        // under the region's conditional owner (whose generation the first
        // branch flip disposes). A second Static-owned listener would double
        // every dispatch on the branch DOM.
        let conditional_owned: HashSet<usize> = self
            .conditionals
            .values()
            .flat_map(|region| region.nodes.keys().copied())
            .collect();
        let nodes = self
            .nodes
            .iter()
            .filter(|(target, _)| !conditional_owned.contains(target))
            .map(|(target, node)| (*target, node.clone()))
            .collect::<Vec<_>>();
        for (target, node) in nodes {
            self.queue_listener(target, node, TypedListenerOwner::Static);
        }
    }

    pub fn queue_row_listeners(&mut self, loop_index: usize, key: &str) {
        let Some(row) = self
            .loops
            .get(&loop_index)
            .and_then(|rows| rows.rows.get(key))
        else {
            return;
        };
        let owner = TypedListenerOwner::Row {
            loop_index,
            row_key: key.to_owned(),
            generation: row.generation,
        };
        let nodes = row
            .nodes
            .iter()
            .map(|(target, node)| (*target, node.clone()))
            .collect::<Vec<_>>();
        for (target, node) in nodes {
            self.queue_listener(target, node, owner.clone());
        }
    }
}

impl TypedRuntime {
    /// The structural address prefix for this emission: the instance path,
    /// or the owning row's canonical address when the node belongs to a
    /// keyed loop row. Row addresses extend the loop's recorded `path_base`,
    /// mirroring the server renderer's `{path}/loop:{node}/key:{key}`.
    pub fn address_path(
        &self,
        row_context: Option<&TypedRowContext>,
    ) -> Result<std::borrow::Cow<'_, str>, JsValue> {
        match row_context {
            None => Ok(std::borrow::Cow::Borrowed(self.path.as_str())),
            Some(context) => {
                let base = self
                    .loops
                    .get(&context.loop_index)
                    .and_then(|rows| rows.path_base.as_deref())
                    .ok_or_else(|| JsValue::from_str("address:loop-row-prefix-missing"))?;
                Ok(std::borrow::Cow::Owned(format!(
                    "{base}{}",
                    Self::encode_address_segment(&context.row_key)
                )))
            }
        }
    }

    /// Row keys may contain `/` and `%`; the address grammar escapes them
    /// exactly like the server renderer's `escapeInstanceSegment`.
    pub fn encode_address_segment(key: &str) -> String {
        key.replace('%', "%25").replace('/', "%2F")
    }

    pub fn instantiate_node(
        &mut self,
        doc: &Document,
        index: usize,
        parent: Option<&Node>,
        row: Option<&HashMap<String, RuntimeValue>>,
        row_index: usize,
        row_context: Option<&TypedRowContext>,
        local: &mut HashMap<usize, Node>,
        row_regions: &mut HashMap<usize, TypedConditionalRegion>,
    ) -> Result<Node, JsValue> {
        // Every recursive mount path (element children, conditional branches,
        // and loop row templates re-entering through `render_loop`) flows back
        // through this wrapper, so a validated acyclic chain deeper than
        // `MAX_MOUNT_DEPTH` fails with a diagnostic instead of overflowing the
        // WASM stack. Depth alone cannot guarantee native stack safety —
        // per-frame cost varies with node kind and build profile — so the
        // wrapper also tracks the stack watermark recorded at the outermost
        // mount and fails closed against `MAX_MOUNT_STACK_BYTES` before the
        // native stack can overflow.
        let stack_pointer = Self::mount_stack_pointer();
        if self.mount_depth == 0 {
            self.mount_stack_base = stack_pointer;
        }
        if self.mount_depth >= plec_ir::limits::MAX_MOUNT_DEPTH {
            return Err(JsValue::from_str("mount depth exceeds limit"));
        }
        if self.mount_stack_base.saturating_sub(stack_pointer)
            > plec_ir::limits::MAX_MOUNT_STACK_BYTES
        {
            return Err(JsValue::from_str("mount stack budget exceeded"));
        }
        self.mount_depth += 1;
        let result = self.instantiate_node_bounded(
            doc,
            index,
            parent,
            row,
            row_index,
            row_context,
            local,
            row_regions,
        );
        self.mount_depth -= 1;
        if self.mount_depth == 0 {
            self.mount_stack_base = 0;
        }
        result
    }

    fn instantiate_node_bounded(
        &mut self,
        doc: &Document,
        index: usize,
        parent: Option<&Node>,
        row: Option<&HashMap<String, RuntimeValue>>,
        row_index: usize,
        row_context: Option<&TypedRowContext>,
        local: &mut HashMap<usize, Node>,
        row_regions: &mut HashMap<usize, TypedConditionalRegion>,
    ) -> Result<Node, JsValue> {
        // The dispatcher clones the node handle and forwards each kind to a
        // dedicated instantiation method. One recursion level then costs the
        // frame of the node kind actually being mounted instead of the union
        // of every kind's locals, keeping a full `MAX_MOUNT_DEPTH` chain
        // within the native stack budget in every build profile.
        match self
            .app
            .nodes
            .get(index)
            .ok_or_else(|| JsValue::from_str("node handle out of range"))?
            .clone()
        {
            TypedNode::Element {
                tag,
                namespace,
                children,
                host_ref,
                ..
            } => self.instantiate_element(
                doc,
                index,
                tag,
                namespace,
                children,
                host_ref,
                parent,
                row,
                row_index,
                row_context,
                local,
                row_regions,
            ),
            TypedNode::HostComponent {
                provider,
                component,
                props,
                ..
            } => self.instantiate_host_component(
                doc,
                index,
                provider,
                component,
                props,
                parent,
                row,
                row_index,
                row_context,
                local,
            ),
            TypedNode::Text { text, .. } => {
                self.instantiate_text(doc, index, text, parent, row, local)
            }
            TypedNode::Loop { r#loop, .. } => {
                self.instantiate_loop_node(doc, index, r#loop, parent, row_context)
            }
            TypedNode::DynamicComponent {
                prop,
                props,
                children,
                ..
            } => self.instantiate_dynamic_component(
                doc,
                index,
                prop,
                props,
                children,
                parent,
                row,
                row_index,
                row_context,
                local,
            ),
            TypedNode::Component {
                component,
                props,
                children,
                ..
            } => self.instantiate_component_call(
                doc,
                index,
                component,
                props,
                children,
                parent,
                row,
                row_index,
                row_context,
                local,
            ),
            TypedNode::Slot { .. } => {
                self.instantiate_slot(doc, index, parent, row, row_context, local)
            }
            TypedNode::Conditional { test, .. } => self.instantiate_conditional(
                doc,
                index,
                test,
                parent,
                row,
                row_index,
                row_context,
                local,
                row_regions,
            ),
        }
    }

    /// The frame address of this wrapper. The stack grows downward on the
    /// supported wasm32 and native targets, so outermost frames carry the
    /// largest addresses and `outermost - current` approximates the native
    /// stack bytes one mount chain has consumed.
    fn mount_stack_pointer() -> usize {
        let marker = 0u8;
        std::hint::black_box(&marker as *const u8 as usize)
    }

    #[allow(clippy::too_many_arguments)]
    fn instantiate_element(
        &mut self,
        doc: &Document,
        index: usize,
        tag: usize,
        namespace: String,
        children: Vec<usize>,
        host_ref: Option<usize>,
        parent: Option<&Node>,
        row: Option<&HashMap<String, RuntimeValue>>,
        row_index: usize,
        row_context: Option<&TypedRowContext>,
        local: &mut HashMap<usize, Node>,
        row_regions: &mut HashMap<usize, TypedConditionalRegion>,
    ) -> Result<Node, JsValue> {
        let tag = self
            .app
            .strings
            .get(tag)
            .ok_or_else(|| JsValue::from_str("tag handle out of range"))?;
        // DOM-sink backstop (plec_ir::sink): validation rejects
        // hostile element tags before mount, but this is the one
        // place executable IR becomes a live DOM element, so the
        // namespace-aware allowlist is re-checked here against
        // substituted artifacts that skipped validation.
        if !plec_ir::sink::is_allowed_element_tag_with_policy(tag, &namespace, &self.tag_policy) {
            return Err(JsValue::from_str("element tag rejected by policy"));
        }
        let element = if namespace == "svg" {
            doc.create_element_ns(Some("http://www.w3.org/2000/svg"), tag)?
        } else {
            doc.create_element(tag)?
        };
        // The canonical structural address: identical grammar to the
        // server-rendered `data-plec-node` marker for the same graph
        // position, so CSR-created and server-created DOM share one
        // identity protocol.
        let address = self.address_path(row_context)?.into_owned();
        element.set_attribute("data-plec-node", &format!("{address}/node:{index}"))?;
        let node: Node = element.into();
        if let Some(parent) = parent {
            parent.append_child(&node)?;
        }
        for child in children {
            self.instantiate_node(
                doc,
                child,
                Some(&node),
                row,
                row_index,
                row_context,
                local,
                row_regions,
            )?;
        }
        if row.is_some() {
            local.insert(index, node.clone());
        } else {
            self.nodes.insert(index, node.clone());
        }
        if let Some(reference) = host_ref {
            let slot = self
                .host_ref_nodes
                .get_mut(reference)
                .ok_or_else(|| JsValue::from_str("host ref handle out of range"))?;
            *slot = Some(node.clone());
        }
        Ok(node)
    }

    #[allow(clippy::too_many_arguments)]
    fn instantiate_host_component(
        &mut self,
        doc: &Document,
        index: usize,
        provider: String,
        component: String,
        props: Vec<TypedComponentProp>,
        parent: Option<&Node>,
        row: Option<&HashMap<String, RuntimeValue>>,
        row_index: usize,
        row_context: Option<&TypedRowContext>,
        local: &mut HashMap<usize, Node>,
    ) -> Result<Node, JsValue> {
        let element = doc.create_element("span")?;
        let address = self.address_path(row_context)?.into_owned();
        element.set_attribute("data-plec-node", &format!("{address}/node:{index}"))?;
        element.set_attribute("data-plec-host", &format!("{provider}:{component}"))?;
        let values = evaluate_host_props(
            &self.app,
            self.cookie_policy.borrow().as_ref(),
            &props,
            &self.states,
            row,
            row_index,
        )?;
        self.mount_host(element.clone(), &provider, &component, values, Some(index))?;
        let node: Node = element.into();
        if let Some(parent) = parent {
            if let Err(error) = parent.append_child(&node) {
                self.dispose_host_boundary(&node)?;
                return Err(error);
            }
        }
        if row.is_some() {
            local.insert(index, node.clone());
        } else {
            self.nodes.insert(index, node.clone());
        }
        Ok(node)
    }

    fn instantiate_text(
        &mut self,
        doc: &Document,
        index: usize,
        text: usize,
        parent: Option<&Node>,
        row: Option<&HashMap<String, RuntimeValue>>,
        local: &mut HashMap<usize, Node>,
    ) -> Result<Node, JsValue> {
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

    fn instantiate_loop_node(
        &mut self,
        doc: &Document,
        index: usize,
        r#loop: usize,
        parent: Option<&Node>,
        row_context: Option<&TypedRowContext>,
    ) -> Result<Node, JsValue> {
        // Rows carry their own canonical `plec:loop:{rowPath}` /
        // `plec:loop-end:{rowPath}` boundary markers, exactly like
        // server-rendered rows. There is deliberately no separate
        // loop-position anchor: the server emits none, and a
        // component-local `plec:loop:{index}` comment would be an
        // unresolvable competitor grammar.
        let address = self.address_path(row_context)?.into_owned();
        let path_base = format!("{address}/loop:{index}/key:");
        if let Some(rows) = self.loops.get_mut(&r#loop) {
            rows.path_base = Some(path_base);
        } else {
            self.loops.insert(
                r#loop,
                TypedLoopRows {
                    order: Vec::new(),
                    rows: HashMap::new(),
                    path_base: Some(path_base),
                },
            );
        }
        self.render_loop(
            r#loop,
            parent.ok_or_else(|| JsValue::from_str("loop parent missing"))?,
        )?;
        let first = self.loops.get(&r#loop).and_then(|state| {
            state
                .order
                .first()
                .and_then(|key| state.rows.get(key))
                .map(|row| row.root.clone())
        });
        match first {
            Some(root) => Ok(root),
            None => Ok(doc.create_text_node("").into()),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn instantiate_dynamic_component(
        &mut self,
        doc: &Document,
        index: usize,
        prop: usize,
        props: Vec<TypedComponentProp>,
        children: Vec<usize>,
        parent: Option<&Node>,
        row: Option<&HashMap<String, RuntimeValue>>,
        row_index: usize,
        row_context: Option<&TypedRowContext>,
        local: &mut HashMap<usize, Node>,
    ) -> Result<Node, JsValue> {
        let parent = parent.ok_or_else(|| JsValue::from_str("component parent missing"))?;
        let address = self.address_path(row_context)?.into_owned();
        let start: Node = doc
            .create_comment(&format!("plec:component:{address}:{index}"))
            .into();
        let end: Node = doc
            .create_comment(&format!("plec:component-end:{address}:{index}"))
            .into();
        parent.append_child(&start)?;
        parent.append_child(&end)?;
        if let Some(target) = self
            .app
            .runtime_host_component_props
            .get(prop)
            .and_then(Clone::clone)
        {
            let boundary = doc.create_element("span")?;
            let address = self.address_path(row_context)?.into_owned();
            boundary.set_attribute("data-plec-node", &format!("{address}/node:{index}"))?;
            boundary.set_attribute(
                "data-plec-host",
                &format!("{}:{}", target.provider, target.component),
            )?;
            let values = evaluate_host_props(
                &self.app,
                self.cookie_policy.borrow().as_ref(),
                &props,
                &self.states,
                row,
                row_index,
            )?;
            self.mount_host(
                boundary.clone(),
                &target.provider,
                &target.component,
                values,
                Some(index),
            )?;
            if let Err(error) = parent.insert_before(&boundary, Some(&end)) {
                let node: Node = boundary.into();
                self.dispose_host_boundary(&node)?;
                let _ = parent.remove_child(&start);
                let _ = parent.remove_child(&end);
                return Err(error);
            }
            parent.remove_child(&start)?;
            parent.remove_child(&end)?;
            if row.is_some() {
                local.insert(index, boundary.clone().into());
            } else {
                self.nodes.insert(index, boundary.clone().into());
            }
            return Ok(boundary.into());
        }
        let Some(component) = self
            .app
            .runtime_component_props
            .get(prop)
            .and_then(|value| *value)
        else {
            if row.is_some() {
                local.insert(index, start.clone());
            } else {
                self.nodes.insert(index, start.clone());
            }
            return Ok(start);
        };
        let (values, callbacks, component_props) =
            self.evaluate_component_call_props(&props, row, row_index)?;
        let key = format!("{index}:{}", self.next_component_instance);
        self.next_component_instance += 1;
        self.component_requests.push(TypedComponentRequest {
            call: index,
            component,
            props: values,
            callbacks,
            component_props,
            children,
            row_context: row_context.cloned(),
            start: start.clone(),
            end,
            key,
            path: format!("{address}/component:{index}"),
            adoption: None,
            ssr_branches: HashMap::new(),
            ssr_loops: HashMap::new(),
            ssr_nested: HashMap::new(),
        });
        if row.is_some() {
            local.insert(index, start.clone());
        } else {
            self.nodes.insert(index, start.clone());
        }
        Ok(start)
    }

    #[allow(clippy::too_many_arguments)]
    fn instantiate_component_call(
        &mut self,
        doc: &Document,
        index: usize,
        component: usize,
        props: Vec<TypedComponentProp>,
        children: Vec<usize>,
        parent: Option<&Node>,
        row: Option<&HashMap<String, RuntimeValue>>,
        row_index: usize,
        row_context: Option<&TypedRowContext>,
        local: &mut HashMap<usize, Node>,
    ) -> Result<Node, JsValue> {
        let parent = parent.ok_or_else(|| JsValue::from_str("component parent missing"))?;
        let address = self.address_path(row_context)?.into_owned();
        let start: Node = doc
            .create_comment(&format!("plec:component:{address}:{index}"))
            .into();
        let end: Node = doc
            .create_comment(&format!("plec:component-end:{address}:{index}"))
            .into();
        parent.append_child(&start)?;
        parent.append_child(&end)?;
        let (values, callbacks, component_props) =
            self.evaluate_component_call_props(&props, row, row_index)?;
        let key = format!("{index}:{}", self.next_component_instance);
        self.next_component_instance += 1;
        self.component_requests.push(TypedComponentRequest {
            call: index,
            component,
            props: values,
            callbacks,
            component_props,
            children,
            row_context: row_context.cloned(),
            start: start.clone(),
            end,
            key,
            path: format!("{address}/component:{index}"),
            adoption: None,
            ssr_branches: HashMap::new(),
            ssr_loops: HashMap::new(),
            ssr_nested: HashMap::new(),
        });
        if row.is_some() {
            local.insert(index, start.clone());
        } else {
            self.nodes.insert(index, start.clone());
        }
        Ok(start)
    }

    /// Shared prop evaluation for static and dynamic component calls: value
    /// props evaluate eagerly, callables record their action (with the row
    /// snapshot that was live at mount), and component-valued props record
    /// their concrete native/host target.
    fn evaluate_component_call_props(
        &mut self,
        props: &[TypedComponentProp],
        row: Option<&HashMap<String, RuntimeValue>>,
        row_index: usize,
    ) -> Result<
        (
            HashMap<String, RuntimeValue>,
            HashMap<String, TypedCallbackSpec>,
            HashMap<String, TypedComponentTarget>,
        ),
        JsValue,
    > {
        let mut values = HashMap::new();
        let mut callbacks = HashMap::new();
        let mut component_props = HashMap::new();
        for prop in props {
            let name = self
                .app
                .strings
                .get(prop.name())
                .ok_or_else(|| JsValue::from_str("component prop name out of range"))?
                .clone();
            match prop {
                TypedComponentProp::Value { expression, .. } => {
                    values.insert(
                        name,
                        typed_eval(
                            &self.app,
                            self.cookie_policy.borrow().as_ref(),
                            *expression,
                            &self.states,
                            row,
                            row_index,
                        )?,
                    );
                }
                TypedComponentProp::Callable { action, .. } => {
                    callbacks.insert(
                        name,
                        TypedCallbackSpec {
                            action: *action,
                            row: row.cloned(),
                        },
                    );
                }
                TypedComponentProp::Component {
                    component, host, ..
                } => {
                    component_props.insert(
                        name,
                        host.clone()
                            .map(TypedComponentTarget::Host)
                            .unwrap_or(TypedComponentTarget::Native(*component)),
                    );
                }
            }
        }
        Ok((values, callbacks, component_props))
    }

    fn instantiate_slot(
        &mut self,
        doc: &Document,
        index: usize,
        parent: Option<&Node>,
        row: Option<&HashMap<String, RuntimeValue>>,
        row_context: Option<&TypedRowContext>,
        local: &mut HashMap<usize, Node>,
    ) -> Result<Node, JsValue> {
        let parent = parent.ok_or_else(|| JsValue::from_str("slot parent missing"))?;
        let address = self.address_path(row_context)?.into_owned();
        let start: Node = doc
            .create_comment(&format!("plec:slot:{address}:{index}"))
            .into();
        let end: Node = doc
            .create_comment(&format!("plec:slot-end:{address}:{index}"))
            .into();
        parent.append_child(&start)?;
        parent.append_child(&end)?;
        self.slot_requests.push(TypedSlotRequest {
            start: start.clone(),
            end,
        });
        if row.is_some() {
            local.insert(index, start.clone());
        } else {
            self.nodes.insert(index, start.clone());
        }
        Ok(start)
    }

    #[allow(clippy::too_many_arguments)]
    fn instantiate_conditional(
        &mut self,
        doc: &Document,
        index: usize,
        test: usize,
        parent: Option<&Node>,
        row: Option<&HashMap<String, RuntimeValue>>,
        row_index: usize,
        row_context: Option<&TypedRowContext>,
        local: &mut HashMap<usize, Node>,
        row_regions: &mut HashMap<usize, TypedConditionalRegion>,
    ) -> Result<Node, JsValue> {
        // Acquire before adding boundary markers to the live parent.
        // Row fragments are detached, but static conditionals mount
        // directly and need the same reserve-before-mutate invariant.
        let mut region_slot = Some(RegionSlot::acquire(self.region_tracker.clone())?);
        let is_row_region = row.is_some();
        let parent = parent.ok_or_else(|| JsValue::from_str("conditional parent missing"))?;
        let address = self.address_path(row_context)?.into_owned();
        let start: Node = doc
            .create_comment(&format!("plec:conditional:{address}:{index}"))
            .into();
        let end: Node = doc
            .create_comment(&format!("plec:conditional-end:{address}:{index}"))
            .into();
        parent.append_child(&start)?;
        parent.append_child(&end)?;
        let mut row_selected = None;
        if is_row_region {
            let TypedNode::Conditional {
                consequent,
                alternate,
                ..
            } = self.app.nodes[index].clone()
            else {
                unreachable!()
            };
            let selected = if typed_truthy(&typed_eval(
                &self.app,
                self.cookie_policy.borrow().as_ref(),
                test,
                &self.states,
                row,
                row_index,
            )?) {
                Some(consequent)
            } else {
                alternate
            };
            row_selected = selected;
            if let Some(selected) = selected {
                let child = self.instantiate_node(
                    doc,
                    selected,
                    Some(parent),
                    row,
                    row_index,
                    row_context,
                    local,
                    row_regions,
                )?;
                parent.insert_before(&child, Some(&end))?;
            }
        } else {
            self.conditionals.insert(
                index,
                TypedConditionalRegion {
                    start: start.clone(),
                    end: end.clone(),
                    selected: None,
                    nodes: HashMap::new(),
                    generation: 0,
                    region_slot: region_slot.take().expect("conditional slot available"),
                },
            );
            self.reconcile_static_conditional(index, &mut UpdateMetrics::default())?;
        }
        if is_row_region {
            local.insert(index, start.clone());
            let selected = row_selected;
            let mut nodes = HashMap::new();
            if let Some(selected) = selected {
                self.collect_instantiated_branch_nodes(selected, local, &mut nodes);
            }
            for target in nodes.keys() {
                local.remove(target);
            }
            row_regions.insert(
                index,
                TypedConditionalRegion {
                    start: start.clone(),
                    end,
                    selected,
                    nodes,
                    generation: self.next_generation,
                    region_slot: region_slot.take().expect("conditional slot available"),
                },
            );
            self.next_generation += 1;
        }
        Ok(start)
    }
}

impl TypedRuntime {
    pub fn reconcile_static_conditional(
        &mut self,
        conditional: usize,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let TypedNode::Conditional {
            test,
            consequent,
            alternate,
            ..
        } = self
            .app
            .nodes
            .get(conditional)
            .cloned()
            .ok_or_else(|| JsValue::from_str("conditional handle out of range"))?
        else {
            return Err(JsValue::from_str("conditional node expected"));
        };
        let selected = if typed_truthy(&typed_eval(
            &self.app,
            self.cookie_policy.borrow().as_ref(),
            test,
            &self.states,
            None,
            0,
        )?) {
            Some(consequent)
        } else {
            alternate
        };
        if self
            .conditionals
            .get(&conditional)
            .map(|region| region.selected == selected)
            .unwrap_or(false)
        {
            return Ok(());
        }
        let mut region = self
            .conditionals
            .remove(&conditional)
            .ok_or_else(|| JsValue::from_str("conditional region missing"))?;
        self.dispose_owner_listeners(&TypedListenerOwner::Conditional {
            conditional,
            generation: region.generation,
            row: None,
        });
        for (index, node) in &region.nodes {
            self.clear_host_ref_for_node(*index, node);
            self.dispose_host_boundary(node)?;
            if let Some(parent) = node.parent_node() {
                parent.remove_child(node)?;
                metrics.dom_operations += 1;
            }
            self.nodes
                .retain(|_, existing| !existing.is_same_node(Some(node)));
        }
        region.nodes.clear();
        region.selected = selected;
        region.generation = self.next_generation;
        self.next_generation += 1;
        if let Some(selected) = selected {
            let parent = region
                .end
                .parent_node()
                .ok_or_else(|| JsValue::from_str("conditional parent missing"))?;
            let child = self.instantiate_node(
                &document()?,
                selected,
                Some(&parent),
                None,
                0,
                None,
                &mut HashMap::new(),
                &mut HashMap::new(),
            )?;
            parent.insert_before(&child, Some(&region.end))?;
            self.collect_branch_nodes(selected, &mut region.nodes);
            self.apply_bindings_to_nodes(&region.nodes, None, 0, metrics)?;
        }
        self.conditionals.insert(conditional, region);
        let region = self.conditionals.get(&conditional).unwrap();
        let owner = TypedListenerOwner::Conditional {
            conditional,
            generation: region.generation,
            row: None,
        };
        let nodes = region
            .nodes
            .iter()
            .map(|(target, node)| (*target, node.clone()))
            .collect::<Vec<_>>();
        for (target, node) in nodes {
            self.queue_listener(target, node, owner.clone());
        }
        Ok(())
    }

    fn collect_branch_nodes(&self, index: usize, output: &mut HashMap<usize, Node>) {
        if let Some(node) = self.nodes.get(&index) {
            output.insert(index, node.clone());
        }
        match self.app.nodes.get(index) {
            Some(TypedNode::Element { children, .. }) => {
                for child in children {
                    self.collect_branch_nodes(*child, output);
                }
            }
            Some(TypedNode::Component { children, .. }) => {
                for child in children {
                    self.collect_branch_nodes(*child, output);
                }
            }
            Some(TypedNode::Conditional {
                consequent,
                alternate,
                ..
            }) => {
                self.collect_branch_nodes(*consequent, output);
                if let Some(alternate) = alternate {
                    self.collect_branch_nodes(*alternate, output);
                }
            }
            _ => {}
        }
    }

    fn collect_instantiated_branch_nodes(
        &self,
        index: usize,
        local: &HashMap<usize, Node>,
        output: &mut HashMap<usize, Node>,
    ) {
        if let Some(node) = local.get(&index) {
            output.insert(index, node.clone());
        }
        match self.app.nodes.get(index) {
            Some(TypedNode::Element { children, .. }) => {
                for child in children {
                    self.collect_instantiated_branch_nodes(*child, local, output);
                }
            }
            Some(TypedNode::Component { children, .. }) => {
                for child in children {
                    self.collect_instantiated_branch_nodes(*child, local, output);
                }
            }
            Some(TypedNode::Conditional {
                consequent,
                alternate,
                ..
            }) => {
                self.collect_instantiated_branch_nodes(*consequent, local, output);
                if let Some(child) = alternate {
                    self.collect_instantiated_branch_nodes(*child, local, output);
                }
            }
            _ => {}
        }
    }

    fn apply_bindings_to_nodes(
        &self,
        nodes: &HashMap<usize, Node>,
        row: Option<&HashMap<String, RuntimeValue>>,
        row_index: usize,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        for binding in &self.app.bindings {
            if let Some(node) = nodes.get(&binding.target) {
                typed_apply_binding(
                    &self.app,
                    self.cookie_policy.borrow().as_ref(),
                    binding,
                    node,
                    &self.states,
                    row,
                    row_index,
                )?;
                metrics.bindings_touched += 1;
            }
        }
        for program in &self.app.prop_programs {
            let Some(node) = nodes.get(&program.target) else {
                continue;
            };
            for write in &program.writes {
                let value = match write.expression {
                    Some(expression) => typed_eval(
                        &self.app,
                        self.cookie_policy.borrow().as_ref(),
                        expression,
                        &self.states,
                        row,
                        row_index,
                    )?,
                    None => write
                        .constant
                        .and_then(|index| self.app.constants.get(index))
                        .cloned()
                        .unwrap_or_default(),
                };
                if write.spread {
                    typed_apply_spread(&self.app, &write.kind, node, value)?;
                } else {
                    typed_apply_value(&self.app, &write.kind, write.name, node, value)?;
                }
            }
        }
        Ok(())
    }
}

impl TypedRuntime {
    pub fn render_loop(&mut self, loop_index: usize, parent: &Node) -> Result<(), JsValue> {
        let loop_def = self
            .app
            .loops
            .get(loop_index)
            .ok_or_else(|| JsValue::from_str("loop handle out of range"))?
            .clone();
        let values = typed_eval(
            &self.app,
            self.cookie_policy.borrow().as_ref(),
            loop_def.source_expression,
            &self.states,
            None,
            0,
        )?;
        let rows = values
            .array()
            .ok_or_else(|| JsValue::from_str("LOOP_SOURCE_NOT_ARRAY"))?;
        let mut projection = Vec::new();
        for (index, value) in rows.into_iter().enumerate() {
            let row = value
                .record()
                .cloned()
                .ok_or_else(|| JsValue::from_str("LOOP_ROW_NOT_OBJECT"))?;
            let key = typed_value_string(&typed_eval(
                &self.app,
                self.cookie_policy.borrow().as_ref(),
                loop_def.key_expression,
                &self.states,
                Some(&row),
                index,
            )?);
            if projection
                .iter()
                .any(|(existing, _): &(String, HashMap<String, RuntimeValue>)| existing == &key)
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
}

impl TypedRuntime {
    pub fn reconcile_input(
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
        if let Some(loop_index) = targets.first().copied() {
            if values.len() > plec_ir::limits::MAX_LOOP_ROWS {
                return Err(JsValue::from_str("LOOP_ROW_LIMIT_EXCEEDED"));
            }
            let loop_def = self.app.loops[loop_index].clone();
            let mut collection = TypedCollection::default();
            for (index, value) in values.iter().cloned().enumerate() {
                let row = runtime_from_json(value)?
                    .record()
                    .cloned()
                    .ok_or_else(|| JsValue::from_str("LOOP_ROW_NOT_OBJECT"))?;
                let key = typed_value_string(&typed_eval(
                    &self.app,
                    self.cookie_policy.borrow().as_ref(),
                    loop_def.key_expression,
                    &self.states,
                    Some(&row),
                    index,
                )?);
                collection.order.push(key.clone());
                collection.rows.insert(key, row);
            }
            self.collections.insert(input_index, collection);
        }
        for loop_index in targets {
            let loop_def = self.app.loops[loop_index].clone();
            let parent = self.parent_for_loop(loop_index)?;
            let mut projection = Vec::new();
            for (index, value) in values.iter().enumerate() {
                let row = runtime_from_json(value.clone())?
                    .record()
                    .cloned()
                    .ok_or_else(|| JsValue::from_str("LOOP_ROW_NOT_OBJECT"))?;
                projection.push((
                    typed_value_string(&typed_eval(
                        &self.app,
                        self.cookie_policy.borrow().as_ref(),
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
}

impl TypedRuntime {
    pub fn parent_for_loop(&self, loop_index: usize) -> Result<Node, JsValue> {
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
}

impl TypedRuntime {
    fn reserve_reconcile_budget(
        &self,
        nodes: usize,
        regions: usize,
        operations: usize,
    ) -> Result<(), JsValue> {
        let mut active = self.reconcile_budget.borrow_mut();
        active
            .get_or_insert_with(ReconcileBudget::new)
            .reserve(nodes, regions, operations)
    }

    fn planned_row_cost(
        &self,
        handle: usize,
        values: &HashMap<String, RuntimeValue>,
        row_index: usize,
    ) -> Result<(usize, usize), JsValue> {
        let node = self
            .app
            .nodes
            .get(handle)
            .ok_or_else(|| JsValue::from_str("planned row node out of range"))?;
        match node {
            TypedNode::Element { children, .. } => {
                children.iter().try_fold((1, 0), |cost, child| {
                    let child = self.planned_row_cost(*child, values, row_index)?;
                    Ok((cost.0 + child.0, cost.1 + child.1))
                })
            }
            TypedNode::Text { .. } | TypedNode::Slot { .. } | TypedNode::HostComponent { .. } => {
                Ok((1, 0))
            }
            TypedNode::Conditional {
                test,
                consequent,
                alternate,
                ..
            } => {
                // The selection is pure, so charge the active branch rather
                // than an inactive largest-branch estimate.
                let selected = typed_truthy(&typed_eval(
                    &self.app,
                    self.cookie_policy.borrow().as_ref(),
                    *test,
                    &self.states,
                    Some(values),
                    row_index,
                )?)
                .then_some(*consequent)
                .or(*alternate);
                let branch = selected
                    .map(|child| self.planned_row_cost(child, values, row_index))
                    .transpose()?
                    .unwrap_or((0, 0));
                Ok((branch.0 + 2, branch.1 + 1))
            }
            // Deferred loop/component work consumes the same shared budget
            // when it is flushed. Only its immediately-created markers count here.
            TypedNode::Loop { .. }
            | TypedNode::Component { .. }
            | TypedNode::DynamicComponent { .. } => Ok((2, 0)),
        }
    }

    fn row_dom_nodes(root: &Node, end: Option<&Node>) -> Vec<Node> {
        let mut nodes = vec![root.clone()];
        while end.is_some_and(|end| !nodes.last().unwrap().is_same_node(Some(end))) {
            let Some(next) = nodes.last().unwrap().next_sibling() else {
                break;
            };
            nodes.push(next);
        }
        nodes
    }

    pub fn reconcile_loop(
        &mut self,
        loop_index: usize,
        parent: &Node,
        projection: Vec<(String, HashMap<String, RuntimeValue>)>,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        if projection.len() > plec_ir::limits::MAX_LOOP_ROWS {
            return Err(JsValue::from_str("LOOP_ROW_LIMIT_EXCEEDED"));
        }
        let desired = projection
            .iter()
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        let fresh = projection
            .iter()
            .filter(|(key, _)| {
                self.loops
                    .get(&loop_index)
                    .is_none_or(|rows| !rows.rows.contains_key(key))
            })
            .collect::<Vec<_>>();
        let template = self
            .app
            .loops
            .get(loop_index)
            .ok_or_else(|| JsValue::from_str("loop handle out of range"))?
            .row_template;
        let (planned_nodes, planned_regions) =
            fresh
                .iter()
                .enumerate()
                .try_fold((0usize, 0usize), |cost, (index, (_, values))| {
                    let row = self.planned_row_cost(template, values, index)?;
                    Ok::<_, JsValue>((cost.0 + row.0 + 2, cost.1 + row.1 + 1))
                })?;
        let stale_operations = self
            .loops
            .get(&loop_index)
            .map(|rows| {
                rows.order
                    .iter()
                    .filter(|key| !desired.contains(key))
                    .filter_map(|key| rows.rows.get(key))
                    .map(|row| Self::row_dom_nodes(&row.root, row.end.as_ref()).len())
                    .sum::<usize>()
            })
            .unwrap_or(0);
        // The complete known plan is reserved before the first stale row is
        // removed or fresh fragment is spliced into the live tree.
        self.reserve_reconcile_budget(
            planned_nodes,
            planned_regions,
            planned_nodes
                .saturating_add(stale_operations)
                .saturating_add(projection.len()),
        )?;
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
                self.dispose_region_listeners(loop_index, &key, row.generation);
                for (index, node) in &row.nodes {
                    self.clear_host_ref_for_node(*index, node);
                    self.dispose_host_boundary(node)?;
                }
                if let Some(parent) = row.root.parent_node() {
                    for node in Self::row_dom_nodes(&row.root, row.end.as_ref()) {
                        parent.remove_child(&node)?;
                        metrics.dom_operations += 1;
                    }
                }
            }
        }
        let mut fresh_keys: HashSet<String> = HashSet::new();
        for (key, values) in projection.iter() {
            let existing = self
                .loops
                .get(&loop_index)
                .and_then(|rows| rows.rows.get(key))
                .map(|row| row.values.clone());
            if let Some(previous) = existing {
                if previous != *values {
                    self.update_typed_row(loop_index, key, values.clone(), None, metrics)?;
                }
            } else {
                fresh_keys.insert(key.clone());
            }
        }
        // LIS members of the surviving previous order are already in their
        // final relative position and must not be touched; every other
        // surviving row relocates exactly once, and new rows splice in at
        // their anchor. The right-to-left walk keeps the anchor one row to
        // the right, so placements never disturb unprocessed rows. Rows move
        // as indivisible DOM ranges (fragment-staged, single splice).
        let previous_order = self
            .loops
            .get(&loop_index)
            .map(|rows| rows.order.clone())
            .unwrap_or_default();
        let surviving = previous_order
            .iter()
            .filter(|key| {
                self.loops
                    .get(&loop_index)
                    .is_some_and(|rows| rows.rows.contains_key(*key) && desired.contains(key))
            })
            .cloned()
            .collect::<Vec<_>>();
        let stable = keyed_reorder_plan(&surviving, &desired);
        let mut anchor: Option<Node> = None;
        for position in (0..desired.len()).rev() {
            let (key, values) = &projection[position];
            let root = if fresh_keys.contains(key) {
                self.insert_typed_row(
                    loop_index,
                    parent,
                    key.clone(),
                    values.clone(),
                    position,
                    anchor.clone(),
                    metrics,
                )?;
                self.loops
                    .get(&loop_index)
                    .and_then(|rows| rows.rows.get(key))
                    .map(|row| row.root.clone())
                    .ok_or_else(|| JsValue::from_str("inserted row missing"))?
            } else if !stable[position] {
                self.relocate_typed_row(loop_index, parent, key, anchor.as_ref(), metrics)?
            } else {
                self.loops
                    .get(&loop_index)
                    .and_then(|rows| rows.rows.get(key))
                    .map(|row| row.root.clone())
                    .ok_or_else(|| JsValue::from_str("row missing"))?
            };
            anchor = Some(root);
        }
        self.loops.entry(loop_index).or_default().order = desired;
        Ok(())
    }

    /// Move one keyed row's claimed DOM range so it sits directly before
    /// `anchor` (or at the parent's tail when `anchor` is `None`). The range
    /// is treated as indivisible: its nodes stage into a detached fragment
    /// and splice back with one mutation, so the live tree never shows a
    /// partially moved row. Accounting stays honest: each staged node counts
    /// as one DOM operation and one moved node, the splice counts as one more
    /// DOM operation, and the whole relocation counts as one logical row move.
    fn relocate_typed_row(
        &mut self,
        loop_index: usize,
        parent: &Node,
        key: &str,
        anchor: Option<&Node>,
        metrics: &mut UpdateMetrics,
    ) -> Result<Node, JsValue> {
        let (root, end) = self
            .loops
            .get(&loop_index)
            .and_then(|rows| rows.rows.get(key))
            .map(|row| (row.root.clone(), row.end.clone()))
            .ok_or_else(|| JsValue::from_str("row missing"))?;
        let fragment: Node = document()?.create_document_fragment().into();
        for node in Self::row_dom_nodes(&root, end.as_ref()) {
            fragment.append_child(&node)?;
            metrics.dom_operations += 1;
            metrics.dom_nodes_moved += 1;
        }
        parent.insert_before(&fragment, anchor)?;
        metrics.dom_operations += 1;
        metrics.row_moves += 1;
        Ok(root)
    }
}

impl TypedRuntime {
    pub fn clear_host_refs(&mut self) {
        self.host_ref_nodes.fill(None);
        self.focus_refs.fill(None);
        self.pending_reactions.clear();
    }
    pub fn insert_typed_row(
        &mut self,
        loop_index: usize,
        parent: &Node,
        key: String,
        values: HashMap<String, RuntimeValue>,
        index: usize,
        before: Option<Node>,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let template = self.app.loops[loop_index].row_template;
        let mut nodes = HashMap::new();
        let doc = document()?;
        let mut conditionals = HashMap::new();
        let row_context = TypedRowContext {
            loop_index,
            row_key: key.clone(),
        };
        // Delta-inserted rows are CSR-created DOM and must carry the same
        // `plec:loop:{rowPath}` boundaries the server rendered for the loop's
        // other rows. The prefix is recorded at loop instantiation/adoption;
        // a missing prefix means the loop was never addressed — fail closed.
        let path_base = self
            .loops
            .get(&loop_index)
            .and_then(|rows| rows.path_base.clone())
            .ok_or_else(|| JsValue::from_str("address:loop-row-prefix-missing"))?;
        let row_path = format!("{path_base}{}", Self::encode_address_segment(&key));
        let start_marker: Node = doc.create_comment(&format!("plec:loop:{row_path}")).into();
        let end_marker: Node = doc
            .create_comment(&format!("plec:loop-end:{row_path}"))
            .into();
        let fragment: Node = doc.create_document_fragment().into();
        fragment.append_child(&start_marker)?;
        let root = self.instantiate_node(
            &doc,
            template,
            Some(&fragment),
            Some(&values),
            index,
            Some(&row_context),
            &mut nodes,
            &mut conditionals,
        )?;
        fragment.append_child(&end_marker)?;
        if let Ok(element) = root.clone().dyn_into::<Element>() {
            element.set_attribute("data-runtime-row-key", &key)?;
        }
        let _conditional_selections = self.row_conditional_selections(template, &values, index)?;
        let generation = self.next_generation;
        self.next_generation += 1;
        self.loops.entry(loop_index).or_default().rows.insert(
            key.clone(),
            TypedRow {
                root: start_marker,
                end: Some(end_marker),
                values: values.clone(),
                nodes,
                conditionals,
                generation,
                region_slot: RegionSlot::acquire(self.region_tracker.clone())?,
            },
        );
        parent.insert_before(&fragment, before.as_ref())?;
        self.update_typed_row(loop_index, &key, values, None, metrics)?;
        self.queue_row_listeners(loop_index, &key);
        let row = self.loops.get(&loop_index).unwrap().rows.get(&key).unwrap();
        let row_owner = TypedListenerOwner::Row {
            loop_index,
            row_key: key.clone(),
            generation: row.generation,
        };
        let requests = row
            .conditionals
            .iter()
            .flat_map(|(conditional, region)| {
                let owner = TypedListenerOwner::Conditional {
                    conditional: *conditional,
                    generation: region.generation,
                    row: Some(Box::new(row_owner.clone())),
                };
                region
                    .nodes
                    .iter()
                    .map(move |(target, node)| (*target, node.clone(), owner.clone()))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        for (target, node, owner) in requests {
            self.queue_listener(target, node, owner);
        }
        metrics.dom_operations += 1;
        metrics.row_inserts += 1;
        Ok(())
    }
}

impl TypedRuntime {
    pub fn update_typed_row(
        &mut self,
        loop_index: usize,
        key: &str,
        values: HashMap<String, RuntimeValue>,
        changed_state: Option<usize>,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let template = self.app.loops[loop_index].row_template;
        let row_index = self
            .loops
            .get(&loop_index)
            .and_then(|rows| rows.order.iter().position(|entry| entry == key))
            .unwrap_or(0);
        let next_conditionals = self.row_conditional_selections(template, &values, row_index)?;
        self.reconcile_row_conditionals(
            loop_index,
            key,
            &values,
            row_index,
            next_conditionals,
            metrics,
        )?;
        let row = self
            .loops
            .get_mut(&loop_index)
            .and_then(|rows| rows.rows.get_mut(key))
            .ok_or_else(|| JsValue::from_str("row missing"))?;
        row.values = values;
        let bindings = self.app.bindings.clone();
        for binding in bindings {
            if let Some(state) = changed_state {
                if !self.app.expressions[binding.expression].instructions.iter().any(|instruction| matches!(instruction, TypedExpressionInstruction::LoadState { state: dependency } if *dependency == state)) {
                    continue;
                }
            }
            if let Some(node) = row.nodes.get(&binding.target).or_else(|| {
                row.conditionals
                    .values()
                    .find_map(|region| region.nodes.get(&binding.target))
            }) {
                typed_apply_binding(
                    &self.app,
                    self.cookie_policy.borrow().as_ref(),
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
        for program in self.app.prop_programs.clone() {
            if let Some(node) = row.nodes.get(&program.target) {
                for write in program.writes {
                    if let Some(state) = changed_state {
                        if write.expression.is_some_and(|expression| !self.app.expressions[expression].instructions.iter().any(|instruction| matches!(instruction, TypedExpressionInstruction::LoadState { state: dependency } if *dependency == state))) {
                            continue;
                        }
                    }
                    let value = match write.expression {
                        Some(expression) => typed_eval(
                            &self.app,
                            self.cookie_policy.borrow().as_ref(),
                            expression,
                            &self.states,
                            Some(&row.values),
                            0,
                        )?,
                        None => write
                            .constant
                            .and_then(|index| self.app.constants.get(index))
                            .cloned()
                            .unwrap_or_default(),
                    };
                    if write.spread {
                        typed_apply_spread(&self.app, &write.kind, node, value)?;
                    } else {
                        typed_apply_value(&self.app, &write.kind, write.name, node, value)?;
                    }
                }
            }
        }
        let (nodes, values) = self
            .loops
            .get(&loop_index)
            .and_then(|rows| rows.rows.get(key))
            .map(|row| (row.nodes.clone(), row.values.clone()))
            .ok_or_else(|| JsValue::from_str("row missing"))?;
        self.queue_row_component_refreshes(nodes, &values, row_index)?;
        Ok(())
    }

    fn reconcile_row_conditionals(
        &mut self,
        loop_index: usize,
        key: &str,
        values: &HashMap<String, RuntimeValue>,
        row_index: usize,
        selected: HashMap<usize, Option<usize>>,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        for (conditional, next) in selected {
            let current = self
                .loops
                .get(&loop_index)
                .and_then(|rows| rows.rows.get(key))
                .and_then(|row| row.conditionals.get(&conditional))
                .map(|region| region.selected);
            // Slot content mounts after its keyed component instance. Its
            // conditional region is therefore absent during the row's first
            // reconciliation and arrives with the correct initial branch.
            let Some(current) = current else {
                continue;
            };
            if current == next {
                continue;
            }
            let mut region = self
                .loops
                .get_mut(&loop_index)
                .and_then(|rows| rows.rows.get_mut(key))
                .and_then(|row| row.conditionals.remove(&conditional))
                .ok_or_else(|| JsValue::from_str("row conditional missing"))?;
            let row_owner = TypedListenerOwner::Row {
                loop_index,
                row_key: key.to_owned(),
                generation: self
                    .loops
                    .get(&loop_index)
                    .and_then(|rows| rows.rows.get(key))
                    .unwrap()
                    .generation,
            };
            self.dispose_owner_listeners(&TypedListenerOwner::Conditional {
                conditional,
                generation: region.generation,
                row: Some(Box::new(row_owner.clone())),
            });
            for node in region.nodes.values() {
                if let Some(parent) = node.parent_node() {
                    parent.remove_child(node)?;
                    metrics.dom_operations += 1;
                }
            }
            region.nodes.clear();
            region.selected = next;
            region.generation = self.next_generation;
            self.next_generation += 1;
            if let Some(branch) = next {
                let parent = region
                    .end
                    .parent_node()
                    .ok_or_else(|| JsValue::from_str("row conditional parent missing"))?;
                let mut local = HashMap::new();
                let mut nested = HashMap::new();
                let row_context = TypedRowContext {
                    loop_index,
                    row_key: key.to_owned(),
                };
                let child = self.instantiate_node(
                    &document()?,
                    branch,
                    Some(&parent),
                    Some(values),
                    row_index,
                    Some(&row_context),
                    &mut local,
                    &mut nested,
                )?;
                parent.insert_before(&child, Some(&region.end))?;
                self.collect_instantiated_branch_nodes(branch, &local, &mut region.nodes);
            }
            let nodes = region
                .nodes
                .iter()
                .map(|(target, node)| (*target, node.clone()))
                .collect::<Vec<_>>();
            let owner = TypedListenerOwner::Conditional {
                conditional,
                generation: region.generation,
                row: Some(Box::new(row_owner)),
            };
            for (target, node) in nodes {
                self.queue_listener(target, node, owner.clone());
            }
            self.loops
                .get_mut(&loop_index)
                .unwrap()
                .rows
                .get_mut(key)
                .unwrap()
                .conditionals
                .insert(conditional, region);
        }
        Ok(())
    }

    fn row_conditional_selections(
        &self,
        node: usize,
        row: &HashMap<String, RuntimeValue>,
        row_index: usize,
    ) -> Result<HashMap<usize, Option<usize>>, JsValue> {
        let mut selections = HashMap::new();
        self.collect_row_conditional_selections(node, row, row_index, &mut selections)?;
        Ok(selections)
    }

    fn collect_row_conditional_selections(
        &self,
        node: usize,
        row: &HashMap<String, RuntimeValue>,
        row_index: usize,
        selections: &mut HashMap<usize, Option<usize>>,
    ) -> Result<(), JsValue> {
        match self.app.nodes.get(node) {
            Some(TypedNode::Element { children, .. }) => {
                for child in children {
                    self.collect_row_conditional_selections(*child, row, row_index, selections)?;
                }
            }
            Some(TypedNode::Component { children, .. }) => {
                for child in children {
                    self.collect_row_conditional_selections(*child, row, row_index, selections)?;
                }
            }
            Some(TypedNode::Conditional {
                test,
                consequent,
                alternate,
                ..
            }) => {
                let selected = if typed_truthy(&typed_eval(
                    &self.app,
                    self.cookie_policy.borrow().as_ref(),
                    *test,
                    &self.states,
                    Some(row),
                    row_index,
                )?) {
                    Some(*consequent)
                } else {
                    *alternate
                };
                selections.insert(node, selected);
                if let Some(selected) = selected {
                    self.collect_row_conditional_selections(selected, row, row_index, selections)?;
                }
            }
            Some(_) => {}
            None => return Err(JsValue::from_str("conditional node handle out of range")),
        }
        Ok(())
    }
}

impl TypedRuntime {
    pub fn apply_static_bindings(&mut self) -> Result<(), JsValue> {
        // One before/after observation at the single re-evaluation point: when
        // this instance adopted server DOM for an imported snapshot, record
        // which server-rendered text values the deterministic recompute
        // replaces. This is outcome-level reporting, not per-binding diffing
        // or reconciliation.
        let observed = if self.ssr_imported {
            self.app
                .bindings
                .iter()
                .filter(|binding| binding.sink == "text")
                .filter_map(|binding| {
                    let node = self.nodes.get(&binding.target)?;
                    Some((binding.target, node.text_content().unwrap_or_default()))
                })
                .collect::<HashMap<usize, String>>()
        } else {
            HashMap::new()
        };
        let row_targets = self
            .loops
            .values()
            .flat_map(|rows| rows.rows.values())
            .flat_map(|row| row.nodes.keys().copied())
            .collect::<HashSet<_>>();
        for binding in self.app.bindings.clone() {
            if row_targets.contains(&binding.target) {
                continue;
            }
            if let Some(node) = self.nodes.get(&binding.target) {
                typed_apply_binding(
                    &self.app,
                    self.cookie_policy.borrow().as_ref(),
                    &binding,
                    node,
                    &self.states,
                    None,
                    0,
                )?;
            }
        }
        for (target, before) in observed {
            if let Some(node) = self.nodes.get(&target) {
                let after = node.text_content().unwrap_or_default();
                if after != before {
                    SSR_TEXT_DIVERGENCES.with(|count| count.set(count.get().saturating_add(1)));
                }
            }
        }
        for program in self.app.prop_programs.clone() {
            if row_targets.contains(&program.target) {
                continue;
            }
            let Some(node) = self.nodes.get(&program.target) else {
                continue;
            };
            for write in program.writes {
                let value = match write.expression {
                    Some(expression) => typed_eval(
                        &self.app,
                        self.cookie_policy.borrow().as_ref(),
                        expression,
                        &self.states,
                        None,
                        0,
                    )?,
                    None => write
                        .constant
                        .and_then(|index| self.app.constants.get(index))
                        .cloned()
                        .unwrap_or_default(),
                };
                if write.spread {
                    typed_apply_spread(&self.app, &write.kind, node, value)?;
                } else {
                    typed_apply_value(&self.app, &write.kind, write.name, node, value)?;
                }
            }
        }
        let rows = self
            .loops
            .iter()
            .flat_map(|(loop_index, rows)| {
                rows.order
                    .iter()
                    .enumerate()
                    .filter_map(|(row_index, key)| {
                        rows.rows.get(key).map(|row| {
                            (
                                *loop_index,
                                row_index,
                                row.nodes.clone(),
                                row.values.clone(),
                            )
                        })
                    })
            })
            .collect::<Vec<_>>();
        for (_, row_index, nodes, values) in rows {
            self.apply_bindings_to_nodes(
                &nodes,
                Some(&values),
                row_index,
                &mut UpdateMetrics::default(),
            )?;
            self.queue_row_component_refreshes(nodes, &values, row_index)?;
        }
        Ok(())
    }
}

impl TypedRuntime {
    fn expression_uses_changed_field(&self, expression: usize, changed: &HashSet<String>) -> bool {
        self.app.expressions.get(expression).is_some_and(|program| {
            program
                .instructions
                .iter()
                .any(|instruction| match instruction {
                    TypedExpressionInstruction::LoadRowField { field } => self
                        .app
                        .strings
                        .get(*field)
                        .is_some_and(|name| changed.contains(name)),
                    _ => false,
                })
        })
    }

    fn expression_uses_index(&self, expression: usize) -> bool {
        self.app.expressions.get(expression).is_some_and(|program| {
            program
                .instructions
                .iter()
                .any(|instruction| matches!(instruction, TypedExpressionInstruction::Index))
        })
    }

    fn component_uses_changed_field(&self, node: usize, changed: &HashSet<String>) -> bool {
        matches!(self.app.nodes.get(node),
            Some(TypedNode::Component { props, .. }
                | TypedNode::DynamicComponent { props, .. }
                | TypedNode::HostComponent { props, .. })
            if props.iter().any(|prop| matches!(prop, TypedComponentProp::Value { expression, .. }
                if self.expression_uses_changed_field(*expression, changed)))
        )
    }

    fn component_uses_index(&self, node: usize) -> bool {
        matches!(self.app.nodes.get(node),
            Some(TypedNode::Component { props, .. }
                | TypedNode::DynamicComponent { props, .. }
                | TypedNode::HostComponent { props, .. })
            if props.iter().any(|prop| matches!(prop, TypedComponentProp::Value { expression, .. }
                if self.expression_uses_index(*expression)))
        )
    }

    fn row_field_targets(
        &self,
        loop_index: usize,
        changed: &HashSet<String>,
    ) -> HashSet<(String, usize)> {
        self.app
            .dependency_edges
            .iter()
            .filter_map(|edge| {
                (edge.source.kind == "rowField" && edge.source.r#loop == Some(loop_index))
                    .then(|| self.app.strings.get(edge.source.handle))
                    .flatten()
                    .filter(|field| changed.contains(*field))
                    .map(|_| (edge.target.kind.clone(), edge.target.handle))
            })
            .collect()
    }

    fn has_row_field_dependency(&self, loop_index: usize, kind: &str, handle: usize) -> bool {
        self.app.dependency_edges.iter().any(|edge| {
            edge.source.kind == "rowField"
                && edge.source.r#loop == Some(loop_index)
                && edge.target.kind == kind
                && edge.target.handle == handle
        })
    }

    /// Re-evaluates only sinks whose programs read one of the supplied row
    /// fields. This is the non-structural counterpart to keyed row deltas.
    fn update_typed_row_fields(
        &mut self,
        loop_index: usize,
        key: &str,
        values: HashMap<String, RuntimeValue>,
        changed: HashSet<String>,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let row_index = self
            .loops
            .get(&loop_index)
            .and_then(|rows| rows.order.iter().position(|entry| entry == key))
            .ok_or_else(|| JsValue::from_str("row order missing"))?;
        let targets = self.row_field_targets(loop_index, &changed);

        let selected = self.row_conditional_selections(
            self.app.loops[loop_index].row_template,
            &values,
            row_index,
        )?.into_iter().filter(|(conditional, _)| {
            targets.contains(&(String::from("conditional"), *conditional)) ||
            (!self.has_row_field_dependency(loop_index, "conditional", *conditional) &&
             matches!(self.app.nodes.get(*conditional), Some(TypedNode::Conditional { test, .. })
                if self.expression_uses_changed_field(*test, &changed)))
        }).collect();
        self.reconcile_row_conditionals(loop_index, key, &values, row_index, selected, metrics)?;

        let (nodes, conditionals) = {
            let row = self
                .loops
                .get_mut(&loop_index)
                .and_then(|rows| rows.rows.get_mut(key))
                .ok_or_else(|| JsValue::from_str("row missing"))?;
            row.values = values.clone();
            (
                row.nodes.clone(),
                row.conditionals
                    .values()
                    .map(|region| region.nodes.clone())
                    .collect::<Vec<_>>(),
            )
        };
        for (binding_index, binding) in self.app.bindings.clone().into_iter().enumerate() {
            if !targets.contains(&(String::from("binding"), binding_index))
                && (self.has_row_field_dependency(loop_index, "binding", binding_index)
                    || !self.expression_uses_changed_field(binding.expression, &changed))
            {
                continue;
            }
            if let Some(node) = nodes.get(&binding.target).or_else(|| {
                conditionals
                    .iter()
                    .find_map(|region| region.get(&binding.target))
            }) {
                typed_apply_binding(
                    &self.app,
                    self.cookie_policy.borrow().as_ref(),
                    &binding,
                    node,
                    &self.states,
                    Some(&values),
                    row_index,
                )?;
                metrics.dom_operations += 1;
                metrics.nodes_touched += 1;
                metrics.bindings_touched += 1;
            }
        }
        for (program_index, program) in self.app.prop_programs.clone().into_iter().enumerate() {
            let Some(node) = nodes.get(&program.target) else {
                continue;
            };
            for write in program.writes {
                let Some(expression) = write.expression else {
                    continue;
                };
                if !targets.contains(&(String::from("propProgram"), program_index))
                    && (self.has_row_field_dependency(loop_index, "propProgram", program_index)
                        || !self.expression_uses_changed_field(expression, &changed))
                {
                    continue;
                }
                let value = typed_eval(
                    &self.app,
                    self.cookie_policy.borrow().as_ref(),
                    expression,
                    &self.states,
                    Some(&values),
                    row_index,
                )?;
                if write.spread {
                    typed_apply_spread(&self.app, &write.kind, node, value)?;
                } else {
                    typed_apply_value(&self.app, &write.kind, write.name, node, value)?;
                }
                metrics.dom_operations += 1;
                metrics.nodes_touched += 1;
                metrics.prop_writes += 1;
            }
        }
        let component_nodes = nodes
            .into_iter()
            .filter(|(node, _)| self.component_uses_changed_field(*node, &changed))
            .collect::<HashMap<_, _>>();
        self.queue_row_component_refreshes(component_nodes, &values, row_index)?;
        Ok(())
    }

    fn update_typed_row_index(
        &mut self,
        loop_index: usize,
        key: &str,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let (values, row_index, nodes) = self
            .loops
            .get(&loop_index)
            .and_then(|rows| {
                rows.rows.get(key).map(|row| {
                    (
                        row.values.clone(),
                        rows.order.iter().position(|entry| entry == key),
                        row.nodes.clone(),
                    )
                })
            })
            .ok_or_else(|| JsValue::from_str("row missing"))?;
        let row_index = row_index.ok_or_else(|| JsValue::from_str("row order missing"))?;
        let selected = self
            .row_conditional_selections(
                self.app.loops[loop_index].row_template,
                &values,
                row_index,
            )?
            .into_iter()
            .filter(|(conditional, _)| {
                matches!(self.app.nodes.get(*conditional), Some(TypedNode::Conditional { test, .. })
                    if self.expression_uses_index(*test))
            })
            .collect();
        self.reconcile_row_conditionals(loop_index, key, &values, row_index, selected, metrics)?;
        for binding in self.app.bindings.clone() {
            if !self.expression_uses_index(binding.expression) {
                continue;
            }
            if let Some(node) = nodes.get(&binding.target) {
                typed_apply_binding(
                    &self.app,
                    self.cookie_policy.borrow().as_ref(),
                    &binding,
                    node,
                    &self.states,
                    Some(&values),
                    row_index,
                )?;
                metrics.dom_operations += 1;
                metrics.nodes_touched += 1;
                metrics.bindings_touched += 1;
            }
        }
        for program in self.app.prop_programs.clone() {
            let Some(node) = nodes.get(&program.target) else {
                continue;
            };
            for write in program.writes {
                let Some(expression) = write.expression else {
                    continue;
                };
                if !self.expression_uses_index(expression) {
                    continue;
                }
                let value = typed_eval(
                    &self.app,
                    self.cookie_policy.borrow().as_ref(),
                    expression,
                    &self.states,
                    Some(&values),
                    row_index,
                )?;
                if write.spread {
                    typed_apply_spread(&self.app, &write.kind, node, value)?;
                } else {
                    typed_apply_value(&self.app, &write.kind, write.name, node, value)?;
                }
                metrics.dom_operations += 1;
                metrics.nodes_touched += 1;
                metrics.prop_writes += 1;
            }
        }
        self.queue_row_component_refreshes(
            nodes
                .into_iter()
                .filter(|(node, _)| self.component_uses_index(*node))
                .collect(),
            &values,
            row_index,
        )?;
        Ok(())
    }

    fn refresh_index_rows(
        &mut self,
        loop_index: usize,
        keys: Vec<String>,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        for key in keys {
            self.update_typed_row_index(loop_index, &key, metrics)?;
        }
        Ok(())
    }

    fn remove_delta_row(
        &mut self,
        loop_index: usize,
        key: &str,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let row = self
            .loops
            .get_mut(&loop_index)
            .and_then(|rows| {
                rows.order.retain(|entry| entry != key);
                rows.rows.remove(key)
            })
            .ok_or_else(|| JsValue::from_str("row missing"))?;
        self.dispose_region_listeners(loop_index, key, row.generation);
        for (index, node) in &row.nodes {
            self.clear_host_ref_for_node(*index, node);
            self.dispose_host_boundary(node)?;
        }
        if let Some(parent) = row.root.parent_node() {
            for node in Self::row_dom_nodes(&row.root, row.end.as_ref()) {
                parent.remove_child(&node)?;
                metrics.dom_operations += 1;
            }
        }
        metrics.row_removes += 1;
        Ok(())
    }

    fn move_delta_row(
        &mut self,
        loop_index: usize,
        key: &str,
        before: Option<&str>,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let (root, end, anchor) = {
            let rows = self
                .loops
                .get(&loop_index)
                .ok_or_else(|| JsValue::from_str("loop rows missing"))?;
            let row = rows
                .rows
                .get(key)
                .ok_or_else(|| JsValue::from_str("row missing"))?;
            let anchor = before
                .and_then(|before| rows.rows.get(before))
                .map(|row| row.root.clone());
            (row.root.clone(), row.end.clone(), anchor)
        };
        if before == Some(key) {
            return Ok(());
        }
        let last = end.as_ref().unwrap_or(&root);
        if anchor.as_ref().is_some_and(|anchor| {
            last.next_sibling()
                .is_some_and(|next| next.is_same_node(Some(anchor)))
        }) {
            return Ok(());
        }
        let parent = root
            .parent_node()
            .ok_or_else(|| JsValue::from_str("row parent missing"))?;
        self.relocate_typed_row(loop_index, &parent, key, anchor.as_ref(), metrics)?;
        let rows = self
            .loops
            .get_mut(&loop_index)
            .ok_or_else(|| JsValue::from_str("loop rows missing"))?;
        rows.order.retain(|entry| entry != key);
        let position = before
            .and_then(|before| rows.order.iter().position(|entry| entry == before))
            .unwrap_or(rows.order.len());
        rows.order.insert(position, key.to_owned());
        metrics.row_moves += 1;
        Ok(())
    }

    pub fn apply_delta(
        &mut self,
        delta: Delta,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
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
            match &delta {
                Delta::Update {
                    row_key, changes, ..
                } => {
                    let (mut values, index) = self
                        .loops
                        .get(&loop_index)
                        .and_then(|rows| {
                            rows.rows.get(row_key).map(|row| {
                                (
                                    row.values.clone(),
                                    rows.order.iter().position(|entry| entry == row_key),
                                )
                            })
                        })
                        .ok_or_else(|| JsValue::from_str("row missing"))?;
                    let index = index.ok_or_else(|| JsValue::from_str("row order missing"))?;
                    let loop_def = self.app.loops[loop_index].clone();
                    let previous_key = typed_value_string(&typed_eval(
                        &self.app,
                        self.cookie_policy.borrow().as_ref(),
                        loop_def.key_expression,
                        &self.states,
                        Some(&values),
                        index,
                    )?);
                    let changed = changes
                        .iter()
                        .map(|(name, value)| Ok((name.clone(), runtime_from_json(value.clone())?)))
                        .collect::<Result<HashMap<_, _>, JsValue>>()?;
                    values.extend(changed.clone());
                    let next_key = typed_value_string(&typed_eval(
                        &self.app,
                        self.cookie_policy.borrow().as_ref(),
                        loop_def.key_expression,
                        &self.states,
                        Some(&values),
                        index,
                    )?);
                    if previous_key != next_key {
                        return Err(JsValue::from_str(
                            "STRUCTURAL_UPDATE_REQUIRES_EXPLICIT_DELTA",
                        ));
                    }
                    self.update_typed_row_fields(
                        loop_index,
                        row_key,
                        values,
                        changed.into_keys().collect(),
                        metrics,
                    )?;
                }
                Delta::Insert {
                    row_key,
                    row,
                    before_row_key,
                    ..
                } => {
                    let values = row
                        .iter()
                        .map(|(key, value)| Ok((key.clone(), runtime_from_json(value.clone())?)))
                        .collect::<Result<HashMap<_, _>, JsValue>>()?;
                    let parent = self.parent_for_loop(loop_index)?;
                    let (position, anchor) = {
                        let rows = self.loops.entry(loop_index).or_default();
                        if rows.rows.contains_key(row_key) {
                            return Err(JsValue::from_str("duplicate row key"));
                        }
                        let position = before_row_key
                            .as_ref()
                            .map(|before| {
                                rows.order
                                    .iter()
                                    .position(|key| key == before)
                                    .ok_or_else(|| JsValue::from_str("insert anchor missing"))
                            })
                            .transpose()?
                            .unwrap_or(rows.order.len());
                        let anchor = before_row_key
                            .as_ref()
                            .and_then(|before| rows.rows.get(before))
                            .map(|row| row.root.clone());
                        rows.order.insert(position, row_key.clone());
                        (position, anchor)
                    };
                    self.insert_typed_row(
                        loop_index,
                        &parent,
                        row_key.clone(),
                        values,
                        position,
                        anchor,
                        metrics,
                    )?;
                    let keys = self
                        .loops
                        .get(&loop_index)
                        .map(|rows| rows.order.iter().skip(position + 1).cloned().collect())
                        .unwrap_or_default();
                    self.refresh_index_rows(loop_index, keys, metrics)?;
                }
                Delta::Remove { row_key, .. } => {
                    let position = self
                        .loops
                        .get(&loop_index)
                        .and_then(|rows| rows.order.iter().position(|key| key == row_key))
                        .ok_or_else(|| JsValue::from_str("row missing"))?;
                    self.remove_delta_row(loop_index, row_key, metrics)?;
                    let keys = self
                        .loops
                        .get(&loop_index)
                        .map(|rows| rows.order.iter().skip(position).cloned().collect())
                        .unwrap_or_default();
                    self.refresh_index_rows(loop_index, keys, metrics)?;
                }
                Delta::Move {
                    row_key,
                    before_row_key,
                    ..
                } => {
                    let previous = self
                        .loops
                        .get(&loop_index)
                        .and_then(|rows| rows.order.iter().position(|key| key == row_key))
                        .ok_or_else(|| JsValue::from_str("row missing"))?;
                    self.move_delta_row(loop_index, row_key, before_row_key.as_deref(), metrics)?;
                    let next = self
                        .loops
                        .get(&loop_index)
                        .and_then(|rows| rows.order.iter().position(|key| key == row_key))
                        .ok_or_else(|| JsValue::from_str("row missing"))?;
                    let (start, end) = if previous <= next {
                        (previous, next)
                    } else {
                        (next, previous)
                    };
                    let keys = self
                        .loops
                        .get(&loop_index)
                        .map(|rows| {
                            rows.order
                                .iter()
                                .skip(start)
                                .take(end - start + 1)
                                .cloned()
                                .collect()
                        })
                        .unwrap_or_default();
                    self.refresh_index_rows(loop_index, keys, metrics)?;
                }
            }
        }
        Ok(())
    }
}
