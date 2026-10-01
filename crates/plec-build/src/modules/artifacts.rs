use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use plec_compiler::{
    CompilerOptions, lower_route_artifacts_with_options, lower_routes,
    read_source_graph_with_options,
};
use plec_model::build_semantic_graph;
use serde::Serialize;

use super::id::sanitize;
use super::json::out;
use super::stage::stage;

use super::build::{BuildError, RuntimeSource, Stage};

pub struct ArtifactOutput {
    pub host_components: BTreeMap<String, BTreeSet<String>>,
    pub server_actions: Vec<super::server::ServerActionImport>,
}

/// Build/dev dependency metadata stored outside `public/`; `plec serve` does
/// not read this file.
#[derive(Serialize)]
struct AssetDependency {
    source: String,
    url: String,
}

/// Emit the Plec compiler artifacts for the routed application in-process.
///
/// This mirrors the `plec-route-manifest --artifacts` pipeline (source graph
/// -> semantic graph -> routes -> executable route artifacts) without
/// spawning another compiler process.
pub fn emit(
    source: &Path,
    app_dir: &Path,
    repo_root: &Path,
    public_dir: &Path,
    host_imports: &std::collections::BTreeMap<String, String>,
    custom_elements: &std::collections::BTreeSet<String>,
    runtime_source: RuntimeSource,
) -> Result<ArtifactOutput, BuildError> {
    let stage = Stage::Compile;

    let source_graph = read_source_graph_with_options(
        source,
        app_dir,
        repo_root,
        &CompilerOptions {
            host_imports: host_imports.clone(),
            custom_elements: custom_elements.clone(),
        },
    )
    .map_err(|error| BuildError::new(stage, error))?;

    let semantic_graph =
        build_semantic_graph(&source_graph.modules, &source_graph.resolved_imports)
            .map_err(|error| BuildError::new(stage, error.to_string()))?;

    let server_actions = discover_server_actions(&source_graph.modules)?;

    let routes = lower_routes(&source_graph.modules, &semantic_graph)
        .map_err(|error| BuildError::new(stage, error.to_string()))?;

    let bundle = lower_route_artifacts_with_options(
        &source_graph.modules,
        &semantic_graph,
        &routes,
        custom_elements,
    )
    .map_err(|error| {
        BuildError::new(
            stage,
            format!("unsupported compiled route application: {error}"),
        )
    })?;

    let graphs_dir = public_dir.join("graphs");

    for artifact in &bundle.graphs {
        let filename = format!("{}.json", sanitize(&artifact.graph_id));

        out(&graphs_dir.join(filename), &artifact.graph, true).map_err(|error| {
            BuildError::with_source(stage, format!("failed to write route graph"), error)
        })?;
    }

    out(
        &public_dir.join("route-manifest.json"),
        &bundle.manifest,
        true,
    )
    .map_err(|error| BuildError::with_source(stage, "failed to write route manifest", error))?;

    out(&public_dir.join("route-artifact.json"), &bundle, false)
        .map_err(|error| BuildError::with_source(stage, "failed to write route artifact", error))?;

    let host_components = collect_host_components(&bundle);
    let mut emitted_urls = BTreeMap::<String, Vec<u8>>::new();
    let mut dependencies = Vec::<AssetDependency>::new();
    for asset in &source_graph.assets {
        let target = public_dir.join(asset.url.trim_start_matches('/'));
        if let Some(previous_bytes) = emitted_urls.get(&asset.url) {
            if previous_bytes != &asset.bytes {
                return Err(BuildError::new(
                    stage,
                    format!(
                        "fingerprinted compiled asset URL collision at {}",
                        asset.url
                    ),
                ));
            }
        } else {
            if target.exists() {
                return Err(BuildError::new(
                    stage,
                    format!(
                        "compiled asset output {} collides with an application public file",
                        asset.url
                    ),
                ));
            }
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|error| {
                    BuildError::with_source(
                        stage,
                        format!("failed to create {}", parent.display()),
                        error,
                    )
                })?;
            }
            std::fs::write(&target, &asset.bytes).map_err(|error| {
                BuildError::with_source(
                    stage,
                    format!("failed to emit compiled asset {}", asset.url),
                    error,
                )
            })?;
            emitted_urls.insert(asset.url.clone(), asset.bytes.clone());
        }
        dependencies.push(AssetDependency {
            source: asset.source_path.to_string_lossy().replace('\\', "/"),
            url: asset.url.clone(),
        });
    }
    let dependency_manifest = public_dir
        .parent()
        .unwrap_or(public_dir)
        .join("plec-assets.json");
    out(&dependency_manifest, &dependencies, true).map_err(|error| {
        BuildError::with_source(stage, "failed to write asset dependency manifest", error)
    })?;
    stage_runtime(app_dir, repo_root, public_dir, runtime_source)?;
    Ok(ArtifactOutput {
        host_components,
        server_actions,
    })
}

fn discover_server_actions(
    modules: &[plec_parser::ParsedModule],
) -> Result<Vec<super::server::ServerActionImport>, BuildError> {
    use swc_ecma_ast::{Decl, Expr, ModuleDecl, ModuleItem, Pat, VarDeclKind};
    let mut actions = Vec::new();
    for module in modules {
        for item in &module.ast.body {
            let ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(export)) = item else {
                continue;
            };
            let Decl::Var(declaration) = &export.decl else {
                continue;
            };
            if declaration.kind != VarDeclKind::Const {
                continue;
            }
            for declarator in &declaration.decls {
                let (Pat::Ident(name), Some(initializer)) = (&declarator.name, &declarator.init)
                else {
                    continue;
                };
                let Expr::Call(call) = initializer.as_ref() else {
                    continue;
                };
                if !matches!(&call.callee, swc_ecma_ast::Callee::Expr(callee) if matches!(callee.as_ref(), Expr::Ident(ident) if ident.sym == "action"))
                {
                    continue;
                }
                let valid = call.args.len() == 1
                    && matches!(call.args[0].expr.as_ref(), Expr::Arrow(arrow) if arrow.is_async);
                if !valid {
                    return Err(BuildError::new(
                        Stage::Compile,
                        format!(
                            "server action {} in {} must be action(async (...) => ...)",
                            name.id.sym, module.id
                        ),
                    ));
                }
                let identity = format!(
                    "{}#{}#{}",
                    module.id.replace('\\', "/"),
                    name.id.sym,
                    module.source
                );
                use sha2::Digest;
                let digest = sha2::Sha256::digest(identity.as_bytes());
                let id = format!(
                    "sa_{}",
                    digest
                        .iter()
                        .take(16)
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<String>()
                );
                actions.push(super::server::ServerActionImport {
                    id,
                    import_path: format!("./{}", module.id.replace('\\', "/")),
                    export_name: name.id.sym.to_string(),
                });
            }
        }
    }
    actions.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(actions)
}

fn collect_host_components(
    bundle: &plec_compiler::RouteArtifactBundle,
) -> BTreeMap<String, BTreeSet<String>> {
    use plec_ir::{ComponentProp, Node};

    let mut components = BTreeMap::<String, BTreeSet<String>>::new();
    let mut insert = |provider: &str, component: &str| {
        components
            .entry(provider.to_owned())
            .or_default()
            .insert(component.to_owned());
    };

    for graph in &bundle.graphs {
        for application in &graph.graph.components {
            for node in &application.nodes {
                let props = match node {
                    Node::HostComponent {
                        provider,
                        component,
                        props,
                        ..
                    } => {
                        insert(provider, component);
                        props
                    }
                    Node::Component { props, .. } | Node::DynamicComponent { props, .. } => props,
                    _ => continue,
                };
                for prop in props {
                    if let ComponentProp::Component {
                        host: Some(target), ..
                    } = prop
                    {
                        insert(&target.provider, &target.component);
                    }
                }
            }
        }
    }

    components
}

/// Stage the prebuilt WASM runtime next to the compiler artifacts.
fn stage_runtime(
    app_dir: &Path,
    repo_root: &Path,
    public_dir: &Path,
    runtime_source: RuntimeSource,
) -> Result<(), BuildError> {
    stage(app_dir, repo_root, public_dir, runtime_source).map_err(|error| {
        BuildError::new(
            Stage::Compile,
            format!("failed to stage Plec runtime: {error}"),
        )
    })
}

#[cfg(test)]
mod server_action_tests {
    use super::*;

    #[test]
    fn server_action_discovery_requires_exported_async_arrow_and_has_stable_ids() {
        let module = |source: &str| plec_parser::parse_module("src/actions.ts", source).unwrap();
        let valid =
            module("export const echo = action(async (value) => ({ echoed: value }));");
        let first = discover_server_actions(std::slice::from_ref(&valid)).unwrap();
        let repeat = discover_server_actions(std::slice::from_ref(&valid)).unwrap();
        assert_eq!(first, repeat);
        assert_eq!(first[0].import_path, "./src/actions.ts");

        let changed = module(
            "export const echo = action(async (value) => ({ echoed: value, changed: true }));",
        );
        let changed = discover_server_actions(std::slice::from_ref(&changed)).unwrap();
        assert_ne!(first[0].id, changed[0].id);

        let unsupported = module("export const echo = action((value) => value);");
        assert!(discover_server_actions(std::slice::from_ref(&unsupported)).is_err());
    }
}
