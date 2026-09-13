use super::artifact;
use super::contract;
use super::repo::Repo;
use super::wasmtest;
use plec_build::modules::host::{resolve_custom_elements, resolve_host_imports};
use plec_compiler::{
    lower_route_artifacts_with_options, lower_routes, read_source_graph_with_options,
    CompilerOptions,
};
use plec_ir::{ComponentApplication, ExecutableComponent, Node, RouteManifest};
use plec_model::build_semantic_graph;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// Health check for the SSR adoption pipeline. Collapses the investigative
/// branches that repeatedly cost whole sessions:
///
/// 1. do all protocol boundaries agree? (versions)
/// 2. does every graph reference resolve through the registry semantics the
///    runtime actually uses? (direct key → registered app component)
/// 3. what execution state must a snapshot record per component?
/// 4. are the DOM markers in a rendered/fixture page well-formed?
/// 5. is the built/staged WASM actually implementing current source?

#[derive(Debug, Clone, Serialize)]
pub struct DoctorOptions {
    pub source: std::path::PathBuf,
    pub route: Option<String>,
    pub html: Option<std::path::PathBuf>,
    pub snapshot: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolutionRow {
    pub target: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct NestedComponentReport {
    pub component: String,
    pub conditionals: Vec<usize>,
    pub loops: Vec<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorReport {
    pub ok: bool,
    pub contract: contract::Report,
    pub resolutions: Vec<ResolutionRow>,
    pub nested: Vec<NestedComponentReport>,
    pub markers: Option<MarkerReport>,
    pub artifacts: artifact::ProvenanceReport,
    pub hints: Vec<String>,
}

/// The compiled application exactly as the build pipeline emits it: one
/// manifest plus a registry of graphs keyed by graph id — the same shape the
/// browser hands to `runtime.register_graph`.
pub(crate) struct CompiledApp {
    pub manifest: RouteManifest,
    /// graph_id → application, in registry order.
    pub registry: BTreeMap<String, ComponentApplication>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct MarkerReport {
    pub component_pairs: usize,
    pub conditional_pairs: usize,
    pub loop_rows: usize,
    pub slot_pairs: usize,
    pub node_addresses: usize,
    pub problems: Vec<String>,
}

pub fn run(repo: &Repo, options: &DoctorOptions) -> Result<DoctorReport, String> {
    let contract_report = contract::scan(repo)?;
    let artifacts = artifact::inspect(repo);

    let compiled = compile_application(repo, &options.source)?;

    let mut resolutions = resolve_graphs(&compiled, options.route.as_deref())?;

    if let Some(snapshot_path) = &options.snapshot {
        let snapshot_rows = resolve_snapshot(repo, snapshot_path, &compiled)?;
        resolutions.extend(snapshot_rows);
    }

    let nested = nested_execution(&compiled);

    let markers = match &options.html {
        Some(html_path) => Some(validate_markers(repo, html_path)?),
        None => None,
    };

    let mut hints = Vec::new();
    if !contract_report.conflicts().is_empty() {
        hints.push("protocol conflict — inspect with: plec workspace contract ssr".into());
    }
    if !artifacts.ok {
        hints.push(
            "stale/missing WASM — rebuild with: yarn workspace plec build:wasm, then \
              rebuild the app (turbo run build --filter=fullstack)"
                .into(),
        );
    }
    if options.html.is_none() {
        hints.push(
            "DOM markers not checked — pass --html <file> with rendered or fixture HTML".into(),
        );
    }
    if wasmtest::load_last_report(repo)
        .map(|report| report.failed > 0)
        .unwrap_or(false)
    {
        hints.push(
            "last WASM run had failures — inspect with: plec workspace test last --failures".into(),
        );
    }

    let mut ok = contract_report.conflicts().is_empty() && artifacts.ok;
    ok &= resolutions.iter().all(|row| row.status.starts_with("✓"));
    ok &= markers
        .as_ref()
        .map(|m| m.problems.is_empty())
        .unwrap_or(true);

    Ok(DoctorReport {
        ok,
        contract: contract_report,
        resolutions,
        nested,
        markers,
        artifacts,
        hints,
    })
}

pub(crate) fn compile_application(repo: &Repo, source: &Path) -> Result<CompiledApp, String> {
    let source = if source.is_absolute() {
        source.to_path_buf()
    } else {
        repo.root.join(source)
    };

    if !source.is_file() {
        return Err(format!(
            "application source not found: {}",
            source.display()
        ));
    }

    // The app dir anchors module resolution, the workspace root anchors
    // workspace-package imports — same contract as the build pipeline.
    let app_dir = source
        .parent()
        .and_then(|dir| dir.parent())
        .map(|dir| dir.to_path_buf())
        .unwrap_or_else(|| repo.root.clone());

    let host_imports =
        resolve_host_imports(&app_dir).map_err(|error| format!("host config: {error}"))?;
    let custom_elements =
        resolve_custom_elements(&app_dir).map_err(|error| format!("host config: {error}"))?;
    let source_graph = read_source_graph_with_options(
        &source,
        &app_dir,
        &repo.root,
        &CompilerOptions {
            host_imports,
            custom_elements: custom_elements.clone(),
        },
    )
    .map_err(|error| format!("source graph: {error}"))?;
    let semantic_graph =
        build_semantic_graph(&source_graph.modules, &source_graph.resolved_imports)
            .map_err(|error| format!("semantic graph: {error}"))?;
    let routes = lower_routes(&source_graph.modules, &semantic_graph)
        .map_err(|error| format!("routes: {error}"))?;
    let bundle = lower_route_artifacts_with_options(
        &source_graph.modules,
        &semantic_graph,
        &routes,
        &custom_elements,
    )
    .map_err(|error| format!("route artifacts: {error}"))?;

    let mut registry = BTreeMap::new();
    for artifact in &bundle.graphs {
        registry.insert(artifact.graph_id.clone(), artifact.graph.clone());
    }

    Ok(CompiledApp {
        manifest: bundle.manifest,
        registry,
    })
}

/// Mirror the runtime's registry resolution semantics
/// (`crates/plec-runtime/src/runtime/lifecycle.rs`):
///
/// 1. a graph id that is a direct registry key resolves immediately;
/// 2. otherwise every registered application's components are searched for a
///    component with that id (the fallback that rescues nested components);
/// 3. anything else fails closed.
fn resolve_graphs(
    compiled: &CompiledApp,
    route_filter: Option<&str>,
) -> Result<Vec<ResolutionRow>, String> {
    let mut targets: Vec<String> = vec![compiled.manifest.root_graph_id.clone()];
    for route in &compiled.manifest.routes {
        if let Some(filter) = route_filter {
            if route.path != filter {
                continue;
            }
        }
        for graph_id in [
            Some(&route.graph_id),
            route.pending_graph_id.as_ref(),
            route.error_graph_id.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            if !targets.contains(graph_id) {
                targets.push(graph_id.clone());
            }
        }
    }

    let mut rows = Vec::new();

    for target in &targets {
        let resolution = resolve_graph(compiled, target);
        rows.push(ResolutionRow {
            target: target.clone(),
            status: resolution_status(&resolution),
        });
    }

    // Component ids: nested components are not registry keys — they resolve
    // through the registered application that owns them, which is exactly
    // the fallback the runtime performs.
    for application in compiled.registry.values() {
        for component in &application.components {
            if compiled.registry.contains_key(&component.id) {
                continue;
            }
            let resolution = resolve_graph(compiled, &component.id);
            rows.push(ResolutionRow {
                target: component.id.clone(),
                status: resolution_status(&resolution),
            });
        }
    }

    Ok(rows)
}

/// Resolve every graph reference inside a captured `PlecSsrSnapshot` — the
/// exact shapes that made nested adoption fail closed in the field.
fn resolve_snapshot(
    repo: &Repo,
    snapshot_path: &Path,
    compiled: &CompiledApp,
) -> Result<Vec<ResolutionRow>, String> {
    let path = if snapshot_path.is_absolute() {
        snapshot_path.to_path_buf()
    } else {
        repo.root.join(snapshot_path)
    };
    let raw = fs::read_to_string(&path)
        .map_err(|error| format!("cannot read snapshot {}: {error}", path.display()))?;
    let snapshot: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|error| format!("snapshot {} is not JSON: {error}", path.display()))?;

    let mut rows = Vec::new();

    let version = snapshot.get("version").and_then(|value| value.as_u64());
    match version {
        Some(version) if version as u32 == plec_ir::SSR_SNAPSHOT_VERSION => {
            rows.push(ResolutionRow {
                target: format!("snapshot version {version}"),
                status: "✓".into(),
            })
        }
        Some(version) => rows.push(ResolutionRow {
            target: format!("snapshot version {version}"),
            status: format!(
                "✗ runtime accepts {} (unsupported:ssr-snapshot-version)",
                plec_ir::SSR_SNAPSHOT_VERSION
            ),
        }),
        None => rows.push(ResolutionRow {
            target: "snapshot version".into(),
            status: "✗ missing version field".into(),
        }),
    }

    if let Some(revision) = snapshot.get("revision").and_then(|value| value.as_str()) {
        let status = if revision == compiled.manifest.revision {
            "✓".into()
        } else {
            format!(
                "✗ manifest revision is {} (stale-revision)",
                compiled.manifest.revision
            )
        };
        rows.push(ResolutionRow {
            target: format!("revision {revision}"),
            status,
        });
    }

    let mut graph_refs: Vec<(String, String)> = Vec::new();
    if let Some(graphs) = snapshot
        .pointer("/structure/graphs")
        .and_then(|value| value.as_object())
    {
        for (instance, entry) in graphs {
            if let Some(graph_id) = entry.get("graphId").and_then(|value| value.as_str()) {
                graph_refs.push((instance.clone(), graph_id.to_string()));
            }
        }
    }
    if let Some(nested) = snapshot
        .pointer("/structure/nested")
        .and_then(|value| value.as_object())
    {
        for (path, entry) in nested {
            if let Some(graph_id) = entry.get("graphId").and_then(|value| value.as_str()) {
                graph_refs.push((path.clone(), graph_id.to_string()));
            }
        }
    }

    for (instance, graph_id) in graph_refs {
        let status = resolution_status(&resolve_graph(compiled, &graph_id));
        rows.push(ResolutionRow {
            target: format!("{instance} → {graph_id}"),
            status,
        });
    }

    Ok(rows)
}

/// Resolution path used by the runtime graph registry. A registered graph
/// selects its application's root component; component ids otherwise resolve
/// by searching registered applications in registry order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum GraphResolution {
    Direct {
        registry_key: String,
        component_index: usize,
        component_id: String,
    },
    RegisteredComponent {
        registry_key: String,
        component_index: usize,
        component_id: String,
    },
    Missing,
}

pub(crate) fn resolve_graph(compiled: &CompiledApp, graph_id: &str) -> GraphResolution {
    if let Some(application) = compiled.registry.get(graph_id) {
        if let Some(component) = application.components.get(application.root_component) {
            return GraphResolution::Direct {
                registry_key: graph_id.into(),
                component_index: application.root_component,
                component_id: component.id.clone(),
            };
        }
        return GraphResolution::Missing;
    }

    for (registry_key, application) in &compiled.registry {
        if let Some((component_index, component)) = application
            .components
            .iter()
            .enumerate()
            .find(|(_, component)| component.id == graph_id)
        {
            return GraphResolution::RegisteredComponent {
                registry_key: registry_key.clone(),
                component_index,
                component_id: component.id.clone(),
            };
        }
    }

    GraphResolution::Missing
}

pub(crate) fn resolved_component<'a>(
    compiled: &'a CompiledApp,
    resolution: &GraphResolution,
) -> Option<&'a ExecutableComponent> {
    let (registry_key, component_index) = match resolution {
        GraphResolution::Direct {
            registry_key,
            component_index,
            ..
        }
        | GraphResolution::RegisteredComponent {
            registry_key,
            component_index,
            ..
        } => (registry_key, component_index),
        GraphResolution::Missing => return None,
    };
    compiled
        .registry
        .get(registry_key)
        .and_then(|application| application.components.get(*component_index))
}

pub(crate) fn resolution_status(resolution: &GraphResolution) -> String {
    match resolution {
        GraphResolution::Direct { .. } => "✓ direct registry key".into(),
        GraphResolution::RegisteredComponent {
            registry_key,
            component_index,
            ..
        } => format!("✓ via registered app {registry_key} → component[{component_index}]"),
        GraphResolution::Missing => "✗ unknown ssr snapshot graph (would fail closed)".into(),
    }
}

/// Which execution state a snapshot v2 must record per component:
/// conditional branch selections and keyed loop rows.
fn nested_execution(compiled: &CompiledApp) -> Vec<NestedComponentReport> {
    let mut reports = Vec::new();

    for application in compiled.registry.values() {
        for component in &application.components {
            let mut conditionals = Vec::new();
            let mut loops = Vec::new();

            for (index, node) in component.nodes.iter().enumerate() {
                match node {
                    Node::Conditional { .. } => conditionals.push(index),
                    Node::Loop { .. } => loops.push(index),
                    _ => {}
                }
            }

            if !conditionals.is_empty() || !loops.is_empty() {
                if reports
                    .iter()
                    .any(|report: &NestedComponentReport| report.component == component.id)
                {
                    continue;
                }
                reports.push(NestedComponentReport {
                    component: component.id.clone(),
                    conditionals,
                    loops,
                });
            }
        }
    }

    reports
}

/// Structural validation of `plec:*` boundary markers and `data-plec-node`
/// addresses in an HTML fragment (docs/dom-address-protocol.md). Checks
/// pairing and address grammar; the expected-set derivation from a compiled
/// graph lives in `markers validate` (dev/markers.rs).
pub fn validate_markers(repo: &Repo, html_path: &Path) -> Result<MarkerReport, String> {
    let path = if html_path.is_absolute() {
        html_path.to_path_buf()
    } else {
        repo.root.join(html_path)
    };
    let html = fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;

    let mut report = MarkerReport::default();
    let mut open_boundaries: Vec<(String, String)> = Vec::new();

    for (marker, body) in marker_comments(&html) {
        let Some((kind, address)) = body.split_once(':') else {
            report.problems.push(format!(
                "line {}: malformed marker <!--plec:{body}-->",
                line_of(&html, marker)
            ));
            continue;
        };

        let is_end = kind.ends_with("-end");
        let base_kind = kind.trim_end_matches("-end");

        match base_kind {
            "conditional" | "component" | "slot" => {
                if is_end {
                    match open_boundaries
                        .iter()
                        .rposition(|(open_kind, open_address)| {
                            open_kind == base_kind && open_address == address
                        }) {
                        Some(position) => {
                            open_boundaries.remove(position);
                            match base_kind {
                                "conditional" => report.conditional_pairs += 1,
                                "component" => report.component_pairs += 1,
                                "slot" => report.slot_pairs += 1,
                                _ => {}
                            }
                        }
                        None => report.problems.push(format!(
                            "line {}: closing <!--plec:{kind}:{address}--> has no matching opener",
                            line_of(&html, marker)
                        )),
                    }
                } else {
                    open_boundaries.push((base_kind.to_string(), address.to_string()));
                }
            }
            "loop" => {
                if is_end {
                    match open_boundaries
                        .iter()
                        .rposition(|(open_kind, open_address)| {
                            open_kind == "loop" && open_address == address
                        }) {
                        Some(position) => {
                            open_boundaries.remove(position);
                            report.loop_rows += 1;
                        }
                        None => report.problems.push(format!(
                            "line {}: closing <!--plec:loop-end:{address}--> has no opener",
                            line_of(&html, marker)
                        )),
                    }
                } else {
                    open_boundaries.push(("loop".into(), address.to_string()));
                }
            }
            "text" => {
                // Text markers are standalone (marker-before-text adjacency).
            }
            other => report.problems.push(format!(
                "line {}: unknown marker kind {other:?}",
                line_of(&html, marker)
            )),
        }
    }

    for (kind, address) in &open_boundaries {
        report.problems.push(format!(
            "unclosed <!--plec:{kind}:{address}--> — the adopter would fail with \
             missing:ssr-{kind} at this boundary"
        ));
    }

    for occurrence in attribute_values(&html, "data-plec-node") {
        report.node_addresses += 1;
        if let Err(problem) = validate_address(&occurrence) {
            report
                .problems
                .push(format!("invalid address {occurrence:?}: {problem}"));
        }
    }

    Ok(report)
}

/// Extract `<!--plec:...-->` comment bodies with their byte offsets.
fn marker_comments(html: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    let mut rest = html;

    let mut offset = 0usize;
    while let Some(start) = rest.find("<!--plec:") {
        let Some(end_rel) = rest[start..].find("-->") else {
            break;
        };
        let body_start = start + "<!--plec:".len();
        let body = rest[body_start..start + end_rel].to_string();
        found.push((offset + start, body));

        let consumed = start + end_rel + 3;
        rest = &rest[consumed..];
        offset += consumed;
    }

    found
}

fn attribute_values(html: &str, name: &str) -> Vec<String> {
    let needle = format!("{name}=\"");
    let mut values = Vec::new();
    let mut rest = html;

    while let Some(start) = rest.find(&needle) {
        let value_start = start + needle.len();
        let Some(end_rel) = rest[value_start..].find('"') else {
            break;
        };
        values.push(rest[value_start..value_start + end_rel].to_string());
        rest = &rest[value_start + end_rel + 1..];
    }

    values
}

/// Address grammar: `root[/outlet:{id}][/component:{i}][/loop:{i}/key:{k}]/node:{i}`.
/// Outlet ids and row keys are strings (with `%2F`-style escaping); component,
/// loop, and node handles are numeric.
fn validate_address(address: &str) -> Result<(), String> {
    let segments: Vec<&str> = address.split('/').collect();
    let Some((first, rest)) = segments.split_first() else {
        return Err("empty address".into());
    };
    if *first != "root" {
        return Err("must start with 'root'".into());
    }

    for segment in rest {
        let Some((kind, value)) = segment.split_once(':') else {
            return Err(format!("segment {segment:?} is not kind:value"));
        };

        match kind {
            "outlet" | "key" => {
                if value.is_empty() {
                    return Err(format!("segment {segment:?} has an empty value"));
                }
            }
            "component" | "loop" | "node" => {
                if value.parse::<usize>().is_err() {
                    return Err(format!("segment {segment:?} needs a numeric handle"));
                }
            }
            other => return Err(format!("unexpected segment kind {other:?}")),
        }
    }

    Ok(())
}

fn line_of(html: &str, offset: usize) -> usize {
    html[..offset.min(html.len())].matches('\n').count() + 1
}

pub fn print_report(report: &DoctorReport) {
    println!("SSR Adoption Doctor\n");

    let show = |value: Option<u32>| {
        value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "?".into())
    };

    println!("Protocol");
    println!(
        "  snapshot version       {}",
        show(report.contract.snapshot.canonical)
    );
    println!(
        "  manifest version       {}",
        show(report.contract.manifest.canonical)
    );
    println!(
        "  bootstrap version      {}",
        show(report.contract.bootstrap.canonical)
    );

    let conflicts = report.contract.conflicts();
    if conflicts.is_empty() {
        println!("  all boundaries agree   ✓");
    } else {
        for conflict in &conflicts {
            println!(
                "  {}:{}  ✗ {}",
                conflict.file, conflict.line, conflict.status
            );
        }
    }

    println!("\nGraph resolution");
    for row in &report.resolutions {
        println!("  {:50} {}", truncate(&row.target, 50), row.status);
    }

    if !report.nested.is_empty() {
        println!("\nNested execution (v2 snapshot records required)");
        for component in &report.nested {
            let parts: Vec<String> = component
                .conditionals
                .iter()
                .map(|node| format!("conditional node {node}"))
                .chain(
                    component
                        .loops
                        .iter()
                        .map(|node| format!("loop node {node}")),
                )
                .collect();
            println!(
                "  {:50} {}",
                truncate(&component.component, 50),
                parts.join(", ")
            );
        }
    }

    println!("\nDOM markers");
    match &report.markers {
        Some(markers) => {
            println!("  component pairs        {}", markers.component_pairs);
            println!("  conditional pairs      {}", markers.conditional_pairs);
            println!("  loop rows              {}", markers.loop_rows);
            println!("  node addresses         {}", markers.node_addresses);
            for problem in &markers.problems {
                println!("  ✗ {problem}");
            }
        }
        None => println!("  not checked — pass --html <file>"),
    }

    println!("\nArtifact provenance");
    println!(
        "  source protocol        {}",
        report.artifacts.source_protocol
    );
    for layer in [&report.artifacts.dist, &report.artifacts.staged] {
        let Some(layer) = layer else { continue };
        let status = if !layer.present {
            "✗ MISSING".to_string()
        } else if layer.problems.is_empty() {
            "✓".to_string()
        } else {
            layer
                .problems
                .iter()
                .map(|problem| format!("✗ {problem}"))
                .collect::<Vec<_>>()
                .join("; ")
        };
        println!("  {:22} {}", truncate(&layer.name, 22), status);
    }

    if !report.hints.is_empty() {
        println!("\nHints");
        for hint in &report.hints {
            println!("  - {hint}");
        }
    }

    println!();
    if report.ok {
        println!("✓ adoption pipeline healthy");
    } else {
        println!("✗ adoption pipeline has problems");
    }
}

fn truncate(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_string();
    }
    format!("{}…", &value[..max.saturating_sub(1)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_address_grammar() {
        assert!(validate_address("root").is_ok());
        assert!(validate_address("root/node:0").is_ok());
        assert!(validate_address("root/outlet:main/node:0").is_ok());
        assert!(validate_address("root/outlet:main/component:1/node:3").is_ok());
        assert!(validate_address("root/outlet:main/component:1/loop:6/key:one/node:7").is_ok());
        assert!(validate_address("root/outlet:main/loop:2/key:one").is_ok());
        assert!(validate_address("page/node:1").is_err());
        assert!(validate_address("root/what:1").is_err());
        assert!(validate_address("root/node:abc").is_err());
    }

    #[test]
    fn pairs_component_boundaries_and_flags_missing_ends() {
        let html = "<!--plec:component:root/outlet:main:1-->\
                    <div data-plec-node=\"root/outlet:main/component:1/node:0\"></div>\
                    <!--plec:component:root/outlet:main:1-->";

        let comments = marker_comments(html);
        assert_eq!(comments.len(), 2);
        assert_eq!(comments[0].1, "component:root/outlet:main:1");

        let addresses = attribute_values(html, "data-plec-node");
        assert_eq!(addresses, vec!["root/outlet:main/component:1/node:0"]);
        assert!(validate_address(&addresses[0]).is_ok());
    }

    #[test]
    fn marker_validation_reports_unclosed_boundaries() {
        let dir = std::env::temp_dir().join("plec-dev-doctor-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("fixture.html");
        std::fs::write(&path, "<!--plec:component:root/outlet:main:1--><div></div>").unwrap();

        let report = validate_markers(&Repo { root: "/".into() }, &path).unwrap();
        assert!(report.problems.len() == 1, "{:?}", report.problems);
        assert!(report.problems[0].contains("unclosed"));
        assert!(report.problems[0].contains("component:root/outlet:main:1"));
    }
}
