use crate::dom::platform::*;
use crate::runtime::lifecycle::*;
use crate::schema::delta::RuntimeValue;
use crate::schema::routing::RouteManifestEntry;
use crate::typed::runtime::*;
use std::collections::HashMap;

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
        let location = window()?.location();
        let location = [
            (
                "location.pathname",
                RuntimeValue::String(location.pathname()?),
            ),
            ("location.search", RuntimeValue::String(location.search()?)),
            ("location.hash", RuntimeValue::String(location.hash()?)),
        ];
        for instance in self.typed.borrow_mut().values_mut() {
            for (name, value) in &location {
                instance
                    .runtime
                    .app
                    .host_inputs
                    .insert((*name).into(), value.clone());
            }
            instance.runtime.apply_static_bindings()?;
        }
        Ok(())
    }

    pub(crate) fn validate_typed_manifest(&self, manifest: &RouteManifest) -> Result<(), JsValue> {
        let registry = self.typed_registry.borrow();
        if !registry.contains_key(&manifest.root_graph_id) {
            return Err(JsValue::from_str("typed root graph is not registered"));
        }
        let mut routes = HashMap::new();
        for route in &manifest.routes {
            if routes.insert(route.id.as_str(), route).is_some() {
                return Err(JsValue::from_str(
                    "typed route manifest has duplicate route id",
                ));
            }
            if !registry.contains_key(&route.graph_id) {
                return Err(JsValue::from_str("typed route graph is not registered"));
            }
            for graph_id in [&route.pending_graph_id, &route.error_graph_id]
                .into_iter()
                .flatten()
            {
                if !registry.contains_key(graph_id) {
                    return Err(JsValue::from_str(
                        "typed route phase graph is not registered",
                    ));
                }
            }
            if let Some(action) = route.loader_action {
                let graph = registry.get(&route.graph_id).expect("checked above");
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
            if !registry
                .get(parent_graph)
                .expect("checked above")
                .route_outlets
                .iter()
                .any(|outlet| outlet.id == route.outlet_id)
            {
                return Err(JsValue::from_str(
                    "typed route parent does not declare its outlet",
                ));
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
        let app = self
            .typed_registry
            .borrow()
            .get(&graph_id)
            .cloned()
            .ok_or_else(|| JsValue::from_str("typed route graph is not registered"))?;
        let mut runtime = TypedRuntime::new(app)?;
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
        let app = self
            .typed_registry
            .borrow()
            .get(graph_id)
            .cloned()
            .ok_or_else(|| JsValue::from_str("typed route graph is not registered"))?;
        let mut next = TypedRuntime::new(app)?;
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
        Ok(())
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
            return Some(vec![TypedRouteMatch {
                route: route.clone(),
                params,
            }]);
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
}
