use crate::dom::platform::*;
use crate::runtime::lifecycle::*;
use crate::schema::delta::RuntimeValue;
use crate::schema::routing::RouteManifestEntry;
use crate::typed::runtime::*;
use js_sys::{Object, Reflect};
use std::collections::HashMap;
use web_sys::{CustomEvent, CustomEventInit};

#[derive(Clone)]
struct TypedRouteMatch {
    route: RouteManifestEntry,
    params: HashMap<String, String>,
}

#[derive(Clone)]
struct TypedLocation {
    pathname: String,
    search: String,
    hash: String,
}

impl PlecRuntime {
    pub(crate) fn adopt_typed_route(&self, href: &str, root: Element) -> Result<(), JsValue> {
        let manifest = self
            .typed_manifest
            .borrow()
            .clone()
            .ok_or_else(|| JsValue::from_str("missing:ssr-manifest"))?;
        let location = typed_location(href);
        if !self.has_typed_graph(&manifest.root_graph_id) {
            return Err(JsValue::from_str("missing:ssr-root-graph"));
        }
        let root_id = graph_instance_id(None, "main", None);
        self.adopt_typed_graph(
            root_id.clone(),
            None,
            "main".into(),
            manifest.root_graph_id.clone(),
            None,
            None,
            root,
            "root".into(),
        )?;
        let mut parent_id = root_id;
        let mut path = "root".to_owned();
        let derived_chain = typed_route_chain(&manifest, &location.pathname);
        // The transferred snapshot chain is the route execution cause. The
        // runtime re-derives the chain from the URL with the same matcher used
        // for fresh navigation; agreement lets the imported identity proceed,
        // disagreement is a contract failure, never a silent remount.
        if let Some(imported) = self.typed_ssr_route_chain.borrow().as_ref() {
            let loaders = self.typed_ssr_loaders.borrow();
            if let Some(detail) = ssr_route_chain_mismatch(imported, &derived_chain, &loaders) {
                return Err(JsValue::from_str(&format!(
                    "mismatch:ssr-route-chain:{detail}"
                )));
            }
        }
        for matched in derived_chain.into_iter() {
            let route = matched.route;
            if !self.has_typed_graph(&route.graph_id) {
                return Err(JsValue::from_str("missing:ssr-route-graph"));
            }
            let match_key = typed_match_key(&route.id, &matched.params, &location);
            let id = graph_instance_id(Some(&parent_id), &route.outlet_id, None);
            let outlet = self.typed_outlet_element(&parent_id, &route.outlet_id)?;
            let child_path = format!("{path}/outlet:{}", route.outlet_id);
            // Loader routes resume from the transferred outcome instead of
            // re-running on first paint. The chain check above guarantees the
            // snapshot phase pairs with the outcome (`active` + resolved,
            // `error` + rejected) and that a loader route imports an outcome.
            let loader = route
                .loader_action
                .map(|action| {
                    let reference = plec_ir::loader_ref(&route.graph_id, action);
                    let outcome = self
                        .typed_ssr_loaders
                        .borrow()
                        .get(&reference)
                        .cloned()
                        .ok_or_else(|| {
                            JsValue::from_str(&format!("mismatch:ssr-loader:{reference}"))
                        })?;
                    Ok::<_, JsValue>((action, outcome))
                })
                .transpose()?;
            if let Some((action, plec_ir::SsrLoaderState::Rejected { message })) = loader {
                let reference = plec_ir::loader_ref(&route.graph_id, action);
                let error_graph_id = route.error_graph_id.clone().ok_or_else(|| {
                    JsValue::from_str(&format!("mismatch:ssr-loader-error-graph:{reference}"))
                })?;
                if !self.has_typed_graph(&error_graph_id) {
                    return Err(JsValue::from_str("missing:ssr-route-error-graph"));
                }
                // The server rendered the error phase, so the SSR DOM belongs
                // to the error graph: claim it there, then restore the
                // recorded error so the phase matches a client-side failure
                // and `retry_typed_route` re-enters the loader.
                self.adopt_typed_graph(
                    id.clone(),
                    Some(parent_id.clone()),
                    route.outlet_id.clone(),
                    error_graph_id,
                    Some(route.id.clone()),
                    Some(match_key),
                    outlet,
                    child_path.clone(),
                )?;
                let mut typed = self.typed.borrow_mut();
                let instance = typed.get_mut(&id).expect("adopted route exists");
                instance.route_state = Some(TypedRouteState {
                    normal_graph_id: route.graph_id,
                    pending_graph_id: route.pending_graph_id,
                    pending_mode: route.pending_mode,
                    error_graph_id: route.error_graph_id,
                    loader_action: Some(action),
                    params: matched.params,
                    location: (
                        location.pathname.clone(),
                        location.search.clone(),
                        location.hash.clone(),
                    ),
                    phase: TypedRoutePhase::Error,
                });
                instance
                    .runtime
                    .set_route_error(ssr_loader_error(&message))?;
                instance.runtime.apply_static_bindings()?;
                drop(typed);
            } else {
                if let Some((_, plec_ir::SsrLoaderState::Resolved { value })) = &loader {
                    // Loader data is a host input: seed it before adoption so
                    // `loadHost("loaderData")` state initializers evaluate
                    // from the imported outcome instead of blank state.
                    self.typed_host_inputs
                        .borrow_mut()
                        .insert("loaderData".into(), ssr_snapshot_value(value));
                    self.typed_ssr_host_inputs
                        .borrow_mut()
                        .insert("loaderData".into());
                }
                self.adopt_typed_graph(
                    id.clone(),
                    Some(parent_id.clone()),
                    route.outlet_id.clone(),
                    route.graph_id.clone(),
                    Some(route.id),
                    Some(match_key),
                    outlet,
                    child_path.clone(),
                )?;
                // Chain agreement above proves these params equal the
                // snapshot's imported values, so this stamps the transferred
                // identity. A resolved outcome replaces the initial loader
                // run; a later fresh navigation still executes the loader.
                self.typed
                    .borrow_mut()
                    .get_mut(&id)
                    .expect("adopted route exists")
                    .route_state = Some(TypedRouteState {
                    normal_graph_id: route.graph_id,
                    pending_graph_id: route.pending_graph_id,
                    pending_mode: route.pending_mode,
                    error_graph_id: route.error_graph_id,
                    loader_action: loader.map(|(action, _)| action),
                    params: matched.params,
                    location: (
                        location.pathname.clone(),
                        location.search.clone(),
                        location.hash.clone(),
                    ),
                    phase: TypedRoutePhase::Normal,
                });
            }
            parent_id = id;
            path = child_path;
        }
        self.flush_component_work()?;
        self.install_typed_event_listeners()?;
        self.install_typed_global_listeners()?;
        self.refresh_navigation_state(&location.pathname)
    }

    pub(crate) fn navigate_internal(&self, href: &str, replace: bool) -> Result<(), JsValue> {
        let state = self
            .router
            .borrow()
            .clone()
            .ok_or_else(|| JsValue::from_str("runtime has not been started"))?;
        let pathname = href.split(['?', '#']).next().unwrap_or(href);
        let route = state
            .manifest
            .routes
            .iter()
            .find(|route| route.path == pathname.trim_start_matches('/'))
            .or_else(|| state.manifest.routes.iter().find(|route| route.path == "*"))
            .cloned()
            .ok_or_else(|| JsValue::from_str("no compiled route for pathname"))?;
        if replace {
            window()?
                .history()?
                .replace_state_with_url(&JsValue::NULL, "", Some(href))?;
        } else {
            window()?
                .history()?
                .push_state_with_url(&JsValue::NULL, "", Some(href))?;
        }
        self.refresh_navigation_state(pathname)?;
        let instance_id =
            self.replace_outlet_instance(state.root_instance_id, route.outlet_id, route.graph_id)?;
        if let Some(loader) = route.loader {
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

    pub(crate) fn refresh_navigation_state(&self, pathname: &str) -> Result<(), JsValue> {
        let location = window()?.location();
        let location = [
            (
                "location.pathname",
                RuntimeValue::String(location.pathname()?),
            ),
            ("location.search", RuntimeValue::String(location.search()?)),
            ("location.hash", RuntimeValue::String(location.hash()?)),
        ];
        // Location is a host input, so it must reach the graph the same way
        // state changes do: re-apply this instance's bindings, then re-evaluate
        // component call props so children see the new location instead of the
        // value snapshotted at instantiation. Without the refresh cascade a
        // prop-drilled pathname stays frozen at its mount-time value.
        for instance in self.typed.borrow_mut().values_mut() {
            for (name, value) in &location {
                instance
                    .runtime
                    .app
                    .host_inputs
                    .insert((*name).into(), value.clone());
            }
            instance.runtime.apply_static_bindings()?;
            instance.runtime.queue_static_component_refreshes()?;
        }
        self.flush_component_work()?;
        // Graph bindings now reflect the new location; this sweep only
        // normalizes links that live outside the application graph, and runs
        // last so stale bindings can no longer overwrite it.
        let links = document()?.query_selector_all("a[href]")?;
        for index in 0..links.length() {
            let Some(node) = links.item(index) else {
                continue;
            };
            let Ok(link) = node.dyn_into::<Element>() else {
                continue;
            };
            if link
                .get_attribute("href")
                .as_deref()
                .map(|href| href.split(['?', '#']).next().unwrap_or(href))
                == Some(pathname)
            {
                link.set_attribute("aria-current", "page")?;
            } else {
                link.remove_attribute("aria-current")?;
            }
        }
        Ok(())
    }

    pub(crate) fn validate_typed_manifest(&self, manifest: &RouteManifest) -> Result<(), JsValue> {
        if !self.has_typed_graph(&manifest.root_graph_id) {
            return Err(JsValue::from_str("typed root graph is not registered"));
        }
        let mut routes = HashMap::new();
        for route in &manifest.routes {
            if routes.insert(route.id.as_str(), route).is_some() {
                return Err(JsValue::from_str(
                    "typed route manifest has duplicate route id",
                ));
            }
            // Route phase graphs are fetched on demand. Validate their
            // executable details when an artifact is already registered, and
            // otherwise defer the check until that graph is mounted.
            if let (Some(action), Some(graph)) = (
                route.loader_action,
                self.typed_graph_application(&route.graph_id),
            ) {
                if !graph
                    .actions
                    .get(action)
                    .map(|action| action.route_loader)
                    .unwrap_or(false)
                {
                    return Err(JsValue::from_str("typed route loader action is invalid"));
                }
            }
        }
        for route in &manifest.routes {
            let parent_graph = match route.parent_id.as_deref() {
                Some(parent) => routes
                    .get(parent)
                    .ok_or_else(|| JsValue::from_str("typed route parent is missing"))?
                    .graph_id
                    .as_str(),
                None => manifest.root_graph_id.as_str(),
            };
            if let Some(parent) = self.typed_graph_application(parent_graph) {
                if !parent
                    .route_outlets
                    .iter()
                    .any(|outlet| outlet.id == route.outlet_id)
                {
                    return Err(JsValue::from_str(
                        "typed route parent does not declare its outlet",
                    ));
                }
            }
        }
        Ok(())
    }

    pub(crate) fn navigate_typed_route(
        &self,
        href: &str,
        root: Element,
        replace: bool,
        write_history: bool,
    ) -> Result<(), JsValue> {
        let manifest = self
            .typed_manifest
            .borrow()
            .clone()
            .ok_or_else(|| JsValue::from_str("typed router manifest missing"))?;
        let location = typed_location(href);
        if write_history {
            if replace {
                window()?
                    .history()?
                    .replace_state_with_url(&JsValue::NULL, "", Some(href))?;
            } else {
                window()?
                    .history()?
                    .push_state_with_url(&JsValue::NULL, "", Some(href))?;
            }
        }
        let root_id = graph_instance_id(None, "main", None);
        if !self.typed.borrow().contains_key(&root_id) {
            if !self.has_typed_graph(&manifest.root_graph_id) {
                self.request_typed_graph(&manifest.root_graph_id)?;
                return Ok(());
            }
            self.mount_typed_graph(
                root_id.clone(),
                None,
                "main".into(),
                manifest.root_graph_id.clone(),
                None,
                None,
                root,
                true,
            )?;
        }
        let mut parent_id = root_id;
        for matched in typed_route_chain(&manifest, &location.pathname) {
            let route = matched.route;
            if !self.has_typed_graph(&route.graph_id) {
                self.request_typed_graph(&route.graph_id)?;
                return Ok(());
            }
            // A loader can transition to either phase as soon as its fetch
            // settles. Load those immutable graphs before starting the loader
            // so a pending/error transition never races lazy graph delivery.
            if route.loader_action.is_some() {
                for graph_id in [
                    route.pending_graph_id.as_deref(),
                    route.error_graph_id.as_deref(),
                ]
                .into_iter()
                .flatten()
                {
                    if !self.has_typed_graph(graph_id) {
                        self.request_typed_graph(graph_id)?;
                        return Ok(());
                    }
                }
            }
            let match_key = typed_match_key(&route.id, &matched.params, &location);
            let current = self.typed.borrow().iter().find_map(|(id, entry)| {
                (entry.parent_id.as_deref() == Some(parent_id.as_str())
                    && entry.outlet_id == route.outlet_id)
                    .then(|| (id.clone(), entry.route_id.clone(), entry.match_key.clone()))
            });
            let created = !matches!(&current, Some((_, route_id, key)) if route_id.as_deref() == Some(route.id.as_str()) && key.as_deref() == Some(match_key.as_str()));
            let id = match current {
                Some((id, route_id, key))
                    if route_id.as_deref() == Some(route.id.as_str())
                        && key.as_deref() == Some(match_key.as_str()) =>
                {
                    id
                }
                Some((id, _, _)) => {
                    self.dispose_typed_instance(&id)?;
                    self.mount_typed_child(
                        &parent_id,
                        &route,
                        &match_key,
                        &matched.params,
                        &location,
                    )?
                }
                None => self.mount_typed_child(
                    &parent_id,
                    &route,
                    &match_key,
                    &matched.params,
                    &location,
                )?,
            };
            if created {
                if let Some(action) = route.loader_action {
                    self.run_typed_loader(&id, action, matched.params, &location)?;
                }
            }
            parent_id = id;
        }
        self.dispose_typed_children(&parent_id)?;
        self.refresh_navigation_state(&location.pathname)
    }

    fn has_typed_graph(&self, graph_id: &str) -> bool {
        self.typed_component_registry
            .borrow()
            .contains_key(graph_id)
            || self.typed_registry.borrow().contains_key(graph_id)
    }

    fn typed_graph_application(&self, graph_id: &str) -> Option<TypedApplication> {
        if let Some(graph) = self
            .typed_component_registry
            .borrow()
            .get(graph_id)
            .cloned()
        {
            return graph.components.get(graph.root_component).cloned();
        }
        self.typed_registry.borrow().get(graph_id).cloned()
    }

    /// URL resolution deliberately remains at the browser boundary.  WASM
    /// only asks for a graph identity and resumes the same location once the
    /// adapter registers the fetched immutable artifact.
    fn request_typed_graph(&self, graph_id: &str) -> Result<(), JsValue> {
        let init = CustomEventInit::new();
        let detail = Object::new();
        Reflect::set(
            &detail,
            &JsValue::from_str("graphId"),
            &JsValue::from_str(graph_id),
        )?;
        init.set_detail(&detail);
        let event = CustomEvent::new_with_event_init_dict("plec:graph-needed", &init)?;
        window()?.dispatch_event(&event)?;
        Ok(())
    }

    fn mount_typed_child(
        &self,
        parent_id: &str,
        route: &RouteManifestEntry,
        match_key: &str,
        params: &HashMap<String, String>,
        location: &TypedLocation,
    ) -> Result<String, JsValue> {
        let id = graph_instance_id(Some(parent_id), &route.outlet_id, None);
        self.mount_typed_graph(
            id.clone(),
            Some(parent_id.into()),
            route.outlet_id.clone(),
            route.graph_id.clone(),
            Some(route.id.clone()),
            Some(match_key.into()),
            self.typed_outlet_element(parent_id, &route.outlet_id)?,
            false,
        )?;
        self.typed.borrow_mut().get_mut(&id).unwrap().route_state = Some(TypedRouteState {
            normal_graph_id: route.graph_id.clone(),
            pending_graph_id: route.pending_graph_id.clone(),
            pending_mode: route.pending_mode.clone(),
            error_graph_id: route.error_graph_id.clone(),
            loader_action: route.loader_action,
            params: params.clone(),
            location: (
                location.pathname.clone(),
                location.search.clone(),
                location.hash.clone(),
            ),
            phase: TypedRoutePhase::Normal,
        });
        Ok(id)
    }
    fn mount_typed_graph(
        &self,
        id: String,
        parent_id: Option<String>,
        outlet_id: String,
        graph_id: String,
        route_id: Option<String>,
        match_key: Option<String>,
        root: Element,
        replace: bool,
    ) -> Result<(), JsValue> {
        let graph = self
            .typed_component_registry
            .borrow()
            .get(&graph_id)
            .cloned();
        let app = match graph {
            Some(graph) => graph
                .components
                .get(graph.root_component)
                .cloned()
                .ok_or_else(|| JsValue::from_str("typed route graph root is missing"))?,
            None => self
                .typed_registry
                .borrow()
                .get(&graph_id)
                .cloned()
                .ok_or_else(|| JsValue::from_str("typed route graph is not registered"))?,
        };
        let mut runtime = TypedRuntime::new(app)?;
        if let Some(graph) = self
            .typed_component_registry
            .borrow()
            .get(&graph_id)
            .cloned()
        {
            runtime.set_component_definitions(graph.components);
        }
        runtime.set_host_inputs(self.typed_host_inputs.borrow().clone())?;
        runtime.graph_generation = self.next_typed_generation();
        if replace {
            root.set_inner_html("");
        }
        runtime.mount(root)?;
        self.typed.borrow_mut().insert(
            id,
            TypedGraphInstance {
                parent_id,
                outlet_id,
                graph_id,
                route_id,
                match_key,
                route_state: None,
                loader_runtime: None,
                component_call: None,
                component_start: None,
                runtime,
            },
        );
        self.mount_component_requests()?;
        self.install_typed_event_listeners()
    }

    fn adopt_typed_graph(
        &self,
        id: String,
        parent_id: Option<String>,
        outlet_id: String,
        graph_id: String,
        route_id: Option<String>,
        match_key: Option<String>,
        root: Element,
        path: String,
    ) -> Result<(), JsValue> {
        let graph = self
            .typed_component_registry
            .borrow()
            .get(&graph_id)
            .cloned()
            .ok_or_else(|| JsValue::from_str("missing:ssr-component-graph"))?;
        let app = graph
            .components
            .get(graph.root_component)
            .cloned()
            .ok_or_else(|| JsValue::from_str("missing:ssr-root-component"))?;
        let mut runtime = TypedRuntime::new(app)?;
        runtime.set_component_definitions(graph.components);
        runtime.ssr_imported = *self.typed_ssr_imported.borrow();
        runtime.set_host_inputs(self.typed_host_inputs.borrow().clone())?;
        runtime.graph_generation = self.next_typed_generation();
        let branches = self
            .typed_ssr_branches
            .borrow()
            .get(&id)
            .cloned()
            .unwrap_or_default();
        let loops = self
            .typed_ssr_loops
            .borrow()
            .get(&id)
            .cloned()
            .unwrap_or_default();
        runtime.adopt(
            root.clone(),
            TypedAdoptionScope::Element(root),
            &path,
            &branches,
            &loops,
        )?;
        self.typed.borrow_mut().insert(
            id,
            TypedGraphInstance {
                parent_id,
                outlet_id,
                graph_id,
                route_id,
                match_key,
                route_state: None,
                loader_runtime: None,
                component_call: None,
                component_start: None,
                runtime,
            },
        );
        Ok(())
    }
    fn typed_outlet_element(&self, parent_id: &str, outlet_id: &str) -> Result<Element, JsValue> {
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
    fn dispose_typed_children(&self, parent_id: &str) -> Result<(), JsValue> {
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
    fn dispose_typed_instance(&self, id: &str) -> Result<(), JsValue> {
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
            if let Some(mut loader) = instance.loader_runtime {
                loader.invalidate_fetches();
                loader.clear_listeners();
            }
            if let Some(node) = instance.runtime.root.and_then(|root| root.first_child()) {
                if let Some(parent) = node.parent_node() {
                    parent.remove_child(&node)?;
                }
            }
        }
        Ok(())
    }
    fn run_typed_loader(
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

    fn show_typed_route_graph(
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
            .cloned();
        let app = match graph.as_ref() {
            Some(graph) => graph
                .components
                .get(graph.root_component)
                .cloned()
                .ok_or_else(|| JsValue::from_str("typed route graph root is missing"))?,
            None => self
                .typed_registry
                .borrow()
                .get(graph_id)
                .cloned()
                .ok_or_else(|| JsValue::from_str("typed route graph is not registered"))?,
        };
        let mut next = TypedRuntime::new(app)?;
        if let Some(graph) = graph {
            next.set_component_definitions(graph.components);
        }
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

    pub(crate) fn retry_typed_route(&self, id: &str) -> Result<(), JsValue> {
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

    pub(crate) fn restore_typed_route_normal(&self, id: &str) -> Result<(), JsValue> {
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

    pub(crate) fn show_typed_route_error(
        &self,
        id: &str,
        error: RuntimeValue,
    ) -> Result<(), JsValue> {
        let graph_id = {
            let mut typed = self.typed.borrow_mut();
            let instance = typed
                .get_mut(id)
                .ok_or_else(|| JsValue::from_str("typed route instance missing"))?;
            if let Some(mut loader) = instance.loader_runtime.take() {
                loader.invalidate_fetches();
                loader.clear_listeners();
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

fn normalize_route_error(error: RuntimeValue) -> RuntimeValue {
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

fn typed_location(href: &str) -> TypedLocation {
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

fn typed_match_key(
    route_id: &str,
    params: &HashMap<String, String>,
    location: &TypedLocation,
) -> String {
    let mut params = params.iter().collect::<Vec<_>>();
    params.sort_by(|left, right| left.0.cmp(right.0));
    format!(
        "{route_id}\n{}\n{}\n{}\n{}",
        location.pathname,
        location.search,
        location.hash,
        params
            .into_iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join("&")
    )
}

/// Compares the server-published snapshot chain with the chain re-derived
/// from the current URL. Returns the detail for a
/// `mismatch:ssr-route-chain:{detail}` failure code, or `None` on agreement.
/// The chain comparison reuses `typed_route_chain` output; it never invents a
/// third matcher. Loader outcomes must pair with the phase that rendered
/// them: `active` requires a resolved outcome, `error` a rejected one.
fn ssr_route_chain_mismatch(
    imported: &[plec_ir::SsrRouteInstance],
    derived: &[TypedRouteMatch],
    loaders: &HashMap<String, plec_ir::SsrLoaderState>,
) -> Option<String> {
    if imported.len() != derived.len() {
        return Some(format!("length:{}:{}", imported.len(), derived.len()));
    }
    for (index, (instance, matched)) in imported.iter().zip(derived).enumerate() {
        let outcome = matched
            .route
            .loader_action
            .map(|action| loaders.get(&plec_ir::loader_ref(&matched.route.graph_id, action)));
        let phase = match (&instance.phase, outcome) {
            (
                plec_ir::SsrRoutePhase::Active,
                Some(Some(plec_ir::SsrLoaderState::Resolved { .. })),
            )
            | (plec_ir::SsrRoutePhase::Active, None) => None,
            // A loader route may only resume as active from its resolved
            // outcome; anything else cannot claim the normal-phase DOM.
            (plec_ir::SsrRoutePhase::Active, Some(_)) => Some("loader"),
            (plec_ir::SsrRoutePhase::Pending, _) => Some("pending"),
            (
                plec_ir::SsrRoutePhase::Error,
                Some(Some(plec_ir::SsrLoaderState::Rejected { .. })),
            ) => None,
            // Error phases resume only from a matching rejected outcome.
            (plec_ir::SsrRoutePhase::Error, _) => Some("error"),
        };
        if let Some(phase) = phase {
            return Some(format!("phase:{index}:{phase}"));
        }
        if instance.route_id != matched.route.id {
            return Some(format!("route:{index}:{}", instance.route_id));
        }
        for (key, value) in &instance.params {
            if matched.params.get(key).map(String::as_str) != Some(value.as_str()) {
                return Some(format!("params:{index}:{key}"));
            }
        }
        if matched.params.len() != instance.params.len() {
            return Some(format!("params:{index}:count"));
        }
    }
    None
}

/// The error record a rejected imported outcome restores. The snapshot
/// carries only the message, so the record uses the transport `http` kind
/// `normalize_route_error` passes through unchanged.
fn ssr_loader_error(message: &str) -> RuntimeValue {
    RuntimeValue::Record(HashMap::from([
        ("kind".into(), RuntimeValue::String("http".into())),
        ("message".into(), RuntimeValue::String(message.into())),
    ]))
}

fn typed_route_chain(manifest: &RouteManifest, pathname: &str) -> Vec<TypedRouteMatch> {
    let parts = pathname
        .trim_matches('/')
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    typed_route_chain_from(manifest, None, &parts, 0, HashMap::new()).unwrap_or_default()
}

fn typed_route_chain_from(
    manifest: &RouteManifest,
    parent: Option<&str>,
    parts: &[&str],
    offset: usize,
    params: HashMap<String, String>,
) -> Option<Vec<TypedRouteMatch>> {
    let mut candidates = manifest
        .routes
        .iter()
        .filter_map(|route| {
            (route.parent_id.as_deref() == parent && !route.path.is_empty() && route.path != "*")
                .then(|| route_match(route, parts, offset, &params))
                .flatten()
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|(static_segments, _, _, _)| std::cmp::Reverse(*static_segments));
    for (_, route, next_offset, next_params) in candidates {
        let mut branch = vec![TypedRouteMatch {
            route: route.clone(),
            params: next_params.clone(),
        }];
        if next_offset < parts.len() {
            if let Some(mut child) =
                typed_route_chain_from(manifest, Some(&route.id), parts, next_offset, next_params)
            {
                branch.append(&mut child);
                return Some(branch);
            }
        } else if let Some(mut child) =
            typed_route_chain_from(manifest, Some(&route.id), parts, next_offset, next_params)
        {
            branch.append(&mut child);
            return Some(branch);
        } else {
            return Some(branch);
        }
    }
    if offset == parts.len() {
        if let Some(route) = manifest
            .routes
            .iter()
            .find(|route| route.parent_id.as_deref() == parent && route.path.is_empty())
        {
            let mut branch = vec![TypedRouteMatch {
                route: route.clone(),
                params: params.clone(),
            }];
            if let Some(mut child) =
                typed_route_chain_from(manifest, Some(&route.id), parts, offset, params)
            {
                branch.append(&mut child);
            }
            return Some(branch);
        }
    }
    manifest
        .routes
        .iter()
        .find(|route| route.parent_id.as_deref() == parent && route.path == "*")
        .map(|route| {
            vec![TypedRouteMatch {
                route: route.clone(),
                params,
            }]
        })
}

fn route_match(
    route: &RouteManifestEntry,
    parts: &[&str],
    offset: usize,
    params: &HashMap<String, String>,
) -> Option<(usize, RouteManifestEntry, usize, HashMap<String, String>)> {
    let segments = route
        .path
        .trim_matches('/')
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if segments.len() > parts.len().saturating_sub(offset) {
        return None;
    }
    let mut params = params.clone();
    let mut static_segments = 0;
    for (index, segment) in segments.iter().enumerate() {
        let value = parts[offset + index];
        if let Some(name) = segment.strip_prefix('$') {
            params.insert(name.into(), decode_path_segment(value));
        } else if *segment == value {
            static_segments += 1;
        } else {
            return None;
        }
    }
    Some((
        static_segments,
        route.clone(),
        offset + segments.len(),
        params,
    ))
}

fn decode_path_segment(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2])) {
                output.push(high * 16 + low);
                index += 3;
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(output).unwrap_or_else(|_| value.into())
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn route(id: &str, parent: Option<&str>, path: &str) -> RouteManifestEntry {
        RouteManifestEntry {
            id: id.into(),
            parent_id: parent.map(str::to_owned),
            path: path.into(),
            graph_id: id.into(),
            pending_graph_id: None,
            pending_mode: "replace".into(),
            error_graph_id: None,
            outlet_id: "main".into(),
            loader: None,
            loader_state_slot_id: None,
            loader_action: None,
        }
    }
    #[test]
    fn typed_matching_prefers_static_nested_routes_and_decodes_params() {
        let manifest = RouteManifest {
            version: Some(3),
            root_graph_id: "root".into(),
            routes: vec![
                route("projects", None, "projects"),
                route("new", Some("projects"), "new"),
                route("project", Some("projects"), "$projectId"),
                route("missing", None, "*"),
            ],
        };
        let matched = typed_route_chain(&manifest, "/projects/a%20b");
        assert_eq!(
            matched
                .iter()
                .map(|matched| matched.route.id.as_str())
                .collect::<Vec<_>>(),
            ["projects", "project"]
        );
        assert_eq!(
            matched[1].params.get("projectId").map(String::as_str),
            Some("a b")
        );
        assert_eq!(
            typed_route_chain(&manifest, "/projects/new")[1].route.id,
            "new"
        );
    }

    #[test]
    fn typed_matching_descends_through_pathless_index_routes() {
        let manifest = RouteManifest {
            version: Some(3),
            root_graph_id: "root".into(),
            routes: vec![route("layout", None, ""), route("home", Some("layout"), "")],
        };
        let matched = typed_route_chain(&manifest, "/");
        assert_eq!(
            matched
                .iter()
                .map(|matched| matched.route.id.as_str())
                .collect::<Vec<_>>(),
            ["layout", "home"]
        );
    }

    #[test]
    fn typed_match_identity_changes_for_params_and_location() {
        let location = TypedLocation {
            pathname: "/projects/a".into(),
            search: "".into(),
            hash: "".into(),
        };
        let first = typed_match_key(
            "project",
            &HashMap::from([("projectId".into(), "a".into())]),
            &location,
        );
        let second = typed_match_key(
            "project",
            &HashMap::from([("projectId".into(), "b".into())]),
            &location,
        );
        let search = typed_match_key(
            "project",
            &HashMap::from([("projectId".into(), "a".into())]),
            &TypedLocation {
                search: "?tab=details".into(),
                ..location
            },
        );
        assert_ne!(first, second);
        assert_ne!(first, search);
    }

    fn instance(route_id: &str, params: &[(&str, &str)]) -> plec_ir::SsrRouteInstance {
        plec_ir::SsrRouteInstance {
            route_id: route_id.into(),
            params: params
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
            phase: plec_ir::SsrRoutePhase::Active,
        }
    }

    #[test]
    fn snapshot_chain_agrees_for_flat_param_catch_all_and_nested_chains() {
        // Flat multi-segment $param route: the server matcher and
        // typed_route_chain agree on the route and decoded params.
        let manifest = RouteManifest {
            version: Some(3),
            root_graph_id: "root".into(),
            routes: vec![
                route("home", None, ""),
                route("project", None, "projects/$projectId"),
                route("missing", None, "*"),
            ],
        };
        let derived = typed_route_chain(&manifest, "/projects/a%20b");
        assert_eq!(
            ssr_route_chain_mismatch(
                &[instance("project", &[("projectId", "a b")])],
                &derived,
                &HashMap::new()
            ),
            None
        );

        // Catch-all agreement: an unmatched path resolves to the same
        // fallback route on both sides.
        let derived = typed_route_chain(&manifest, "/nowhere");
        assert_eq!(
            ssr_route_chain_mismatch(&[instance("missing", &[])], &derived, &HashMap::new()),
            None
        );

        // Nested pathless-index chain agreement when the snapshot carries the
        // full descent.
        let nested = RouteManifest {
            version: Some(3),
            root_graph_id: "root".into(),
            routes: vec![route("layout", None, ""), route("home", Some("layout"), "")],
        };
        let derived = typed_route_chain(&nested, "/");
        assert_eq!(
            derived
                .iter()
                .map(|m| m.route.id.as_str())
                .collect::<Vec<_>>(),
            ["layout", "home"]
        );
        assert_eq!(
            ssr_route_chain_mismatch(
                &[instance("layout", &[]), instance("home", &[])],
                &derived,
                &HashMap::new()
            ),
            None
        );
    }

    #[test]
    fn snapshot_chain_mismatches_pin_server_and_typed_matcher_gaps() {
        let nested = RouteManifest {
            version: Some(3),
            root_graph_id: "root".into(),
            routes: vec![route("layout", None, ""), route("home", Some("layout"), "")],
        };
        // Known semantic gap: the flat server matcher publishes only the first
        // pathless route (the layout) while the typed chain descends into the
        // index child. This disagreement must be detectable, never silent.
        let derived = typed_route_chain(&nested, "/");
        assert_eq!(
            ssr_route_chain_mismatch(&[instance("layout", &[])], &derived, &HashMap::new()),
            Some("length:1:2".into())
        );
        // Param value disagreement (server rendered one id, URL holds another).
        let manifest = RouteManifest {
            version: Some(3),
            root_graph_id: "root".into(),
            routes: vec![route("project", None, "projects/$projectId")],
        };
        let derived = typed_route_chain(&manifest, "/projects/a");
        assert_eq!(
            ssr_route_chain_mismatch(
                &[instance("project", &[("projectId", "b")])],
                &derived,
                &HashMap::new()
            ),
            Some("params:0:projectId".into())
        );
        // Extra derived params (snapshot claims none for a $param route).
        assert_eq!(
            ssr_route_chain_mismatch(&[instance("project", &[])], &derived, &HashMap::new()),
            Some("params:0:count".into())
        );
        // Route id disagreement at the same position.
        assert_eq!(
            ssr_route_chain_mismatch(&[instance("home", &[])], &derived, &HashMap::new()),
            Some("route:0:home".into())
        );
        // Reserved phases cannot adopt as active.
        let mut reserved = instance("project", &[("projectId", "a")]);
        reserved.phase = plec_ir::SsrRoutePhase::Error;
        assert_eq!(
            ssr_route_chain_mismatch(&[reserved], &derived, &HashMap::new()),
            Some("phase:0:error".into())
        );
    }
}
