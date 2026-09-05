use plec_parser::ParsedModule;
use plec_model::{resolve_export, resolve_local_symbol, SemanticGraph, SymbolKind};
use std::collections::HashMap;
use swc_common::Span;
use swc_ecma_ast::{
    ArrowExpr, ArrowFunctionBody, Callee, Decl, Expr, FnDecl, Function, FunctionBody, JSXElement,
    JSXFragment, ModuleDecl, ModuleItem, Pat, Stmt, VarDeclarator,
};
use swc_ecma_visit::{Visit, VisitWith};

pub type ModuleId = String;

#[derive(Debug, Clone, PartialEq)]
pub enum ComponentDiscoveryError {
    RootNotFound {
        entry_module_id: ModuleId,
        name: String,
    },
    NotAFunction {
        module_id: ModuleId,
        name: String,
        kind: SymbolKind,
    },
    DeclarationNotFound {
        module_id: ModuleId,
        name: String,
        span: Span,
    },
    NoReturnedJsx {
        module_id: ModuleId,
        name: String,
    },
    UnsupportedComponentShape {
        module_id: ModuleId,
        name: String,
        reason: String,
    },
    ControlFlowReturnUnsupported {
        module_id: ModuleId,
        name: String,
    },
}

impl std::fmt::Display for ComponentDiscoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RootNotFound {
                entry_module_id,
                name,
            } => write!(
                f,
                "Root component '{name}' not found in entry module '{entry_module_id}'"
            ),
            Self::NotAFunction {
                module_id,
                name,
                kind,
            } => write!(
                f,
                "Symbol '{name}' in module '{module_id}' is not a function (kind: {:?})",
                kind
            ),
            Self::DeclarationNotFound {
                module_id, name, ..
            } => write!(
                f,
                "Could not locate declaration for '{name}' in module '{module_id}'"
            ),
            Self::NoReturnedJsx { module_id, name } => write!(
                f,
                "Component '{name}' in module '{module_id}' does not return JSX"
            ),
            Self::UnsupportedComponentShape {
                module_id,
                name,
                reason,
            } => write!(
                f,
                "Component '{name}' in module '{module_id}' has unsupported shape: {reason}"
            ),
            Self::ControlFlowReturnUnsupported { module_id, name } => write!(
                f,
                "Component '{name}' in module '{module_id}' returns through statement control flow, which structural HIR does not support"
            ),
        }
    }
}

impl std::error::Error for ComponentDiscoveryError {}

#[derive(Debug, Clone, Copy)]
pub enum ComponentDeclaration<'a> {
    Function(&'a FnDecl),
    Arrow {
        declarator: &'a VarDeclarator,
        arrow: &'a ArrowExpr,
    },
    FunctionExpression {
        declarator: &'a VarDeclarator,
        function: &'a Function,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum ReturnedComponentExpression<'a> {
    JsxElement(&'a JSXElement),
    JsxFragment(&'a JSXFragment),
    StructuralExpression(&'a Expr),
    StaticallyAbsent,
    NonJsxReturn(&'a Expr),
}

#[derive(Debug, Clone)]
pub struct RootComponent<'a> {
    pub symbol: plec_model::ResolvedSymbol,
    pub declaration: ComponentDeclaration<'a>,
    pub returned: ReturnedComponentExpression<'a>,
}

pub fn discover_root_component<'a>(
    parsed_modules: &'a [ParsedModule],
    semantic_graph: &SemanticGraph,
    entry_module_id: &str,
    root_name: Option<&str>,
) -> Result<RootComponent<'a>, ComponentDiscoveryError> {
    let module_map: HashMap<&str, &ParsedModule> =
        parsed_modules.iter().map(|m| (m.id.as_str(), m)).collect();

    let symbol = match root_name {
        Some(name) => resolve_entry_root(semantic_graph, entry_module_id, name)?,
        None => discover_default_root(parsed_modules, entry_module_id)?,
    };

    let module = module_map.get(symbol.module_id.as_str()).ok_or_else(|| {
        ComponentDiscoveryError::DeclarationNotFound {
            module_id: symbol.module_id.clone(),
            name: symbol.local_name.clone(),
            span: symbol.span,
        }
    })?;

    let declaration = locate_declaration(module, &symbol)?;
    let returned = extract_returned_expression(&declaration)?;

    Ok(RootComponent {
        symbol,
        declaration,
        returned,
    })
}

fn resolve_entry_root(
    semantic_graph: &SemanticGraph,
    entry_module_id: &str,
    name: &str,
) -> Result<plec_model::ResolvedSymbol, ComponentDiscoveryError> {
    if let Some(symbol) = resolve_local_symbol(semantic_graph, entry_module_id, name) {
        if matches!(symbol.kind, SymbolKind::Function) {
            return Ok(symbol);
        }
        return Err(ComponentDiscoveryError::NotAFunction {
            module_id: entry_module_id.to_string(),
            name: name.to_string(),
            kind: symbol.kind,
        });
    }

    if let Some(symbol) = resolve_export(semantic_graph, entry_module_id, name) {
        if matches!(symbol.kind, SymbolKind::Function) {
            return Ok(symbol);
        }
        return Err(ComponentDiscoveryError::NotAFunction {
            module_id: symbol.module_id.clone(),
            name: name.to_string(),
            kind: symbol.kind,
        });
    }

    Err(ComponentDiscoveryError::RootNotFound {
        entry_module_id: entry_module_id.to_string(),
        name: name.to_string(),
    })
}

fn discover_default_root(
    parsed_modules: &[ParsedModule],
    entry_module_id: &str,
) -> Result<plec_model::ResolvedSymbol, ComponentDiscoveryError> {
    let entry_module = parsed_modules
        .iter()
        .find(|m| m.id == entry_module_id)
        .ok_or_else(|| ComponentDiscoveryError::RootNotFound {
            entry_module_id: entry_module_id.to_string(),
            name: "<default>".to_string(),
        })?;

    for item in &entry_module.ast.body {
        let (decl, exported) = match item {
            ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(export_decl)) => {
                (&export_decl.decl, true)
            }
            ModuleItem::Stmt(Stmt::Decl(decl)) => (decl, false),
            _ => continue,
        };

        match decl {
            Decl::Fn(fn_decl) => {
                if !is_component_name(fn_decl.ident.sym.as_ref()) {
                    continue;
                }
                if fn_decl
                    .function
                    .body
                    .as_ref()
                    .is_some_and(function_body_returns_jsx)
                {
                    return Ok(plec_model::ResolvedSymbol {
                        module_id: entry_module_id.to_string(),
                        local_name: fn_decl.ident.sym.to_string(),
                        exported_name: exported.then(|| fn_decl.ident.sym.to_string()),
                        kind: SymbolKind::Function,
                        span: fn_decl.ident.span,
                    });
                }
            }
            Decl::Var(var_decl) => {
                for declarator in &var_decl.decls {
                    let Pat::Ident(ident) = &declarator.name else {
                        continue;
                    };
                    if !is_component_name(ident.id.sym.as_ref()) {
                        continue;
                    }

                    let Some(init) = declarator.init.as_deref() else {
                        continue;
                    };
                    let Some(function) = unwrap_function_like(init) else {
                        continue;
                    };
                    if !function_like_returns_jsx(function) {
                        continue;
                    }

                    return Ok(plec_model::ResolvedSymbol {
                        module_id: entry_module_id.to_string(),
                        local_name: ident.id.sym.to_string(),
                        exported_name: exported.then(|| ident.id.sym.to_string()),
                        kind: SymbolKind::Function,
                        span: ident.id.span,
                    });
                }
            }
            _ => {}
        }
    }

    Err(ComponentDiscoveryError::RootNotFound {
        entry_module_id: entry_module_id.to_string(),
        name: "<default>".to_string(),
    })
}

fn is_component_name(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

/// Determine which return variants qualify a function as a component for default discovery.
///
/// Only JSX-returning variants qualify. `StaticallyAbsent` and `NonJsxReturn` do not.
fn is_component_return(returned: ReturnedComponentExpression<'_>) -> bool {
    matches!(
        returned,
        ReturnedComponentExpression::JsxElement(_)
            | ReturnedComponentExpression::JsxFragment(_)
            | ReturnedComponentExpression::StructuralExpression(_)
    )
}

fn function_body_returns_jsx(body: &FunctionBody) -> bool {
    extract_from_function_body(body).is_some_and(is_component_return)
}

fn function_like_returns_jsx(function: FunctionLike<'_>) -> bool {
    returned_from_function_like(function).is_some_and(is_component_return)
}

#[derive(Clone, Copy)]
enum FunctionLike<'a> {
    Arrow(&'a ArrowExpr),
    Function(&'a Function),
}

fn unwrap_function_like<'a>(expr: &'a Expr) -> Option<FunctionLike<'a>> {
    match expr {
        Expr::Arrow(arrow) => Some(FunctionLike::Arrow(arrow)),
        Expr::Fn(fn_expr) => Some(FunctionLike::Function(&fn_expr.function)),
        Expr::Call(call) if is_component_factory_call(call) => call
            .args
            .first()
            .and_then(|arg| unwrap_function_like(arg.expr.as_ref())),
        _ => None,
    }
}

fn is_component_factory_call(call: &swc_ecma_ast::CallExpr) -> bool {
    let Callee::Expr(callee) = &call.callee else {
        return false;
    };
    match callee.as_ref() {
        Expr::Ident(ident) => ident.sym == "forwardRef",
        Expr::Member(member) => {
            matches!(&*member.obj, Expr::Ident(obj) if obj.sym == "React")
                && matches!(&member.prop, swc_ecma_ast::MemberProp::Ident(prop) if prop.sym == "forwardRef")
        }
        _ => false,
    }
}

fn returned_from_function_like<'a>(
    function: FunctionLike<'a>,
) -> Option<ReturnedComponentExpression<'a>> {
    match function {
        FunctionLike::Arrow(arrow) => extract_from_arrow_body(arrow),
        FunctionLike::Function(function) => {
            function.body.as_ref().and_then(extract_from_function_body)
        }
    }
}

/// Locate the declaration backing a semantic symbol.
///
/// This intentionally does not use `Visit`: `Visit` callbacks borrow nodes only for the
/// duration of the callback, so a visitor cannot safely retain those references in
/// `ComponentDeclaration<'a>`. Sema symbols are module-level declarations anyway, so
/// matching the module body directly is both safer and more precise.
fn locate_declaration<'a>(
    module: &'a ParsedModule,
    symbol: &plec_model::ResolvedSymbol,
) -> Result<ComponentDeclaration<'a>, ComponentDiscoveryError> {
    let target = symbol.local_name.as_str();

    for item in &module.ast.body {
        let decl = match item {
            ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(export_decl)) => Some(&export_decl.decl),
            ModuleItem::Stmt(Stmt::Decl(decl)) => Some(decl),
            _ => None,
        };

        let Some(decl) = decl else {
            continue;
        };

        match decl {
            Decl::Fn(fn_decl) if fn_decl.ident.sym == target => {
                return Ok(ComponentDeclaration::Function(fn_decl));
            }
            Decl::Var(var_decl) => {
                for declarator in &var_decl.decls {
                    let Pat::Ident(ident) = &declarator.name else {
                        continue;
                    };

                    if ident.id.sym == target {
                        return extract_declaration_from_var(declarator);
                    }
                }
            }
            _ => {}
        }
    }

    Err(ComponentDiscoveryError::DeclarationNotFound {
        module_id: module.id.clone(),
        name: symbol.local_name.clone(),
        span: symbol.span,
    })
}

fn extract_declaration_from_var<'a>(
    declarator: &'a VarDeclarator,
) -> Result<ComponentDeclaration<'a>, ComponentDiscoveryError> {
    let init = declarator
        .init
        .as_deref()
        .ok_or_else(|| unsupported_var(declarator, "missing initializer"))?;

    match unwrap_function_like(init) {
        Some(FunctionLike::Arrow(arrow)) => Ok(ComponentDeclaration::Arrow { declarator, arrow }),
        Some(FunctionLike::Function(function)) => Ok(ComponentDeclaration::FunctionExpression {
            declarator,
            function,
        }),
        None => Err(unsupported_var(
            declarator,
            "not a supported function or component factory",
        )),
    }
}

fn unsupported_var(declarator: &VarDeclarator, reason: &str) -> ComponentDiscoveryError {
    ComponentDiscoveryError::UnsupportedComponentShape {
        module_id: "<unknown>".to_string(),
        name: get_declarator_name(declarator),
        reason: reason.to_string(),
    }
}

fn get_declarator_name(declarator: &VarDeclarator) -> String {
    match &declarator.name {
        Pat::Ident(ident) => ident.id.sym.to_string(),
        _ => "<unknown>".to_string(),
    }
}

pub fn extract_returned_expression<'a>(
    declaration: &ComponentDeclaration<'a>,
) -> Result<ReturnedComponentExpression<'a>, ComponentDiscoveryError> {
    match declaration {
        ComponentDeclaration::Function(fn_decl) => {
            let module_id = "<unknown>".to_string();
            let name = fn_decl.ident.sym.to_string();
            let body = fn_decl.function.body.as_ref().ok_or_else(|| {
                ComponentDiscoveryError::NoReturnedJsx {
                    module_id: module_id.clone(),
                    name: name.clone(),
                }
            })?;
            extract_unconditional_return(body, module_id, name)
        }
        ComponentDeclaration::Arrow { declarator, arrow } => match arrow.body.as_ref() {
            ArrowFunctionBody::Expr(expr) => {
                find_returned_in_expr(expr.as_ref()).ok_or_else(|| {
                    ComponentDiscoveryError::NoReturnedJsx {
                        module_id: "<unknown>".to_string(),
                        name: get_declarator_name(declarator),
                    }
                })
            }
            ArrowFunctionBody::FunctionBody(body) => extract_unconditional_return(
                body,
                "<unknown>".to_string(),
                get_declarator_name(declarator),
            ),
        },
        ComponentDeclaration::FunctionExpression {
            declarator,
            function,
        } => {
            let module_id = "<unknown>".to_string();
            let name = get_declarator_name(declarator);
            let body =
                function
                    .body
                    .as_ref()
                    .ok_or_else(|| ComponentDiscoveryError::NoReturnedJsx {
                        module_id: module_id.clone(),
                        name: name.clone(),
                    })?;
            extract_unconditional_return(body, module_id, name)
        }
    }
}

fn extract_unconditional_return<'a>(
    body: &'a FunctionBody,
    module_id: ModuleId,
    name: String,
) -> Result<ReturnedComponentExpression<'a>, ComponentDiscoveryError> {
    let direct_returns = body
        .stmts
        .iter()
        .filter(|stmt| matches!(stmt, Stmt::Return(_)))
        .count();
    if direct_returns != 1 || body.stmts.iter().any(stmt_contains_nested_return) {
        return Err(ComponentDiscoveryError::ControlFlowReturnUnsupported { module_id, name });
    }
    body.stmts
        .iter()
        .find_map(|stmt| match stmt {
            Stmt::Return(return_stmt) => match return_stmt.arg.as_deref() {
                Some(expr) => find_returned_in_expr(expr),
                None => Some(ReturnedComponentExpression::StaticallyAbsent),
            },
            _ => None,
        })
        .ok_or(ComponentDiscoveryError::NoReturnedJsx { module_id, name })
}

fn stmt_contains_nested_return(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Return(_) => false,
        Stmt::Block(block) => block.stmts.iter().any(|statement| {
            matches!(statement, Stmt::Return(_)) || stmt_contains_nested_return(statement)
        }),
        Stmt::If(_)
        | Stmt::Labeled(_)
        | Stmt::With(_)
        | Stmt::While(_)
        | Stmt::DoWhile(_)
        | Stmt::For(_)
        | Stmt::ForIn(_)
        | Stmt::ForOf(_)
        | Stmt::Switch(_)
        | Stmt::Try(_) => true,
        _ => false,
    }
}

fn extract_from_function_body<'a>(
    body: &'a FunctionBody,
) -> Option<ReturnedComponentExpression<'a>> {
    find_returned_in_stmts(&body.stmts)
}

fn extract_from_arrow_body<'a>(arrow: &'a ArrowExpr) -> Option<ReturnedComponentExpression<'a>> {
    match arrow.body.as_ref() {
        ArrowFunctionBody::FunctionBody(body) => extract_from_function_body(body),
        ArrowFunctionBody::Expr(expr) => find_returned_in_expr(expr.as_ref()),
    }
}

/// Find the first component-relevant return in a slice of statements.
fn find_returned_in_stmts<'a>(stmts: &'a [Stmt]) -> Option<ReturnedComponentExpression<'a>> {
    for stmt in stmts {
        if let Some(returned) = find_returned_in_stmt(stmt) {
            return Some(returned);
        }
    }

    None
}

fn find_returned_in_stmt<'a>(stmt: &'a Stmt) -> Option<ReturnedComponentExpression<'a>> {
    match stmt {
        Stmt::Return(return_stmt) => match return_stmt.arg.as_deref() {
            Some(expr) => find_returned_in_expr(expr),
            None => Some(ReturnedComponentExpression::StaticallyAbsent),
        },

        Stmt::Block(block) => find_returned_in_stmts(&block.stmts),

        Stmt::If(if_stmt) => find_returned_in_stmt(if_stmt.cons.as_ref())
            .or_else(|| if_stmt.alt.as_deref().and_then(find_returned_in_stmt)),

        Stmt::Labeled(labeled) => find_returned_in_stmt(labeled.body.as_ref()),

        Stmt::With(with_stmt) => find_returned_in_stmt(with_stmt.body.as_ref()),

        Stmt::While(while_stmt) => find_returned_in_stmt(while_stmt.body.as_ref()),

        Stmt::DoWhile(do_while) => find_returned_in_stmt(do_while.body.as_ref()),

        Stmt::For(for_stmt) => find_returned_in_stmt(for_stmt.body.as_ref()),

        Stmt::ForIn(for_in) => find_returned_in_stmt(for_in.body.as_ref()),

        Stmt::ForOf(for_of) => find_returned_in_stmt(for_of.body.as_ref()),

        Stmt::Switch(switch_stmt) => switch_stmt
            .cases
            .iter()
            .flat_map(|case| case.cons.iter())
            .find_map(find_returned_in_stmt),

        Stmt::Try(try_stmt) => find_returned_in_stmts(&try_stmt.block.stmts)
            .or_else(|| {
                try_stmt
                    .handler
                    .as_ref()
                    .and_then(|handler| find_returned_in_stmts(&handler.body.stmts))
            })
            .or_else(|| {
                try_stmt
                    .finalizer
                    .as_ref()
                    .and_then(|f| find_returned_in_stmts(&f.stmts))
            }),

        // Do not descend into declarations: nested functions have their own return scope.
        _ => None,
    }
}

fn find_returned_in_expr<'a>(expr: &'a Expr) -> Option<ReturnedComponentExpression<'a>> {
    let unwrapped = unwrap_parens(expr);

    match unwrapped {
        Expr::JSXElement(jsx) => Some(ReturnedComponentExpression::JsxElement(jsx)),
        Expr::JSXFragment(fragment) => Some(ReturnedComponentExpression::JsxFragment(fragment)),
        Expr::Lit(swc_ecma_ast::Lit::Null(_)) => {
            Some(ReturnedComponentExpression::StaticallyAbsent)
        }
        Expr::Lit(swc_ecma_ast::Lit::Bool(value)) if !value.value => {
            Some(ReturnedComponentExpression::StaticallyAbsent)
        }
        Expr::Ident(ident) if ident.sym == "undefined" => {
            Some(ReturnedComponentExpression::StaticallyAbsent)
        }
        Expr::Unary(unary) if matches!(unary.op, swc_ecma_ast::UnaryOp::Void) => {
            Some(ReturnedComponentExpression::StaticallyAbsent)
        }
        _ if contains_jsx(unwrapped) => {
            Some(ReturnedComponentExpression::StructuralExpression(unwrapped))
        }
        _ => Some(ReturnedComponentExpression::NonJsxReturn(unwrapped)),
    }
}

fn unwrap_parens(expr: &Expr) -> &Expr {
    let mut current = expr;
    while let Expr::Paren(paren) = current {
        current = paren.expr.as_ref();
    }
    current
}

fn contains_jsx(expr: &Expr) -> bool {
    struct JsxVisitor(bool);

    impl Visit for JsxVisitor {
        fn visit_jsx_element(&mut self, _: &JSXElement) {
            self.0 = true;
        }

        fn visit_jsx_fragment(&mut self, _: &JSXFragment) {
            self.0 = true;
        }
    }

    let mut visitor = JsxVisitor(false);
    expr.visit_with(&mut visitor);
    visitor.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use plec_parser::parse_module;
    use plec_model::build_semantic_graph;
    use std::collections::HashMap;

    fn build_test_graph(
        modules: Vec<(&str, &str)>,
    ) -> (
        Vec<ParsedModule>,
        SemanticGraph,
        HashMap<(ModuleId, String), ModuleId>,
    ) {
        let mut parsed = Vec::new();
        let mut resolved_imports = HashMap::new();

        for (id, source) in modules {
            let module = parse_module(id, source).expect("parse should succeed");
            parsed.push(module);
        }

        // Build a simple import map for testing
        for module in &parsed {
            for import in &module.imports {
                let source = import
                    .source
                    .trim_start_matches("./")
                    .trim_start_matches('.');
                let target_id = if source.contains('/') {
                    source.to_string()
                } else {
                    format!("{source}.tsx")
                };
                resolved_imports.insert((module.id.clone(), import.source.clone()), target_id);
            }

            for export in &module.exports {
                if let plec_parser::ExportKind::ReExport { source, .. }
                | plec_parser::ExportKind::ReExportAll { source } = &export.kind
                {
                    let source_trimmed = source.trim_start_matches("./").trim_start_matches('.');
                    let target_id = if source_trimmed.contains('/') {
                        source_trimmed.to_string()
                    } else {
                        format!("{source_trimmed}.tsx")
                    };
                    resolved_imports.insert((module.id.clone(), source.to_string()), target_id);
                }
            }
        }

        let semantic_graph =
            build_semantic_graph(&parsed, &resolved_imports).expect("graph should build");

        (parsed, semantic_graph, resolved_imports)
    }

    #[test]
    fn function_declaration_basic() {
        let (parsed, graph, _) = build_test_graph(vec![(
            "App.tsx",
            r#"
                export function App() {
                    return <div>Hello</div>;
                }
            "#,
        )]);

        let result = discover_root_component(&parsed, &graph, "App.tsx", None);
        assert!(result.is_ok());

        let root = result.unwrap();
        assert_eq!(root.symbol.local_name, "App");
        assert!(matches!(
            root.returned,
            ReturnedComponentExpression::JsxElement(_)
        ));
    }

    #[test]
    fn arrow_block_body() {
        let (parsed, graph, _) = build_test_graph(vec![(
            "App.tsx",
            r#"
                const App = () => {
                    return <div>Hello</div>;
                }
            "#,
        )]);

        let result = discover_root_component(&parsed, &graph, "App.tsx", None);
        assert!(result.is_ok());

        let root = result.unwrap();
        assert_eq!(root.symbol.local_name, "App");
        assert!(matches!(
            root.returned,
            ReturnedComponentExpression::JsxElement(_)
        ));
    }

    #[test]
    fn arrow_expression_body() {
        let (parsed, graph, _) = build_test_graph(vec![(
            "App.tsx",
            r#"
                const App = () => <div>Hello</div>;
            "#,
        )]);

        let result = discover_root_component(&parsed, &graph, "App.tsx", None);
        assert!(result.is_ok());

        let root = result.unwrap();
        assert_eq!(root.symbol.local_name, "App");
        assert!(matches!(
            root.returned,
            ReturnedComponentExpression::JsxElement(_)
        ));
    }

    #[test]
    fn function_expression() {
        let (parsed, graph, _) = build_test_graph(vec![(
            "App.tsx",
            r#"
                const App = function() {
                    return <div>Hello</div>;
                }
            "#,
        )]);

        let result = discover_root_component(&parsed, &graph, "App.tsx", None);
        assert!(result.is_ok());

        let root = result.unwrap();
        assert_eq!(root.symbol.local_name, "App");
        assert!(matches!(
            root.returned,
            ReturnedComponentExpression::JsxElement(_)
        ));
    }

    #[test]
    fn jsx_fragment() {
        let (parsed, graph, _) = build_test_graph(vec![(
            "App.tsx",
            r#"
                export function App() {
                    return <><div>A</div><div>B</div></>;
                }
            "#,
        )]);

        let result = discover_root_component(&parsed, &graph, "App.tsx", None);
        assert!(result.is_ok());

        let root = result.unwrap();
        assert!(matches!(
            root.returned,
            ReturnedComponentExpression::JsxFragment(_)
        ));
    }

    #[test]
    fn explicit_root_among_multiple_functions() {
        let (parsed, graph, _) = build_test_graph(vec![(
            "App.tsx",
            r#"
                function NotRoot() {
                    return <span />;
                }

                export function Root() {
                    return <div>Root</div>;
                }
            "#,
        )]);

        let result = discover_root_component(&parsed, &graph, "App.tsx", Some("Root"));
        assert!(result.is_ok());

        let root = result.unwrap();
        assert_eq!(root.symbol.local_name, "Root");
        assert!(matches!(
            root.returned,
            ReturnedComponentExpression::JsxElement(_)
        ));
    }

    #[test]
    fn cross_module_re_export() {
        let (parsed, graph, _) = build_test_graph(vec![
            ("App.tsx", r#"export function App() { return <div />; }"#),
            ("index.tsx", r#"export { App } from "./App";"#),
        ]);

        let result = discover_root_component(&parsed, &graph, "index.tsx", Some("App"));
        assert!(result.is_ok());

        let root = result.unwrap();
        assert_eq!(root.symbol.module_id, "App.tsx");
        assert_eq!(root.symbol.local_name, "App");
        assert!(matches!(
            root.returned,
            ReturnedComponentExpression::JsxElement(_)
        ));
    }

    #[test]
    fn non_function_root_error() {
        let (parsed, graph, _) = build_test_graph(vec![(
            "App.tsx",
            r#"
                const App = 42;
            "#,
        )]);

        let result = discover_root_component(&parsed, &graph, "App.tsx", Some("App"));
        assert!(result.is_err());

        match result.unwrap_err() {
            ComponentDiscoveryError::NotAFunction { .. } => (),
            other => panic!("expected NotAFunction, got {:?}", other),
        }
    }

    #[test]
    fn missing_root_error() {
        let (parsed, graph, _) = build_test_graph(vec![(
            "App.tsx",
            r#"
                function NotRoot() {
                    return <div />;
                }
            "#,
        )]);

        let result = discover_root_component(&parsed, &graph, "App.tsx", Some("Missing"));
        assert!(result.is_err());

        match result.unwrap_err() {
            ComponentDiscoveryError::RootNotFound { .. } => (),
            other => panic!("expected RootNotFound, got {:?}", other),
        }
    }

    #[test]
    fn function_returning_no_jsx() {
        let (parsed, graph, _) = build_test_graph(vec![(
            "App.tsx",
            r#"
                export function App() {
                    return "not jsx";
                }
            "#,
        )]);

        let result = discover_root_component(&parsed, &graph, "App.tsx", None);
        // Default discovery should skip functions that don't return JSX
        assert!(result.is_err());

        match result.unwrap_err() {
            ComponentDiscoveryError::RootNotFound { .. } => (),
            other => panic!("expected RootNotFound, got {:?}", other),
        }
    }

    #[test]
    fn static_absent_return() {
        let (parsed, graph, _) = build_test_graph(vec![(
            "App.tsx",
            r#"
                export function App() {
                    return null;
                }
            "#,
        )]);

        let result = discover_root_component(&parsed, &graph, "App.tsx", Some("App"));
        assert!(result.is_ok());

        let root = result.unwrap();
        assert!(matches!(
            root.returned,
            ReturnedComponentExpression::StaticallyAbsent
        ));
    }

    #[test]
    fn rejects_statement_level_control_flow_returns() {
        let (parsed, graph, _) = build_test_graph(vec![(
            "App.tsx",
            r#"
                export function App() {
                    if (true) {
                        return <Ready />;
                    }
                    return <Loading />;
                }
            "#,
        )]);

        let result = discover_root_component(&parsed, &graph, "App.tsx", Some("App"));
        assert!(matches!(
            result,
            Err(ComponentDiscoveryError::ControlFlowReturnUnsupported { .. })
        ));
    }

    #[test]
    fn conditional_jsx() {
        let (parsed, graph, _) = build_test_graph(vec![(
            "App.tsx",
            r#"
                export function App() {
                    return ready ? <Ready /> : <Loading />;
                }
            "#,
        )]);

        let result = discover_root_component(&parsed, &graph, "App.tsx", None);
        assert!(result.is_ok());

        let root = result.unwrap();
        assert!(matches!(
            root.returned,
            ReturnedComponentExpression::StructuralExpression(_)
        ));
    }

    #[test]
    fn logical_and_jsx() {
        let (parsed, graph, _) = build_test_graph(vec![(
            "App.tsx",
            r#"
                export function App() {
                    return ready && <Ready />;
                }
            "#,
        )]);

        let result = discover_root_component(&parsed, &graph, "App.tsx", None);
        assert!(result.is_ok());

        let root = result.unwrap();
        assert!(matches!(
            root.returned,
            ReturnedComponentExpression::StructuralExpression(_)
        ));
    }

    #[test]
    fn same_symbol_names_different_modules() {
        let (parsed, graph, _) = build_test_graph(vec![
            ("A.tsx", r#"export function App() { return <div>A</div>; }"#),
            ("B.tsx", r#"export function App() { return <div>B</div>; }"#),
        ]);

        let result_a = discover_root_component(&parsed, &graph, "A.tsx", Some("App"));
        assert!(result_a.is_ok());

        let result_b = discover_root_component(&parsed, &graph, "B.tsx", Some("App"));
        assert!(result_b.is_ok());

        assert_eq!(result_a.unwrap().symbol.module_id, "A.tsx");
        assert_eq!(result_b.unwrap().symbol.module_id, "B.tsx");
    }

    #[test]
    fn forward_ref_factory() {
        let (parsed, graph, _) = build_test_graph(vec![(
            "App.tsx",
            r#"
                const App = React.forwardRef(() => {
                    return <div />;
                });
            "#,
        )]);

        let result = discover_root_component(&parsed, &graph, "App.tsx", Some("App"));
        assert!(result.is_ok());

        let root = result.unwrap();
        assert!(matches!(
            root.declaration,
            ComponentDeclaration::Arrow { .. }
        ));
    }

    #[test]
    fn anonymous_default_export_rejected() {
        let (parsed, graph, _) = build_test_graph(vec![(
            "App.tsx",
            r#"
                export default function() {
                    return <div />;
                }
            "#,
        )]);

        let result = discover_root_component(&parsed, &graph, "App.tsx", None);
        // Anonymous default exports should not be auto-discovered
        assert!(result.is_err());
    }

    #[test]
    fn lowercased_function_skipped() {
        let (parsed, graph, _) = build_test_graph(vec![(
            "App.tsx",
            r#"
                function helper() {
                    return <div />;
                }
            "#,
        )]);

        let result = discover_root_component(&parsed, &graph, "App.tsx", None);
        // Lowercase names are not components
        assert!(result.is_err());

        match result.unwrap_err() {
            ComponentDiscoveryError::RootNotFound { .. } => (),
            other => panic!("expected RootNotFound, got {:?}", other),
        }
    }

    #[test]
    fn exported_arrow_block() {
        let (parsed, graph, _) = build_test_graph(vec![(
            "App.tsx",
            r#"
                export const App = () => {
                    return <div>Exported</div>;
                };
            "#,
        )]);

        let result = discover_root_component(&parsed, &graph, "App.tsx", None);
        assert!(result.is_ok());

        let root = result.unwrap();
        assert_eq!(root.symbol.local_name, "App");
        assert!(matches!(
            root.returned,
            ReturnedComponentExpression::JsxElement(_)
        ));
    }

    #[test]
    fn declaration_lookup_using_span() {
        let (parsed, graph, _) = build_test_graph(vec![(
            "App.tsx",
            r#"
                export function App() {
                    return <div />;
                }
            "#,
        )]);

        let result = discover_root_component(&parsed, &graph, "App.tsx", Some("App"));
        assert!(result.is_ok());

        let root = result.unwrap();
        // Verify the declaration was found via sema resolution
        assert!(matches!(
            root.declaration,
            ComponentDeclaration::Function(_)
        ));
        assert_eq!(root.symbol.local_name, "App");
    }

    #[test]
    fn nested_in_control_flow_not_discovered() {
        let (parsed, graph, _) = build_test_graph(vec![(
            "App.tsx",
            r#"
                if (true) {
                    const App = () => <div />;
                }
            "#,
        )]);

        let result = discover_root_component(&parsed, &graph, "App.tsx", None);
        // Module-scoped discovery only; nested declarations should not be found
        assert!(result.is_err());

        match result.unwrap_err() {
            ComponentDiscoveryError::RootNotFound { .. } => (),
            other => panic!("expected RootNotFound, got {:?}", other),
        }
    }

    #[test]
    fn module_scope_still_discovered() {
        let (parsed, graph, _) = build_test_graph(vec![(
            "App.tsx",
            r#"
                const App = () => <div />;
            "#,
        )]);

        let result = discover_root_component(&parsed, &graph, "App.tsx", None);
        // Module-scoped declarations should still be discovered
        assert!(result.is_ok());

        let root = result.unwrap();
        assert_eq!(root.symbol.local_name, "App");
        assert!(matches!(
            root.returned,
            ReturnedComponentExpression::JsxElement(_)
        ));
    }
}
