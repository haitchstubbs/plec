//! Typed route-instance state transitions (phase graphs, loaders, disposal).
//! Shared by the router (navigation) and the action/fetch VMs, so these
//! live below the router crate boundary.

use crate::prelude::*;
use crate::runtime::*;

#[derive(Clone)]
pub struct TypedLocation {
    pub pathname: String,
    pub search: String,
    pub hash: String,
}

impl RuntimeState {
    pub fn typed_outlet_element(
        &self,
        parent_id: &str,
        outlet_id: &str,
    ) -> Result<Element, JsValue> {
        let typed = self.typed.borrow();
        let parent = typed
            .get(parent_id)
            .ok_or_else(|| JsValue::from_str("typed parent graph missing"))?;
        let node = parent
            .runtime
            .app
            .route_outlets
            .iter()
            .find(|outlet| outlet.id == outlet_id)
            .map(|outlet| outlet.node)
            .ok_or_else(|| JsValue::from_str("typed route outlet is undeclared"))?;
        parent
            .runtime
            .nodes
            .get(&node)
            .cloned()
            .ok_or_else(|| JsValue::from_str("typed route outlet is not mounted"))?
            .dyn_into()
            .map_err(|_| JsValue::from_str("typed route outlet is not an element"))
    }
    pub fn dispose_typed_children(&self, parent_id: &str) -> Result<(), JsValue> {
        let children = self
            .typed
            .borrow()
            .iter()
            .filter_map(|(id, instance)| {
                (instance.parent_id.as_deref() == Some(parent_id)).then(|| id.clone())
            })
            .collect::<Vec<_>>();
        for child in children {
            self.dispose_typed_instance(&child)?;
        }
        Ok(())
    }
    pub fn dispose_typed_instance(&self, id: &str) -> Result<(), JsValue> {
        let children = self
            .typed
            .borrow()
            .iter()
            .filter_map(|(child, entry)| {
                (entry.parent_id.as_deref() == Some(id)).then(|| child.clone())
            })
            .collect::<Vec<_>>();
        for child in children {
            self.dispose_typed_instance(&child)?;
        }
        if let Some(mut instance) = self.typed.borrow_mut().remove(id) {
            instance.runtime.invalidate_fetches();
            instance.runtime.clear_listeners();
            instance.runtime.dispose_host_components();
            if let Some(mut loader) = instance.loader_runtime {
                loader.invalidate_fetches();
                loader.clear_listeners();
                loader.dispose_host_components();
            }
            if let Some(node) = instance.runtime.root.and_then(|root| root.first_child()) {
                if let Some(parent) = node.parent_node() {
                    parent.remove_child(&node)?;
                }
            }
        }
        Ok(())
    }
    pub fn run_typed_loader(
        &self,
        id: &str,
        action: usize,
        params: HashMap<String, String>,
        location: &TypedLocation,
    ) -> Result<(), JsValue> {
        #[cfg(not(feature = "fetch"))]
        {
            let _ = (id, action, params, location);
            return Err(JsValue::from_str("fetch capability is disabled"));
        }
        #[cfg(feature = "fetch")]
        {
            let pending_graph =
                {
                    let mut typed = self.typed.borrow_mut();
                    let instance = typed
                        .get_mut(id)
                        .ok_or_else(|| JsValue::from_str("typed route instance missing"))?;
                    let action_def =
                        instance.runtime.app.actions.get(action).ok_or_else(|| {
                            JsValue::from_str("typed route loader action is invalid")
                        })?;
                    let values = if action_def.parameter_slots.is_empty() {
                        Vec::new()
                    } else {
                        vec![
                            (
                                0,
                                RuntimeValue::Record(
                                    params
                                        .into_iter()
                                        .map(|(key, value)| (key, RuntimeValue::String(value)))
                                        .collect(),
                                ),
                            ),
                            (
                                1,
                                RuntimeValue::Record(HashMap::from([
                                    (
                                        "pathname".into(),
                                        RuntimeValue::String(location.pathname.clone()),
                                    ),
                                    (
                                        "search".into(),
                                        RuntimeValue::String(location.search.clone()),
                                    ),
                                    ("hash".into(), RuntimeValue::String(location.hash.clone())),
                                ])),
                            ),
                        ]
                    };
                    let mut metrics = UpdateMetrics::default();
                    instance.runtime.execute_action_with_frame(
                        action,
                        &values,
                        None,
                        None,
                        &mut metrics,
                    )?;
                    let pending = instance.runtime.take_pending_fetches();
                    let state = instance
                        .route_state
                        .as_mut()
                        .ok_or_else(|| JsValue::from_str("typed loader route state missing"))?;
                    state.params = match &values.first().map(|(_, value)| value) {
                        Some(RuntimeValue::Record(values)) => values
                            .iter()
                            .filter_map(|(key, value)| match value {
                                RuntimeValue::String(value) => Some((key.clone(), value.clone())),
                                _ => None,
                            })
                            .collect(),
                        _ => HashMap::new(),
                    };
                    state.location = (
                        location.pathname.clone(),
                        location.search.clone(),
                        location.hash.clone(),
                    );
                    state.phase = TypedRoutePhase::Loading;
                    (
                        pending,
                        (state.pending_mode != "retain")
                            .then(|| state.pending_graph_id.clone())
                            .flatten(),
                    )
                };
            if let Some(graph_id) = pending_graph.1 {
                self.show_typed_route_graph(id, &graph_id, true, None)?;
            }
            for mut request in pending_graph.0 {
                request.instance_id = id.into();
                if self.start_typed_fetch(request).is_err() {
                    self.show_typed_route_error(
                        id,
                        RuntimeValue::Record(HashMap::from([
                            ("kind".into(), RuntimeValue::String("runtime".into())),
                            (
                                "message".into(),
                                RuntimeValue::String("route loader failed".into()),
                            ),
                        ])),
                    )?;
                    break;
                }
            }
            Ok(())
        }
    }

    pub fn show_typed_route_graph(
        &self,
        id: &str,
        graph_id: &str,
        preserve_loader: bool,
        error: Option<RuntimeValue>,
    ) -> Result<(), JsValue> {
        let graph = self
            .typed_component_registry
            .borrow()
            .get(graph_id)
            .cloned()
            .ok_or_else(|| JsValue::from_str("typed route graph is not registered"))?;
        let app = graph
            .components
            .get(graph.root_component)
            .cloned()
            .ok_or_else(|| JsValue::from_str("typed route graph root is missing"))?;
        let mut next = TypedRuntime::new_with_tag_policy(
            app,
            self.region_tracker.clone(),
            self.reconcile_budget.clone(),
            self.cookie_policy.clone(),
            self.effective_tag_policy(),
        )?;
        next.set_component_definitions(graph.components);
        next.set_host_inputs(self.typed_host_inputs.borrow().clone())?;
        next.graph_generation = self.next_typed_generation();
        if let Some(error) = error {
            next.set_route_error(error)?;
        }
        let root = {
            let mut typed = self.typed.borrow_mut();
            let instance = typed
                .get_mut(id)
                .ok_or_else(|| JsValue::from_str("typed route instance missing"))?;
            instance.runtime.clear_listeners();
            instance
                .runtime
                .root
                .clone()
                .ok_or_else(|| JsValue::from_str("typed route is not mounted"))?
        };
        {
            let mut typed = self.typed.borrow_mut();
            let instance = typed
                .get_mut(id)
                .ok_or_else(|| JsValue::from_str("typed route instance missing"))?;
            instance.runtime.dispose_host_components();
        }
        next.mount(root)?;
        {
            let mut typed = self.typed.borrow_mut();
            let instance = typed
                .get_mut(id)
                .ok_or_else(|| JsValue::from_str("typed route instance missing"))?;
            let old = std::mem::replace(&mut instance.runtime, next);
            instance.graph_id = graph_id.into();
            if preserve_loader {
                instance.loader_runtime = Some(old);
            } else {
                let mut old = old;
                old.invalidate_fetches();
            }
        }
        // Phase graphs can start with a component call (for example, the shared
        // page frame), so drain the new runtime's component requests after it is
        // registered just like an initial graph mount.
        self.mount_component_requests()?;
        self.install_typed_event_listeners()
    }

    pub fn retry_typed_route(&self, id: &str) -> Result<(), JsValue> {
        let (graph_id, action, params, location, phase) = {
            let typed = self.typed.borrow();
            let instance = typed
                .get(id)
                .ok_or_else(|| JsValue::from_str("typed route instance missing"))?;
            let state = instance
                .route_state
                .as_ref()
                .ok_or_else(|| JsValue::from_str("typed event is not a route"))?;
            (
                state.normal_graph_id.clone(),
                state.loader_action,
                state.params.clone(),
                TypedLocation {
                    pathname: state.location.0.clone(),
                    search: state.location.1.clone(),
                    hash: state.location.2.clone(),
                },
                state.phase == TypedRoutePhase::Error,
            )
        };
        if !phase {
            return Ok(());
        }
        self.show_typed_route_graph(id, &graph_id, false, None)?;
        if let Some(action) = action {
            self.run_typed_loader(id, action, params, &location)?;
        }
        Ok(())
    }

    pub fn restore_typed_route_normal(&self, id: &str) -> Result<(), JsValue> {
        {
            let mut typed = self.typed.borrow_mut();
            let instance = typed
                .get_mut(id)
                .ok_or_else(|| JsValue::from_str("typed route instance missing"))?;
            let mut loader = instance
                .loader_runtime
                .take()
                .ok_or_else(|| JsValue::from_str("typed route loader runtime missing"))?;
            let root = instance
                .runtime
                .root
                .clone()
                .ok_or_else(|| JsValue::from_str("typed route is not mounted"))?;
            instance.runtime.clear_listeners();
            // Loader data is a host input: state initializers such as
            // loadHost("loaderData") only re-evaluate when host inputs are
            // (re)applied, so seed the graph before its first paint.
            loader.set_host_inputs(self.typed_host_inputs.borrow().clone())?;
            loader.mount(root)?;
            let mut previous = std::mem::replace(&mut instance.runtime, loader);
            previous.invalidate_fetches();
            previous.dispose_host_components();
            instance.graph_id = instance
                .route_state
                .as_ref()
                .unwrap()
                .normal_graph_id
                .clone();
            instance.route_state.as_mut().unwrap().phase = TypedRoutePhase::Normal;
        }
        self.mount_component_requests()?;
        Ok(())
    }

    pub fn show_typed_route_error(&self, id: &str, error: RuntimeValue) -> Result<(), JsValue> {
        let graph_id = {
            let mut typed = self.typed.borrow_mut();
            let instance = typed
                .get_mut(id)
                .ok_or_else(|| JsValue::from_str("typed route instance missing"))?;
            if let Some(mut loader) = instance.loader_runtime.take() {
                loader.invalidate_fetches();
                loader.clear_listeners();
                loader.dispose_host_components();
            }
            let state = instance
                .route_state
                .as_mut()
                .ok_or_else(|| JsValue::from_str("typed loader route state missing"))?;
            state.phase = TypedRoutePhase::Error;
            state.error_graph_id.clone()
        };
        if let Some(graph_id) = graph_id {
            self.show_typed_route_graph(id, &graph_id, false, Some(normalize_route_error(error)))?;
        }
        Ok(())
    }
}

pub fn normalize_route_error(error: RuntimeValue) -> RuntimeValue {
    let RuntimeValue::Record(record) = error else {
        return RuntimeValue::Record(HashMap::from([
            ("kind".into(), RuntimeValue::String("runtime".into())),
            (
                "message".into(),
                RuntimeValue::String("route loader failed".into()),
            ),
        ]));
    };
    let Some(RuntimeValue::String(kind)) = record.get("kind") else {
        return RuntimeValue::Record(HashMap::from([
            ("kind".into(), RuntimeValue::String("runtime".into())),
            (
                "message".into(),
                RuntimeValue::String("route loader failed".into()),
            ),
        ]));
    };
    if !["http", "network", "abort", "decode"].contains(&kind.as_str()) {
        return RuntimeValue::Record(HashMap::from([
            ("kind".into(), RuntimeValue::String("runtime".into())),
            (
                "message".into(),
                RuntimeValue::String("route loader failed".into()),
            ),
        ]));
    }
    let mut normalized = HashMap::from([
        ("kind".into(), RuntimeValue::String(kind.clone())),
        (
            "message".into(),
            record
                .get("message")
                .cloned()
                .unwrap_or_else(|| RuntimeValue::String("route loader failed".into())),
        ),
    ]);
    for field in ["status", "statusText", "body", "url"] {
        if let Some(value) = record.get(field) {
            normalized.insert(field.into(), value.clone());
        }
    }
    RuntimeValue::Record(normalized)
}

pub fn typed_location(href: &str) -> TypedLocation {
    let (before_hash, hash) = href
        .split_once('#')
        .map_or((href, ""), |(path, hash)| (path, hash));
    let (pathname, search) = before_hash
        .split_once('?')
        .map_or((before_hash, ""), |(path, search)| (path, search));
    TypedLocation {
        pathname: if pathname.is_empty() {
            "/".into()
        } else {
            pathname.into()
        },
        search: if search.is_empty() {
            "".into()
        } else {
            format!("?{search}")
        },
        hash: if hash.is_empty() {
            "".into()
        } else {
            format!("#{hash}")
        },
    }
}

pub fn ssr_snapshot_value(value: &plec_ir::SsrSnapshotValue) -> RuntimeValue {
    match value {
        plec_ir::SsrSnapshotValue::Null => RuntimeValue::Null,
        plec_ir::SsrSnapshotValue::Bool(value) => RuntimeValue::Bool(*value),
        plec_ir::SsrSnapshotValue::Number(value) => RuntimeValue::Number(*value),
        plec_ir::SsrSnapshotValue::String(value) => RuntimeValue::String(value.clone()),
        plec_ir::SsrSnapshotValue::Array(values) => {
            RuntimeValue::Array(values.iter().map(ssr_snapshot_value).collect())
        }
        plec_ir::SsrSnapshotValue::Record(values) => RuntimeValue::Record(
            values
                .iter()
                .map(|(key, value)| (key.clone(), ssr_snapshot_value(value)))
                .collect(),
        ),
    }
}
