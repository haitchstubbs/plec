use std::collections::HashMap;

use plec_hir::{ComponentId, HirApplication, HirRoute, HirRouteApplication, HirRouteMetadata};
use plec_ir::{
    ActionInstruction, ActionProgram, CapabilityRequest, ComponentApplication,
    ExpressionInstruction, ExpressionProgram, ReturnOutcome, RouteManifest, RouteManifestEntry,
    RouteMetadata, RouteOutlet, StateSlot, Value,
};
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
    let root = routes
        .iter()
        .find(|route| route.parent.is_none())
        .ok_or_else(|| RouteError("route tree has no root route".into()))?;
    if routes.iter().filter(|route| route.parent.is_none()).count() != 1 {
        return Err(RouteError("route tree must have one root route".into()));
    }
    Ok(HirRouteApplication {
        root: root.component.clone(),
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
    for route in &routes.routes {
        phases.push(route.component.clone());
        phases.extend(route.pending_component.clone());
        phases.extend(route.error_component.clone());
    }
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
        let url = static_route_loader_url(modules, route)?;
        let artifact = artifacts
            .iter_mut()
            .find(|artifact| artifact.graph_id == graph_id(&route.component))
            .ok_or_else(|| RouteError("route loader graph is missing".into()))?;
        let action = attach_route_loader(&mut artifact.graph, url)?;
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

/// Compile the deliberately narrow loader subset needed for the experiment:
/// an inline or named route loader whose body contains a static `fetch(url)`.
/// The runtime owns cancellation, status handling, JSON decoding, and route
/// phase transitions, so author-provided `signal` plumbing is unnecessary in
/// the action graph.
fn static_route_loader_url(
    modules: &[ParsedModule],
    route: &HirRoute,
) -> Result<String, RouteError> {
    let (module_id, local) = route
        .id
        .rsplit_once('#')
        .ok_or_else(|| RouteError("route loader id is invalid".into()))?;
    let module = modules
        .iter()
        .find(|module| module.id == module_id)
        .ok_or_else(|| RouteError("route loader module is missing".into()))?;
    let loader = module
        .ast
        .body
        .iter()
        .filter_map(exported_var)
        .flat_map(|declaration| declaration.decls.iter())
        .find(|declaration| matches!(&declaration.name, Pat::Ident(name) if name.id.sym == *local))
        .and_then(|declaration| declaration.init.as_deref())
        .and_then(|expression| match expression {
            Expr::Call(call) if callee_name(&call.callee) == Some("createRoute") => {
                call.args.first()
            }
            _ => None,
        })
        .and_then(|argument| match argument.expr.as_ref() {
            Expr::Object(options) => prop(&options.props, "loader"),
            _ => None,
        })
        .ok_or_else(|| RouteError("route loader declaration is missing".into()))?;
    fetch_url_from_loader(loader)
        .ok_or_else(|| RouteError("route loader must contain a fetch() with a static URL".into()))
}

fn fetch_url_from_loader(loader: &Expr) -> Option<String> {
    match loader {
        Expr::Arrow(arrow) => match arrow.body.as_ref() {
            ArrowFunctionBody::Expr(expression) => fetch_url_from_expression(expression),
            ArrowFunctionBody::FunctionBody(body) => fetch_url_from_statements(&body.stmts),
        },
        Expr::Fn(function) => function
            .function
            .body
            .as_ref()
            .and_then(|body| fetch_url_from_statements(&body.stmts)),
        Expr::Ident(_) => None,
        _ => fetch_url_from_expression(loader),
    }
}

fn fetch_url_from_statements(statements: &[Stmt]) -> Option<String> {
    statements.iter().find_map(|statement| match statement {
        Stmt::Decl(Decl::Var(declaration)) => declaration.decls.iter().find_map(|declaration| {
            declaration
                .init
                .as_deref()
                .and_then(fetch_url_from_expression)
        }),
        Stmt::Expr(expression) => fetch_url_from_expression(&expression.expr),
        Stmt::Return(returned) => returned.arg.as_deref().and_then(fetch_url_from_expression),
        Stmt::Block(block) => fetch_url_from_statements(&block.stmts),
        _ => None,
    })
}

fn fetch_url_from_expression(expression: &Expr) -> Option<String> {
    match expression {
        Expr::Await(awaited) => fetch_url_from_expression(&awaited.arg),
        Expr::Call(call) if callee_name(&call.callee) == Some("fetch") => call
            .args
            .first()
            .and_then(|argument| match argument.expr.as_ref() {
                Expr::Lit(swc_ecma_ast::Lit::Str(url)) => {
                    Some(url.value.to_string_lossy().into_owned())
                }
                _ => None,
            }),
        Expr::Paren(parenthesized) => fetch_url_from_expression(&parenthesized.expr),
        Expr::TsAs(assertion) => fetch_url_from_expression(&assertion.expr),
        Expr::TsTypeAssertion(assertion) => fetch_url_from_expression(&assertion.expr),
        _ => None,
    }
}

fn attach_route_loader(
    application: &mut ComponentApplication,
    url: String,
) -> Result<usize, RouteError> {
    let component = application
        .components
        .get_mut(application.root_component)
        .ok_or_else(|| RouteError("route loader root component is missing".into()))?;
    let url_constant = component.constants.len();
    component.constants.push(Value::String(url));
    let url_expression = component.expressions.len();
    component.expressions.push(ExpressionProgram {
        instructions: vec![
            ExpressionInstruction::Constant {
                constant: url_constant,
            },
            ExpressionInstruction::Return,
        ],
    });
    let error_expression = component.expressions.len();
    component.expressions.push(ExpressionProgram {
        instructions: vec![
            ExpressionInstruction::LoadFrame { slot: 1 },
            ExpressionInstruction::Return,
        ],
    });
    let null_constant = component.constants.len();
    component.constants.push(Value::Null);
    let null_expression = component.expressions.len();
    component.expressions.push(ExpressionProgram {
        instructions: vec![
            ExpressionInstruction::Constant {
                constant: null_constant,
            },
            ExpressionInstruction::Return,
        ],
    });
    let loader_state = component.state_slots.len();
    component.state_slots.push(StateSlot {
        initial_expression: null_expression,
        frame_slot: loader_state,
    });
    // `responseJson` stays an action-local transport envelope. The shared
    // client/server loader executor returns it, then each host exports `body`
    // at its loader-data boundary.
    let result_expression = component.expressions.len();
    component.expressions.push(ExpressionProgram {
        instructions: vec![
            ExpressionInstruction::LoadFrame { slot: 0 },
            ExpressionInstruction::Return,
        ],
    });
    let loader_action = component.actions.len();
    component.actions.push(ActionProgram {
        frame_slots: 2,
        parameter_slots: vec![],
        loader_result_state: Some(loader_state),
        route_loader: true,
        instructions: vec![
            ActionInstruction::CapabilityRequest {
                request: CapabilityRequest::Fetch {
                    url: url_expression,
                    method: "GET",
                    headers: vec![],
                    body: None,
                    decode: "responseJson",
                    require_ok: true,
                },
                success_pc: 1,
                failure_pc: 2,
                finally_pc: None,
                result_slot: 0,
                error_slot: 1,
            },
            ActionInstruction::Return {
                outcome: ReturnOutcome::Success,
                value: Some(result_expression),
            },
            ActionInstruction::Return {
                outcome: ReturnOutcome::Failure,
                value: Some(error_expression),
            },
        ],
    });
    Ok(loader_action)
}

fn stable_revision(artifacts: &[RouteArtifact]) -> String {
    // FNV-1a is intentionally small and deterministic; this is a cache-bust
    // revision, not a security digest.
    let mut hash = 0xcbf29ce484222325_u64;
    for artifact in artifacts {
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
    for route in &routes.routes {
        roots.push(route.component.clone());
        roots.extend(route.pending_component.clone());
        roots.extend(route.error_component.clone());
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
    }
}
