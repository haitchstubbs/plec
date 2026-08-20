use swc_ecma_ast::{Ident, Module, ModuleDecl, ModuleItem};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportSpecifier {
    Named { imported: String, local: String },
    Default(String),
    Namespace(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    pub source: String,
    pub specifiers: Vec<ImportSpecifier>,
    pub type_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportKind {
    Local {
        local: String,
        exported: Option<String>,
    },
    ReExport {
        source: String,
        local: String,
        exported: Option<String>,
    },
    ReExportAll {
        source: String,
    },
    Default(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Export {
    pub kind: ExportKind,
}

fn module_export_name_to_string(name: &swc_ecma_ast::ModuleExportName) -> String {
    match name {
        swc_ecma_ast::ModuleExportName::Ident(ident) => ident.sym.to_string(),
        swc_ecma_ast::ModuleExportName::Str(s) => {
            // Wtf8Atom doesn't implement Display, as_str() returns Option<&str>
            s.value
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| s.value.to_string_lossy().to_string())
        }
    }
}

fn ident_to_string(ident: &Ident) -> String {
    ident.sym.to_string()
}

pub fn collect_imports(module: &Module) -> Vec<Import> {
    module
        .body
        .iter()
        .filter_map(|item| match item {
            ModuleItem::ModuleDecl(ModuleDecl::Import(import_decl)) => {
                let source = import_decl.src.value.to_string_lossy().into_owned();
                let type_only = import_decl.type_only;

                let specifiers = import_decl
                    .specifiers
                    .iter()
                    .map(|specifier| match specifier {
                        swc_ecma_ast::ImportSpecifier::Named(named) => {
                            let imported = named
                                .imported
                                .as_ref()
                                .map(module_export_name_to_string)
                                .unwrap_or_else(|| ident_to_string(&named.local));

                            let local = ident_to_string(&named.local);

                            ImportSpecifier::Named { imported, local }
                        }
                        swc_ecma_ast::ImportSpecifier::Default(default) => {
                            let local = ident_to_string(&default.local);
                            ImportSpecifier::Default(local)
                        }
                        swc_ecma_ast::ImportSpecifier::Namespace(namespace) => {
                            let local = ident_to_string(&namespace.local);
                            ImportSpecifier::Namespace(local)
                        }
                    })
                    .collect();

                Some(Import {
                    source,
                    specifiers,
                    type_only,
                })
            }

            _ => None,
        })
        .collect()
}

pub fn collect_exports(module: &Module) -> Vec<Export> {
    let mut exports = Vec::new();

    for item in &module.body {
        match item {
            ModuleItem::ModuleDecl(ModuleDecl::ExportNamed(export_named)) => {
                if let Some(src) = &export_named.src {
                    let source = src.value.to_string_lossy().into_owned();

                    for specifier in &export_named.specifiers {
                        match specifier {
                            swc_ecma_ast::ExportSpecifier::Named(named) => {
                                let local = module_export_name_to_string(&named.orig);
                                let exported =
                                    named.exported.as_ref().map(module_export_name_to_string);

                                exports.push(Export {
                                    kind: ExportKind::ReExport {
                                        source: source.clone(),
                                        local,
                                        exported,
                                    },
                                });
                            }
                            swc_ecma_ast::ExportSpecifier::Default(default_spec) => {
                                // ExportDefaultSpecifier has only `exported` field
                                // The original name is always "default"
                                let local = "default".to_string();
                                let exported = Some(ident_to_string(&default_spec.exported));

                                exports.push(Export {
                                    kind: ExportKind::ReExport {
                                        source: source.clone(),
                                        local,
                                        exported,
                                    },
                                });
                            }
                            swc_ecma_ast::ExportSpecifier::Namespace(namespace_spec) => {
                                // export * as Foo from "./bar"
                                let exported =
                                    Some(module_export_name_to_string(&namespace_spec.name));
                                exports.push(Export {
                                    kind: ExportKind::ReExport {
                                        source: source.clone(),
                                        local: "*".to_string(),
                                        exported,
                                    },
                                });
                            }
                        }
                    }
                } else {
                    for specifier in &export_named.specifiers {
                        match specifier {
                            swc_ecma_ast::ExportSpecifier::Named(named) => {
                                let local = module_export_name_to_string(&named.orig);
                                let exported =
                                    named.exported.as_ref().map(module_export_name_to_string);

                                exports.push(Export {
                                    kind: ExportKind::Local { local, exported },
                                });
                            }
                            swc_ecma_ast::ExportSpecifier::Default(default_spec) => {
                                // ExportDefaultSpecifier has only `exported` field
                                // The original name is always "default"
                                let local = "default".to_string();
                                let exported = Some(ident_to_string(&default_spec.exported));

                                exports.push(Export {
                                    kind: ExportKind::Local { local, exported },
                                });
                            }
                            swc_ecma_ast::ExportSpecifier::Namespace(namespace_spec) => {
                                // export * as Foo (local namespace export)
                                let exported =
                                    Some(module_export_name_to_string(&namespace_spec.name));
                                exports.push(Export {
                                    kind: ExportKind::Local {
                                        local: "*".to_string(),
                                        exported,
                                    },
                                });
                            }
                        }
                    }
                }
            }
            ModuleItem::ModuleDecl(ModuleDecl::ExportAll(export_all)) => {
                let source = export_all.src.value.to_string_lossy().into_owned();

                exports.push(Export {
                    kind: ExportKind::ReExportAll { source },
                });
            }
            ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultExpr(export_default_expr)) => {
                let name = match &*export_default_expr.expr {
                    swc_ecma_ast::Expr::Ident(ident) => ident.sym.to_string(),
                    _ => "default".to_string(),
                };

                exports.push(Export {
                    kind: ExportKind::Default(name),
                });
            }
            ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultDecl(export_default_decl)) => {
                let name = match &export_default_decl.decl {
                    swc_ecma_ast::DefaultDecl::Class(class_decl) => class_decl
                        .ident
                        .as_ref()
                        .map(|i| i.sym.to_string())
                        .unwrap_or_default(),
                    swc_ecma_ast::DefaultDecl::Fn(fn_decl) => fn_decl
                        .ident
                        .as_ref()
                        .map(|i| i.sym.to_string())
                        .unwrap_or_default(),
                    swc_ecma_ast::DefaultDecl::TsInterfaceDecl(interface_decl) => {
                        interface_decl.id.sym.to_string()
                    }
                };

                exports.push(Export {
                    kind: ExportKind::Default(name),
                });
            }
            ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(export_decl)) => {
                let name = match &export_decl.decl {
                    swc_ecma_ast::Decl::Class(class_decl) => class_decl.ident.sym.to_string(),
                    swc_ecma_ast::Decl::Fn(fn_decl) => fn_decl.ident.sym.to_string(),
                    swc_ecma_ast::Decl::Var(var_decl) => var_decl
                        .decls
                        .first()
                        .and_then(|d| match &d.name {
                            swc_ecma_ast::Pat::Ident(ident) => Some(ident_to_string(&ident.id)),
                            _ => None,
                        })
                        .unwrap_or_default(),
                    swc_ecma_ast::Decl::TsInterface(ts_interface) => {
                        ts_interface.id.sym.to_string()
                    }
                    swc_ecma_ast::Decl::TsTypeAlias(type_alias) => type_alias.id.sym.to_string(),
                    swc_ecma_ast::Decl::TsEnum(ts_enum) => ts_enum.id.sym.to_string(),
                    swc_ecma_ast::Decl::TsModule(ts_module) => match &ts_module.id {
                        swc_ecma_ast::TsModuleName::Ident(ident) => ident.sym.to_string(),
                        swc_ecma_ast::TsModuleName::Str(s) => s
                            .value
                            .as_str()
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| s.value.to_string_lossy().to_string()),
                    },
                    swc_ecma_ast::Decl::Using(using_decl) => using_decl
                        .decls
                        .first()
                        .and_then(|d| match &d.name {
                            swc_ecma_ast::Pat::Ident(ident) => Some(ident_to_string(&ident.id)),
                            _ => None,
                        })
                        .unwrap_or_default(),
                };

                exports.push(Export {
                    kind: ExportKind::Local {
                        local: name,
                        exported: None,
                    },
                });
            }
            _ => {}
        }
    }

    exports
}

pub fn module_dependencies(module: &Module) -> Vec<String> {
    let mut deps = std::collections::HashSet::new();

    for item in &module.body {
        match item {
            ModuleItem::ModuleDecl(ModuleDecl::Import(import_decl)) => {
                deps.insert(import_decl.src.value.to_string_lossy().into_owned());
            }
            ModuleItem::ModuleDecl(ModuleDecl::ExportAll(export_all)) => {
                deps.insert(export_all.src.value.to_string_lossy().into_owned());
            }
            ModuleItem::ModuleDecl(ModuleDecl::ExportNamed(export_named)) => {
                if let Some(src) = &export_named.src {
                    deps.insert(src.value.to_string_lossy().into_owned());
                }
            }
            _ => {}
        }
    }

    deps.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_module_for_test(source: &str) -> Module {
        crate::parse(source).expect("source should parse")
    }

    #[test]
    fn collects_named_import() {
        let source = r#"
            import { Foo } from "./foo";
        "#;
        let module = parse_module_for_test(source);
        let imports = collect_imports(&module);

        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source, "./foo");
        assert!(!imports[0].type_only);
        assert_eq!(imports[0].specifiers.len(), 1);
        match &imports[0].specifiers[0] {
            ImportSpecifier::Named { imported, local } => {
                assert_eq!(imported, "Foo");
                assert_eq!(local, "Foo");
            }
            _ => panic!("expected Named specifier"),
        }
    }

    #[test]
    fn collects_aliased_named_import() {
        let source = r#"
            import { Foo as Bar } from "./foo";
        "#;
        let module = parse_module_for_test(source);
        let imports = collect_imports(&module);

        assert_eq!(imports.len(), 1);
        match &imports[0].specifiers[0] {
            ImportSpecifier::Named { imported, local } => {
                assert_eq!(imported, "Foo");
                assert_eq!(local, "Bar");
            }
            _ => panic!("expected Named specifier"),
        }
    }

    #[test]
    fn collects_default_import() {
        let source = r#"
            import Foo from "./foo";
        "#;
        let module = parse_module_for_test(source);
        let imports = collect_imports(&module);

        assert_eq!(imports.len(), 1);
        match &imports[0].specifiers[0] {
            ImportSpecifier::Default(local) => {
                assert_eq!(local, "Foo");
            }
            _ => panic!("expected Default specifier"),
        }
    }

    #[test]
    fn collects_namespace_import() {
        let source = r#"
            import * as Foo from "./foo";
        "#;
        let module = parse_module_for_test(source);
        let imports = collect_imports(&module);

        assert_eq!(imports.len(), 1);
        match &imports[0].specifiers[0] {
            ImportSpecifier::Namespace(local) => {
                assert_eq!(local, "Foo");
            }
            _ => panic!("expected Namespace specifier"),
        }
    }

    #[test]
    fn collects_side_effect_import() {
        let source = r#"
            import "./setup";
        "#;
        let module = parse_module_for_test(source);
        let imports = collect_imports(&module);

        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source, "./setup");
        assert!(imports[0].specifiers.is_empty());
    }

    #[test]
    fn collects_type_only_import() {
        let source = r#"
            import type { Foo } from "./foo";
        "#;
        let module = parse_module_for_test(source);
        let imports = collect_imports(&module);

        assert_eq!(imports.len(), 1);
        assert!(imports[0].type_only);
    }

    #[test]
    fn collects_multiple_imports_from_same_module() {
        let source = r#"
            import { Foo, Bar } from "./foo";
        "#;
        let module = parse_module_for_test(source);
        let imports = collect_imports(&module);

        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].specifiers.len(), 2);
    }

    #[test]
    fn collects_mixed_imports_from_same_module() {
        let source = r#"
            import Foo, { Bar, Baz as Qux } from "./foo";
        "#;
        let module = parse_module_for_test(source);
        let imports = collect_imports(&module);

        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].specifiers.len(), 3);
    }

    #[test]
    fn collects_local_named_export() {
        let source = r#"
            export { Foo };
        "#;
        let module = parse_module_for_test(source);
        let exports = collect_exports(&module);

        assert_eq!(exports.len(), 1);
        match &exports[0].kind {
            ExportKind::Local { local, exported } => {
                assert_eq!(local, "Foo");
                assert!(exported.is_none());
            }
            _ => panic!("expected Local export"),
        }
    }

    #[test]
    fn collects_aliased_local_export() {
        let source = r#"
            export { Foo as Bar };
        "#;
        let module = parse_module_for_test(source);
        let exports = collect_exports(&module);

        assert_eq!(exports.len(), 1);
        match &exports[0].kind {
            ExportKind::Local { local, exported } => {
                assert_eq!(local, "Foo");
                assert_eq!(exported.as_ref().unwrap(), "Bar");
            }
            _ => panic!("expected Local export"),
        }
    }

    #[test]
    fn collects_re_export() {
        let source = r#"
            export { Foo } from "./foo";
        "#;
        let module = parse_module_for_test(source);
        let exports = collect_exports(&module);

        assert_eq!(exports.len(), 1);
        match &exports[0].kind {
            ExportKind::ReExport {
                source,
                local,
                exported,
            } => {
                assert_eq!(source, "./foo");
                assert_eq!(local, "Foo");
                assert!(exported.is_none());
            }
            _ => panic!("expected ReExport"),
        }
    }

    #[test]
    fn collects_aliased_re_export() {
        let source = r#"
            export { Foo as Bar } from "./foo";
        "#;
        let module = parse_module_for_test(source);
        let exports = collect_exports(&module);

        assert_eq!(exports.len(), 1);
        match &exports[0].kind {
            ExportKind::ReExport {
                source,
                local,
                exported,
            } => {
                assert_eq!(source, "./foo");
                assert_eq!(local, "Foo");
                assert_eq!(exported.as_ref().unwrap(), "Bar");
            }
            _ => panic!("expected ReExport"),
        }
    }

    #[test]
    fn collects_export_all() {
        let source = r#"
            export * from "./foo";
        "#;
        let module = parse_module_for_test(source);
        let exports = collect_exports(&module);

        assert_eq!(exports.len(), 1);
        match &exports[0].kind {
            ExportKind::ReExportAll { source } => {
                assert_eq!(source, "./foo");
            }
            _ => panic!("expected ReExportAll"),
        }
    }

    #[test]
    fn collects_default_export() {
        let source = r#"
            export default Foo;
        "#;
        let module = parse_module_for_test(source);
        let exports = collect_exports(&module);

        assert_eq!(exports.len(), 1);
        match &exports[0].kind {
            ExportKind::Default(name) => {
                assert_eq!(name, "Foo");
            }
            _ => panic!("expected Default export"),
        }
    }

    #[test]
    fn collects_default_function_export() {
        let source = r#"
            export default function Foo() {}
        "#;
        let module = parse_module_for_test(source);
        let exports = collect_exports(&module);

        assert_eq!(exports.len(), 1);
        match &exports[0].kind {
            ExportKind::Default(name) => {
                assert_eq!(name, "Foo");
            }
            _ => panic!("expected Default export"),
        }
    }

    #[test]
    fn collects_default_class_export() {
        let source = r#"
            export default class Foo {}
        "#;
        let module = parse_module_for_test(source);
        let exports = collect_exports(&module);

        assert_eq!(exports.len(), 1);
        match &exports[0].kind {
            ExportKind::Default(name) => {
                assert_eq!(name, "Foo");
            }
            _ => panic!("expected Default export"),
        }
    }

    #[test]
    fn collects_anonymous_default_function_export() {
        let source = r#"
            export default function() {}
        "#;
        let module = parse_module_for_test(source);
        let exports = collect_exports(&module);

        assert_eq!(exports.len(), 1);
        match &exports[0].kind {
            ExportKind::Default(name) => {
                assert!(name.is_empty());
            }
            _ => panic!("expected Default export"),
        }
    }

    #[test]
    fn collects_function_declaration_export() {
        let source = r#"
            export function Foo() {}
        "#;
        let module = parse_module_for_test(source);
        let exports = collect_exports(&module);

        assert_eq!(exports.len(), 1);
        match &exports[0].kind {
            ExportKind::Local { local, exported } => {
                assert_eq!(local, "Foo");
                assert!(exported.is_none());
            }
            _ => panic!("expected Local export"),
        }
    }

    #[test]
    fn collects_class_declaration_export() {
        let source = r#"
            export class Foo {}
        "#;
        let module = parse_module_for_test(source);
        let exports = collect_exports(&module);

        assert_eq!(exports.len(), 1);
        match &exports[0].kind {
            ExportKind::Local { local, exported } => {
                assert_eq!(local, "Foo");
                assert!(exported.is_none());
            }
            _ => panic!("expected Local export"),
        }
    }

    #[test]
    fn collects_const_declaration_export() {
        let source = r#"
            export const Foo = 1;
        "#;
        let module = parse_module_for_test(source);
        let exports = collect_exports(&module);

        assert_eq!(exports.len(), 1);
        match &exports[0].kind {
            ExportKind::Local { local, exported } => {
                assert_eq!(local, "Foo");
                assert!(exported.is_none());
            }
            _ => panic!("expected Local export"),
        }
    }

    #[test]
    fn collects_multiple_exports() {
        let source = r#"
            export { Foo, Bar };
        "#;
        let module = parse_module_for_test(source);
        let exports = collect_exports(&module);

        assert_eq!(exports.len(), 2);
    }

    #[test]
    fn collects_imports_and_exports_together() {
        let source = r#"
            import { Foo } from "./foo";
            export { Bar };
        "#;
        let module = parse_module_for_test(source);

        assert_eq!(collect_imports(&module).len(), 1);
        assert_eq!(collect_exports(&module).len(), 1);
    }

    #[test]
    fn handles_empty_module() {
        let source = "";
        let module = parse_module_for_test(source);

        assert!(collect_imports(&module).is_empty());
        assert!(collect_exports(&module).is_empty());
    }

    #[test]
    fn extracts_module_dependencies_from_imports() {
        let source = r#"
            import { Foo } from "./foo";
        "#;
        let module = parse_module_for_test(source);
        let deps = module_dependencies(&module);

        assert_eq!(deps, vec!["./foo"]);
    }

    #[test]
    fn extracts_module_dependencies_from_re_exports() {
        let source = r#"
            export { Foo } from "./foo";
        "#;
        let module = parse_module_for_test(source);
        let deps = module_dependencies(&module);

        assert_eq!(deps, vec!["./foo"]);
    }

    #[test]
    fn extracts_module_dependencies_from_export_all() {
        let source = r#"
            export * from "./foo";
        "#;
        let module = parse_module_for_test(source);
        let deps = module_dependencies(&module);

        assert_eq!(deps, vec!["./foo"]);
    }

    #[test]
    fn extracts_multiple_unique_dependencies() {
        let source = r#"
            import { Foo } from "./foo";
            import { Bar } from "./bar";
            export * from "./baz";
        "#;
        let module = parse_module_for_test(source);
        let deps = module_dependencies(&module);

        assert_eq!(deps.len(), 3);
        assert!(deps.contains(&"./foo".to_string()));
        assert!(deps.contains(&"./bar".to_string()));
        assert!(deps.contains(&"./baz".to_string()));
    }

    #[test]
    fn deduplicates_same_module_referenced_multiple_times() {
        let source = r#"
            import { Foo } from "./foo";
            import { Bar } from "./foo";
        "#;
        let module = parse_module_for_test(source);
        let deps = module_dependencies(&module);

        assert_eq!(deps, vec!["./foo"]);
    }

    #[test]
    fn local_exports_not_dependencies() {
        let source = r#"
            export { Foo };
        "#;
        let module = parse_module_for_test(source);
        let deps = module_dependencies(&module);

        assert!(deps.is_empty());
    }

    #[test]
    fn collects_namespace_re_export() {
        let source = r#"
            export * as Foo from "./foo";
        "#;
        let module = parse_module_for_test(source);
        let exports = collect_exports(&module);

        assert_eq!(exports.len(), 1);
        match &exports[0].kind {
            ExportKind::ReExport {
                source,
                local,
                exported,
            } => {
                assert_eq!(source, "./foo");
                assert_eq!(local, "*");
                assert_eq!(exported.as_ref().unwrap(), "Foo");
            }
            _ => panic!("expected ReExport"),
        }
    }

    #[test]
    fn namespace_re_export_is_dependency() {
        let source = r#"
            export * as Foo from "./foo";
        "#;
        let module = parse_module_for_test(source);
        let deps = module_dependencies(&module);

        assert_eq!(deps, vec!["./foo"]);
    }
}
