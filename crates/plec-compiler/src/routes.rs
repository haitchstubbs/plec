use std::collections::HashMap;

use plec_hir::{ComponentId, HirRoute, HirRouteApplication};
use plec_ir::{RouteManifest, RouteManifestEntry};
use plec_parser::ParsedModule;
use plec_sema::{resolve_local_symbol, SemanticGraph};
use swc_ecma_ast::{
    ArrowFunctionBody, Callee, Decl, Expr, KeyValueProp, ModuleItem, Pat, Prop, PropName, PropOrSpread, Stmt,
    VarDecl,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteError(pub String);

impl std::fmt::Display for RouteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { self.0.fmt(f) }
}
impl std::error::Error for RouteError {}

/// Discover the deliberately static Plec route surface. Route construction is
/// source semantics; the runtime only receives this lowered representation.
pub fn lower_routes(modules: &[ParsedModule], graph: &SemanticGraph) -> Result<HirRouteApplication, RouteError> {
    let mut routes = Vec::new();
    let mut route_locals = HashMap::new();
    let mut has_router = false;
    for module in modules {
        has_router |= module.ast.body.iter().any(has_create_router);
        for item in &module.ast.body {
            let Some(var) = exported_var(item) else { continue };
            for declaration in &var.decls {
                let Pat::Ident(name) = &declaration.name else { continue };
                let Some(Expr::Call(call)) = declaration.init.as_deref() else { continue };
                let Some(factory) = callee_name(&call.callee) else { continue };
                if factory != "createRootRoute" && factory != "createRoute" { continue; }
                let options = call.args.first().and_then(|arg| match arg.expr.as_ref() { Expr::Object(value) => Some(value), _ => None })
                    .ok_or_else(|| RouteError(format!("{} must receive a static route options object", name.id.sym)))?;
                let component = component_option(options.props.as_slice(), "component", module, graph)?;
                let parent = if factory == "createRootRoute" { None } else { parent_option(options.props.as_slice(), module, graph)? };
                let path = string_option(options.props.as_slice(), "path")?.unwrap_or_default();
                let pending_component = optional_component_option(options.props.as_slice(), "pendingComponent", module, graph)?;
                let pending_mode = pending_mode_option(options.props.as_slice())?;
                let error_component = optional_component_option(options.props.as_slice(), "errorComponent", module, graph)?;
                let loader = optional_ident_option(options.props.as_slice(), "loader")?;
                let outlet_id = string_option(options.props.as_slice(), "outletId")?.unwrap_or_else(|| "main".into());
                let id = format!("{}#{}", module.id, name.id.sym);
                route_locals.insert((module.id.clone(), name.id.sym.to_string()), id.clone());
                routes.push(HirRoute { id, parent, path, component, pending_component, pending_mode, error_component, loader, outlet_id });
            }
        }
    }
    if !has_router { return Err(RouteError("createRouter({ routeTree }) is required".into())); }
    for route in &mut routes {
        if let Some(parent) = &route.parent {
            let (module, local) = parent.split_once('#').ok_or_else(|| RouteError("route parent must be an imported Route symbol".into()))?;
            route.parent = route_locals.get(&(module.into(), local.into())).cloned();
            if route.parent.is_none() { return Err(RouteError("route parent is not a discovered route".into())); }
        }
    }
    let root = routes.iter().find(|route| route.parent.is_none()).ok_or_else(|| RouteError("route tree has no root route".into()))?;
    if routes.iter().filter(|route| route.parent.is_none()).count() != 1 { return Err(RouteError("route tree must have one root route".into())); }
    Ok(HirRouteApplication { root: root.component.clone(), routes })
}

pub fn lower_route_manifest(routes: &HirRouteApplication) -> RouteManifest {
    RouteManifest {
        version: 3,
        revision: "rust-route-v1".into(),
        root_graph_id: graph_id(&routes.root),
        routes: routes.routes.iter().map(|route| RouteManifestEntry {
            id: route.id.clone(), parent_id: route.parent.clone(), path: route.path.clone(), graph_id: graph_id(&route.component),
            pending_graph_id: route.pending_component.as_ref().map(graph_id), pending_mode: route.pending_mode.clone(), error_graph_id: route.error_component.as_ref().map(graph_id),
            // Loader action zero is reserved by route-graph lowering; callers
            // cannot supply this runtime handle.
            loader_action: route.loader.as_ref().map(|_| 0), outlet_id: route.outlet_id.clone(),
        }).collect(),
    }
}

fn graph_id(component: &ComponentId) -> String { format!("{}#{}", component.module_id, component.local_name) }
fn exported_var(item: &ModuleItem) -> Option<&VarDecl> {
    match item { ModuleItem::ModuleDecl(swc_ecma_ast::ModuleDecl::ExportDecl(value)) => match &value.decl { Decl::Var(value) => Some(value), _ => None }, ModuleItem::Stmt(Stmt::Decl(Decl::Var(value))) => Some(value), _ => None }
}
fn has_create_router(item: &ModuleItem) -> bool { exported_var(item).map(|var| var.decls.iter().any(|decl| matches!(decl.init.as_deref(), Some(Expr::Call(call)) if callee_name(&call.callee) == Some("createRouter")))).unwrap_or(false) }
fn callee_name(callee: &Callee) -> Option<&str> { match callee { Callee::Expr(expr) => match expr.as_ref() { Expr::Ident(value) => Some(value.sym.as_ref()), _ => None }, _ => None } }
fn prop<'a>(props: &'a [PropOrSpread], name: &str) -> Option<&'a Expr> { props.iter().find_map(|entry| match entry { PropOrSpread::Prop(value) => match value.as_ref() { Prop::KeyValue(KeyValueProp { key, value }) if prop_name(key) == Some(name) => Some(value.as_ref()), _ => None }, _ => None }) }
fn prop_name(name: &PropName) -> Option<&str> { match name { PropName::Ident(value) => Some(value.sym.as_ref()), PropName::Str(value) => value.value.as_str(), _ => None } }
fn string_option(props: &[PropOrSpread], name: &str) -> Result<Option<String>, RouteError> {
    for entry in props {
        let PropOrSpread::Prop(value) = entry else { continue };
        match value.as_ref() {
            Prop::KeyValue(KeyValueProp { key, value }) if prop_name(key) == Some(name) => return match value.as_ref() {
                Expr::Lit(swc_ecma_ast::Lit::Str(value)) => Ok(Some(value.value.to_string_lossy().into_owned())),
                _ => Err(RouteError(format!("route {name} must be a string literal"))),
            },
            Prop::Shorthand(value) if value.sym == name => return Err(RouteError(format!("route {name} must be a string literal"))),
            _ => {}
        }
    }
    Ok(None)
}
fn pending_mode_option(props: &[PropOrSpread]) -> Result<String, RouteError> {
    match string_option(props, "pendingMode")?.as_deref() {
        None | Some("replace") => Ok("replace".into()),
        Some("retain") => Ok("retain".into()),
        Some(_) => Err(RouteError("route pendingMode must be 'replace' or 'retain'".into())),
    }
}
fn optional_ident_option(props: &[PropOrSpread], name: &str) -> Result<Option<String>, RouteError> { match prop(props, name) { None => Ok(None), Some(Expr::Ident(value)) => Ok(Some(value.sym.to_string())), Some(Expr::Arrow(_)) | Some(Expr::Fn(_)) => Ok(Some(name.into())), Some(_) => Err(RouteError(format!("route {name} must be a local function"))) } }
fn component_option(props: &[PropOrSpread], name: &str, module: &ParsedModule, graph: &SemanticGraph) -> Result<ComponentId, RouteError> { optional_component_option(props, name, module, graph)?.ok_or_else(|| RouteError(format!("route {name} is required"))) }
fn optional_component_option(props: &[PropOrSpread], name: &str, module: &ParsedModule, graph: &SemanticGraph) -> Result<Option<ComponentId>, RouteError> { match prop(props, name) { None => Ok(None), Some(Expr::Ident(value)) => resolve_local_symbol(graph, &module.id, value.sym.as_ref()).map(|symbol| Some(ComponentId::new(symbol.module_id, symbol.local_name))).ok_or_else(|| RouteError(format!("route {name} must reference a resolvable component"))), Some(_) => Err(RouteError(format!("route {name} must be a component symbol"))) } }
fn parent_option(props: &[PropOrSpread], module: &ParsedModule, graph: &SemanticGraph) -> Result<Option<String>, RouteError> { let Some(value) = prop(props, "getParentRoute") else { return Err(RouteError("createRoute requires getParentRoute".into())) }; let returned = match value { Expr::Arrow(arrow) => match arrow.body.as_ref() { ArrowFunctionBody::Expr(expr) => expr.as_ref(), _ => return Err(RouteError("getParentRoute must return a route symbol".into())) }, _ => return Err(RouteError("getParentRoute must be an arrow function".into())) }; match returned { Expr::Ident(value) => resolve_local_symbol(graph, &module.id, value.sym.as_ref()).map(|symbol| Some(format!("{}#{}", symbol.module_id, symbol.local_name))).ok_or_else(|| RouteError("getParentRoute must return a resolvable route symbol".into())), _ => Err(RouteError("getParentRoute must return a route symbol".into())) } }

#[cfg(test)]
mod tests {
    use super::*;
    use plec_parser::parse_module;
    use plec_sema::build_semantic_graph;

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
        assert_eq!(manifest.routes.len(), 2);
        assert_eq!(manifest.routes[1].parent_id.as_deref(), Some("routes.tsx#Root"));
        assert_eq!(manifest.routes[1].loader_action, Some(0));
        assert_eq!(manifest.routes[1].pending_graph_id.as_deref(), Some("routes.tsx#Pending"));
        assert_eq!(manifest.routes[1].pending_mode, "retain");
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
        assert!(lower_routes(&modules, &graph).unwrap_err().to_string().contains("path must be a string literal"));
    }

    #[test]
    fn rejects_unknown_pending_modes() {
        let modules = vec![parse_module("routes.tsx", r#"
            function Layout() { return <main />; }
            export const Root = createRootRoute({ component: Layout, pendingMode: 'later' });
            export const router = createRouter({ routeTree: Root });
        "#).unwrap()];
        let graph = build_semantic_graph(&modules, &HashMap::new()).unwrap();
        assert!(lower_routes(&modules, &graph).unwrap_err().to_string().contains("pendingMode"));
    }
}
