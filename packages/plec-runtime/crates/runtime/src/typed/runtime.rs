use crate::dom::{bindings::*, platform::*};
use crate::eval::typed_vm::*;
use crate::runtime::lifecycle::*;
use crate::schema::typed::TypedComponentProp;
use crate::typed::cookie::*;
use crate::typed::events::*;
#[cfg(feature = "fetch")]
use crate::typed::fetch::*;

#[derive(Default)]
pub(crate) struct TypedLoopRows {
    pub(crate) order: Vec<String>,
    pub(crate) rows: HashMap<String, TypedRow>,
}

pub(crate) struct TypedRow {
    pub(crate) root: Node,
    pub(crate) end: Option<Node>,
    pub(crate) values: HashMap<String, RuntimeValue>,
    pub(crate) nodes: HashMap<usize, Node>,
    /// Concrete, row-owned conditional regions. A branch change must never
    /// replace the keyed row which contains it.
    pub(crate) conditionals: HashMap<usize, TypedConditionalRegion>,
    pub(crate) generation: u64,
}

/// Mutable ownership for a static conditional branch. The graph node table is
/// immutable; this region owns the concrete nodes that exist for one branch.
pub(crate) struct TypedConditionalRegion {
    pub(crate) start: Node,
    pub(crate) end: Node,
    pub(crate) selected: Option<usize>,
    pub(crate) nodes: HashMap<usize, Node>,
    pub(crate) generation: u64,
}

/// A listener is requested as part of creating a concrete node/region. The
/// public wrapper drains this list once mutable DOM work has released its
/// borrow; it is intentionally not a recovery scan of the whole graph.
pub(crate) struct TypedListenerRequest {
    pub(crate) target: usize,
    pub(crate) node: Node,
    pub(crate) owner: TypedListenerOwner,
}

pub(crate) struct TypedComponentRequest {
    pub(crate) call: usize,
    pub(crate) component: usize,
    pub(crate) props: HashMap<String, RuntimeValue>,
    pub(crate) callbacks: HashMap<String, TypedCallbackSpec>,
    pub(crate) parent: Node,
    pub(crate) start: Node,
    pub(crate) end: Node,
    pub(crate) key: String,
}

#[derive(Clone)]
pub(crate) struct TypedCallback {
    pub(crate) parent_id: String,
    pub(crate) action: usize,
    pub(crate) row: Option<HashMap<String, RuntimeValue>>,
}

#[derive(Clone)]
pub(crate) struct TypedCallbackSpec {
    pub(crate) action: usize,
    pub(crate) row: Option<HashMap<String, RuntimeValue>>,
}

pub(crate) struct TypedComponentRefresh {
    pub(crate) call: usize,
    pub(crate) start: Node,
    pub(crate) props: HashMap<String, RuntimeValue>,
}

pub(crate) struct TypedRuntime {
    pub(crate) app: TypedApplication,
    pub(crate) root: Option<Element>,
    pub(crate) nodes: HashMap<usize, Node>,
    pub(crate) states: Vec<RuntimeValue>,
    pub(crate) collections: HashMap<usize, TypedCollection>,
    pub(crate) loops: HashMap<usize, TypedLoopRows>,
    pub(crate) conditionals: HashMap<usize, TypedConditionalRegion>,
    pub(crate) listeners: Vec<TypedListener>,
    pub(crate) listener_requests: Vec<TypedListenerRequest>,
    pub(crate) component_requests: Vec<TypedComponentRequest>,
    pub(crate) component_refreshes: Vec<TypedComponentRefresh>,
    pub(crate) callback_requests: Vec<TypedCallback>,
    pub(crate) callbacks: Vec<Option<TypedCallback>>,
    pub(crate) next_generation: u64,
    pub(crate) graph_generation: u64,
    pub(crate) host_inputs: HashMap<String, RuntimeValue>,
    pub(crate) host_refs: HashMap<String, Node>,
    pub(crate) pending_cookies: Vec<TypedPendingCookie>,
    pub(crate) next_cookie_id: u64,
    pub(crate) next_component_instance: u64,
    #[cfg(feature = "fetch")]
    pub(crate) pending_fetches: Vec<TypedPendingFetch>,
    #[cfg(feature = "fetch")]
    pub(crate) next_fetch_id: u64,
    #[cfg(feature = "fetch")]
    pub(crate) abort_controllers: HashMap<u64, AbortController>,
}

/** Mutable typed graph ownership. Definitions live in `typed_registry`; this
 * record exists only for one mounted route position. */
pub(crate) struct TypedGraphInstance {
    pub(crate) parent_id: Option<String>,
    pub(crate) outlet_id: String,
    pub(crate) graph_id: String,
    pub(crate) route_id: Option<String>,
    pub(crate) match_key: Option<String>,
    pub(crate) route_state: Option<TypedRouteState>,
    /// The normal graph remains alive only while its route loader is pending.
    pub(crate) loader_runtime: Option<TypedRuntime>,
    pub(crate) component_call: Option<usize>,
    pub(crate) component_start: Option<Node>,
    pub(crate) runtime: TypedRuntime,
}

impl TypedGraphInstance {
    pub(crate) fn runtime_for_generation(&self, generation: u64) -> Option<&TypedRuntime> {
        if self.runtime.graph_generation == generation {
            Some(&self.runtime)
        } else {
            self.loader_runtime
                .as_ref()
                .filter(|runtime| runtime.graph_generation == generation)
        }
    }

    pub(crate) fn runtime_for_generation_mut(
        &mut self,
        generation: u64,
    ) -> Option<&mut TypedRuntime> {
        if self.runtime.graph_generation == generation {
            Some(&mut self.runtime)
        } else {
            self.loader_runtime
                .as_mut()
                .filter(|runtime| runtime.graph_generation == generation)
        }
    }
}

pub(crate) struct TypedRouteState {
    pub(crate) normal_graph_id: String,
    pub(crate) pending_graph_id: Option<String>,
    pub(crate) error_graph_id: Option<String>,
    pub(crate) loader_action: Option<usize>,
    pub(crate) params: HashMap<String, String>,
    pub(crate) location: (String, String, String),
    pub(crate) phase: TypedRoutePhase,
}

#[derive(PartialEq, Eq)]
pub(crate) enum TypedRoutePhase {
    Normal,
    Loading,
    Error,
}

#[wasm_bindgen::prelude::wasm_bindgen]
impl PlecRuntime {
    #[wasm_bindgen(constructor)]
    pub fn new() -> PlecRuntime {
        PlecRuntime {
            registry: Rc::new(RefCell::new(HashMap::new())),
            instances: Rc::new(RefCell::new(HashMap::new())),
            router: Rc::new(RefCell::new(None)),
            router_listeners: Rc::new(RefCell::new(Vec::new())),
            typed: Rc::new(RefCell::new(HashMap::new())),
            typed_root: Rc::new(RefCell::new(None)),
            typed_generation: Rc::new(RefCell::new(0)),
            typed_registry: Rc::new(RefCell::new(HashMap::new())),
            typed_manifest: Rc::new(RefCell::new(None)),
            typed_host_inputs: Rc::new(RefCell::new(HashMap::new())),
            typed_components: Rc::new(RefCell::new(None)),
        }
    }
}

#[wasm_bindgen::prelude::wasm_bindgen]
impl PlecRuntime {
    pub fn mount(&self, root: Element) -> Result<JsValue, JsValue> {
        if !self.typed.borrow().is_empty() {
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
}

#[wasm_bindgen::prelude::wasm_bindgen]
impl PlecRuntime {
    pub fn apply_delta(&self, delta: JsValue) -> Result<JsValue, JsValue> {
        if !self.typed.borrow().is_empty() {
            return self.apply_typed_delta(delta);
        }
        let instance_id = self.legacy_instance_id()?;
        self.apply_delta_for(&instance_id, delta)
    }
}

impl PlecRuntime {
    pub(crate) fn mount_typed(&self, root: Element) -> Result<JsValue, JsValue> {
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
        serde_wasm_bindgen::to_value(&metrics).map_err(error)
    }
}

impl PlecRuntime {
    pub(crate) fn flush_component_work(&self) -> Result<(), JsValue> {
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
                return Ok(());
            }
            for (parent, refresh) in refreshes {
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
                    let values = instance
                        .runtime
                        .app
                        .parameters
                        .iter()
                        .map(|parameter| {
                            let name = instance
                                .runtime
                                .app
                                .strings
                                .get(parameter.name)
                                .ok_or_else(|| {
                                    JsValue::from_str("component parameter handle out of range")
                                })?;
                            Ok(refresh
                                .props
                                .get(name)
                                .cloned()
                                .unwrap_or(RuntimeValue::Null))
                        })
                        .collect::<Result<Vec<_>, JsValue>>()?;
                    let changed = instance.runtime.app.runtime_props != values;
                    instance.runtime.app.runtime_props = values;
                    if changed {
                        let mut metrics = UpdateMetrics::default();
                        for prop in 0..instance.runtime.app.parameters.len() {
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
                }
            }
        }
    }

    pub(crate) fn mount_component_requests(&self) -> Result<(), JsValue> {
        let definitions = self.typed_components.borrow().clone();
        let Some(definitions) = definitions else {
            return Ok(());
        };
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
                let mut app = definitions
                    .components
                    .get(request.component)
                    .ok_or_else(|| JsValue::from_str("component target out of range"))?
                    .clone();
                app.runtime_props = app
                    .parameters
                    .iter()
                    .map(|parameter| {
                        let name = app.strings.get(parameter.name).ok_or_else(|| {
                            JsValue::from_str("component parameter handle out of range")
                        })?;
                        Ok(request
                            .props
                            .get(name)
                            .cloned()
                            .unwrap_or(RuntimeValue::Null))
                    })
                    .collect::<Result<Vec<_>, JsValue>>()?;
                let mut runtime = TypedRuntime::new(app)?;
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
                        }))
                    })
                    .collect::<Result<Vec<_>, JsValue>>()?;
                runtime.set_host_inputs(self.typed_host_inputs.borrow().clone())?;
                runtime.graph_generation = self.next_typed_generation();
                runtime.mount_before(&request.parent, &request.end)?;
                let id = format!("{parent_id}/component:{}:{}", request.call, request.key);
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

impl PlecRuntime {
    pub(crate) fn initialize_typed_input(
        &self,
        input_id: &str,
        rows: JsValue,
    ) -> Result<JsValue, JsValue> {
        if self.typed_components.borrow().is_some() {
            let rows: Vec<Value> = serde_wasm_bindgen::from_value(rows).map_err(error)?;
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

impl PlecRuntime {
    pub(crate) fn initialize_typed_input_for(
        &self,
        id: &str,
        input_id: &str,
        rows: JsValue,
    ) -> Result<JsValue, JsValue> {
        let rows: Vec<Value> = serde_wasm_bindgen::from_value(rows).map_err(error)?;
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

impl PlecRuntime {
    pub(crate) fn apply_typed_delta(&self, delta: JsValue) -> Result<JsValue, JsValue> {
        let delta: Delta = serde_wasm_bindgen::from_value(delta).map_err(error)?;
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
        let mut metrics = UpdateMetrics::default();
        for id in ids {
            self.typed
                .borrow_mut()
                .get_mut(&id)
                .ok_or_else(|| JsValue::from_str("typed application missing"))?
                .runtime
                .apply_delta(delta.clone(), &mut metrics)?;
        }
        self.flush_component_work()?;
        self.install_typed_event_listeners()?;
        serde_wasm_bindgen::to_value(&metrics).map_err(error)
    }
}

impl TypedRuntime {
    pub(crate) fn set_route_error(&mut self, error: RuntimeValue) -> Result<(), JsValue> {
        let state = self
            .app
            .route_error_state
            .ok_or_else(|| JsValue::from_str("typed error graph has no route error state"))?;
        self.states[state] = error;
        Ok(())
    }

    pub(crate) fn set_host_inputs(
        &mut self,
        inputs: HashMap<String, RuntimeValue>,
    ) -> Result<(), JsValue> {
        self.host_inputs = inputs;
        self.app.host_inputs = self.host_inputs.clone();
        self.states = self
            .app
            .state_slots
            .iter()
            .map(|slot| typed_eval(&self.app, slot.initial_expression, &[], None, 0))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(())
    }
    pub(crate) fn new(app: TypedApplication) -> Result<Self, JsValue> {
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
            conditionals: HashMap::new(),
            collections: HashMap::new(),
            listeners: Vec::new(),
            listener_requests: Vec::new(),
            component_requests: Vec::new(),
            component_refreshes: Vec::new(),
            callback_requests: Vec::new(),
            callbacks: Vec::new(),
            next_generation: 1,
            graph_generation: 1,
            host_inputs: HashMap::new(),
            host_refs: HashMap::new(),
            pending_cookies: Vec::new(),
            next_cookie_id: 0,
            next_component_instance: 0,
            #[cfg(feature = "fetch")]
            pending_fetches: Vec::new(),
            #[cfg(feature = "fetch")]
            next_fetch_id: 0,
            #[cfg(feature = "fetch")]
            abort_controllers: HashMap::new(),
        })
    }
}

#[cfg(not(feature = "fetch"))]
impl TypedRuntime {
    pub(crate) fn take_pending_fetches(&mut self) -> Vec<()> {
        Vec::new()
    }
}

impl TypedRuntime {
    fn queue_component_refreshes(
        &mut self,
        nodes: Vec<(usize, Node)>,
        row: Option<&HashMap<String, RuntimeValue>>,
    ) -> Result<(), JsValue> {
        for (call, start) in nodes {
            let Some(TypedNode::Component { props, .. }) = self.app.nodes.get(call).cloned() else {
                continue;
            };
            let props = props
                .into_iter()
                .filter_map(|prop| match prop {
                    TypedComponentProp::Value { name, expression } => Some((name, expression)),
                    TypedComponentProp::Callable { .. } => None,
                })
                .map(|(name, expression)| {
                    let name = self
                        .app
                        .strings
                        .get(name)
                        .ok_or_else(|| JsValue::from_str("component prop name out of range"))?
                        .clone();
                    let value = typed_eval(&self.app, expression, &self.states, row, 0)?;
                    Ok((name, value))
                })
                .collect::<Result<HashMap<_, _>, JsValue>>()?;
            self.component_refreshes
                .push(TypedComponentRefresh { call, start, props });
        }
        Ok(())
    }

    pub(crate) fn queue_static_component_refreshes(&mut self) -> Result<(), JsValue> {
        self.queue_component_refreshes(
            self.nodes
                .iter()
                .map(|(call, node)| (*call, node.clone()))
                .collect(),
            None,
        )
    }

    fn queue_row_component_refreshes(
        &mut self,
        nodes: HashMap<usize, Node>,
        values: &HashMap<String, RuntimeValue>,
    ) -> Result<(), JsValue> {
        self.queue_component_refreshes(nodes.into_iter().collect(), Some(values))
    }

    pub(crate) fn mount(&mut self, root: Element) -> Result<MountMetrics, JsValue> {
        self.invalidate_fetches();
        self.clear_listeners();
        root.set_inner_html("");
        self.nodes.clear();
        self.loops.clear();
        self.conditionals.clear();
        self.listener_requests.clear();
        let doc = document()?;
        let node = self.instantiate_node(
            &doc,
            self.app.root_node,
            None,
            None,
            0,
            &mut HashMap::new(),
            &mut HashMap::new(),
        )?;
        root.append_child(&node)?;
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
}

impl TypedRuntime {
    pub(crate) fn mount_before(&mut self, parent: &Node, end: &Node) -> Result<(), JsValue> {
        let doc = document()?;
        let node = self.instantiate_node(
            &doc,
            self.app.root_node,
            None,
            None,
            0,
            &mut HashMap::new(),
            &mut HashMap::new(),
        )?;
        parent.insert_before(&node, Some(end))?;
        self.apply_static_bindings()?;
        self.queue_static_listeners();
        Ok(())
    }
}

impl TypedRuntime {
    /// Invalidation happens before aborting so graph disposal is silent: a
    /// rejected browser promise can never resume stale action code.
    pub(crate) fn invalidate_fetches(&mut self) {
        self.graph_generation = self.graph_generation.saturating_add(1);
        #[cfg(feature = "fetch")]
        {
            self.pending_fetches.clear();
            for (_, controller) in self.abort_controllers.drain() {
                controller.abort();
            }
        }
    }
}

impl TypedRuntime {
    pub(crate) fn clear_listeners(&mut self) {
        for listener in self.listeners.drain(..) {
            let listener = listener.listener;
            let _ = listener.element.remove_event_listener_with_callback(
                &listener.event_type,
                listener.callback.as_ref().unchecked_ref(),
            );
        }
        self.listener_requests.clear();
    }
}

impl TypedRuntime {
    pub(crate) fn dispose_region_listeners(
        &mut self,
        loop_index: usize,
        key: &str,
        generation: u64,
    ) {
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

    pub(crate) fn dispose_owner_listeners(&mut self, owner: &TypedListenerOwner) {
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

    pub(crate) fn queue_listener(&mut self, target: usize, node: Node, owner: TypedListenerOwner) {
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
        let nodes = self
            .nodes
            .iter()
            .map(|(target, node)| (*target, node.clone()))
            .collect::<Vec<_>>();
        for (target, node) in nodes {
            self.queue_listener(target, node, TypedListenerOwner::Static);
        }
    }

    pub(crate) fn queue_row_listeners(&mut self, loop_index: usize, key: &str) {
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
    pub(crate) fn instantiate_node(
        &mut self,
        doc: &Document,
        index: usize,
        parent: Option<&Node>,
        row: Option<&HashMap<String, RuntimeValue>>,
        row_index: usize,
        local: &mut HashMap<usize, Node>,
        row_regions: &mut HashMap<usize, TypedConditionalRegion>,
    ) -> Result<Node, JsValue> {
        match self
            .app
            .nodes
            .get(index)
            .ok_or_else(|| JsValue::from_str("node handle out of range"))?
            .clone()
        {
            TypedNode::Element { tag, children, .. } => {
                let tag = self
                    .app
                    .strings
                    .get(tag)
                    .ok_or_else(|| JsValue::from_str("tag handle out of range"))?;
                let element = if [
                    "svg", "path", "circle", "rect", "line", "polyline", "polygon", "ellipse", "g",
                ]
                .contains(&tag.as_str())
                {
                    doc.create_element_ns(Some("http://www.w3.org/2000/svg"), tag)?
                } else {
                    doc.create_element(tag)?
                };
                element.set_attribute("data-runtime-node", &index.to_string())?;
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
                        local,
                        row_regions,
                    )?;
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
            TypedNode::Component {
                component, props, ..
            } => {
                let parent = parent.ok_or_else(|| JsValue::from_str("component parent missing"))?;
                let start: Node = doc
                    .create_comment(&format!("plec:component:{index}"))
                    .into();
                let end: Node = doc
                    .create_comment(&format!("plec:component-end:{index}"))
                    .into();
                parent.append_child(&start)?;
                parent.append_child(&end)?;
                let mut values = HashMap::new();
                let mut callbacks = HashMap::new();
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
                                typed_eval(&self.app, expression, &self.states, row, row_index)?,
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
                    }
                }
                let key = format!("{index}:{}", self.next_component_instance);
                self.next_component_instance += 1;
                self.component_requests.push(TypedComponentRequest {
                    call: index,
                    component,
                    props: values,
                    callbacks,
                    parent: parent.clone(),
                    start: start.clone(),
                    end,
                    key,
                });
                if row.is_some() {
                    local.insert(index, start.clone());
                } else {
                    self.nodes.insert(index, start.clone());
                }
                Ok(start)
            }
            TypedNode::Conditional { test, .. } => {
                let parent =
                    parent.ok_or_else(|| JsValue::from_str("conditional parent missing"))?;
                let start: Node = doc
                    .create_comment(&format!("plec:conditional:{index}"))
                    .into();
                let end: Node = doc
                    .create_comment(&format!("plec:conditional-end:{index}"))
                    .into();
                parent.append_child(&start)?;
                parent.append_child(&end)?;
                let mut row_selected = None;
                if row.is_some() {
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
                        },
                    );
                    self.reconcile_static_conditional(index, &mut UpdateMetrics::default())?;
                }
                if row.is_some() {
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
                        },
                    );
                    self.next_generation += 1;
                }
                Ok(start)
            }
        }
    }
}

impl TypedRuntime {
    pub(crate) fn reconcile_static_conditional(
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
        let selected = if typed_truthy(&typed_eval(&self.app, test, &self.states, None, 0)?) {
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
        for node in region.nodes.values() {
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
                &mut HashMap::new(),
                &mut HashMap::new(),
            )?;
            parent.insert_before(&child, Some(&region.end))?;
            self.collect_branch_nodes(selected, &mut region.nodes);
            self.apply_bindings_to_nodes(&region.nodes, None, metrics)?;
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
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        for binding in &self.app.bindings {
            if let Some(node) = nodes.get(&binding.target) {
                typed_apply_binding(&self.app, binding, node, &self.states, row, 0)?;
                metrics.bindings_touched += 1;
            }
        }
        for program in &self.app.prop_programs {
            let Some(node) = nodes.get(&program.target) else {
                continue;
            };
            for write in &program.writes {
                let value = match write.expression {
                    Some(expression) => typed_eval(&self.app, expression, &self.states, row, 0)?,
                    None => write
                        .constant
                        .and_then(|index| self.app.constants.get(index))
                        .cloned()
                        .unwrap_or_default(),
                };
                typed_apply_value(&self.app, &write.kind, Some(write.name), node, value)?;
            }
        }
        Ok(())
    }
}

impl TypedRuntime {
    pub(crate) fn render_loop(&mut self, loop_index: usize, parent: &Node) -> Result<(), JsValue> {
        let loop_def = self
            .app
            .loops
            .get(loop_index)
            .ok_or_else(|| JsValue::from_str("loop handle out of range"))?
            .clone();
        let values = typed_eval(&self.app, loop_def.source_expression, &self.states, None, 0)?;
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
    pub(crate) fn reconcile_input(
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
                let row = runtime_from_json(value.clone())?
                    .record()
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
}

impl TypedRuntime {
    pub(crate) fn parent_for_loop(&self, loop_index: usize) -> Result<Node, JsValue> {
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

    pub(crate) fn reconcile_loop(
        &mut self,
        loop_index: usize,
        parent: &Node,
        projection: Vec<(String, HashMap<String, RuntimeValue>)>,
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
                self.dispose_region_listeners(loop_index, &key, row.generation);
                if let Some(parent) = row.root.parent_node() {
                    for node in Self::row_dom_nodes(&row.root, row.end.as_ref()) {
                        parent.remove_child(&node)?;
                        metrics.dom_operations += 1;
                    }
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
                    self.update_typed_row(loop_index, &key, values, None, metrics)?;
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
                    .flat_map(|key| {
                        rows.rows
                            .get(key)
                            .map(|row| Self::row_dom_nodes(&row.root, row.end.as_ref()))
                            .unwrap_or_default()
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for root in roots {
            parent.append_child(&root)?;
        }
        self.loops.entry(loop_index).or_default().order = desired;
        Ok(())
    }
}

impl TypedRuntime {
    pub(crate) fn insert_typed_row(
        &mut self,
        loop_index: usize,
        parent: &Node,
        key: String,
        values: HashMap<String, RuntimeValue>,
        index: usize,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let template = self.app.loops[loop_index].row_template;
        let mut nodes = HashMap::new();
        let doc = document()?;
        let mut conditionals = HashMap::new();
        let root = self.instantiate_node(
            &doc,
            template,
            Some(parent),
            Some(&values),
            index,
            &mut nodes,
            &mut conditionals,
        )?;
        let end = matches!(self.app.nodes[template], TypedNode::Component { .. })
            .then(|| root.next_sibling())
            .flatten();
        if let Ok(element) = root.clone().dyn_into::<Element>() {
            element.set_attribute("data-runtime-row-key", &key)?;
        }
        let _conditional_selections = self.row_conditional_selections(template, &values, index)?;
        let generation = self.next_generation;
        self.next_generation += 1;
        self.loops.entry(loop_index).or_default().rows.insert(
            key.clone(),
            TypedRow {
                root,
                end,
                values: values.clone(),
                nodes,
                conditionals,
                generation,
            },
        );
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
        Ok(())
    }
}

impl TypedRuntime {
    pub(crate) fn update_typed_row(
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
                        Some(expression) => {
                            typed_eval(&self.app, expression, &self.states, Some(&row.values), 0)?
                        }
                        None => write
                            .constant
                            .and_then(|index| self.app.constants.get(index))
                            .cloned()
                            .unwrap_or_default(),
                    };
                    typed_apply_value(&self.app, &write.kind, Some(write.name), node, value)?;
                }
            }
        }
        let (nodes, values) = self
            .loops
            .get(&loop_index)
            .and_then(|rows| rows.rows.get(key))
            .map(|row| (row.nodes.clone(), row.values.clone()))
            .ok_or_else(|| JsValue::from_str("row missing"))?;
        self.queue_row_component_refreshes(nodes, &values)?;
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
            if current == Some(next) {
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
                let child = self.instantiate_node(
                    &document()?,
                    branch,
                    Some(&parent),
                    Some(values),
                    row_index,
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
            Some(TypedNode::Conditional {
                test,
                consequent,
                alternate,
                ..
            }) => {
                let selected = if typed_truthy(&typed_eval(
                    &self.app,
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
    pub(crate) fn apply_static_bindings(&mut self) -> Result<(), JsValue> {
        for binding in self.app.bindings.clone() {
            if let Some(node) = self.nodes.get(&binding.target) {
                typed_apply_binding(&self.app, &binding, node, &self.states, None, 0)?;
            }
        }
        for program in self.app.prop_programs.clone() {
            let Some(node) = self.nodes.get(&program.target) else {
                continue;
            };
            for write in program.writes {
                let value = match write.expression {
                    Some(expression) => typed_eval(&self.app, expression, &self.states, None, 0)?,
                    None => write
                        .constant
                        .and_then(|index| self.app.constants.get(index))
                        .cloned()
                        .unwrap_or_default(),
                };
                typed_apply_value(&self.app, &write.kind, Some(write.name), node, value)?;
            }
        }
        Ok(())
    }
}

impl TypedRuntime {
    pub(crate) fn apply_delta(
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
                    row.extend(
                        changes
                            .clone()
                            .into_iter()
                            .map(|(key, value)| Ok((key, runtime_from_json(value)?)))
                            .collect::<Result<HashMap<_, _>, JsValue>>()?,
                    );
                }
                Delta::Insert {
                    row_key,
                    row,
                    before_row_key,
                    ..
                } => {
                    values.insert(
                        row_key.clone(),
                        row.clone()
                            .into_iter()
                            .map(|(key, value)| Ok((key, runtime_from_json(value)?)))
                            .collect::<Result<HashMap<_, _>, JsValue>>()?,
                    );
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
