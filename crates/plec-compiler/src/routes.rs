use std::collections::HashMap;

use plec_hir::{ComponentId, HirApplication, HirRoute, HirRouteApplication, HirRouteMetadata};
use plec_ir::{ActionProgram, ComponentApplication, RouteManifest, RouteManifestEntry, RouteMetadata, RouteOutlet};
use plec_model::{resolve_local_symbol, SemanticGraph};
use plec_parser::ParsedModule;
use serde::Serialize;
use swc_ecma_ast::{
    ArrowFunctionBody, Callee, Decl, Expr, KeyValueProp, ModuleItem, Pat, Prop, PropName,
    PropOrSpread, Stmt, VarDecl,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteError(pub String);

impl std::fmt::Display for RouteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for RouteError {}

/// The complete Rust-owned browser build input.  Graphs intentionally remain
/// independent artifacts: a mounted layout keeps its definition while a route
/// outlet can receive a newly fetched graph with the same component closure.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteArtifactBundle {
    pub manifest: RouteManifest,
    pub graphs: Vec<RouteArtifact>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteArtifact {
    pub graph_id: String,
    pub graph: ComponentApplication,
}

/// Discover the deliberately static Plec route surface. Route construction is
/// source semantics; the runtime only receives this lowered representation.
pub fn lower_routes(
    modules: &[ParsedModule],
    graph: &SemanticGraph,
) -> Result<HirRouteApplication, RouteError> {
    let mut routes = Vec::new();
    let mut route_locals = HashMap::new();
    let mut has_router = false;
    for module in modules {
        has_router |= module.ast.body.iter().any(has_create_router);
        for item in &module.ast.body {
            let Some(var) = exported_var(item) else {
                continue;
            };
            for declaration in &var.decls {
                let Pat::Ident(name) = &declaration.name else {
                    continue;
                };
                let Some(Expr::Call(call)) = declaration.init.as_deref() else {
                    continue;
                };
                let Some(factory) = callee_name(&call.callee) else {
                    continue;
                };
                if factory != "createRootRoute" && factory != "createRoute" {
                    continue;
                }
                let options = call
                    .args
                    .first()
                    .and_then(|arg| match arg.expr.as_ref() {
                        Expr::Object(value) => Some(value),
                        _ => None,
                    })
                    .ok_or_else(|| {
                        RouteError(format!(
                            "{} must receive a static route options object",
                            name.id.sym
                        ))
                    })?;
                let component =
                    component_option(options.props.as_slice(), "component", module, graph)?;
                let parent = if factory == "createRootRoute" {
                    None
                } else {
                    parent_option(options.props.as_slice(), module, graph)?
                };
                let path = string_option(options.props.as_slice(), "path")?.unwrap_or_default();
                let pending_component = optional_component_option(
                    options.props.as_slice(),
                    "pendingComponent",
                    module,
                    graph,
                )?;
                let pending_mode = pending_mode_option(options.props.as_slice())?;
                let error_component = optional_component_option(
                    options.props.as_slice(),
                    "errorComponent",
                    module,
                    graph,
                )?;
                let not_found_component = optional_component_option(
                    options.props.as_slice(),
                    "notFoundComponent",
                    module,
                    graph,
                )?;
                let loader = optional_ident_option(options.props.as_slice(), "loader")?;
                let outlet_id = string_option(options.props.as_slice(), "outletId")?
                    .unwrap_or_else(|| "main".into());
                let metadata = route_metadata_option(options.props.as_slice())?;
                let id = format!("{}#{}", module.id, name.id.sym);
                route_locals.insert((module.id.clone(), name.id.sym.to_string()), id.clone());
                routes.push(HirRoute {
                    id,
                    parent,
                    path,
                    component,
                    pending_component,
                    pending_mode,
                    error_component,
                    not_found_component,
                    loader,
                    outlet_id,
                    metadata,
                });
            }
        }
    }
    if !has_router {
        return Err(RouteError("createRouter({ routeTree }) is required".into()));
    }
    for route in &mut routes {
        if let Some(parent) = &route.parent {
            let (module, local) = parent.split_once('#').ok_or_else(|| {
                RouteError("route parent must be an imported Route symbol".into())
            })?;
            route.parent = route_locals.get(&(module.into(), local.into())).cloned();
            if route.parent.is_none() {
                return Err(RouteError("route parent is not a discovered route".into()));
            }
        }
    }
    // Source graph traversal may discover modules in different equivalent
    // orders. Keep manifest and phase artifact output stable across processes.
    routes.sort_unstable_by(|left, right| left.id.cmp(&right.id));
    let root = routes
        .iter()
        .find(|route| route.parent.is_none())
        .ok_or_else(|| RouteError("route tree has no root route".into()))?;
    if routes.iter().filter(|route| route.parent.is_none()).count() != 1 {
        return Err(RouteError("route tree must have one root route".into()));
    }
    Ok(HirRouteApplication {
        root: root.component.clone(),
        root_not_found_component: root.not_found_component.clone(),
        routes,
    })
}

pub fn lower_route_manifest(routes: &HirRouteApplication) -> RouteManifest {
    let root_route = routes
        .routes
        .iter()
        .find(|route| route.parent.is_none())
        .expect("route application has one root route");
    RouteManifest {
        version: 3,
        revision: "rust-route-v1".into(),
        root_graph_id: graph_id(&routes.root),
        root_not_found_graph_id: routes.root_not_found_component.as_ref().map(graph_id),
        routes: routes
            .routes
            .iter()
            // The root graph is mounted independently and persistently by the
            // runtime. Emitting its route would mount the same layout again in
            // its own outlet.
            .filter(|route| route.id != root_route.id)
            .map(|route| RouteManifestEntry {
                id: route.id.clone(),
                parent_id: (route.parent.as_deref() != Some(root_route.id.as_str()))
                    .then(|| route.parent.clone())
                    .flatten(),
                path: route.path.clone(),
                graph_id: graph_id(&route.component),
                pending_graph_id: route.pending_component.as_ref().map(graph_id),
                pending_mode: route.pending_mode.clone(),
                error_graph_id: route.error_component.as_ref().map(graph_id),
                not_found_graph_id: route.not_found_component.as_ref().map(graph_id),
                // Loader action zero is reserved by route-graph lowering; callers
                // cannot supply this runtime handle.
                loader_action: route.loader.as_ref().map(|_| 0),
                outlet_id: route.outlet_id.clone(),
                meta: (!route.metadata.title.is_none() || !route.metadata.description.is_none())
                    .then(|| RouteMetadata {
                        title: route.metadata.title.clone(),
                        description: route.metadata.description.clone(),
                    }),
            })
            .collect(),
    }
}

/// Compile every independently mountable route phase.  Unlike the historical
/// registry artifact, each output is self-contained and has a root component
/// matching its graph id.  This is the artifact boundary used by lazy browser
/// loading.
pub fn lower_route_artifacts(
    modules: &[ParsedModule],
    graph: &SemanticGraph,
    routes: &HirRouteApplication,
) -> Result<RouteArtifactBundle, RouteError> {
    lower_route_artifacts_with_options(modules, graph, routes, &std::collections::BTreeSet::new())
}

/// Like [`lower_route_artifacts`], but compiles intrinsic JSX elements under
/// the configured trusted custom-element list.
pub fn lower_route_artifacts_with_options(
    modules: &[ParsedModule],
    graph: &SemanticGraph,
    routes: &HirRouteApplication,
    custom_elements: &std::collections::BTreeSet<String>,
) -> Result<RouteArtifactBundle, RouteError> {
    let mut phases = vec![routes.root.clone()];
    if let Some(not_found) = &routes.root_not_found_component {
        phases.push(not_found.clone());
    }
    for route in &routes.routes {
        phases.push(route.component.clone());
        phases.extend(route.pending_component.clone());
        phases.extend(route.error_component.clone());
        phases.extend(route.not_found_component.clone());
    }
    phases.sort_by(|left, right| {
        left.module_id
            .cmp(&right.module_id)
            .then_with(|| left.local_name.cmp(&right.local_name))
    });
    phases.dedup();

    let mut artifacts = Vec::new();
    for phase in phases {
        let root = crate::discover_root_component(
            modules,
            graph,
            &phase.module_id,
            Some(&phase.local_name),
        )
        .map_err(|error| RouteError(error.to_string()))?;
        let application =
            crate::lower_application_with_options(modules, &root, graph, custom_elements)
                .map_err(|error| RouteError(error.to_string()))?;
        let executable = crate::lower_application_to_executable(&application)
            .map_err(|error| RouteError(error.to_string()))?;
        artifacts.push(RouteArtifact {
            graph_id: graph_id(&phase),
            graph: executable,
        });
    }

    // Route outlets belong to the persistent parent graph, never to a merged
    // application registry.  A child replacement therefore cannot recreate
    // its parent layout.
    for route in routes.routes.iter().filter(|route| route.parent.is_some()) {
        let parent = route
            .parent
            .as_ref()
            .and_then(|id| routes.routes.iter().find(|candidate| &candidate.id == id));
        let parent_component = parent.map(|route| &route.component).unwrap_or(&routes.root);
        let artifact = artifacts
            .iter_mut()
            .find(|artifact| artifact.graph_id == graph_id(parent_component))
            .ok_or_else(|| RouteError("route parent artifact is missing".into()))?;
        let root = artifact
            .graph
            .components
            .get_mut(artifact.graph.root_component)
            .ok_or_else(|| RouteError("route artifact root component is missing".into()))?;
        if !root
            .route_outlets
            .iter()
            .any(|outlet| outlet.id == route.outlet_id)
        {
            root.route_outlets.push(RouteOutlet {
                id: route.outlet_id.clone(),
                node: root.root_node,
            });
        }
    }

    let mut manifest = lower_route_manifest(routes);
    for route in routes.routes.iter().filter(|route| route.loader.is_some()) {
        let loader = crate::loader::loader_declaration(modules, route)?;
        let artifact = artifacts
            .iter_mut()
            .find(|artifact| artifact.graph_id == graph_id(&route.component))
            .ok_or_else(|| RouteError("route loader graph is missing".into()))?;
        let (action, uses_not_found) = attach_route_loader(&mut artifact.graph, loader)?;
        if uses_not_found && !chain_declares_not_found_boundary(routes, route) {
            return Err(RouteError(format!(
                "route {} throws notFound() without a notFoundComponent on this route, an ancestor, or the root route",
                route.id
            )));
        }
        manifest
            .routes
            .iter_mut()
            .find(|entry| entry.id == route.id)
            .ok_or_else(|| RouteError("route loader manifest entry is missing".into()))?
            .loader_action = Some(action);
    }
    // A revision must be stable across processes and derived from the Rust
    // compiler's canonical route/graph content rather than a JS build step.
    manifest.revision = stable_revision(&artifacts);
    Ok(RouteArtifactBundle {
        manifest,
        graphs: artifacts,
    })
}

/// Compiles the loader body into a terminal loader action on the route
/// component graph. See `crate::loader` for the grammar contract.
/// `uses_not_found` reports whether the loader can resolve as not found so
/// the caller can require a declared boundary.
fn attach_route_loader(
    graph: &mut ComponentApplication,
    loader: &Expr,
) -> Result<(usize, bool), RouteError> {
    let component = graph
        .components
        .get_mut(graph.root_component)
        .ok_or_else(|| RouteError("route loader root component is missing".into()))?;
    let compiled = crate::loader::compile_loader(
        loader,
        crate::loader::LoaderPools {
            strings: &mut component.strings,
            constants: &mut component.constants,
            expressions: &mut component.expressions,
            state_slots: &mut component.state_slots,
        },
    )?;
    let action = component.actions.len();
    component.actions.push(ActionProgram {
        frame_slots: compiled.frame_slots,
        parameter_slots: compiled.parameter_slots,
        loader_result_state: compiled.loader_result_state,
        route_loader: true,
        loader_decode_body: compiled.loader_decode_body,
        instructions: compiled.instructions,
    });
    Ok((action, compiled.uses_not_found))
}

fn stable_revision(artifacts: &[RouteArtifact]) -> String {
    // FNV-1a is intentionally small and deterministic; this is a cache-bust
    // revision, not a security digest.
    let mut hash = 0xcbf29ce484222325_u64;
    // Route phase discovery can produce equivalent artifacts in different
    // orders. Hash by graph identity so revision does not depend on that order.
    let mut ordered = artifacts.iter().collect::<Vec<_>>();
    ordered.sort_unstable_by(|left, right| left.graph_id.cmp(&right.graph_id));
    for artifact in ordered {
        let json = serde_json::to_vec(artifact).expect("route artifacts serialize");
        for byte in json {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    format!("rust-route-{hash:016x}")
}

/// Lower every statically reachable route phase into one component registry.
/// Route manifest graph IDs are canonical component IDs, so the runtime can
/// select an already-validated component without consulting JavaScript.
pub fn lower_route_application_to_executable(
    modules: &[ParsedModule],
    graph: &SemanticGraph,
    routes: &HirRouteApplication,
) -> Result<ComponentApplication, RouteError> {
    let mut roots = vec![routes.root.clone()];
    if let Some(not_found) = &routes.root_not_found_component {
        roots.push(not_found.clone());
    }
    for route in &routes.routes {
        roots.push(route.component.clone());
        roots.extend(route.pending_component.clone());
        roots.extend(route.error_component.clone());
        roots.extend(route.not_found_component.clone());
    }
    let mut components = Vec::new();
    for id in roots {
        let root =
            crate::discover_root_component(modules, graph, &id.module_id, Some(&id.local_name))
                .map_err(|error| RouteError(error.to_string()))?;
        let application = crate::lower_application(modules, &root, graph)
            .map_err(|error| RouteError(error.to_string()))?;
        for component in application.components {
            if !components
                .iter()
                .any(|existing: &plec_hir::HirComponent| existing.id == component.id)
            {
                components.push(component);
            }
        }
    }
    let mut executable = crate::lower_application_to_executable(&HirApplication {
        root: routes.root.clone(),
        components,
    })
    .map_err(|error| RouteError(error.to_string()))?;
    for route in routes.routes.iter().filter(|route| route.parent.is_some()) {
        let parent = route
            .parent
            .as_ref()
            .and_then(|id| routes.routes.iter().find(|candidate| &candidate.id == id));
        let parent_component = parent.map(|route| &route.component).unwrap_or(&routes.root);
        let component_id = graph_id(parent_component);
        let component = executable
            .components
            .iter_mut()
            .find(|component| component.id == component_id)
            .ok_or_else(|| RouteError("route parent component is not executable".into()))?;
        if !component
            .route_outlets
            .iter()
            .any(|outlet| outlet.id == route.outlet_id)
        {
            component.route_outlets.push(plec_ir::RouteOutlet {
                id: route.outlet_id.clone(),
                node: component.root_node,
            });
        }
    }
    Ok(executable)
}

/// A not-found outcome must resolve to a declared boundary: this route, any
/// ancestor, or the root route's boundary. Checked at compile time so the
/// runtime never has to guess where a not-found outcome renders.
fn chain_declares_not_found_boundary(routes: &HirRouteApplication, route: &HirRoute) -> bool {
    let mut current = Some(route);
    while let Some(candidate) = current {
        if candidate.not_found_component.is_some() {
            return true;
        }
        current = candidate
            .parent
            .as_ref()
            .and_then(|id| routes.routes.iter().find(|candidate| &candidate.id == id));
    }
    routes.root_not_found_component.is_some()
}

fn graph_id(component: &ComponentId) -> String {
    format!("{}#{}", component.module_id, component.local_name)
}
fn exported_var(item: &ModuleItem) -> Option<&VarDecl> {
    match item {
        ModuleItem::ModuleDecl(swc_ecma_ast::ModuleDecl::ExportDecl(value)) => match &value.decl {
            Decl::Var(value) => Some(value),
            _ => None,
        },
        ModuleItem::Stmt(Stmt::Decl(Decl::Var(value))) => Some(value),
        _ => None,
    }
}
fn has_create_router(item: &ModuleItem) -> bool {
    exported_var(item).map(|var| var.decls.iter().any(|decl| matches!(decl.init.as_deref(), Some(Expr::Call(call)) if callee_name(&call.callee) == Some("createRouter")))).unwrap_or(false)
}
fn callee_name(callee: &Callee) -> Option<&str> {
    match callee {
        Callee::Expr(expr) => match expr.as_ref() {
            Expr::Ident(value) => Some(value.sym.as_ref()),
            _ => None,
        },
        _ => None,
    }
}
fn prop<'a>(props: &'a [PropOrSpread], name: &str) -> Option<&'a Expr> {
    props.iter().find_map(|entry| match entry {
        PropOrSpread::Prop(value) => match value.as_ref() {
            Prop::KeyValue(KeyValueProp { key, value }) if prop_name(key) == Some(name) => {
                Some(value.as_ref())
            }
            _ => None,
        },
        _ => None,
    })
}
fn prop_name(name: &PropName) -> Option<&str> {
    match name {
        PropName::Ident(value) => Some(value.sym.as_ref()),
        PropName::Str(value) => value.value.as_str(),
        _ => None,
    }
}
fn string_option(props: &[PropOrSpread], name: &str) -> Result<Option<String>, RouteError> {
    for entry in props {
        let PropOrSpread::Prop(value) = entry else {
            continue;
        };
        match value.as_ref() {
            Prop::KeyValue(KeyValueProp { key, value }) if prop_name(key) == Some(name) => {
                return match value.as_ref() {
                    Expr::Lit(swc_ecma_ast::Lit::Str(value)) => {
                        Ok(Some(value.value.to_string_lossy().into_owned()))
                    }
                    _ => Err(RouteError(format!("route {name} must be a string literal"))),
                }
            }
            Prop::Shorthand(value) if value.sym == name => {
                return Err(RouteError(format!("route {name} must be a string literal")))
            }
            _ => {}
        }
    }
    Ok(None)
}
fn route_metadata_option(props: &[PropOrSpread]) -> Result<HirRouteMetadata, RouteError> {
    let Some(Expr::Object(meta)) = prop(props, "meta") else {
        return Ok(HirRouteMetadata::default());
    };
    Ok(HirRouteMetadata {
        title: string_option(&meta.props, "title")?,
        description: string_option(&meta.props, "description")?,
    })
}
fn pending_mode_option(props: &[PropOrSpread]) -> Result<String, RouteError> {
    match string_option(props, "pendingMode")?.as_deref() {
        None | Some("replace") => Ok("replace".into()),
        Some("retain") => Ok("retain".into()),
        Some(_) => Err(RouteError(
            "route pendingMode must be 'replace' or 'retain'".into(),
        )),
    }
}
fn optional_ident_option(props: &[PropOrSpread], name: &str) -> Result<Option<String>, RouteError> {
    match prop(props, name) {
        None => Ok(None),
        Some(Expr::Ident(value)) => Ok(Some(value.sym.to_string())),
        Some(Expr::Arrow(_)) | Some(Expr::Fn(_)) => Ok(Some(name.into())),
        Some(_) => Err(RouteError(format!("route {name} must be a local function"))),
    }
}
fn component_option(
    props: &[PropOrSpread],
    name: &str,
    module: &ParsedModule,
    graph: &SemanticGraph,
) -> Result<ComponentId, RouteError> {
    optional_component_option(props, name, module, graph)?
        .ok_or_else(|| RouteError(format!("route {name} is required")))
}
fn optional_component_option(
    props: &[PropOrSpread],
    name: &str,
    module: &ParsedModule,
    graph: &SemanticGraph,
) -> Result<Option<ComponentId>, RouteError> {
    match prop(props, name) {
        None => Ok(None),
        Some(Expr::Ident(value)) => resolve_local_symbol(graph, &module.id, value.sym.as_ref())
            .map(|symbol| Some(ComponentId::new(symbol.module_id, symbol.local_name)))
            .ok_or_else(|| {
                RouteError(format!(
                    "route {name} must reference a resolvable component"
                ))
            }),
        Some(_) => Err(RouteError(format!(
            "route {name} must be a component symbol"
        ))),
    }
}
fn parent_option(
    props: &[PropOrSpread],
    module: &ParsedModule,
    graph: &SemanticGraph,
) -> Result<Option<String>, RouteError> {
    let Some(value) = prop(props, "getParentRoute") else {
        return Err(RouteError("createRoute requires getParentRoute".into()));
    };
    let returned = match value {
        Expr::Arrow(arrow) => match arrow.body.as_ref() {
            ArrowFunctionBody::Expr(expr) => expr.as_ref(),
            _ => {
                return Err(RouteError(
                    "getParentRoute must return a route symbol".into(),
                ))
            }
        },
        _ => {
            return Err(RouteError(
                "getParentRoute must be an arrow function".into(),
            ))
        }
    };
    match returned {
        Expr::Ident(value) => resolve_local_symbol(graph, &module.id, value.sym.as_ref())
            .map(|symbol| Some(format!("{}#{}", symbol.module_id, symbol.local_name)))
            .ok_or_else(|| {
                RouteError("getParentRoute must return a resolvable route symbol".into())
            }),
        _ => Err(RouteError(
            "getParentRoute must return a route symbol".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plec_model::build_semantic_graph;
    use plec_parser::parse_module;

    fn compile_loader_program(source: &str) -> Result<(plec_ir::ActionProgram, Vec<String>), String> {
        let modules = vec![parse_module("routes.tsx", source).unwrap()];
        let graph = build_semantic_graph(&modules, &HashMap::new()).unwrap();
        let routes = lower_routes(&modules, &graph).unwrap();
        let mut bundle = lower_route_artifacts(&modules, &graph, &routes).unwrap();
        let route = routes
            .routes
            .iter()
            .find(|route| route.loader.is_some())
            .ok_or("route loader is missing".to_owned())?;
        let graph_id = format!("routes.tsx#{}", route.component.local_name);
        let artifact = bundle
            .graphs
            .iter_mut()
            .find(|artifact| artifact.graph_id == graph_id)
            .ok_or("loader graph is missing".to_owned())?;
        let component = artifact
            .graph
            .components
            .get(artifact.graph.root_component)
            .ok_or("root component is missing".to_owned())?;
        let action = component
            .actions
            .last()
            .ok_or("loader action is missing".to_owned())?;
        Ok((
            action.clone(),
            component.strings.clone(),
        ))
    }

    #[test]
    fn lowers_conditional_loader_bodies_to_terminal_outcomes() {
        let (action, strings) = compile_loader_program(
            r#"
            function Layout() { return <main />; }
            function Missing() { return <p />; }
            function Page() { return <p />; }
            export const Root = createRootRoute({ component: Layout });
            export const UserRoute = createRoute({
                getParentRoute: () => Root,
                path: 'users/$id',
                component: Page,
                notFoundComponent: Missing,
                loader: async ({ params }) => {
                    const user = await fetch(`/api/users/${params.id}`);
                    if (!user.canView) throw redirect('/login');
                    if (user.missing) throw notFound();
                    return user;
                },
            });
            export const router = createRouter({ routeTree: Root.addChildren([UserRoute]) });
        "#,
        )
        .unwrap();
        // The shared parameter contract: params and location.
        assert_eq!(action.parameter_slots, vec![0, 1]);
        assert!(action.route_loader);
        // Loader fetches decode to the response body.
        assert!(action.loader_decode_body);
        let outcomes: Vec<&plec_ir::ActionInstruction> = action
            .instructions
            .iter()
            .filter(|instruction| matches!(instruction, plec_ir::ActionInstruction::Return { .. }))
            .collect();
        let mut success = 0;
        let mut redirect = false;
        let mut not_found = false;
        for instruction in &outcomes {
            if let plec_ir::ActionInstruction::Return { outcome, .. } = instruction {
                match outcome {
                    plec_ir::ReturnOutcome::Success => success += 1,
                    plec_ir::ReturnOutcome::Redirect => redirect = true,
                    plec_ir::ReturnOutcome::NotFound => not_found = true,
                    plec_ir::ReturnOutcome::Failure => {}
                }
            }
        }
        assert_eq!(success, 1);
        assert!(redirect);
        assert!(not_found);
        // The template URL reads the `id` param through a field access.
        assert!(strings.iter().any(|value| value == "id"));
        assert!(strings.iter().any(|value| value == "body"));
        assert!(strings.iter().any(|value| value == "location"));
    }

    #[test]
    fn compiles_terminal_only_loader_outcomes_without_fetch() {
        for outcome in ["throw redirect('/login');", "throw notFound();"] {
            let source = format!(
                r#"
                function Layout() {{ return <main />; }}
                function Missing() {{ return <p />; }}
                function Page() {{ return <p />; }}
                export const Root = createRootRoute({{ component: Layout, notFoundComponent: Missing }});
                export const UserRoute = createRoute({{
                    getParentRoute: () => Root,
                    path: 'users/$id',
                    component: Page,
                    notFoundComponent: Missing,
                    loader: async () => {{ {outcome} }},
                }});
                export const router = createRouter({{ routeTree: Root.addChildren([UserRoute]) }});
            "#
            );

            let (action, _) = compile_loader_program(&source).unwrap();
            assert!(action.instructions.iter().any(|instruction| matches!(
                instruction,
                plec_ir::ActionInstruction::Return {
                    outcome: plec_ir::ReturnOutcome::Redirect | plec_ir::ReturnOutcome::NotFound,
                    ..
                }
            )));
        }
    }

    #[test]
    fn rejects_loader_without_fetch_or_terminal_outcome() {
        let modules = vec![parse_module(
            "routes.tsx",
            r#"
            function Layout() { return <main />; }
            function Page() { return <p />; }
            export const Root = createRootRoute({ component: Layout });
            export const UserRoute = createRoute({
                getParentRoute: () => Root,
                path: 'users/$id',
                component: Page,
                loader: async () => {},
            });
            export const router = createRouter({ routeTree: Root.addChildren([UserRoute]) });
        "#,
        )
        .unwrap()];
        let graph = build_semantic_graph(&modules, &HashMap::new()).unwrap();
        let routes = lower_routes(&modules, &graph).unwrap();
        let error = lower_route_artifacts(&modules, &graph, &routes)
            .unwrap_err()
            .to_string();
        assert!(error.contains(
            "must await fetch(url) or produce a redirect/not-found outcome"
        ));
    }

    #[test]
    fn rejects_not_found_without_a_declared_boundary() {
        let modules = vec![parse_module(
            "routes.tsx",
            r#"
            function Layout() { return <main />; }
            function Page() { return <p />; }
            export const Root = createRootRoute({ component: Layout });
            export const UserRoute = createRoute({
                getParentRoute: () => Root,
                path: 'users/$id',
                component: Page,
                loader: async () => {
                    const user = await fetch('/api/users/1');
                    if (user.missing) throw notFound();
                    return user;
                },
            });
            export const router = createRouter({ routeTree: Root.addChildren([UserRoute]) });
        "#,
        )
        .unwrap()];
        let graph = build_semantic_graph(&modules, &HashMap::new()).unwrap();
        let routes = lower_routes(&modules, &graph).unwrap();
        let error = lower_route_artifacts(&modules, &graph, &routes).unwrap_err();
        assert!(error
            .to_string()
            .contains("notFoundComponent on this route, an ancestor, or the root route"));
    }

    #[test]
    fn rejects_unsupported_loader_statements_and_expressions() {
        let reject = |body: &str, expected: &str| {
            let source = format!(
                r#"
                function Layout() {{ return <main />; }}
                function Missing() {{ return <p />; }}
                function Page() {{ return <p />; }}
                export const Root = createRootRoute({{ component: Layout, notFoundComponent: Missing }});
                export const UserRoute = createRoute({{
                    getParentRoute: () => Root,
                    path: 'users/$id',
                    component: Page,
                    loader: async ({{ params }}) => {{
                        {body}
                    }},
                }});
                export const router = createRouter({{ routeTree: Root.addChildren([UserRoute]) }});
            "#
            );
            let modules = vec![parse_module("routes.tsx", &source).unwrap()];
            let graph = build_semantic_graph(&modules, &HashMap::new()).unwrap();
            let routes = lower_routes(&modules, &graph).unwrap();
            let error = lower_route_artifacts(&modules, &graph, &routes).unwrap_err();
            assert!(
                error.to_string().contains(expected),
                "expected {expected:?} in {error}"
            );
        };
        reject(
            "const user = await fetch('/api/x'); while (false) {}",
            "loop",
        );
        reject(
            "const user = await fetch('/api/x'); try {} catch {}",
            "try/catch",
        );
        reject(
            "const user = await fetch('/api/x', { method: 'POST' });",
            "supports only the { signal } init",
        );
        reject(
            "const helper = () => 1;",
            "route loaders can only await fetch(url)",
        );
        reject("return fetch('/api/x');", "function call");
        reject(
            "const user = await fetch('/api/x'); return user.map(one => one);",
            "function call",
        );
    }

    #[test]
    fn manifest_carries_not_found_boundaries() {
        let modules = vec![parse_module(
            "routes.tsx",
            r#"
            function Layout() { return <main />; }
            function RootMissing() { return <p />; }
            function Missing() { return <p />; }
            function Page() { return <p />; }
            export const Root = createRootRoute({ component: Layout, notFoundComponent: RootMissing });
            export const UserRoute = createRoute({
                getParentRoute: () => Root,
                path: 'users/$id',
                component: Page,
                notFoundComponent: Missing,
            });
            export const router = createRouter({ routeTree: Root.addChildren([UserRoute]) });
        "#,
        )
        .unwrap()];
        let graph = build_semantic_graph(&modules, &HashMap::new()).unwrap();
        let routes = lower_routes(&modules, &graph).unwrap();
        let manifest = lower_route_manifest(&routes);
        assert_eq!(
            manifest.root_not_found_graph_id.as_deref(),
            Some("routes.tsx#RootMissing")
        );
        assert_eq!(
            manifest.routes[0].not_found_graph_id.as_deref(),
            Some("routes.tsx#Missing")
        );
        // The boundary graphs are emitted as independently mountable phases.
        let bundle = lower_route_artifacts(&modules, &graph, &routes).unwrap();
        assert!(bundle
            .graphs
            .iter()
            .any(|artifact| artifact.graph_id == "routes.tsx#RootMissing"));
        assert!(bundle
            .graphs
            .iter()
            .any(|artifact| artifact.graph_id == "routes.tsx#Missing"));
    }

    #[test]
    fn lowers_static_route_tree_to_a_deterministic_manifest() {
        let modules = vec![parse_module("routes.tsx", r#"
            function Layout() { return <main />; }
            function Home() { return <p />; }
            function Pending() { return <p />; }
            function Failure() { return <p />; }
            export const Root = createRootRoute({ component: Layout });
            export const HomeRoute = createRoute({ getParentRoute: () => Root, path: '', component: Home, loader: async () => await fetch('/data'), pendingComponent: Pending, pendingMode: 'retain', errorComponent: Failure });
            export const router = createRouter({ routeTree: Root.addChildren([HomeRoute]) });
        "#).unwrap()];
        let graph = build_semantic_graph(&modules, &HashMap::new()).unwrap();
        let routes = lower_routes(&modules, &graph).unwrap();
        let manifest = lower_route_manifest(&routes);
        assert_eq!(manifest.version, 3);
        assert_eq!(manifest.root_graph_id, "routes.tsx#Layout");
        assert_eq!(manifest.routes.len(), 1);
        assert_eq!(manifest.routes[0].parent_id.as_deref(), None);
        assert_eq!(manifest.routes[0].loader_action, Some(0));
        assert_eq!(
            manifest.routes[0].pending_graph_id.as_deref(),
            Some("routes.tsx#Pending")
        );
        assert_eq!(manifest.routes[0].pending_mode, "retain");
    }

    #[test]
    fn rejects_dynamic_route_paths() {
        let modules = vec![parse_module("routes.tsx", r#"
            function Layout() { return <main />; }
            const path = 'home';
            export const Root = createRootRoute({ component: Layout });
            export const router = createRouter({ routeTree: Root });
            export const Home = createRoute({ getParentRoute: () => Root, path, component: Layout });
        "#).unwrap()];
        let graph = build_semantic_graph(&modules, &HashMap::new()).unwrap();
        assert!(lower_routes(&modules, &graph)
            .unwrap_err()
            .to_string()
            .contains("path must be a string literal"));
    }

    #[test]
    fn rejects_unknown_pending_modes() {
        let modules = vec![parse_module(
            "routes.tsx",
            r#"
            function Layout() { return <main />; }
            export const Root = createRootRoute({ component: Layout, pendingMode: 'later' });
            export const router = createRouter({ routeTree: Root });
        "#,
        )
        .unwrap()];
        let graph = build_semantic_graph(&modules, &HashMap::new()).unwrap();
        assert!(lower_routes(&modules, &graph)
            .unwrap_err()
            .to_string()
            .contains("pendingMode"));
    }

    #[test]
    fn lowers_route_components_into_one_runtime_registry() {
        let modules = vec![parse_module("routes.tsx", r#"
            export function Layout() { return <main />; }
            export function Child() { return <p>child</p>; }
            export const Root = createRootRoute({ component: Layout });
            export const ChildRoute = createRoute({ getParentRoute: () => Root, path: 'child', component: Child });
            export const router = createRouter({ routeTree: Root.addChildren([ChildRoute]) });
        "#).unwrap()];
        let graph = build_semantic_graph(&modules, &HashMap::new()).unwrap();
        let routes = lower_routes(&modules, &graph).unwrap();
        let application = lower_route_application_to_executable(&modules, &graph, &routes).unwrap();
        assert_eq!(application.version, "0.10");
        assert!(application
            .components
            .iter()
            .any(|component| component.id == "routes.tsx#Layout"));
        assert!(application
            .components
            .iter()
            .any(|component| component.id == "routes.tsx#Child"));
        let layout = application
            .components
            .iter()
            .find(|component| component.id == "routes.tsx#Layout")
            .unwrap();
        assert_eq!(layout.route_outlets[0].id, "main");
    }

    #[test]
    fn lowers_route_search_access_in_a_route_component() {
        let modules = vec![parse_module(
            "routes.tsx",
            r#"
                function Page() {
                    const search = Route.useSearch();
                    return <p>{search.tab}</p>;
                }
                export const Root = createRootRoute({ component: Page });
                export const Home = createRoute({
                    getParentRoute: () => Root,
                    path: 'home',
                    component: Page,
                });
                export const router = createRouter({ routeTree: Root.addChildren([Home]) });
            "#,
        )
        .unwrap()];
        let graph = build_semantic_graph(&modules, &HashMap::new()).unwrap();
        let routes = lower_routes(&modules, &graph).unwrap();
        let artifacts = lower_route_artifacts(&modules, &graph, &routes).unwrap();
        assert!(artifacts.graphs.iter().any(|graph| {
            graph
                .graph
                .components
                .iter()
                .any(|component| component.host_slots.iter().any(|slot| slot.kind == "routeSearch"))
        }));
    }

    #[test]
    fn emits_independent_component_graphs_for_each_route_phase() {
        let modules = vec![parse_module("routes.tsx", r#"
            export function Layout() { return <main />; }
            export function Child() { return <p>child</p>; }
            export function Pending() { return <p>pending</p>; }
            export function Failure() { return <p>failed</p>; }
            export const Root = createRootRoute({ component: Layout });
            export const ChildRoute = createRoute({ getParentRoute: () => Root, path: 'child', component: Child, pendingComponent: Pending, errorComponent: Failure });
            export const router = createRouter({ routeTree: Root.addChildren([ChildRoute]) });
        "#).unwrap()];
        let graph = build_semantic_graph(&modules, &HashMap::new()).unwrap();
        let routes = lower_routes(&modules, &graph).unwrap();
        let artifacts = lower_route_artifacts(&modules, &graph, &routes).unwrap();
        assert_eq!(artifacts.manifest.version, 3);
        assert_eq!(artifacts.graphs.len(), 4);
        assert!(artifacts
            .graphs
            .iter()
            .all(|artifact| artifact.graph.version == "0.10"));
        let layout = artifacts
            .graphs
            .iter()
            .find(|artifact| artifact.graph_id == "routes.tsx#Layout")
            .unwrap();
        assert_eq!(
            layout.graph.components[layout.graph.root_component].route_outlets[0].id,
            "main"
        );
        assert!(artifacts.manifest.revision.starts_with("rust-route-"));
        let reversed = artifacts.graphs.iter().rev().cloned().collect::<Vec<_>>();
        assert_eq!(
            artifacts.manifest.revision,
            stable_revision(&reversed),
            "route revision must not depend on artifact ordering"
        );
    }

    #[test]
    fn lowers_route_component_with_mutation_and_callable_props() {
        let modules = vec![
            parse_module("routes.tsx", r#"
            import { useMutation } from "./hooks";
            type Todo = { id: string; title: string; completed: boolean };
            function Row({ todo, onUpdated }: { todo: Todo; onUpdated(updated: Todo): void }) {
                const update = useMutation(async (patch: { title: string }) => {
                    const response = await fetch(`/api/todos/${encodeURIComponent(todo.id)}`, {
                        method: 'PATCH',
                        headers: { 'content-type': 'application/json' },
                        body: JSON.stringify(patch),
                    });
                    if (!response.ok) throw new Error('rejected');
                    const updated = (await response.json()) as Todo;
                    return updated;
                });
                return <li>{todo.title}{update.pending ? 'Saving…' : ''}</li>;
            }
            function Todos() {
                const [todos, setTodos] = useState([{ id: '1', title: 'x', completed: false }]);
                const create = useMutation(async (title: string) => {
                    const response = await fetch('/api/todos', {
                        method: 'POST',
                        headers: { 'content-type': 'application/json' },
                        body: JSON.stringify({ title }),
                    });
                    if (!response.ok) throw new Error('rejected');
                    const todo = (await response.json()) as Todo;
                    return todo;
                });
                const message = create.error instanceof Error ? create.error.message : 'idle';
                function submit(event: Event) {
                    event.preventDefault();
                    void create.run('next');
                }
                return <form onSubmit={submit}><Row todo={todos[0]} onUpdated={(updated: Todo) => setTodos([updated])} />{message && <p role="alert">{message}</p>}</form>;
            }
            export const Root = createRootRoute({ component: Todos });
            export const router = createRouter({ routeTree: Root });
        "#).unwrap(),
            parse_module("hooks.ts", r#"
            export function useMutation(callback: unknown) { return callback; }
        "#).unwrap(),
        ];
        let resolved_imports = HashMap::from([(
            ("routes.tsx".to_string(), "./hooks".to_string()),
            "hooks.ts".to_string(),
        )]);
        let graph = build_semantic_graph(&modules, &resolved_imports).unwrap();
        let routes = lower_routes(&modules, &graph).unwrap();
        let artifacts = lower_route_artifacts(&modules, &graph, &routes).unwrap();
        assert_eq!(artifacts.graphs.len(), 1);
        // The serialized artifact is loaded by hosts through the schema's
        // typed application contract; mutation instructions must round-trip
        // through that deserialization (camelCase slot fields).
        let serialized = serde_json::to_value(&artifacts.graphs[0].graph).unwrap();
        let loaded: plec_schema::typed::TypedComponentApplication =
            serde_json::from_value(serialized).unwrap();
        assert!(loaded.components.iter().any(|component| {
            component
                .actions
                .iter()
                .flat_map(|action| action.instructions.iter())
                .any(|instruction| {
                    matches!(
                        instruction,
                        plec_schema::typed::TypedActionInstruction::MutationPublish { .. }
                    )
                })
        }));
    }

    #[test]
    fn lowers_route_component_with_typed_mutation_form_handler() {
        let modules = vec![
            parse_module(
                "routes.tsx",
                r#"
                import { useMutation } from "./hooks";

                function TodosPage() {
                    const createTodo = useMutation(async (_event: Event) => {
                        return "created";
                    });
                    const updateTodo = useMutation(async (id: string) => {
                        return id;
                    });
                    return <form onSubmit={createTodo}><button type="submit">Add</button></form>;
                }

                export const Root = createRootRoute({ component: TodosPage });
                export const router = createRouter({ routeTree: Root });
            "#,
            )
            .unwrap(),
            parse_module(
                "hooks.ts",
                r#"
                export function useMutation(callback: unknown) { return callback; }
            "#,
            )
            .unwrap(),
        ];
        let resolved_imports = HashMap::from([(
            ("routes.tsx".to_string(), "./hooks".to_string()),
            "hooks.ts".to_string(),
        )]);
        let graph = build_semantic_graph(&modules, &resolved_imports).unwrap();
        let routes = lower_routes(&modules, &graph).unwrap();

        lower_route_artifacts(&modules, &graph, &routes)
            .expect("typed mutation form handlers should lower in route artifacts");
    }
}
