use js_sys::{Object, Reflect};
use plec_client::prelude::*;
use plec_client::route::{ssr_snapshot_value, typed_location, TypedLocation};
use plec_client::runtime::*;
use plec_dom::platform::{document, window};
use plec_schema::delta::RuntimeValue;
use plec_schema::routing::{match_route_chain, RouteManifestEntry, RouteMatch as TypedRouteMatch};
use std::collections::HashMap;
use web_sys::{CustomEvent, CustomEventInit};

pub fn adopt_typed_route(state: &RuntimeState, href: &str, root: Element) -> Result<(), JsValue> {
    let manifest = state
        .typed_manifest
        .borrow()
        .clone()
        .ok_or_else(|| JsValue::from_str("missing:ssr-manifest"))?;
    let location = typed_location(href);
    state.set_typed_route_search(&location.search)?;
    // The boundary owner is derived up front so the root graph selection can
    // honor a root not-found boundary before any instance is claimed.
    let agreement = match state.typed_ssr_route_chain.borrow().as_ref() {
        Some(imported) => {
            let loaders = state.typed_ssr_loaders.borrow();
            ssr_chain_agreement(imported, &typed_route_chain(&manifest, &location.pathname), &loaders, &manifest)
        }
        None => SsrChainAgreement::Exact,
    };
    let root_boundary = matches!(agreement, SsrChainAgreement::RootNotFoundBoundary);
    let adopted_root_graph = if root_boundary {
        manifest
            .root_not_found_graph_id
            .clone()
            .ok_or_else(|| JsValue::from_str("missing:ssr-root-not-found-graph"))?
    } else {
        manifest.root_graph_id.clone()
    };
    if !has_typed_graph(state, &adopted_root_graph) {
        return Err(JsValue::from_str("missing:ssr-root-graph"));
    }
    let root_id = graph_instance_id(None, "main", None);
    let root_loader_data = state.typed_host_inputs.borrow().get("loaderData").cloned();
    adopt_typed_graph(
        state,
        root_id.clone(),
        None,
        "main".into(),
        adopted_root_graph,
        None,
        None,
        root,
        "root".into(),
        root_loader_data.as_ref(),
        None,
    )?;
    let mut parent_id = root_id;
    let mut path = "root".to_owned();
    let derived_chain = typed_route_chain(&manifest, &location.pathname);
    // The transferred snapshot chain is the route execution cause. The
    // runtime re-derives the chain from the URL with the same matcher used
    // for fresh navigation; agreement lets the imported identity proceed,
    // disagreement is a contract failure, never a silent remount. A
    // not-found boundary truncates the derived chain to the boundary owner.
    let mut owner_position: Option<usize> = None;
    match agreement {
        SsrChainAgreement::Exact => {}
        SsrChainAgreement::NotFoundBoundary(depth) => {
            owner_position = Some(depth - 1);
        }
        SsrChainAgreement::RootNotFoundBoundary => {
            owner_position = Some(usize::MAX);
        }
        SsrChainAgreement::Mismatch(detail) => {
            return Err(JsValue::from_str(&format!(
                "mismatch:ssr-route-chain:{detail}"
            )));
        }
    }
    let derived_chain = match owner_position {
        Some(usize::MAX) => Vec::new(),
        Some(depth) => derived_chain[..=depth].to_vec(),
        None => derived_chain,
    };
    for (index, matched) in derived_chain.into_iter().enumerate() {
        let route = matched.route;
        if Some(index) == owner_position.filter(|depth| *depth != usize::MAX) {
            // The boundary owner: the server rendered its not-found graph.
            let Some(boundary_graph_id) = route.not_found_graph_id.clone() else {
                return Err(JsValue::from_str("mismatch:ssr-not-found-graph"));
            };
            if !has_typed_graph(state, &boundary_graph_id) {
                return Err(JsValue::from_str("missing:ssr-route-not-found-graph"));
            }
            let loader = route
                .loader_action
                .map(|action| {
                    let reference = plec_ir::loader_ref(&route.graph_id, action);
                    let outcome = state
                        .typed_ssr_loaders
                        .borrow()
                        .get(&reference)
                        .cloned()
                        .ok_or_else(|| {
                            JsValue::from_str(&format!("mismatch:ssr-loader:{reference}"))
                        })?;
                    Ok::<_, JsValue>(Some((action, outcome)))
                })
                .transpose()?
                .flatten();
            let match_key = typed_match_key(&route.id, &matched.params, &location);
            let id = graph_instance_id(Some(&parent_id), &route.outlet_id, None);
            let outlet = state.typed_outlet_element(&parent_id, &route.outlet_id)?;
            let child_path = format!("{path}/outlet:{}", route.outlet_id);
            adopt_typed_graph(
                state,
                id.clone(),
                Some(parent_id.clone()),
                route.outlet_id.clone(),
                boundary_graph_id,
                Some(route.id),
                Some(match_key),
                outlet,
                child_path.clone(),
                None,
                Some(&matched.params),
            )?;
            state
                .typed
                .borrow_mut()
                .get_mut(&id)
                .expect("adopted route exists")
                .route_state = Some(TypedRouteState {
                normal_graph_id: route.graph_id,
                pending_graph_id: route.pending_graph_id,
                pending_mode: route.pending_mode,
                error_graph_id: route.error_graph_id,
                not_found_graph_id: route.not_found_graph_id,
                loader_action: loader.map(|(action, _)| action),
                params: matched.params,
                location: (
                    location.pathname.clone(),
                    location.search.clone(),
                    location.hash.clone(),
                ),
                phase: TypedRoutePhase::NotFound,
            });
            parent_id = id;
            path = child_path;
            continue;
        }
        if !has_typed_graph(state, &route.graph_id) {
            return Err(JsValue::from_str("missing:ssr-route-graph"));
        }
        let match_key = typed_match_key(&route.id, &matched.params, &location);
        let id = graph_instance_id(Some(&parent_id), &route.outlet_id, None);
        let outlet = state.typed_outlet_element(&parent_id, &route.outlet_id)?;
        let child_path = format!("{path}/outlet:{}", route.outlet_id);
        // Loader routes resume from the transferred outcome instead of
        // re-running on first paint. The chain check above guarantees the
        // snapshot phase pairs with the outcome (`active` + resolved,
        // `error` + rejected) and that a loader route imports an outcome.
        let loader = route
            .loader_action
            .map(|action| {
                let reference = plec_ir::loader_ref(&route.graph_id, action);
                let outcome = state
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
            if !has_typed_graph(state, &error_graph_id) {
                return Err(JsValue::from_str("missing:ssr-route-error-graph"));
            }
            // The server rendered the error phase, so the SSR DOM belongs
            // to the error graph: claim it there, then restore the
            // recorded error so the phase matches a client-side failure
            // and `retry_typed_route` re-enters the loader.
            adopt_typed_graph(
                state,
                id.clone(),
                Some(parent_id.clone()),
                route.outlet_id.clone(),
                error_graph_id,
                Some(route.id.clone()),
                Some(match_key),
                outlet,
                child_path.clone(),
                None,
                Some(&matched.params),
            )?;
            let mut typed = state.typed.borrow_mut();
            let instance = typed.get_mut(&id).expect("adopted route exists");
            instance.route_state = Some(TypedRouteState {
                normal_graph_id: route.graph_id,
                pending_graph_id: route.pending_graph_id,
                pending_mode: route.pending_mode,
                error_graph_id: route.error_graph_id,
                not_found_graph_id: route.not_found_graph_id,
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
            let loader_data = match &loader {
                Some((_, plec_ir::SsrLoaderState::Resolved { value })) => {
                    Some(ssr_snapshot_value(value))
                }
                _ if *state.typed_ssr_imported.borrow() => state
                    .typed
                    .borrow()
                    .get(&parent_id)
                    .and_then(|instance| instance.loader_data.clone()),
                _ => None,
            };
            adopt_typed_graph(
                state,
                id.clone(),
                Some(parent_id.clone()),
                route.outlet_id.clone(),
                route.graph_id.clone(),
                Some(route.id),
                Some(match_key),
                outlet,
                child_path.clone(),
                loader_data.as_ref(),
                Some(&matched.params),
            )?;
            // Chain agreement above proves these params equal the
            // snapshot's imported values, so this stamps the transferred
            // identity. A resolved outcome replaces the initial loader
            // run; a later fresh navigation still executes the loader.
            state
                .typed
                .borrow_mut()
                .get_mut(&id)
                .expect("adopted route exists")
                .route_state = Some(TypedRouteState {
                normal_graph_id: route.graph_id,
                pending_graph_id: route.pending_graph_id,
                pending_mode: route.pending_mode,
                error_graph_id: route.error_graph_id,
                not_found_graph_id: route.not_found_graph_id,
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
    state.flush_component_work()?;
    state.install_typed_event_listeners()?;
    state.install_typed_global_listeners()?;
    refresh_navigation_state(state, &location.pathname)
}

pub fn refresh_navigation_state(state: &RuntimeState, pathname: &str) -> Result<(), JsValue> {
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
    for instance in state.typed.borrow_mut().values_mut() {
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
    state.flush_component_work()?;
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

pub fn validate_typed_manifest(
    state: &RuntimeState,
    manifest: &RouteManifest,
) -> Result<(), JsValue> {
    if !has_typed_graph(state, &manifest.root_graph_id) {
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
            typed_graph_application(state, &route.graph_id),
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
        if let Some(parent) = typed_graph_application(state, parent_graph) {
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

pub fn navigate_typed_route(
    state: &RuntimeState,
    href: &str,
    root: Element,
    replace: bool,
    write_history: bool,
) -> Result<(), JsValue> {
    let manifest = state
        .typed_manifest
        .borrow()
        .clone()
        .ok_or_else(|| JsValue::from_str("typed router manifest missing"))?;
    let location = typed_location(href);
    state.set_typed_route_search(&location.search)?;
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
    if !state.typed.borrow().contains_key(&root_id) {
        if !has_typed_graph(state, &manifest.root_graph_id) {
            request_typed_graph(state, &manifest.root_graph_id)?;
            return Ok(());
        }
        mount_typed_graph(
            state,
            root_id.clone(),
            None,
            "main".into(),
            manifest.root_graph_id.clone(),
            None,
            None,
            root,
            "root".into(),
            true,
            None,
        )?;
    } else {
        // A root not-found boundary from a previous navigation restores the
        // persistent root graph before the new chain mounts into it.
        if let Some(boundary) = &manifest.root_not_found_graph_id {
            let showing_boundary = state
                .typed
                .borrow()
                .get(&root_id)
                .map(|instance| &instance.graph_id == boundary)
                .unwrap_or(false);
            if showing_boundary {
                state.show_typed_route_graph(&root_id, &manifest.root_graph_id, false, None)?;
            }
        }
    }
    let mut parent_id = root_id;
    let mut nearest_not_found_graph = None;
    for matched in typed_route_chain(&manifest, &location.pathname) {
        let route = matched.route;
        if route.not_found_graph_id.is_some() {
            nearest_not_found_graph = route.not_found_graph_id.clone();
        }
        if !has_typed_graph(state, &route.graph_id) {
            request_typed_graph(state, &route.graph_id)?;
            return Ok(());
        }
        // A loader can transition to either phase as soon as its fetch
        // settles. Load those immutable graphs before starting the loader
        // so a pending/error transition never races lazy graph delivery.
        if route.loader_action.is_some() {
            for graph_id in [
                route.pending_graph_id.as_deref(),
                route.error_graph_id.as_deref(),
                route
                    .not_found_graph_id
                    .as_deref()
                    .or(nearest_not_found_graph.as_deref())
                    .or(manifest.root_not_found_graph_id.as_deref()),
            ]
            .into_iter()
            .flatten()
            {
                if !has_typed_graph(state, graph_id) {
                    request_typed_graph(state, graph_id)?;
                    return Ok(());
                }
            }
        }
        let match_key = typed_match_key(&route.id, &matched.params, &location);
        let current = state.typed.borrow().iter().find_map(|(id, entry)| {
            (entry.parent_id.as_deref() == Some(parent_id.as_str())
                && entry.outlet_id == route.outlet_id)
                .then(|| (id.clone(), entry.route_id.clone(), entry.match_key.clone()))
        });
        let created = !matches!(&current, Some((_, route_id, key)) if route_id.as_deref() == Some(route.id.as_str()) && key.as_deref() == Some(match_key.as_str()));
        let search_changed = !created
            && current.as_ref().is_some_and(|(id, _, _)| {
                state
                    .typed
                    .borrow()
                    .get(id)
                    .and_then(|instance| instance.route_state.as_ref())
                    .is_some_and(|route| route.location.1 != location.search)
            });
        let id = match current {
            Some((id, route_id, key))
                if route_id.as_deref() == Some(route.id.as_str())
                    && key.as_deref() == Some(match_key.as_str()) =>
            {
                id
            }
            Some((id, _, _)) => {
                state.dispose_typed_instance(&id)?;
                mount_typed_child(
                    state,
                    &parent_id,
                    &route,
                    &match_key,
                    &matched.params,
                    &location,
                )?
            }
            None => mount_typed_child(
                state,
                &parent_id,
                &route,
                &match_key,
                &matched.params,
                &location,
            )?,
        };
        if !created {
            state.update_typed_route_search(&id, &location.search)?;
        }
        if created || search_changed {
            if let Some(action) = route.loader_action {
                state.run_typed_loader(&id, action, matched.params, &location)?;
            }
        }
        parent_id = id;
    }
    state.dispose_typed_children(&parent_id)?;
    refresh_navigation_state(state, &location.pathname)
}

/// Resolves a terminal loader redirect outcome: navigate to the target with
/// explicit history semantics. Loop protection shares `MAX_REDIRECT_HOPS`
/// with the SSR host; a loop aborts the navigation with a diagnostic instead
/// of committing any route.
pub fn handle_loader_redirect(
    state: &RuntimeState,
    href: &str,
    replace: bool,
) -> Result<(), JsValue> {
    if !href.starts_with('/')
        || href.starts_with("//")
        || href.contains(['#', '\\'])
        || href.chars().any(char::is_whitespace)
        || href.chars().any(char::is_control)
    {
        return Err(JsValue::from_str(
            "route loader redirects must use a valid absolute application path without a fragment",
        ));
    }
    let depth = next_redirect_depth(state.typed_redirect_depth.get(), href)
        .map_err(|error| JsValue::from_str(&error))?;
    state.typed_redirect_depth.set(depth);
    let root = state
        .typed_root
        .borrow()
        .clone()
        .ok_or_else(|| JsValue::from_str("typed router root is missing"))?;
    // Navigation may pause for lazy graph delivery or a loader fetch. Keep
    // this hop charged until a fresh user navigation resets the budget; a
    // synchronous decrement here would let asynchronous redirect loops run
    // forever.
    navigate_typed_route(state, href, root, replace, true)
}

fn next_redirect_depth(current: usize, href: &str) -> Result<usize, String> {
    let depth = current.saturating_add(1);
    if depth > plec_ir::limits::MAX_REDIRECT_HOPS {
        return Err(format!(
            "redirect loop exceeded {} hops at {href}",
            plec_ir::limits::MAX_REDIRECT_HOPS
        ));
    }
    Ok(depth)
}

/// Resolves a terminal loader not-found outcome. The boundary owner is the
/// deepest instance from the origin (inclusive) declaring
/// `notFoundComponent`; its descendants are discarded and the owner
/// transitions to the `NotFound` phase. Without any route boundary, the
/// root boundary applies.
pub fn handle_loader_not_found(state: &RuntimeState, origin_id: &str) -> Result<(), JsValue> {
    let manifest = state
        .typed_manifest
        .borrow()
        .clone()
        .ok_or_else(|| JsValue::from_str("typed router manifest missing"))?;
    let owner = {
        let typed = state.typed.borrow();
        let mut cursor = Some(origin_id.to_owned());
        loop {
            match cursor {
                Some(id) => {
                    let Some(instance) = typed.get(&id) else {
                        break None;
                    };
                    if instance
                        .route_state
                        .as_ref()
                        .and_then(|route| route.not_found_graph_id.as_ref())
                        .is_some()
                    {
                        break Some(id);
                    }
                    cursor = instance.parent_id.clone();
                }
                None => break None,
            }
        }
    };
    if let Some(owner_id) = owner {
        state.dispose_typed_children(&owner_id)?;
        state.show_typed_route_not_found(&owner_id)?;
        state.flush_component_work()?;
        return state.install_typed_event_listeners();
    }
    match &manifest.root_not_found_graph_id {
        Some(boundary_graph_id) => {
            let root_id = graph_instance_id(None, "main", None);
            state.dispose_typed_children(&root_id)?;
            state.show_typed_route_graph(&root_id, boundary_graph_id, false, None)?;
            state.flush_component_work()?;
            state.install_typed_event_listeners()
        }
        // Compiled applications cannot reach this: the compiler rejects
        // `notFound()` without any boundary in the chain. Defensive only.
        None => Ok(()),
    }
}

fn has_typed_graph(state: &RuntimeState, graph_id: &str) -> bool {
    state
        .typed_component_registry
        .borrow()
        .contains_key(graph_id)
}

fn typed_graph_application(state: &RuntimeState, graph_id: &str) -> Option<TypedApplication> {
    let graph = state
        .typed_component_registry
        .borrow()
        .get(graph_id)
        .cloned()?;
    graph.components.get(graph.root_component).cloned()
}

/// URL resolution deliberately remains at the browser boundary.  WASM
/// only asks for a graph identity and resumes the same location once the
/// adapter registers the fetched immutable artifact.
fn request_typed_graph(_state: &RuntimeState, graph_id: &str) -> Result<(), JsValue> {
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
    state: &RuntimeState,
    parent_id: &str,
    route: &RouteManifestEntry,
    match_key: &str,
    params: &HashMap<String, String>,
    location: &TypedLocation,
) -> Result<String, JsValue> {
    let id = graph_instance_id(Some(parent_id), &route.outlet_id, None);
    // The child's structural address extends the parent instance's own
    // address, mirroring the server renderer's outlet descent.
    let parent_path = state
        .typed
        .borrow()
        .get(parent_id)
        .map(|instance| instance.runtime.path.clone())
        .ok_or_else(|| JsValue::from_str("typed parent route is not mounted"))?;
    let path = format!("{parent_path}/outlet:{}", route.outlet_id);
    mount_typed_graph(
        state,
        id.clone(),
        Some(parent_id.into()),
        route.outlet_id.clone(),
        route.graph_id.clone(),
        Some(route.id.clone()),
        Some(match_key.into()),
        state.typed_outlet_element(parent_id, &route.outlet_id)?,
        path,
        false,
        Some(params),
    )?;
    state.typed.borrow_mut().get_mut(&id).unwrap().route_state = Some(TypedRouteState {
        normal_graph_id: route.graph_id.clone(),
        pending_graph_id: route.pending_graph_id.clone(),
        pending_mode: route.pending_mode.clone(),
        error_graph_id: route.error_graph_id.clone(),
        not_found_graph_id: route.not_found_graph_id.clone(),
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
    state: &RuntimeState,
    id: String,
    parent_id: Option<String>,
    outlet_id: String,
    graph_id: String,
    route_id: Option<String>,
    match_key: Option<String>,
    root: Element,
    path: String,
    replace: bool,
    route_params: Option<&HashMap<String, String>>,
) -> Result<(), JsValue> {
    state.ensure_typed_instance_absent(
        &id,
        &format!("route mount (graph {graph_id}, outlet {outlet_id})"),
    )?;
    let graph = state
        .typed_component_registry
        .borrow()
        .get(&graph_id)
        .cloned()
        .ok_or_else(|| JsValue::from_str("typed route graph is not registered"))?;
    let app = graph
        .components
        .get(graph.root_component)
        .cloned()
        .ok_or_else(|| JsValue::from_str("typed route graph root is missing"))?;
    let mut runtime = TypedRuntime::new_with_tag_policy(
        app,
        state.region_tracker.clone(),
        state.reconcile_budget.clone(),
        state.cookie_policy.clone(),
        state.effective_tag_policy(),
    )?;
    runtime.set_component_definitions(graph.components);
    runtime.host_registry = state.host_registry.clone();
    runtime.host_dispatch = Some(state.clone());
    runtime.set_host_inputs(state.typed_host_inputs_for_location(None, route_params, None)?)?;
    runtime.graph_generation = state.next_typed_generation();
    // Fresh client mounts use the exact structural address the server
    // renderer would have given this route position, so CSR-created DOM
    // carries canonical `data-plec-node` / boundary-marker addresses.
    runtime.path = path;
    if replace {
        root.set_inner_html("");
    }
    runtime.mount(root)?;
    state.typed.borrow_mut().insert(
        id,
        TypedGraphInstance {
            parent_id,
            outlet_id,
            graph_id,
            route_id,
            match_key,
            route_state: None,
            loader_data: None,
            loader_runtime: None,
            component_call: None,
            component_start: None,
            runtime,
        },
    );
    state.mount_component_requests()?;
    state.install_typed_event_listeners()
}

fn adopt_typed_graph(
    state: &RuntimeState,
    id: String,
    parent_id: Option<String>,
    outlet_id: String,
    graph_id: String,
    route_id: Option<String>,
    match_key: Option<String>,
    root: Element,
    path: String,
    loader_data: Option<&RuntimeValue>,
    route_params: Option<&HashMap<String, String>>,
) -> Result<(), JsValue> {
    state.ensure_typed_instance_absent(
        &id,
        &format!("route adoption (graph {graph_id}, outlet {outlet_id})"),
    )?;
    let graph = state
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
    let mut runtime = TypedRuntime::new_with_tag_policy(
        app,
        state.region_tracker.clone(),
        state.reconcile_budget.clone(),
        state.cookie_policy.clone(),
        state.effective_tag_policy(),
    )?;
    runtime.set_component_definitions(graph.components);
    runtime.host_registry = state.host_registry.clone();
    runtime.host_dispatch = Some(state.clone());
    runtime.ssr_imported = *state.typed_ssr_imported.borrow();
    // Adopted instances keep emitting addresses under the claimed path:
    // branch flips and delta rows must match the server grammar.
    runtime.path = path.clone();
    let host_inputs = if parent_id.is_none() {
        state.typed_host_inputs.borrow().clone()
    } else {
        state.typed_host_inputs_for_location(loader_data, route_params, None)?
    };
    runtime.set_host_inputs(host_inputs)?;
    runtime.graph_generation = state.next_typed_generation();
    let branches = state
        .typed_ssr_branches
        .borrow()
        .get(&id)
        .cloned()
        .unwrap_or_default();
    let loops = state
        .typed_ssr_loops
        .borrow()
        .get(&id)
        .cloned()
        .unwrap_or_default();
    // Nested component records below this instance's marker path: each
    // adopted component extracts its own entry when its request is
    // queued, so the records follow the instance chain they describe.
    let nested_prefix = format!("{path}/");
    let nested = state
        .typed_ssr_nested
        .borrow()
        .iter()
        .filter(|(nested_path, _)| nested_path.starts_with(&nested_prefix))
        .map(|(nested_path, value)| (nested_path.clone(), value.clone()))
        .collect();
    runtime.adopt(
        root.clone(),
        TypedAdoptionScope::Element(root),
        &path,
        &branches,
        &loops,
        &nested,
    )?;
    state.typed.borrow_mut().insert(
        id,
        TypedGraphInstance {
            parent_id,
            outlet_id,
            graph_id,
            route_id,
            match_key,
            route_state: None,
            loader_data: loader_data.cloned(),
            loader_runtime: None,
            component_call: None,
            component_start: None,
            runtime,
        },
    );
    Ok(())
}

fn typed_match_key(
    route_id: &str,
    params: &HashMap<String, String>,
    location: &TypedLocation,
) -> String {
    let mut params = params.iter().collect::<Vec<_>>();
    params.sort_by(|left, right| left.0.cmp(right.0));
    format!(
        "{route_id}\n{}\n{}",
        location.pathname,
        params
            .into_iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join("&")
    )
}

/// Compares the server-published snapshot chain with the chain re-derived
/// from the current URL. Loader outcomes must pair with the phase that
/// rendered them: `active` requires a resolved outcome, `error` a rejected
/// one, `notFound` a not-found outcome or the owner's own resolved one.
enum SsrChainAgreement {
    /// The snapshot chain equals the URL-derived chain.
    Exact,
    /// The snapshot truncates at a not-found boundary owner (`depth` entries).
    /// The truncated suffix never commits; the owner adopts its boundary.
    NotFoundBoundary(usize),
    /// The snapshot renders only the root not-found boundary.
    RootNotFoundBoundary,
    /// A contract failure (`mismatch:ssr-route-chain:{detail}`).
    Mismatch(String),
}

fn ssr_chain_agreement(
    imported: &[plec_ir::SsrRouteInstance],
    derived: &[TypedRouteMatch],
    loaders: &HashMap<String, plec_ir::SsrLoaderState>,
    manifest: &RouteManifest,
) -> SsrChainAgreement {
    if imported.is_empty()
        && derived.is_empty()
        && manifest.root_not_found_graph_id.is_some()
    {
        return SsrChainAgreement::RootNotFoundBoundary;
    }
    if imported.len() > derived.len() {
        return SsrChainAgreement::Mismatch(format!(
            "length:{}:{}",
            imported.len(),
            derived.len()
        ));
    }
    if imported.len() < derived.len() {
        // Truncation is only legitimate as a not-found boundary: the last
        // imported entry must be the derived route at the same position,
        // must own a not-found boundary, and must carry the NotFound phase.
        // An empty chain is only legitimate as the root boundary.
        if imported.is_empty() {
            return if manifest.root_not_found_graph_id.is_some() {
                SsrChainAgreement::RootNotFoundBoundary
            } else {
                SsrChainAgreement::Mismatch("root-boundary".into())
            };
        }
        let last = &imported[imported.len() - 1];
        let owner = &derived[imported.len() - 1];
        let owns_boundary = last.phase == plec_ir::SsrRoutePhase::NotFound
            && last.route_id == owner.route.id
            && owner.route.not_found_graph_id.is_some();
        return if owns_boundary {
            SsrChainAgreement::NotFoundBoundary(imported.len())
        } else {
            SsrChainAgreement::Mismatch(format!(
                "length:{}:{}",
                imported.len(),
                derived.len()
            ))
        };
    }
    if let Some(last) = imported.last() {
        if last.phase == plec_ir::SsrRoutePhase::NotFound {
            let owner = &derived[imported.len() - 1];
            return if last.route_id == owner.route.id
                && owner.route.not_found_graph_id.is_some()
            {
                SsrChainAgreement::NotFoundBoundary(imported.len())
            } else {
                SsrChainAgreement::Mismatch(format!(
                    "not-found-owner:{}",
                    last.route_id
                ))
            };
        }
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
            // A not-found phase pairs with its not-found outcome (the owner
            // raised the outcome itself) or with the owner's own resolved
            // outcome (a deeper route raised it). No outcome is valid for a
            // boundary owner without a loader.
            (
                plec_ir::SsrRoutePhase::NotFound,
                Some(Some(
                    plec_ir::SsrLoaderState::NotFound
                    | plec_ir::SsrLoaderState::Resolved { .. },
                )),
            )
            | (plec_ir::SsrRoutePhase::NotFound, None) => None,
            (plec_ir::SsrRoutePhase::NotFound, _) => Some("not-found"),
        };
        if let Some(phase) = phase {
            return SsrChainAgreement::Mismatch(format!("phase:{index}:{phase}"));
        }
        if instance.route_id != matched.route.id {
            return SsrChainAgreement::Mismatch(format!("route:{index}:{}", instance.route_id));
        }
        for (key, value) in &instance.params {
            if matched.params.get(key).map(String::as_str) != Some(value.as_str()) {
                return SsrChainAgreement::Mismatch(format!("params:{index}:{key}"));
            }
        }
        if matched.params.len() != instance.params.len() {
            return SsrChainAgreement::Mismatch(format!("params:{index}:count"));
        }
    }
    SsrChainAgreement::Exact
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
    match_route_chain(manifest, pathname).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirect_hop_budget_accumulates_across_deferred_hops() {
        // Each call represents a redirect outcome after an asynchronous
        // loader/graph wait; the chain must retain its accumulated count.
        let mut depth = 0;
        for _ in 0..plec_ir::limits::MAX_REDIRECT_HOPS {
            depth = next_redirect_depth(depth, "/next").unwrap();
        }
        assert_eq!(depth, plec_ir::limits::MAX_REDIRECT_HOPS);
        assert!(next_redirect_depth(depth, "/loop")
            .unwrap_err()
            .contains("redirect loop exceeded"));
    }

    fn route(id: &str, parent: Option<&str>, path: &str) -> RouteManifestEntry {
        RouteManifestEntry {
            id: id.into(),
            parent_id: parent.map(str::to_owned),
            path: path.into(),
            graph_id: id.into(),
            pending_graph_id: None,
            pending_mode: "replace".into(),
            error_graph_id: None,
            not_found_graph_id: None,
            outlet_id: "main".into(),
            loader_action: None,
        }
    }
    #[test]
    fn typed_matching_prefers_static_nested_routes_and_decodes_params() {
        let manifest = RouteManifest {
            version: Some(3),
            root_graph_id: "root".into(),
            root_not_found_graph_id: None,
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
            root_not_found_graph_id: None,
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
        // Query-only navigation preserves the route instance; the search
        // input updates independently of route match identity.
        assert_eq!(first, search);
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

    fn agreement(
        imported: &[plec_ir::SsrRouteInstance],
        derived: &[TypedRouteMatch],
        manifest: &RouteManifest,
    ) -> SsrChainAgreement {
        ssr_chain_agreement(imported, derived, &HashMap::new(), manifest)
    }

    #[test]
    fn snapshot_chain_agrees_for_flat_param_catch_all_and_nested_chains() {
        // Flat multi-segment $param route: the server matcher and
        // typed_route_chain agree on the route and decoded params.
        let manifest = RouteManifest {
            version: Some(3),
            root_graph_id: "root".into(),
            root_not_found_graph_id: None,
            routes: vec![
                route("home", None, ""),
                route("project", None, "projects/$projectId"),
                route("missing", None, "*"),
            ],
        };
        let derived = typed_route_chain(&manifest, "/projects/a%20b");
        assert!(matches!(
            agreement(
                &[instance("project", &[("projectId", "a b")])],
                &derived,
                &manifest
            ),
            SsrChainAgreement::Exact
        ));

        // Catch-all agreement: an unmatched path resolves to the same
        // fallback route on both sides.
        let derived = typed_route_chain(&manifest, "/nowhere");
        assert!(matches!(
            agreement(&[instance("missing", &[])], &derived, &manifest),
            SsrChainAgreement::Exact
        ));

        // Nested pathless-index chain agreement when the snapshot carries the
        // full descent.
        let nested = RouteManifest {
            version: Some(3),
            root_graph_id: "root".into(),
            root_not_found_graph_id: None,
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
        assert!(matches!(
            agreement(
                &[instance("layout", &[]), instance("home", &[])],
                &derived,
                &nested
            ),
            SsrChainAgreement::Exact
        ));
    }

    #[test]
    fn snapshot_chain_mismatches_pin_server_and_typed_matcher_gaps() {
        let nested = RouteManifest {
            version: Some(3),
            root_graph_id: "root".into(),
            root_not_found_graph_id: None,
            routes: vec![route("layout", None, ""), route("home", Some("layout"), "")],
        };
        // A truncated transferred chain is malformed and must be detectable,
        // never silently adopted: the truncated route owns no boundary.
        let derived = typed_route_chain(&nested, "/");
        assert!(matches!(
            agreement(&[instance("layout", &[])], &derived, &nested),
            SsrChainAgreement::Mismatch(_)
        ));
        // Param value disagreement (server rendered one id, URL holds another).
        let manifest = RouteManifest {
            version: Some(3),
            root_graph_id: "root".into(),
            root_not_found_graph_id: None,
            routes: vec![route("project", None, "projects/$projectId")],
        };
        let derived = typed_route_chain(&manifest, "/projects/a");
        assert!(matches!(
            agreement(
                &[instance("project", &[("projectId", "b")])],
                &derived,
                &manifest
            ),
            SsrChainAgreement::Mismatch(_)
        ));
        // Extra derived params (snapshot claims none for a $param route).
        assert!(matches!(
            agreement(&[instance("project", &[])], &derived, &manifest),
            SsrChainAgreement::Mismatch(_)
        ));
        // Route id disagreement at the same position.
        assert!(matches!(
            agreement(&[instance("home", &[])], &derived, &manifest),
            SsrChainAgreement::Mismatch(_)
        ));
        // Reserved phases cannot adopt as active.
        let mut reserved = instance("project", &[("projectId", "a")]);
        reserved.phase = plec_ir::SsrRoutePhase::Error;
        assert!(matches!(
            agreement(&[reserved], &derived, &manifest),
            SsrChainAgreement::Mismatch(_)
        ));
    }

    #[test]
    fn truncated_chain_is_only_legitimate_as_a_not_found_boundary() {
        // A boundary-owning route may truncate the chain behind it.
        let mut projects = route("projects", None, "projects");
        projects.not_found_graph_id = Some("projects-404".into());
        let manifest = RouteManifest {
            version: Some(3),
            root_graph_id: "root".into(),
            root_not_found_graph_id: None,
            routes: vec![projects, route("settings", Some("projects"), "settings")],
        };
        let derived = typed_route_chain(&manifest, "/projects/settings");
        let mut owner_instance = instance("projects", &[]);
        owner_instance.phase = plec_ir::SsrRoutePhase::NotFound;
        assert!(matches!(
            agreement(&[owner_instance], &derived, &manifest),
            SsrChainAgreement::NotFoundBoundary(1)
        ));
        let mut self_owner_instance = instance("projects", &[]);
        self_owner_instance.phase = plec_ir::SsrRoutePhase::NotFound;
        assert!(matches!(
            agreement(
                &[self_owner_instance],
                &typed_route_chain(&manifest, "/projects"),
                &manifest
            ),
            SsrChainAgreement::NotFoundBoundary(1)
        ));

        // The truncation must end at a route the snapshot names with the
        // NotFound phase; an Active truncation is a contract failure.
        assert!(matches!(
            agreement(&[instance("home", &[])], &derived, &manifest),
            SsrChainAgreement::Mismatch(_)
        ));

        // An empty chain is only legitimate as the root boundary.
        let mut root_boundary = manifest.clone();
        root_boundary.root_not_found_graph_id = Some("root-404".into());
        assert!(matches!(
            agreement(&[], &derived, &root_boundary),
            SsrChainAgreement::RootNotFoundBoundary
        ));
        assert!(matches!(
            agreement(
                &[],
                &[],
                &RouteManifest {
                    version: Some(3),
                    root_graph_id: "root".into(),
                    root_not_found_graph_id: Some("root-404".into()),
                    routes: Vec::new(),
                }
            ),
            SsrChainAgreement::RootNotFoundBoundary
        ));
        assert!(matches!(
            agreement(&[], &derived, &manifest),
            SsrChainAgreement::Mismatch(_)
        ));
    }
}
