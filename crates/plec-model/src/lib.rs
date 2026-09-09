use plec_parser::{ExportKind, ImportSpecifier, ParsedModule};
use std::collections::{HashMap, HashSet};
use swc_common::Span;
use swc_ecma_ast::{
    CallExpr, Callee, Decl, Expr, Function, Lit, Module, ModuleDecl, ModuleItem, Pat,
    TsFnOrConstructorType, TsType, TsTypeElement,
};

pub type ModuleId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    Function,
    Variable,
    Class,
    Namespace,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SymbolRef {
    pub module_id: ModuleId,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSymbol {
    pub module_id: ModuleId,
    pub local_name: String,
    pub exported_name: Option<String>,
    pub kind: SymbolKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct LocalSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ImportSymbol {
    pub local_name: String,
    pub imported_name: String,
    pub target_module_id: ModuleId,
    pub type_only: bool,
}

#[derive(Debug, Clone)]
pub struct ExportSymbol {
    pub exported_name: String,
    pub local_name: Option<String>,
    pub kind: ExportSymbolKind,
}

#[derive(Debug, Clone)]
pub enum ExportSymbolKind {
    Local,
    ReExport { target_module_id: ModuleId },
}

#[derive(Debug, Clone)]
pub struct SemanticModule {
    pub id: ModuleId,
    pub locals: HashMap<String, LocalSymbol>,
    pub component_props: HashMap<String, HashMap<String, ComponentPropKind>>,
    pub imports: HashMap<String, ImportSymbol>,
    pub exports: HashMap<String, ExportSymbol>,
    pub export_all: Vec<ModuleId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentPropKind {
    Value,
    Callable,
}

#[derive(Debug, Clone)]
pub struct SemanticGraph {
    pub modules: HashMap<ModuleId, SemanticModule>,
}

impl SemanticGraph {
    pub fn new() -> Self {
        Self {
            modules: HashMap::new(),
        }
    }

    pub fn add_module(&mut self, module: SemanticModule) {
        self.modules.insert(module.id.clone(), module);
    }

    pub fn get_module(&self, id: &str) -> Option<&SemanticModule> {
        self.modules.get(id)
    }
}

pub fn build_semantic_graph(
    parsed_modules: &[ParsedModule],
    resolved_imports: &HashMap<(ModuleId, String), ModuleId>,
) -> Result<SemanticGraph, String> {
    let mut graph = SemanticGraph::new();

    for parsed in parsed_modules {
        let mut semantic = SemanticModule {
            id: parsed.id.clone(),
            locals: HashMap::new(),
            component_props: HashMap::new(),
            imports: HashMap::new(),
            exports: HashMap::new(),
            export_all: Vec::new(),
        };

        collect_local_declarations(&parsed.ast, &mut semantic.locals);
        collect_component_props(&parsed.ast, &mut semantic.component_props);

        for import in &parsed.imports {
            let Some(target_module_id) =
                resolved_imports.get(&(parsed.id.clone(), import.source.clone()))
            else {
                continue;
            };

            for specifier in &import.specifiers {
                match specifier {
                    ImportSpecifier::Named { imported, local } => {
                        semantic.imports.insert(
                            local.clone(),
                            ImportSymbol {
                                local_name: local.clone(),
                                imported_name: imported.clone(),
                                target_module_id: target_module_id.clone(),
                                type_only: import.type_only,
                            },
                        );
                    }
                    ImportSpecifier::Default(local) => {
                        semantic.imports.insert(
                            local.clone(),
                            ImportSymbol {
                                local_name: local.clone(),
                                imported_name: "default".to_string(),
                                target_module_id: target_module_id.clone(),
                                type_only: import.type_only,
                            },
                        );
                    }
                    ImportSpecifier::Namespace(local) => {
                        semantic.imports.insert(
                            local.clone(),
                            ImportSymbol {
                                local_name: local.clone(),
                                imported_name: "*".to_string(),
                                target_module_id: target_module_id.clone(),
                                type_only: import.type_only,
                            },
                        );
                    }
                }
            }
        }

        for export in &parsed.exports {
            match &export.kind {
                ExportKind::Local { local, exported } => {
                    let exported_name = exported.as_ref().unwrap_or(local).clone();
                    semantic.exports.insert(
                        exported_name.clone(),
                        ExportSymbol {
                            exported_name,
                            local_name: Some(local.clone()),
                            kind: ExportSymbolKind::Local,
                        },
                    );
                }
                ExportKind::ReExport {
                    source,
                    local,
                    exported,
                } => {
                    let Some(target_module_id) =
                        resolved_imports.get(&(parsed.id.clone(), source.clone()))
                    else {
                        continue;
                    };
                    let exported_name = exported.as_ref().unwrap_or(local).clone();
                    semantic.exports.insert(
                        exported_name.clone(),
                        ExportSymbol {
                            exported_name,
                            local_name: Some(local.clone()),
                            kind: ExportSymbolKind::ReExport {
                                target_module_id: target_module_id.clone(),
                            },
                        },
                    );
                }
                ExportKind::ReExportAll { source } => {
                    let Some(target_module_id) =
                        resolved_imports.get(&(parsed.id.clone(), source.clone()))
                    else {
                        continue;
                    };
                    semantic.export_all.push(target_module_id.clone());
                }
                ExportKind::Default(name) => {
                    semantic.exports.insert(
                        "default".to_string(),
                        ExportSymbol {
                            exported_name: "default".to_string(),
                            local_name: if name.is_empty() {
                                None
                            } else {
                                Some(name.clone())
                            },
                            kind: ExportSymbolKind::Local,
                        },
                    );
                }
            }
        }

        graph.add_module(semantic);
    }

    Ok(graph)
}

fn collect_component_props(
    ast: &Module,
    component_props: &mut HashMap<String, HashMap<String, ComponentPropKind>>,
) {
    for item in &ast.body {
        let decl = match item {
            ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(export_decl)) => Some(&export_decl.decl),
            ModuleItem::Stmt(Stmt::Decl(decl)) => Some(decl),
            _ => None,
        };
        let Some(decl) = decl else { continue };

        match decl {
            Decl::Fn(fn_decl) => {
                collect_component_props_from_function(
                    fn_decl.ident.sym.as_ref(),
                    &fn_decl.function,
                    component_props,
                );
            }
            Decl::Var(var_decl) => {
                for declarator in &var_decl.decls {
                    let Pat::Ident(ident) = &declarator.name else {
                        continue;
                    };
                    let Some(init) = declarator.init.as_deref() else {
                        continue;
                    };
                    match init {
                        Expr::Arrow(arrow) => {
                            if let Some(param) = arrow.params.first() {
                                collect_component_props_from_pat(
                                    ident.id.sym.as_ref(),
                                    param,
                                    component_props,
                                );
                            }
                        }
                        Expr::Fn(function) => collect_component_props_from_function(
                            ident.id.sym.as_ref(),
                            &function.function,
                            component_props,
                        ),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}

fn collect_component_props_from_function(
    component: &str,
    function: &Function,
    component_props: &mut HashMap<String, HashMap<String, ComponentPropKind>>,
) {
    if let Some(param) = function.params.first() {
        collect_component_props_from_pat(component, &param.pat, component_props);
    }
}

fn collect_component_props_from_pat(
    component: &str,
    pat: &Pat,
    component_props: &mut HashMap<String, HashMap<String, ComponentPropKind>>,
) {
    let Pat::Object(object) = pat else { return };
    let Some(type_ann) = &object.type_ann else {
        return;
    };
    let TsType::TsTypeLit(type_lit) = &*type_ann.type_ann else {
        return;
    };

    let props = type_lit
        .members
        .iter()
        .filter_map(component_prop_from_type_element)
        .collect::<HashMap<_, _>>();
    if !props.is_empty() {
        component_props.insert(component.to_string(), props);
    }
}

fn component_prop_from_type_element(member: &TsTypeElement) -> Option<(String, ComponentPropKind)> {
    match member {
        TsTypeElement::TsMethodSignature(method) => {
            ts_property_name(&method.key).map(|name| (name, ComponentPropKind::Callable))
        }
        TsTypeElement::TsPropertySignature(property) => {
            let kind = match property
                .type_ann
                .as_deref()
                .map(|ann| ann.type_ann.as_ref())
            {
                Some(TsType::TsFnOrConstructorType(TsFnOrConstructorType::TsFnType(_))) => {
                    ComponentPropKind::Callable
                }
                _ => ComponentPropKind::Value,
            };
            ts_property_name(&property.key).map(|name| (name, kind))
        }
        _ => None,
    }
}

fn ts_property_name(key: &Expr) -> Option<String> {
    match key {
        Expr::Ident(ident) => Some(ident.sym.to_string()),
        Expr::Lit(Lit::Str(value)) => value.value.as_str().map(str::to_string),
        _ => None,
    }
}

fn collect_decl(decl: &Decl, locals: &mut HashMap<String, LocalSymbol>) {
    match decl {
        Decl::Fn(fn_decl) => {
            let name = fn_decl.ident.sym.to_string();
            locals.insert(
                name.clone(),
                LocalSymbol {
                    name,
                    kind: SymbolKind::Function,
                    span: fn_decl.ident.span,
                },
            );
        }
        Decl::Var(var_decl) => {
            for decl in &var_decl.decls {
                if let Pat::Ident(ident) = &decl.name {
                    let name = ident.id.sym.to_string();
                    let kind = if is_function_valued(&decl.init) {
                        SymbolKind::Function
                    } else {
                        SymbolKind::Variable
                    };
                    locals.insert(
                        name.clone(),
                        LocalSymbol {
                            name,
                            kind,
                            span: decl.span,
                        },
                    );
                }
            }
        }
        Decl::Class(class_decl) => {
            let name = class_decl.ident.sym.to_string();
            locals.insert(
                name.clone(),
                LocalSymbol {
                    name,
                    kind: SymbolKind::Class,
                    span: class_decl.ident.span,
                },
            );
        }
        _ => {}
    }
}

fn collect_local_declarations(ast: &Module, locals: &mut HashMap<String, LocalSymbol>) {
    for item in &ast.body {
        match item {
            ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(export_decl)) => {
                collect_decl(&export_decl.decl, locals);
            }
            ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultDecl(export_default)) => {
                if let DefaultDecl::Fn(fn_decl) = &export_default.decl {
                    if let Some(ident) = &fn_decl.ident {
                        let name = ident.sym.to_string();
                        locals.insert(
                            name.clone(),
                            LocalSymbol {
                                name,
                                kind: SymbolKind::Function,
                                span: export_default.span,
                            },
                        );
                    }
                }
                if let DefaultDecl::Class(class_decl) = &export_default.decl {
                    if let Some(ident) = &class_decl.ident {
                        let name = ident.sym.to_string();
                        locals.insert(
                            name.clone(),
                            LocalSymbol {
                                name,
                                kind: SymbolKind::Class,
                                span: export_default.span,
                            },
                        );
                    }
                }
            }
            ModuleItem::Stmt(stmt) => {
                if let Stmt::Decl(decl) = stmt {
                    collect_decl(decl, locals);
                }
            }
            _ => {}
        }
    }
}

use swc_ecma_ast::{DefaultDecl, Stmt};

fn is_function_valued(expr: &Option<Box<Expr>>) -> bool {
    match expr {
        None => false,
        Some(expr) => {
            let expr = unwrap_component_factory(expr.as_ref());
            match &*expr {
                Expr::Arrow(_) => true,
                Expr::Fn(_) => true,
                _ => false,
            }
        }
    }
}

fn unwrap_component_factory(expr: &Expr) -> &Expr {
    match expr {
        Expr::Call(call) if is_component_factory_call(call) => call
            .args
            .first()
            .and_then(|arg| match &*arg.expr {
                Expr::Arrow(_) | Expr::Fn(_) => Some(arg.expr.as_ref()),
                _ => None,
            })
            .unwrap_or(expr),
        _ => expr,
    }
}

fn is_component_factory_call(call: &CallExpr) -> bool {
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

pub fn resolve_local_symbol(
    graph: &SemanticGraph,
    module_id: &str,
    local_name: &str,
) -> Option<ResolvedSymbol> {
    let module = graph.get_module(module_id)?;

    if let Some(local) = module.locals.get(local_name) {
        return Some(ResolvedSymbol {
            module_id: module.id.clone(),
            local_name: local.name.clone(),
            exported_name: None,
            kind: local.kind,
            span: local.span,
        });
    }

    if let Some(import) = module.imports.get(local_name) {
        if import.type_only {
            return None;
        }
        if import.imported_name == "*" {
            return Some(ResolvedSymbol {
                module_id: import.target_module_id.clone(),
                local_name: "*".to_string(),
                exported_name: None,
                kind: SymbolKind::Namespace,
                span: Span::new(swc_common::BytePos(0), swc_common::BytePos(0)),
            });
        }
        if import.target_module_id.starts_with("host:") {
            return Some(ResolvedSymbol {
                module_id: import.target_module_id.clone(),
                local_name: import.imported_name.clone(),
                exported_name: Some(import.imported_name.clone()),
                kind: SymbolKind::Function,
                span: Span::new(swc_common::BytePos(0), swc_common::BytePos(0)),
            });
        }
        return resolve_export(graph, &import.target_module_id, &import.imported_name);
    }

    None
}

pub fn resolve_export(
    graph: &SemanticGraph,
    module_id: &str,
    exported_name: &str,
) -> Option<ResolvedSymbol> {
    resolve_export_with_visited(graph, module_id, exported_name, &mut HashSet::new())
}

fn resolve_export_with_visited(
    graph: &SemanticGraph,
    module_id: &str,
    exported_name: &str,
    visited: &mut HashSet<(ModuleId, String)>,
) -> Option<ResolvedSymbol> {
    if module_id.starts_with("host:") {
        return Some(ResolvedSymbol {
            module_id: module_id.to_string(),
            local_name: exported_name.to_string(),
            exported_name: Some(exported_name.to_string()),
            kind: SymbolKind::Function,
            span: Span::new(swc_common::BytePos(0), swc_common::BytePos(0)),
        });
    }
    let key = (module_id.to_string(), exported_name.to_string());
    if !visited.insert(key) {
        return None;
    }

    let module = graph.get_module(module_id)?;

    if let Some(export) = module.exports.get(exported_name) {
        match &export.kind {
            ExportSymbolKind::Local => {
                if let Some(local_name) = &export.local_name {
                    if let Some(local) = module.locals.get(local_name) {
                        return Some(ResolvedSymbol {
                            module_id: module.id.clone(),
                            local_name: local.name.clone(),
                            exported_name: Some(export.exported_name.clone()),
                            kind: local.kind,
                            span: local.span,
                        });
                    }
                }
                return Some(ResolvedSymbol {
                    module_id: module.id.clone(),
                    local_name: export.local_name.clone().unwrap_or_default(),
                    exported_name: Some(export.exported_name.clone()),
                    kind: SymbolKind::Variable,
                    span: Span::new(swc_common::BytePos(0), swc_common::BytePos(0)),
                });
            }
            ExportSymbolKind::ReExport { target_module_id } => {
                if let Some(local_name) = &export.local_name {
                    let mut result =
                        resolve_export_with_visited(graph, target_module_id, local_name, visited);
                    if let Some(symbol) = &mut result {
                        symbol.exported_name = Some(export.exported_name.clone());
                    }
                    return result;
                }
            }
        }
    }

    for target_module_id in &module.export_all {
        if let Some(result) =
            resolve_export_with_visited(graph, target_module_id, exported_name, visited)
        {
            return Some(ResolvedSymbol {
                module_id: result.module_id,
                local_name: result.local_name,
                exported_name: Some(exported_name.to_string()),
                kind: result.kind,
                span: result.span,
            });
        }
    }

    None
}

pub fn resolve_function(
    graph: &SemanticGraph,
    module_id: &str,
    name: &str,
) -> Option<ResolvedSymbol> {
    let symbol = resolve_local_symbol(graph, module_id, name)?;
    if matches!(symbol.kind, SymbolKind::Function) {
        Some(symbol)
    } else {
        None
    }
}

pub fn resolve_component(
    graph: &SemanticGraph,
    module_id: &str,
    name: &str,
) -> Option<ResolvedSymbol> {
    let symbol = resolve_local_symbol(graph, module_id, name)?;
    if matches!(symbol.kind, SymbolKind::Function | SymbolKind::Variable) {
        Some(symbol)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plec_parser::{collect_exports, collect_imports, parse_module};

    fn build_test_graph(
        modules: Vec<(&std::path::Path, &str)>,
    ) -> (SemanticGraph, HashMap<(ModuleId, String), ModuleId>) {
        let mut parsed = Vec::new();
        let mut resolved_imports = HashMap::new();
        let mut path_to_id = HashMap::new();

        for (path, source) in &modules {
            let file_name = path.file_name().unwrap().to_string_lossy();
            path_to_id.insert(file_name.to_string(), file_name.to_string());
            let module = parse_module(file_name, *source).expect("module should parse");
            parsed.push(module);
        }

        for (i, (path, _)) in modules.iter().enumerate() {
            for import in collect_imports(&parsed[i].ast) {
                let specifier = import.source.strip_prefix("./").unwrap_or(&import.source);
                let target_path = path.parent().unwrap().join(specifier).with_extension("tsx");
                let target_id = target_path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string();
                resolved_imports.insert((parsed[i].id.clone(), import.source.clone()), target_id);
            }

            for export in collect_exports(&parsed[i].ast) {
                if let ExportKind::ReExport { source, .. } | ExportKind::ReExportAll { source } =
                    export.kind
                {
                    let specifier = source.strip_prefix("./").unwrap_or(&source);
                    let target_path = path.parent().unwrap().join(specifier).with_extension("tsx");
                    let target_id = target_path
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .to_string();
                    resolved_imports.insert((parsed[i].id.clone(), source.clone()), target_id);
                }
            }
        }

        (
            build_semantic_graph(&parsed, &resolved_imports).expect("graph should build"),
            resolved_imports,
        )
    }

    #[test]
    fn collects_function_declaration() {
        let source = r#"
            function Foo() {}
        "#;
        let module = parse_module("test.tsx", source).expect("parse should succeed");
        let mut locals = HashMap::new();
        collect_local_declarations(&module.ast, &mut locals);

        assert_eq!(locals.len(), 1);
        let foo = locals.get("Foo").unwrap();
        assert_eq!(foo.name, "Foo");
        assert_eq!(foo.kind, SymbolKind::Function);
    }

    #[test]
    fn collects_arrow_function() {
        let source = r#"
            const Bar = () => {};
        "#;
        let module = parse_module("test.tsx", source).expect("parse should succeed");
        let mut locals = HashMap::new();
        collect_local_declarations(&module.ast, &mut locals);

        assert_eq!(locals.len(), 1);
        let bar = locals.get("Bar").unwrap();
        assert_eq!(bar.name, "Bar");
        assert_eq!(bar.kind, SymbolKind::Function);
    }

    #[test]
    fn distinguishes_function_from_variable() {
        let source = r#"
            const Foo = () => {};
            const Bar = 1;
        "#;
        let module = parse_module("test.tsx", source).expect("parse should succeed");
        let mut locals = HashMap::new();
        collect_local_declarations(&module.ast, &mut locals);

        assert_eq!(locals.get("Foo").unwrap().kind, SymbolKind::Function);
        assert_eq!(locals.get("Bar").unwrap().kind, SymbolKind::Variable);
    }

    #[test]
    fn collects_class_declaration() {
        let source = r#"
            class Foo {}
        "#;
        let module = parse_module("test.tsx", source).expect("parse should succeed");
        let mut locals = HashMap::new();
        collect_local_declarations(&module.ast, &mut locals);

        assert_eq!(locals.len(), 1);
        let foo = locals.get("Foo").unwrap();
        assert_eq!(foo.kind, SymbolKind::Class);
    }

    #[test]
    fn collects_inline_component_callable_props() {
        let (graph, _) = build_test_graph(vec![(
            std::path::Path::new("test.tsx"),
            r#"
                function Child({ value, onSave, onChange }: {
                    value: string;
                    onSave(): void;
                    onChange: (next: string) => void;
                }) {}
            "#,
        )]);

        let props = graph
            .get_module("test.tsx")
            .unwrap()
            .component_props
            .get("Child")
            .unwrap();
        assert_eq!(props.get("value"), Some(&ComponentPropKind::Value));
        assert_eq!(props.get("onSave"), Some(&ComponentPropKind::Callable));
        assert_eq!(props.get("onChange"), Some(&ComponentPropKind::Callable));
    }

    #[test]
    fn resolves_named_import() {
        let (graph, _) = build_test_graph(vec![
            (
                std::path::Path::new("foo.tsx"),
                r#"export function Foo() {}"#,
            ),
            (
                std::path::Path::new("bar.tsx"),
                r#"import { Foo } from "./foo";"#,
            ),
        ]);

        let symbol = resolve_local_symbol(&graph, "bar.tsx", "Foo");
        assert!(symbol.is_some());
        let symbol = symbol.unwrap();
        assert_eq!(symbol.module_id, "foo.tsx");
        assert_eq!(symbol.local_name, "Foo");
        assert_eq!(symbol.kind, SymbolKind::Function);
    }

    #[test]
    fn resolves_aliased_import() {
        let (graph, _) = build_test_graph(vec![
            (
                std::path::Path::new("foo.tsx"),
                r#"export function Foo() {}"#,
            ),
            (
                std::path::Path::new("bar.tsx"),
                r#"import { Foo as Bar } from "./foo";"#,
            ),
        ]);

        let symbol = resolve_local_symbol(&graph, "bar.tsx", "Bar");
        assert!(symbol.is_some());
        let symbol = symbol.unwrap();
        assert_eq!(symbol.module_id, "foo.tsx");
        assert_eq!(symbol.local_name, "Foo");
    }

    #[test]
    fn resolves_default_import() {
        let (graph, _) = build_test_graph(vec![
            (
                std::path::Path::new("foo.tsx"),
                r#"const Foo = () => {}; export default Foo;"#,
            ),
            (
                std::path::Path::new("bar.tsx"),
                r#"import Foo from "./foo";"#,
            ),
        ]);

        let symbol = resolve_local_symbol(&graph, "bar.tsx", "Foo");
        assert!(symbol.is_some());
        let symbol = symbol.unwrap();
        assert_eq!(symbol.module_id, "foo.tsx");
        assert_eq!(symbol.local_name, "Foo");
        assert_eq!(symbol.exported_name, Some("default".to_string()));
    }

    #[test]
    fn resolves_local_named_export() {
        let source = r#"const Foo = () => {}; export { Foo };"#;
        let (graph, _) = build_test_graph(vec![(std::path::Path::new("test.tsx"), source)]);

        let symbol = resolve_export(&graph, "test.tsx", "Foo");
        assert!(symbol.is_some());
        let symbol = symbol.unwrap();
        assert_eq!(symbol.module_id, "test.tsx");
        assert_eq!(symbol.local_name, "Foo");
        assert_eq!(symbol.exported_name, Some("Foo".to_string()));
    }

    #[test]
    fn resolves_aliased_local_export() {
        let source = r#"const Foo = () => {}; export { Foo as Bar };"#;
        let (graph, _) = build_test_graph(vec![(std::path::Path::new("test.tsx"), source)]);

        let symbol = resolve_export(&graph, "test.tsx", "Bar");
        assert!(symbol.is_some());
        let symbol = symbol.unwrap();
        assert_eq!(symbol.local_name, "Foo");
        assert_eq!(symbol.exported_name, Some("Bar".to_string()));
    }

    #[test]
    fn resolves_re_export() {
        let (graph, _) = build_test_graph(vec![
            (std::path::Path::new("B.tsx"), r#"export function Foo() {}"#),
            (
                std::path::Path::new("A.tsx"),
                r#"export { Foo } from "./B";"#,
            ),
        ]);

        let symbol = resolve_export(&graph, "A.tsx", "Foo");
        assert!(symbol.is_some());
        let symbol = symbol.unwrap();
        assert_eq!(symbol.module_id, "B.tsx");
        assert_eq!(symbol.local_name, "Foo");
    }

    #[test]
    fn resolves_aliased_re_export() {
        let (graph, _) = build_test_graph(vec![
            (std::path::Path::new("B.tsx"), r#"export function Foo() {}"#),
            (
                std::path::Path::new("A.tsx"),
                r#"export { Foo as Bar } from "./B";"#,
            ),
        ]);

        let symbol = resolve_export(&graph, "A.tsx", "Bar");
        assert!(symbol.is_some());
        let symbol = symbol.unwrap();
        assert_eq!(symbol.module_id, "B.tsx");
        assert_eq!(symbol.local_name, "Foo");
        assert_eq!(symbol.exported_name, Some("Bar".to_string()));
    }

    #[test]
    fn resolves_re_export_chain() {
        let (graph, _) = build_test_graph(vec![
            (std::path::Path::new("C.tsx"), r#"export function Foo() {}"#),
            (
                std::path::Path::new("B.tsx"),
                r#"export { Foo } from "./C";"#,
            ),
            (
                std::path::Path::new("A.tsx"),
                r#"export { Foo } from "./B";"#,
            ),
        ]);

        let symbol = resolve_export(&graph, "A.tsx", "Foo");
        assert!(symbol.is_some());
        let symbol = symbol.unwrap();
        assert_eq!(symbol.module_id, "C.tsx");
        assert_eq!(symbol.local_name, "Foo");
    }

    #[test]
    fn resolves_export_all() {
        let (graph, _) = build_test_graph(vec![
            (std::path::Path::new("B.tsx"), r#"export function Foo() {}"#),
            (std::path::Path::new("A.tsx"), r#"export * from "./B";"#),
        ]);

        let symbol = resolve_export(&graph, "A.tsx", "Foo");
        assert!(symbol.is_some());
        let symbol = symbol.unwrap();
        assert_eq!(symbol.module_id, "B.tsx");
        assert_eq!(symbol.local_name, "Foo");
    }

    #[test]
    fn handles_circular_re_export() {
        let (graph, _) = build_test_graph(vec![
            (std::path::Path::new("A.tsx"), r#"export * from "./B";"#),
            (std::path::Path::new("B.tsx"), r#"export * from "./A";"#),
        ]);

        let symbol = resolve_export(&graph, "A.tsx", "Foo");
        assert!(symbol.is_none());
    }

    #[test]
    fn returns_none_for_missing_export() {
        let (graph, _) = build_test_graph(vec![(
            std::path::Path::new("test.tsx"),
            r#"const Foo = () => {};"#,
        )]);

        let symbol = resolve_export(&graph, "test.tsx", "Bar");
        assert!(symbol.is_none());
    }

    #[test]
    fn type_only_import_does_not_resolve() {
        let (graph, _) = build_test_graph(vec![
            (
                std::path::Path::new("foo.tsx"),
                r#"export function Foo() {}"#,
            ),
            (
                std::path::Path::new("bar.tsx"),
                r#"import type { Foo } from "./foo";"#,
            ),
        ]);

        let symbol = resolve_local_symbol(&graph, "bar.tsx", "Foo");
        assert!(symbol.is_none());
    }

    #[test]
    fn same_symbol_name_in_different_modules() {
        let (graph, _) = build_test_graph(vec![
            (
                std::path::Path::new("A.tsx"),
                r#"export function Foo() { return "A"; }"#,
            ),
            (
                std::path::Path::new("B.tsx"),
                r#"export function Foo() { return "B"; }"#,
            ),
        ]);

        let a_symbol = resolve_export(&graph, "A.tsx", "Foo");
        let b_symbol = resolve_export(&graph, "B.tsx", "Foo");

        assert!(a_symbol.is_some());
        assert!(b_symbol.is_some());
        assert_eq!(a_symbol.unwrap().module_id, "A.tsx");
        assert_eq!(b_symbol.unwrap().module_id, "B.tsx");
    }

    #[test]
    fn resolve_function_finds_only_functions() {
        let (graph, _) = build_test_graph(vec![(
            std::path::Path::new("test.tsx"),
            r#"
                function Foo() {}
                const Bar = 1;
            "#,
        )]);

        assert!(resolve_function(&graph, "test.tsx", "Foo").is_some());
        assert!(resolve_function(&graph, "test.tsx", "Bar").is_none());
    }

    #[test]
    fn resolve_component_finds_functions() {
        let (graph, _) = build_test_graph(vec![(
            std::path::Path::new("test.tsx"),
            r#"function Foo() {}"#,
        )]);

        assert!(resolve_component(&graph, "test.tsx", "Foo").is_some());
    }

    #[test]
    fn multiple_export_all_resolves_from_both_sources() {
        let (graph, _) = build_test_graph(vec![
            (std::path::Path::new("B.tsx"), r#"export function Foo() {}"#),
            (std::path::Path::new("C.tsx"), r#"export function Bar() {}"#),
            (
                std::path::Path::new("A.tsx"),
                r#"export * from "./B"; export * from "./C";"#,
            ),
        ]);

        let foo = resolve_export(&graph, "A.tsx", "Foo");
        let bar = resolve_export(&graph, "A.tsx", "Bar");

        assert!(foo.is_some());
        assert_eq!(foo.unwrap().module_id, "B.tsx");
        assert!(bar.is_some());
        assert_eq!(bar.unwrap().module_id, "C.tsx");
    }

    #[test]
    fn class_declaration_has_correct_kind() {
        let (graph, _) = build_test_graph(vec![(
            std::path::Path::new("test.tsx"),
            r#"class MyClass {}"#,
        )]);

        let symbol = resolve_local_symbol(&graph, "test.tsx", "MyClass");
        assert!(symbol.is_some());
        assert_eq!(symbol.unwrap().kind, SymbolKind::Class);
    }

    #[test]
    fn anonymous_default_function_resolves_with_empty_local_name() {
        let (graph, _) = build_test_graph(vec![(
            std::path::Path::new("test.tsx"),
            r#"export default function() {}"#,
        )]);

        let symbol = resolve_export(&graph, "test.tsx", "default");
        assert!(symbol.is_some());
        let symbol = symbol.unwrap();
        assert_eq!(symbol.local_name, "");
    }

    #[test]
    fn named_default_function_resolves_with_name() {
        let (graph, _) = build_test_graph(vec![(
            std::path::Path::new("test.tsx"),
            r#"export default function Foo() {}"#,
        )]);

        let symbol = resolve_export(&graph, "test.tsx", "default");
        assert!(symbol.is_some());
        let symbol = symbol.unwrap();
        assert_eq!(symbol.local_name, "Foo");
        assert_eq!(symbol.kind, SymbolKind::Function);
    }

    #[test]
    fn anonymous_default_class_resolves_with_empty_local_name() {
        let (graph, _) = build_test_graph(vec![(
            std::path::Path::new("test.tsx"),
            r#"export default class {}"#,
        )]);

        let symbol = resolve_export(&graph, "test.tsx", "default");
        assert!(symbol.is_some());
        let symbol = symbol.unwrap();
        assert_eq!(symbol.local_name, "");
    }

    #[test]
    fn named_default_class_resolves_with_name() {
        let (graph, _) = build_test_graph(vec![(
            std::path::Path::new("test.tsx"),
            r#"export default class Foo {}"#,
        )]);

        let symbol = resolve_export(&graph, "test.tsx", "default");
        assert!(symbol.is_some());
        let symbol = symbol.unwrap();
        assert_eq!(symbol.local_name, "Foo");
        assert_eq!(symbol.kind, SymbolKind::Class);
    }

    #[test]
    fn namespace_import_resolves_to_namespace_kind() {
        let (graph, _) = build_test_graph(vec![
            (
                std::path::Path::new("foo.tsx"),
                r#"export function Foo() {}"#,
            ),
            (
                std::path::Path::new("bar.tsx"),
                r#"import * as Foo from "./foo";"#,
            ),
        ]);

        let symbol = resolve_local_symbol(&graph, "bar.tsx", "Foo");
        assert!(symbol.is_some());
        assert_eq!(symbol.unwrap().kind, SymbolKind::Namespace);
    }

    #[test]
    fn default_export_from_variable() {
        let (graph, _) = build_test_graph(vec![(
            std::path::Path::new("test.tsx"),
            r#"const Foo = () => {}; export default Foo;"#,
        )]);

        let symbol = resolve_export(&graph, "test.tsx", "default");
        assert!(symbol.is_some());
        let symbol = symbol.unwrap();
        assert_eq!(symbol.local_name, "Foo");
        assert_eq!(symbol.kind, SymbolKind::Function);
    }

    #[test]
    fn resolved_symbol_preserves_span_for_function() {
        let source = r#"function Foo() {}"#;
        let module = parse_module("test.tsx", source).expect("parse should succeed");
        let mut locals = HashMap::new();
        collect_local_declarations(&module.ast, &mut locals);

        let foo = locals.get("Foo").unwrap();
        assert!(foo.span.lo.0 > 0);
    }

    #[test]
    fn resolved_symbol_preserves_span_for_class() {
        let source = r#"class Foo {}"#;
        let module = parse_module("test.tsx", source).expect("parse should succeed");
        let mut locals = HashMap::new();
        collect_local_declarations(&module.ast, &mut locals);

        let foo = locals.get("Foo").unwrap();
        assert!(foo.span.lo.0 > 0);
    }

    #[test]
    fn resolved_symbol_preserves_span_for_variable() {
        let source = r#"const Foo = 1;"#;
        let module = parse_module("test.tsx", source).expect("parse should succeed");
        let mut locals = HashMap::new();
        collect_local_declarations(&module.ast, &mut locals);

        let foo = locals.get("Foo").unwrap();
        assert!(foo.span.lo.0 > 0);
    }

    #[test]
    fn local_function_resolution_still_works() {
        let (graph, _) = build_test_graph(vec![(
            std::path::Path::new("test.tsx"),
            r#"function Foo() {}"#,
        )]);

        let symbol = resolve_local_symbol(&graph, "test.tsx", "Foo");
        assert!(symbol.is_some());
        let symbol = symbol.unwrap();
        assert_eq!(symbol.module_id, "test.tsx");
        assert_eq!(symbol.local_name, "Foo");
        assert_eq!(symbol.kind, SymbolKind::Function);
    }

    #[test]
    fn aliased_re_export_still_resolves() {
        let (graph, _) = build_test_graph(vec![
            (std::path::Path::new("B.tsx"), r#"export function Foo() {}"#),
            (
                std::path::Path::new("A.tsx"),
                r#"export { Foo as Bar } from "./B";"#,
            ),
        ]);

        let symbol = resolve_export(&graph, "A.tsx", "Bar");
        assert!(symbol.is_some());
        assert_eq!(symbol.unwrap().module_id, "B.tsx");
    }

    #[test]
    fn default_import_still_resolves() {
        let (graph, _) = build_test_graph(vec![
            (
                std::path::Path::new("foo.tsx"),
                r#"const Foo = () => {}; export default Foo;"#,
            ),
            (
                std::path::Path::new("bar.tsx"),
                r#"import Foo from "./foo";"#,
            ),
        ]);

        let symbol = resolve_local_symbol(&graph, "bar.tsx", "Foo");
        assert!(symbol.is_some());
        assert_eq!(symbol.unwrap().module_id, "foo.tsx");
    }

    #[test]
    fn cycle_detection_uses_hashset() {
        let (graph, _) = build_test_graph(vec![
            (
                std::path::Path::new("A.tsx"),
                r#"export { Foo } from "./B";"#,
            ),
            (
                std::path::Path::new("B.tsx"),
                r#"export { Foo } from "./A";"#,
            ),
        ]);

        let symbol = resolve_export(&graph, "A.tsx", "Foo");
        assert!(symbol.is_none());
    }
}
