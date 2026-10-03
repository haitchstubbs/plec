use plec_model::{SemanticGraph, resolve_local_symbol};
use sha2::Digest;
use swc_ecma_ast::{Callee, Decl, Expr, ModuleDecl, ModuleItem, Pat, VarDeclKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerActionDeclaration {
    pub id: String,
    pub module_id: String,
    pub export_name: String,
}

/// Discover only exported const declarations whose callee resolves to the
/// `action` export in Plec's core server module.
pub fn discover_server_actions(
    modules: &[plec_parser::ParsedModule],
    graph: &SemanticGraph,
) -> Result<Vec<ServerActionDeclaration>, String> {
    let mut actions = Vec::new();
    for module in modules {
        for item in &module.ast.body {
            let ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(export)) = item else {
                continue;
            };
            let Decl::Var(declaration) = &export.decl else {
                continue;
            };
            for declarator in &declaration.decls {
                let Some(initializer) = &declarator.init else {
                    continue;
                };
                let Expr::Call(call) = initializer.as_ref() else {
                    continue;
                };
                let Callee::Expr(callee) = &call.callee else {
                    continue;
                };
                let Expr::Ident(callee) = callee.as_ref() else {
                    continue;
                };
                let Some(symbol) =
                    resolve_plec_action_symbol(graph, &module.id, callee.sym.as_ref())
                else {
                    continue;
                };
                if symbol.local_name != "action" || !is_plec_core_action_module(&symbol.module_id) {
                    continue;
                }

                let Pat::Ident(name) = &declarator.name else {
                    return Err(format!(
                        "server action in {} must initialize an exported const identifier",
                        module.id
                    ));
                };
                if declaration.kind != VarDeclKind::Const
                    || call.args.len() != 1
                    || !matches!(call.args[0].expr.as_ref(), Expr::Arrow(arrow) if arrow.is_async)
                {
                    return Err(format!(
                        "server action {} in {} must be exported const name = action(async (...) => ...)",
                        name.id.sym, module.id
                    ));
                }
                let export_name = name.id.sym.to_string();
                actions.push(ServerActionDeclaration {
                    id: server_action_id(&module.id, &export_name, &module.source),
                    module_id: module.id.clone(),
                    export_name,
                });
            }
        }
    }
    actions.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(actions)
}

fn resolve_plec_action_symbol(
    graph: &SemanticGraph,
    module_id: &str,
    local_name: &str,
) -> Option<plec_model::ResolvedSymbol> {
    let mut symbol = resolve_local_symbol(graph, module_id, local_name)?;
    // A package barrel may `import { action } from './server'; export { action }`.
    // SemanticGraph resolves the barrel export to its local name, so follow
    // that import binding to the defining Plec server module.
    for _ in 0..graph.modules.len() {
        let Some(module) = graph.get_module(&symbol.module_id) else {
            break;
        };
        let Some(import) = module.imports.get(&symbol.local_name) else {
            break;
        };
        if import.type_only || import.imported_name != "action" {
            break;
        }
        let Some(next) = resolve_local_symbol(graph, &module.id, &import.local_name) else {
            break;
        };
        if next.module_id == symbol.module_id && next.local_name == symbol.local_name {
            break;
        }
        symbol = next;
    }
    Some(symbol)
}

fn is_plec_core_action_module(module_id: &str) -> bool {
    // These are the canonical module IDs emitted by `read_source_graph` for
    // the package's source/workspace and published runtime module layouts.
    // Package resolution itself remains owned by that source graph; this is
    // only the narrow identity check for the defining `action` symbol.
    let module_id = module_id.replace('\\', "/");
    module_id.ends_with("packages/plec/src/server.ts")
        || module_id.ends_with("packages/plec/dist/server.js")
        || module_id.ends_with("node_modules/@plec/core/dist/server.js")
}

fn server_action_id(module_id: &str, export_name: &str, source: &str) -> String {
    let identity = format!(
        "{}#{}#{}",
        module_id.replace('\\', "/"),
        export_name,
        source
    );
    let hash = sha2::Sha256::digest(identity.as_bytes());
    format!(
        "sa_{}",
        hash.iter()
            .take(16)
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn discover(app: &str, import_source: Option<&str>) -> Vec<ServerActionDeclaration> {
        let core = plec_parser::parse_module(
            "packages/plec/src/server.ts",
            "export function action(fn) { return fn; }",
        )
        .unwrap();
        let barrel = plec_parser::parse_module(
            "packages/plec/src/index.ts",
            "import { action } from './server'; export { action };",
        )
        .unwrap();
        let app_module = plec_parser::parse_module("src/app.tsx", app).unwrap();
        let imports = import_source
            .map(|source| {
                HashMap::from([
                    (
                        ("src/app.tsx".into(), source.into()),
                        "packages/plec/src/index.ts".into(),
                    ),
                    (
                        ("packages/plec/src/index.ts".into(), "./server".into()),
                        "packages/plec/src/server.ts".into(),
                    ),
                ])
            })
            .unwrap_or_default();
        let modules = vec![app_module, barrel, core];
        let graph = plec_model::build_semantic_graph(&modules, &imports).unwrap();
        discover_server_actions(&modules, &graph).unwrap()
    }

    #[test]
    fn recognizes_canonical_and_aliased_plec_action_imports() {
        for (source, local) in [
            (
                "import { action } from '@plec/core'; export const foo = action(async () => 1);",
                "foo",
            ),
            (
                "import { action as serverAction } from '@plec/core'; export const foo = serverAction(async () => 1);",
                "foo",
            ),
        ] {
            assert_eq!(discover(source, Some("@plec/core"))[0].export_name, local);
        }
    }

    #[test]
    fn ignores_local_and_third_party_action_names() {
        assert!(
            discover(
                "function action(fn) { return fn; } export const foo = action(async () => 1);",
                None
            )
            .is_empty()
        );
        let app =
            "import { action } from 'another-package'; export const foo = action(async () => 1);";
        // The unresolved third-party import is intentionally not mapped to Plec.
        assert!(discover(app, None).is_empty());
    }

    #[test]
    fn discovery_ids_are_stable_and_change_with_action_source() {
        let first = discover(
            "import { action } from '@plec/core'; export const foo = action(async () => 1);",
            Some("@plec/core"),
        );
        let repeat = discover(
            "import { action } from '@plec/core'; export const foo = action(async () => 1);",
            Some("@plec/core"),
        );
        let changed = discover(
            "import { action } from '@plec/core'; export const foo = action(async () => 2);",
            Some("@plec/core"),
        );
        assert_eq!(first, repeat);
        assert_ne!(first[0].id, changed[0].id);
    }

    #[test]
    fn workspace_source_resolution_discovers_aliased_core_action_semantically() {
        use std::fs;
        let repo = tempfile::tempdir().unwrap();
        let app = repo.path().join("apps/app");
        let core = repo.path().join("packages/plec");
        fs::create_dir_all(app.join("src")).unwrap();
        fs::create_dir_all(core.join("src")).unwrap();
        fs::write(app.join("package.json"), r#"{"name":"app"}"#).unwrap();
        fs::write(
            core.join("package.json"),
            r#"{"name":"@plec/core","exports":{".":"./dist/index.js","./server":"./dist/server.js"}}"#,
        )
        .unwrap();
        fs::write(
            core.join("src/server.ts"),
            "export function action(fn) { return fn; }",
        )
        .unwrap();
        fs::write(
            core.join("src/index.ts"),
            "export { action } from './server';",
        )
        .unwrap();
        fs::write(
            app.join("src/App.tsx"),
            "import { action as serverAction } from '@plec/core'; export const foo = serverAction(async () => 1);",
        )
        .unwrap();

        let source = crate::read_source_graph(app.join("src/App.tsx"), &app, repo.path()).unwrap();
        let graph =
            plec_model::build_semantic_graph(&source.modules, &source.resolved_imports).unwrap();
        let actions = discover_server_actions(&source.modules, &graph).unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].export_name, "foo");
        let action_module = source
            .modules
            .iter()
            .find(|module| {
                module.imports.iter().any(|import| {
                    import.specifiers.iter().any(|specifier| {
                        matches!(specifier, plec_parser::ImportSpecifier::Named { local, .. } if local == "serverAction")
                    })
                })
            })
            .unwrap();
        let resolved = resolve_plec_action_symbol(&graph, &action_module.id, "serverAction")
            .expect("semantic import resolves");
        assert!(is_plec_core_action_module(&resolved.module_id));
    }

    #[test]
    fn canonical_relative_module_identity_makes_action_ids_root_independent() {
        use std::{fs, path::Path};

        fn compile_id(root: &Path) -> String {
            let app = root.join("apps/app");
            let core = root.join("packages/plec");
            fs::create_dir_all(app.join("src")).unwrap();
            fs::create_dir_all(core.join("src")).unwrap();
            fs::write(app.join("package.json"), r#"{"name":"app"}"#).unwrap();
            fs::write(
                core.join("package.json"),
                r#"{"name":"@plec/core","exports":{".":"./dist/index.js"}}"#,
            )
            .unwrap();
            fs::write(
                core.join("src/server.ts"),
                "export function action(fn) { return fn; }",
            )
            .unwrap();
            fs::write(
                core.join("src/index.ts"),
                "export { action } from './server';",
            )
            .unwrap();
            fs::write(
                app.join("src/App.tsx"),
                "import { action } from '@plec/core'; export const foo = action(async () => 1);",
            )
            .unwrap();
            let source = crate::read_source_graph(app.join("src/App.tsx"), &app, root).unwrap();
            let graph = plec_model::build_semantic_graph(&source.modules, &source.resolved_imports)
                .unwrap();
            discover_server_actions(&source.modules, &graph).unwrap()[0]
                .id
                .clone()
        }

        let root_a = tempfile::tempdir().unwrap();
        let root_b = tempfile::tempdir().unwrap();
        assert_ne!(root_a.path(), root_b.path());
        let id_a = compile_id(root_a.path());
        let id_b = compile_id(root_b.path());
        assert_eq!(id_a, id_b);
    }
}
