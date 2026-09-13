use super::doctor::{
    compile_application, resolve_graph, resolved_component, CompiledApp, GraphResolution,
};
use super::repo::Repo;
use plec_ir::{
    limits::MAX_SSR_RENDER_DEPTH, ComponentApplication, ExecutableComponent, Node, PlecSsrSnapshot,
    RouteManifest, RouteManifestEntry, RouteOutlet, SsrGraphStructure, SsrSelectedBranch,
    ROOT_GRAPH_INSTANCE_ID,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressReport {
    pub address: String,
    pub segments: Vec<AddressSegment>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressSegment {
    pub kind: String,
    pub value: String,
    pub depth: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationReport {
    pub ok: bool,
    pub graph: String,
    /// True when a snapshot supplied structural records, making the expected
    /// marker sequence fully concrete (strict mode).
    pub strict: bool,
    pub expected: Vec<String>,
    pub actual: Vec<String>,
    pub problems: Vec<String>,
}

/// Options for [`validate`].
#[derive(Debug, Clone)]
pub struct ValidateOptions<'a> {
    pub graph: &'a str,
    pub html: &'a Path,
    /// Captured `PlecSsrSnapshot` whose structural records select branches
    /// and keyed loop rows for an exact (strict) expected sequence.
    pub snapshot: Option<&'a Path>,
    /// Matched route path (e.g. `/about`) selecting which manifest child
    /// renders into each route outlet during the walk.
    pub route: Option<&'a str>,
}

pub fn explain(address: &str) -> Result<AddressReport, String> {
    let segments = parse_address(address)?;
    Ok(AddressReport {
        address: address.into(),
        segments,
    })
}

pub fn print_explain(report: &AddressReport) {
    println!("{}", report.address);
    for segment in &report.segments {
        println!(
            "{}{}: {}",
            "  ".repeat(segment.depth),
            segment.kind,
            segment.value
        );
    }
}

pub fn validate(
    repo: &Repo,
    source: &Path,
    options: &ValidateOptions,
) -> Result<ValidationReport, String> {
    let compiled = compile_application(repo, source)?;
    let resolution = resolve_graph(&compiled, options.graph);
    let Some(component) = resolved_component(&compiled, &resolution) else {
        return Err(format!("graph {:?} is not registered", options.graph));
    };
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
        GraphResolution::Missing => unreachable!(),
    };
    let application = compiled.registry.get(&registry_key).unwrap();

    let snapshot = match options.snapshot {
        Some(path) => Some(read_snapshot(repo, path)?),
        None => None,
    };

    // The root graph renders as the `root/outlet:main` instance; any other
    // graph is located in the snapshot by scanning instance records for its
    // graph id (instance ids otherwise need the full matched route chain).
    let instance = if registry_key == compiled.manifest.root_graph_id {
        Some(ROOT_GRAPH_INSTANCE_ID.to_string())
    } else {
        snapshot
            .as_ref()
            .and_then(|s| scan_instance(s, &registry_key))
    };
    let entry = manifest_entry_for_graph(&compiled.manifest, &registry_key);

    let mut walker = Walker {
        compiled: &compiled,
        snapshot: snapshot.as_ref(),
        route_filter: options.route,
        sink_optional: false,
        expected: Vec::new(),
        optional: Vec::new(),
    };
    walker.walk_root(
        application,
        component_index,
        entry,
        WalkScope {
            path: "root".into(),
            instance,
            nested_path: None,
            in_row: false,
        },
        None,
        0,
    );
    let _ = component;

    let path = if options.html.is_absolute() {
        options.html.to_path_buf()
    } else {
        repo.root.join(options.html)
    };
    let body =
        fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let actual = extract_markers(&body);

    let mut problems = check_grammar(&actual);
    problems.extend(check_missing(&walker.expected, &actual));
    if let Some(problem) = check_order(&walker.expected, &actual) {
        problems.push(problem);
    }
    problems.extend(check_unexpected(
        &walker.expected,
        &walker.optional,
        &actual,
    ));
    problems.extend(check_adjacency(&body));

    Ok(ValidationReport {
        ok: problems.is_empty(),
        graph: options.graph.into(),
        strict: snapshot.is_some(),
        expected: walker.expected,
        actual,
        problems,
    })
}

pub fn print_validation(report: &ValidationReport) {
    println!("graph {}", report.graph);
    if report.strict {
        println!("  mode               strict (snapshot-selected branches and keys)");
    } else {
        println!("  mode               structural (unselected branches tolerated)");
    }
    println!("  expected markers  {}", report.expected.len());
    println!("  actual markers    {}", report.actual.len());
    for problem in &report.problems {
        println!("  ✗ {problem}");
    }
    if report.ok {
        println!("✓ markers valid");
    } else {
        println!("✗ markers invalid");
    }
}

fn read_snapshot(repo: &Repo, path: &Path) -> Result<PlecSsrSnapshot, String> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo.root.join(path)
    };
    let raw = fs::read_to_string(&path)
        .map_err(|error| format!("cannot read snapshot {}: {error}", path.display()))?;
    serde_json::from_str(&raw).map_err(|error| {
        format!(
            "snapshot {} is not a PlecSsrSnapshot: {error}",
            path.display()
        )
    })
}

/// The first snapshot instance record mounting this graph id. Instance ids
/// are route-chain-qualified; when a graph mounts more than once the first
/// record wins (deterministic; the common single-instance case is exact).
fn scan_instance(snapshot: &PlecSsrSnapshot, graph_id: &str) -> Option<String> {
    snapshot
        .structure
        .graphs
        .iter()
        .find(|(_, entry)| entry.graph_id == graph_id)
        .map(|(instance, _)| instance.clone())
}

fn manifest_entry_for_graph<'m>(
    manifest: &'m RouteManifest,
    graph_id: &str,
) -> Option<&'m RouteManifestEntry> {
    if graph_id == manifest.root_graph_id {
        return None;
    }
    manifest
        .routes
        .iter()
        .find(|entry| entry.graph_id == graph_id)
}

// ---------------------------------------------------------------------------
// Graph walk — the structural mirror of the SSR renderer
// (crates/plec-server/src/ssr/render.rs). Marker emission order is the
// expected HTML order.
// ---------------------------------------------------------------------------

/// The caller context one component walk executes in.
#[derive(Debug, Clone)]
struct WalkScope {
    /// Marker path (`root`, `root/outlet:main/component:1`, …).
    path: String,
    /// Snapshot `structure.graphs` key owning this instance's records.
    instance: Option<String>,
    /// Snapshot `structure.nested` key (the component's marker path) owning
    /// a nested instance's records.
    nested_path: Option<String>,
    /// Inside a keyed loop row: the renderer records graph-level branch
    /// selections only outside rows, so row interiors never read the
    /// instance's own records.
    in_row: bool,
}

/// The implicit `children` content a component's `Slot` renders: caller-owned
/// nodes rendered with the caller's scope, like the renderer's `SlotFrame`.
#[derive(Debug, Clone)]
struct SlotFrame {
    graph_id: String,
    component_index: usize,
    route_id: Option<String>,
    nodes: Vec<usize>,
    scope: WalkScope,
    slot: Option<Box<SlotFrame>>,
}

/// Structural branch/loop records for the scope being walked.
#[derive(Debug, Clone, Default)]
struct StructureRecords {
    branches: BTreeMap<usize, SsrSelectedBranch>,
    loops: BTreeMap<usize, Vec<String>>,
}

impl StructureRecords {
    fn of(snapshot: Option<&PlecSsrSnapshot>, scope: &WalkScope) -> Self {
        let Some(snapshot) = snapshot else {
            return Self::default();
        };
        // Nested instances always read the nested map (keyed by marker
        // path); graph instances read the instance map, except inside loop
        // rows where the renderer records nothing graph-level.
        let entry: Option<&SsrGraphStructure> = if let Some(nested) = &scope.nested_path {
            snapshot.structure.nested.get(nested)
        } else if let (Some(instance), false) = (&scope.instance, scope.in_row) {
            snapshot.structure.graphs.get(instance)
        } else {
            None
        };
        let Some(entry) = entry else {
            return Self::default();
        };
        Self {
            branches: entry
                .branches
                .iter()
                .map(|selection| (selection.node, selection.selected))
                .collect(),
            loops: entry
                .loops
                .iter()
                .map(|rows| (rows.node, rows.keys.clone()))
                .collect(),
        }
    }
}

/// One `RouteOutlet` plus the manifest child selected to render into it.
struct OutletChild<'a> {
    outlet: &'a RouteOutlet,
    entry: &'a RouteManifestEntry,
}

struct Walker<'a> {
    compiled: &'a CompiledApp,
    snapshot: Option<&'a PlecSsrSnapshot>,
    route_filter: Option<&'a str>,
    /// When set, [`Walker::push`] writes to `optional` instead of `expected`:
    /// used for conditional interiors whose selected branch is unknown, so
    /// either branch's markers are tolerated but never required.
    sink_optional: bool,
    /// Markers the compiled graph requires of the HTML.
    expected: Vec<String>,
    /// Markers that are legal in the HTML but not required.
    optional: Vec<String>,
}

impl<'a> Walker<'a> {
    fn push(&mut self, marker: String) {
        if self.sink_optional {
            self.optional.push(marker);
        } else {
            self.expected.push(marker);
        }
    }

    fn push_all_optional<I: IntoIterator<Item = String>>(&mut self, markers: I) {
        self.optional.extend(markers);
    }

    fn walk_root(
        &mut self,
        app: &'a ComponentApplication,
        component_index: usize,
        route: Option<&'a RouteManifestEntry>,
        scope: WalkScope,
        slot: Option<&SlotFrame>,
        depth: usize,
    ) {
        let Some(component) = app.components.get(component_index) else {
            return;
        };
        let mut guard = BTreeSet::new();
        self.walk_node(
            app,
            component_index,
            component,
            route,
            component.root_node,
            scope,
            slot,
            &mut guard,
            depth,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn walk_node(
        &mut self,
        app: &'a ComponentApplication,
        component_index: usize,
        component: &'a ExecutableComponent,
        route: Option<&'a RouteManifestEntry>,
        index: usize,
        scope: WalkScope,
        slot: Option<&SlotFrame>,
        guard: &mut BTreeSet<usize>,
        depth: usize,
    ) {
        if depth >= MAX_SSR_RENDER_DEPTH || !guard.insert(index) {
            return;
        }
        let Some(node) = component.nodes.get(index) else {
            return;
        };
        match node {
            Node::Element { children, .. } => {
                self.push(format!("node:{}/node:{index}", scope.path));
                for child in children {
                    self.walk_node(
                        app,
                        component_index,
                        component,
                        route,
                        *child,
                        scope.clone(),
                        slot,
                        guard,
                        depth + 1,
                    );
                }
                if let Some(child) = self.outlet_child(component, route, index) {
                    self.walk_outlet(&scope, child, depth);
                }
            }
            Node::Text { .. } => self.push(format!("text:{}:{index}", scope.path)),

            Node::Conditional {
                consequent,
                alternate,
                ..
            } => {
                self.push(format!("conditional:{}:{index}", scope.path));
                let records = StructureRecords::of(self.snapshot, &scope);
                match records.branches.get(&index).copied() {
                    Some(SsrSelectedBranch::Consequent) => {
                        self.walk_node(
                            app,
                            component_index,
                            component,
                            route,
                            *consequent,
                            scope.clone(),
                            slot,
                            guard,
                            depth + 1,
                        );
                    }
                    Some(SsrSelectedBranch::Alternate) => {
                        if let Some(alternate) = alternate {
                            self.walk_node(
                                app,
                                component_index,
                                component,
                                route,
                                *alternate,
                                scope.clone(),
                                slot,
                                guard,
                                depth + 1,
                            );
                        }
                    }
                    Some(SsrSelectedBranch::None) => {}
                    None => {
                        // No recorded selection: either branch's interior is
                        // legal in the HTML, so both become optional.
                        for branch in [*consequent, alternate.unwrap_or(*consequent)] {
                            let mut branch_guard = BTreeSet::new();
                            let sink = std::mem::replace(&mut self.sink_optional, true);
                            self.walk_node(
                                app,
                                component_index,
                                component,
                                route,
                                branch,
                                scope.clone(),
                                slot,
                                &mut branch_guard,
                                depth + 1,
                            );
                            self.sink_optional = sink;
                        }
                    }
                }
                self.push(format!("conditional-end:{}:{index}", scope.path));
            }

            Node::Component {
                component: target,
                children,
                ..
            } => {
                let child_path = format!("{}/component:{index}", scope.path);
                self.push(format!("component:{}:{index}", scope.path));
                let frame = SlotFrame {
                    graph_id: self.graph_id_of(app),
                    component_index,
                    route_id: route.map(|entry| entry.id.clone()),
                    nodes: children.clone(),
                    scope: scope.clone(),
                    slot: slot.cloned().map(Box::new),
                };
                let child_scope = WalkScope {
                    path: child_path,
                    instance: None,
                    nested_path: Some(format!("{}/component:{index}", scope.path)),
                    in_row: false,
                };
                self.walk_root(app, *target, None, child_scope, Some(&frame), depth + 1);
                self.push(format!("component-end:{}:{index}", scope.path));
            }

            Node::Slot { .. } => {
                self.push(format!("slot:{}:{index}", scope.path));
                if let Some(frame) = slot {
                    self.render_slot_frame(frame, depth);
                }
                self.push(format!("slot-end:{}:{index}", scope.path));
            }

            Node::Loop { r#loop, .. } => {
                let records = StructureRecords::of(self.snapshot, &scope);
                match records.loops.get(&index) {
                    Some(keys) => {
                        for key in keys {
                            self.walk_loop_row(
                                app,
                                component_index,
                                component,
                                route,
                                index,
                                *r#loop,
                                &scope,
                                slot,
                                depth,
                                &escape_instance_segment(key),
                            );
                        }
                    }
                    None => self.walk_loop_row(
                        app,
                        component_index,
                        component,
                        route,
                        index,
                        *r#loop,
                        &scope,
                        slot,
                        depth,
                        "*",
                    ),
                }
            }

            // A dynamic component's target is a runtime value: it renders
            // either as a component boundary pair (component-valued prop) or
            // as a host-owned boundary span. Expect neither shape strictly;
            // tolerate both.
            Node::DynamicComponent { .. } => self.push_all_optional([
                format!("component:{}:{index}", scope.path),
                format!("component-end:{}:{index}", scope.path),
                format!("node:{}/node:{index}", scope.path),
            ]),

            // Host providers own their subtree; the runtime owns only the
            // boundary span, which carries a node address.
            Node::HostComponent { .. } => {
                self.push(format!("node:{}/node:{index}", scope.path));
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn walk_loop_row(
        &mut self,
        app: &'a ComponentApplication,
        component_index: usize,
        component: &'a ExecutableComponent,
        route: Option<&'a RouteManifestEntry>,
        node_index: usize,
        loop_handle: usize,
        scope: &WalkScope,
        slot: Option<&SlotFrame>,
        depth: usize,
        escaped_key: &str,
    ) {
        let Some(program) = component.loops.get(loop_handle) else {
            return;
        };
        let row_path = format!("{}/loop:{node_index}/key:{escaped_key}", scope.path);
        self.push(format!("loop:{row_path}"));
        let row_scope = WalkScope {
            path: row_path.clone(),
            instance: scope.instance.clone(),
            nested_path: scope.nested_path.clone(),
            in_row: true,
        };
        let mut guard = BTreeSet::new();
        self.walk_node(
            app,
            component_index,
            component,
            route,
            program.row_template,
            row_scope,
            slot,
            &mut guard,
            depth + 1,
        );
        self.push(format!("loop-end:{row_path}"));
    }

    /// Caller-owned slot content, rendered at the caller's path between the
    /// child's slot markers — the renderer's frame semantics.
    fn render_slot_frame(&mut self, frame: &SlotFrame, depth: usize) {
        let Some(app) = self.compiled.registry.get(&frame.graph_id) else {
            return;
        };
        let Some(component) = app.components.get(frame.component_index) else {
            return;
        };
        let route = frame.route_id.as_deref().and_then(|id| {
            self.compiled
                .manifest
                .routes
                .iter()
                .find(|entry| entry.id == id)
        });
        let mut guard = BTreeSet::new();
        for node in &frame.nodes {
            self.walk_node(
                app,
                frame.component_index,
                component,
                route,
                *node,
                frame.scope.clone(),
                frame.slot.as_deref(),
                &mut guard,
                depth + 1,
            );
        }
    }

    fn graph_id_of(&self, app: &ComponentApplication) -> String {
        self.compiled
            .registry
            .iter()
            .find(|(_, application)| std::ptr::eq(*application, app))
            .map(|(key, _)| key.clone())
            .unwrap_or_default()
    }

    /// The manifest child rendering into this element's route outlet, if any.
    fn outlet_child(
        &self,
        component: &'a ExecutableComponent,
        route: Option<&'a RouteManifestEntry>,
        node: usize,
    ) -> Option<OutletChild<'a>> {
        let outlet = component
            .route_outlets
            .iter()
            .find(|outlet| outlet.node == node)?;
        let entry = select_outlet_route(
            &self.compiled.manifest,
            route,
            &outlet.id,
            self.route_filter,
        )?;
        Some(OutletChild { outlet, entry })
    }

    fn walk_outlet(&mut self, parent: &WalkScope, child: OutletChild<'a>, depth: usize) {
        let OutletChild { outlet, entry } = child;
        let Some(app) = self.compiled.registry.get(&entry.graph_id) else {
            return;
        };
        let instance = match &parent.instance {
            Some(parent) => Some(format!("{parent}/outlet:{}", outlet.id)),
            None => self
                .snapshot
                .and_then(|snapshot| scan_instance(snapshot, &entry.graph_id)),
        };
        self.walk_root(
            app,
            app.root_component,
            Some(entry),
            WalkScope {
                path: format!("{}/outlet:{}", parent.path, outlet.id),
                instance,
                nested_path: None,
                in_row: false,
            },
            None,
            depth + 1,
        );
    }
}

/// Children of `parent` (top-level routes when `parent` is `None`, i.e. the
/// root graph) that render into `outlet_id`, reduced to the entry the walk
/// follows: an exact `--route` path match, else the deepest candidate whose
/// composed path prefixes the filter, else the first non-wildcard path in
/// manifest order.
fn select_outlet_route<'b>(
    manifest: &'b RouteManifest,
    parent: Option<&'b RouteManifestEntry>,
    outlet_id: &str,
    route_filter: Option<&str>,
) -> Option<&'b RouteManifestEntry> {
    let candidates: Vec<&RouteManifestEntry> = manifest
        .routes
        .iter()
        .filter(|entry| entry.outlet_id == outlet_id)
        .filter(|entry| match (parent, &entry.parent_id) {
            (None, None) => true,
            (Some(parent), Some(parent_id)) => parent.id == *parent_id,
            _ => false,
        })
        .collect();
    if candidates.is_empty() {
        return None;
    }
    if let Some(filter) = route_filter {
        if let Some(exact) = candidates
            .iter()
            .find(|entry| full_path(manifest, entry) == filter)
        {
            return Some(exact);
        }
        let prefixed: Vec<_> = candidates
            .iter()
            .filter(|entry| {
                let full = full_path(manifest, entry);
                filter == &full || filter.starts_with(&format!("{full}/"))
            })
            .collect();
        if let Some(deepest) = prefixed
            .into_iter()
            .max_by_key(|entry| full_path(manifest, entry).len())
        {
            return Some(deepest);
        }
    }
    candidates
        .iter()
        .find(|entry| entry.path != "*")
        .copied()
        .or_else(|| candidates.first().copied())
}

/// Composed path of an entry (`/about`, `/projects/$id`, …).
fn full_path(manifest: &RouteManifest, entry: &RouteManifestEntry) -> String {
    let mut segments = vec![entry.path.clone()];
    let mut parent_id = entry.parent_id.clone();
    while let Some(id) = parent_id {
        let Some(parent) = manifest.routes.iter().find(|candidate| candidate.id == id) else {
            break;
        };
        segments.push(parent.path.clone());
        parent_id = parent.parent_id.clone();
    }
    let mut path = String::new();
    for segment in segments.iter().rev() {
        if !segment.is_empty() {
            path.push('/');
            path.push_str(segment);
        }
    }
    if path.is_empty() {
        path.push('/');
    }
    path
}

/// Instance-id segment escaping, mirroring the renderer's
/// `escape_instance_segment` (`%` then `/`).
fn escape_instance_segment(value: &str) -> String {
    value.replace('%', "%25").replace('/', "%2F")
}

// ---------------------------------------------------------------------------
// HTML extraction and checks
// ---------------------------------------------------------------------------

fn problem_code(marker: &str) -> &'static str {
    if marker.starts_with("component") {
        "missing:ssr-component"
    } else if marker.starts_with("slot") {
        "missing:ssr-slot"
    } else if marker.starts_with("loop") {
        "missing:ssr-loop"
    } else if marker.starts_with("text:") {
        "missing:ssr-text"
    } else if marker.starts_with("conditional") {
        "mismatch:ssr-branch"
    } else {
        "missing:ssr-node"
    }
}

/// Grammar and duplicate checks over the extracted marker sequence.
fn check_grammar(actual: &[String]) -> Vec<String> {
    let mut problems = Vec::new();
    let mut seen = BTreeSet::new();
    for marker in actual {
        if !seen.insert(marker) {
            problems.push(format!("duplicate:ssr-marker:{marker}"));
        }
        let Some((kind, address)) = marker.split_once(':') else {
            problems.push(format!("invalid:ssr-marker:{marker}"));
            continue;
        };
        let valid = if kind == "text" {
            parse_text_address(address).is_ok()
        } else if matches!(
            kind,
            "component" | "component-end" | "conditional" | "conditional-end" | "slot" | "slot-end"
        ) {
            parse_boundary_address(address).is_ok()
        } else if matches!(kind, "loop" | "loop-end" | "node") {
            parse_address(address).is_ok()
        } else {
            false
        };
        if !valid {
            problems.push(format!("invalid:ssr-marker:{marker}"));
        }
    }
    problems
}

/// Every expected marker must occur in the HTML (loop rows may be wildcard).
fn check_missing(expected: &[String], actual: &[String]) -> Vec<String> {
    let mut problems = Vec::new();
    for marker in expected {
        if !actual
            .iter()
            .any(|candidate| marker_matches(marker, candidate))
        {
            let hint = if let Some(address) = marker.strip_prefix("node:") {
                format!("expected data-plec-node=\"{address}\"")
            } else {
                format!("expected <!--plec:{marker}-->")
            };
            problems.push(format!("{}: {marker}; {hint}", problem_code(marker)));
        }
    }
    problems
}

/// Order check: the expected sequence must be a subsequence of the actual
/// marker order, with wildcard loop-row blocks repeatable (each additional
/// keyed row re-consumes its block). A skipped actual marker that matches an
/// expected marker still ahead is an order inversion, not a skip.
fn check_order(expected: &[String], actual: &[String]) -> Option<String> {
    let mut position = 0usize;
    // Repeatable wildcard row blocks, as (start, end) index pairs. Blocks
    // stay registered once opened: any later row of the same loop restarts
    // its block, and nested loops open their own.
    let mut open_blocks: Vec<(usize, usize)> = Vec::new();

    for (scan, marker) in actual.iter().enumerate() {
        if !(position < expected.len() && marker_matches(&expected[position], marker)) {
            // A repeated keyed row restarts its most recently opened block.
            if let Some(&(start, _)) = open_blocks
                .iter()
                .rev()
                .find(|&&(start, _)| marker_matches(&expected[start], marker))
            {
                position = start;
            }
        }
        if position < expected.len() && marker_matches(&expected[position], marker) {
            let matched = &expected[position];
            if matched.starts_with("loop:")
                && matched.contains("key:*")
                && !open_blocks.iter().any(|&(start, _)| start == position)
            {
                if let Some(end) = loop_end_of(expected, position) {
                    open_blocks.push((position, end));
                }
            }
            position += 1;
        } else if position < expected.len()
            && actual[scan..]
                .iter()
                .any(|candidate| marker_matches(&expected[position], candidate))
        {
            // Only a true inversion is an order problem: the skipped marker
            // matches an expectation still ahead, and the expectation at the
            // cursor really does occur later in the document. Otherwise the
            // skip is optional content and the missing check reports the gap.
            if let Some(ahead) = expected[position..]
                .iter()
                .skip(1)
                .position(|candidate| marker_matches(candidate, marker))
                .map(|offset| position + 1 + offset)
            {
                return Some(format!(
                    "order:ssr-marker:{marker} renders before expected {}",
                    expected[ahead]
                ));
            }
        }
    }
    None
}

/// Index of the `loop-end` closing the wildcard row block opened at `start`.
fn loop_end_of(expected: &[String], start: usize) -> Option<usize> {
    let row_path = expected[start].strip_prefix("loop:")?;
    let closing = format!("loop-end:{row_path}");
    expected
        .iter()
        .enumerate()
        .skip(start + 1)
        .find(|(_, marker)| **marker == closing)
        .map(|(index, _)| index)
}

/// Every actual marker must be expected (or a wildcard match) or fall inside
/// an unselected conditional interior (the optional set).
fn check_unexpected(expected: &[String], optional: &[String], actual: &[String]) -> Vec<String> {
    let mut problems = Vec::new();
    for marker in actual {
        let required = expected
            .iter()
            .chain(optional)
            .any(|candidate| marker_matches(candidate, marker));
        if !required {
            problems.push(format!("unexpected:ssr-marker:{marker}"));
        }
    }
    problems
}

/// Text markers must be adjacent to their text (or the `<!---->` sentinel).
fn check_adjacency(body: &str) -> Vec<String> {
    let mut problems = Vec::new();
    for (offset, marker) in marker_comments_with_offsets(body) {
        if marker.starts_with("text:") {
            let after = &body[offset + marker.len() + 12..];
            let next = after.trim_start();
            if !next.starts_with("<!---->") && next.starts_with('<') {
                problems.push(format!("adjacency:ssr-text:{marker}"));
            }
        }
    }
    problems
}

fn marker_matches(expected: &str, actual: &str) -> bool {
    if !expected.contains("key:*") {
        return expected == actual;
    }
    let expected_segments: Vec<&str> = expected.split('/').collect();
    let actual_segments: Vec<&str> = actual.split('/').collect();
    if expected_segments.len() != actual_segments.len() {
        return false;
    }
    expected_segments
        .iter()
        .zip(actual_segments)
        .all(|(pattern, value)| segment_matches(pattern, value))
}

/// One address segment. `key:*` is a wildcard for the escaped row key; it
/// may sit mid-segment because text/boundary markers suffix a node handle
/// after the path (`text:…/key:*:2`).
fn segment_matches(pattern: &str, value: &str) -> bool {
    let Some(split) = pattern.find("key:*") else {
        return pattern == value;
    };
    let (prefix, suffix) = (&pattern[..split], &pattern[split + "key:*".len()..]);
    value.len() >= prefix.len() + suffix.len()
        && value.starts_with(prefix)
        && value.ends_with(suffix)
}

/// All markers in document order: `<!--plec:...-->` comment bodies and
/// `data-plec-node` attribute values interleaved by byte offset.
fn extract_markers(html: &str) -> Vec<String> {
    let mut occurrences: Vec<(usize, String)> = Vec::new();

    let mut cursor = 0usize;
    while let Some(relative) = html[cursor..].find("<!--plec:") {
        let start = cursor + relative;
        let body_start = start + "<!--plec:".len();
        let Some(end) = html[body_start..].find("-->") else {
            break;
        };
        occurrences.push((start, html[body_start..body_start + end].to_owned()));
        cursor = body_start + end + 3;
    }

    let needle = "data-plec-node=\"";
    let mut cursor = 0usize;
    while let Some(relative) = html[cursor..].find(needle) {
        let start = cursor + relative;
        let value_start = start + needle.len();
        let Some(end) = html[value_start..].find('"') else {
            break;
        };
        occurrences.push((
            start,
            format!("node:{}", &html[value_start..value_start + end]),
        ));
        cursor = value_start + end + 1;
    }

    occurrences.sort_by_key(|(offset, _)| *offset);
    occurrences.into_iter().map(|(_, marker)| marker).collect()
}

fn marker_comments_with_offsets(html: &str) -> Vec<(usize, String)> {
    let mut result = Vec::new();
    let mut cursor = 0;
    while let Some(relative) = html[cursor..].find("<!--plec:") {
        let start = cursor + relative;
        let body_start = start + "<!--plec:".len();
        let Some(end) = html[body_start..].find("-->") else {
            break;
        };
        result.push((start, html[body_start..body_start + end].to_owned()));
        cursor = body_start + end + 3;
    }
    result
}

fn parse_text_address(address: &str) -> Result<Vec<AddressSegment>, String> {
    let Some((path, index)) = address.rsplit_once(':') else {
        return Err("text marker needs a numeric node handle".into());
    };
    if index.parse::<usize>().is_err() {
        return Err("text marker needs a numeric node handle".into());
    }
    parse_address(&format!("{path}/node:{index}"))
}

fn parse_boundary_address(address: &str) -> Result<Vec<AddressSegment>, String> {
    let Some((path, index)) = address.rsplit_once(':') else {
        return Err("boundary marker needs a numeric node handle".into());
    };
    if index.parse::<usize>().is_err() {
        return Err("boundary marker needs a numeric node handle".into());
    }
    parse_address(path)
}

fn parse_address(address: &str) -> Result<Vec<AddressSegment>, String> {
    let parts: Vec<_> = address.split('/').collect();
    if parts.first() != Some(&"root") {
        return Err("must start with 'root'".into());
    }
    let mut result = vec![AddressSegment {
        kind: "root".into(),
        value: "root".into(),
        depth: 0,
    }];
    for (depth, part) in parts.iter().enumerate().skip(1) {
        let Some((kind, value)) = part.split_once(':') else {
            return Err(format!("segment {part:?} is not kind:value"));
        };
        if value.is_empty() {
            return Err(format!("segment {part:?} has an empty value"));
        }
        match kind {
            "outlet" | "key" => {}
            "component" | "loop" | "node" if value.parse::<usize>().is_ok() => {}
            _ => return Err(format!("invalid segment {part:?}")),
        }
        result.push(AddressSegment {
            kind: kind.into(),
            value: value.into(),
            depth,
        });
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explains_address() {
        let r = explain("root/component:2/loop:1/key:a%2Fb/node:3").unwrap();
        assert_eq!(r.segments.len(), 5);
    }

    #[test]
    fn rejects_bad_address() {
        assert!(explain("page/node:1").is_err());
        assert!(explain("root/node:x").is_err());
    }

    #[test]
    fn extracts_markers_in_document_order() {
        let html = "<div data-plec-node=\"root/node:0\"><!--plec:text:root:1-->hi\
                    <!--plec:component:root:2--></div>";
        assert_eq!(
            extract_markers(html),
            vec![
                "node:root/node:0".to_string(),
                "text:root:1".to_string(),
                "component:root:2".to_string(),
            ]
        );
    }

    #[test]
    fn wildcard_rows_match_any_key() {
        assert!(marker_matches(
            "loop:root/loop:0/key:*",
            "loop:root/loop:0/key:a"
        ));
        assert!(marker_matches(
            "node:root/loop:0/key:*/node:3",
            "node:root/loop:0/key:b/node:3"
        ));
        assert!(!marker_matches(
            "node:root/loop:0/key:*/node:3",
            "node:root/loop:0/key:b/node:4"
        ));
        assert!(!marker_matches(
            "loop:root/loop:0/key:*",
            "loop:root/loop:1/key:a"
        ));
        // Wildcards sit mid-segment for markers that suffix a node handle.
        assert!(marker_matches(
            "text:root/loop:0/key:*:2",
            "text:root/loop:0/key:seven:2"
        ));
        assert!(!marker_matches(
            "text:root/loop:0/key:*:2",
            "text:root/loop:0/key:seven:3"
        ));
    }

    #[test]
    fn order_accepts_repeated_wildcard_rows() {
        let expected = vec![
            "node:root/node:0".to_string(),
            "loop:root/loop:1/key:*".to_string(),
            "node:root/loop:1/key:*/node:2".to_string(),
            "loop-end:root/loop:1/key:*".to_string(),
            "text:root:3".to_string(),
        ];
        let actual = vec![
            "node:root/node:0".to_string(),
            "loop:root/loop:1/key:a".to_string(),
            "node:root/loop:1/key:a/node:2".to_string(),
            "loop-end:root/loop:1/key:a".to_string(),
            "loop:root/loop:1/key:b".to_string(),
            "node:root/loop:1/key:b/node:2".to_string(),
            "loop-end:root/loop:1/key:b".to_string(),
            "text:root:3".to_string(),
        ];
        assert!(check_order(&expected, &actual).is_none());
    }

    #[test]
    fn order_accepts_nested_wildcard_rows() {
        let expected = vec![
            "loop:root/loop:0/key:*".to_string(),
            "loop:root/loop:0/key:*/loop:2/key:*".to_string(),
            "loop-end:root/loop:0/key:*/loop:2/key:*".to_string(),
            "loop-end:root/loop:0/key:*".to_string(),
        ];
        let keys = ["a", "b"];
        let mut actual = Vec::new();
        for key in keys {
            for inner in keys {
                actual.push(format!("loop:root/loop:0/key:{key}"));
                actual.push(format!("loop:root/loop:0/key:{key}/loop:2/key:{inner}"));
                actual.push(format!("loop-end:root/loop:0/key:{key}/loop:2/key:{inner}"));
                actual.push(format!("loop-end:root/loop:0/key:{key}"));
            }
        }
        assert!(check_order(&expected, &actual).is_none());
    }

    #[test]
    fn order_flags_inversion() {
        let expected = vec![
            "component:root:1".to_string(),
            "component-end:root:1".to_string(),
        ];
        let actual = vec![
            "component-end:root:1".to_string(),
            "component:root:1".to_string(),
        ];
        let problem = check_order(&expected, &actual).unwrap();
        assert!(problem.starts_with("order:ssr-marker:"), "{problem}");
    }

    #[test]
    fn order_tolerates_optional_interior_between_boundaries() {
        let expected = vec![
            "conditional:root:1".to_string(),
            "conditional-end:root:1".to_string(),
        ];
        let actual = vec![
            "conditional:root:1".to_string(),
            "node:root/node:5".to_string(),
            "conditional-end:root:1".to_string(),
        ];
        assert!(check_order(&expected, &actual).is_none());
    }

    #[test]
    fn unexpected_markers_reported_against_expected_and_optional() {
        let expected = vec!["node:root/node:0".to_string()];
        let optional = vec!["node:root/node:9".to_string()];
        let actual = vec![
            "node:root/node:0".to_string(),
            "node:root/node:9".to_string(),
            "component-end:root:4".to_string(),
        ];
        let problems = check_unexpected(&expected, &optional, &actual);
        assert_eq!(
            problems,
            vec!["unexpected:ssr-marker:component-end:root:4".to_string()]
        );
    }

    #[test]
    fn missing_component_end_keeps_adoption_code() {
        let problems = check_missing(
            &["component-end:root/component:1:2".to_string()],
            &["component:root/component:1:2".to_string()],
        );
        assert_eq!(
            problems,
            vec![
                "missing:ssr-component: component-end:root/component:1:2; expected \
                 <!--plec:component-end:root/component:1:2-->"
                    .to_string()
            ]
        );
    }

    fn route_entry(
        id: &str,
        parent_id: Option<&str>,
        path: &str,
        graph_id: &str,
    ) -> RouteManifestEntry {
        RouteManifestEntry {
            id: id.into(),
            parent_id: parent_id.map(str::to_string),
            path: path.into(),
            graph_id: graph_id.into(),
            pending_graph_id: None,
            pending_mode: "replace".into(),
            error_graph_id: None,
            loader_action: None,
            outlet_id: "main".into(),
            meta: None,
        }
    }

    #[test]
    fn composes_full_route_paths_and_selects_children() {
        let manifest = RouteManifest {
            version: 3,
            revision: "test".into(),
            root_graph_id: "root".into(),
            routes: vec![
                route_entry("home", None, "", "home-graph"),
                route_entry("about", None, "about", "about-graph"),
                route_entry("detail", Some("about"), "detail", "detail-graph"),
            ],
        };
        assert_eq!(full_path(&manifest, &manifest.routes[0]), "/");
        assert_eq!(full_path(&manifest, &manifest.routes[1]), "/about");
        assert_eq!(full_path(&manifest, &manifest.routes[2]), "/about/detail");

        // No filter: the first non-wildcard child in manifest order.
        assert_eq!(
            select_outlet_route(&manifest, None, "main", None).map(|e| e.id.clone()),
            Some("home".into())
        );

        // Exact filter at the top level.
        assert_eq!(
            select_outlet_route(&manifest, None, "main", Some("/about")).map(|e| e.id.clone()),
            Some("about".into())
        );

        // A deeper filter still selects its chain root at the top level, and
        // the nested child below it.
        assert_eq!(
            select_outlet_route(&manifest, None, "main", Some("/about/detail"))
                .map(|e| e.id.clone()),
            Some("about".into())
        );
        assert_eq!(
            select_outlet_route(
                &manifest,
                manifest.routes.iter().find(|e| e.id == "about"),
                "main",
                Some("/about/detail")
            )
            .map(|e| e.id.clone()),
            Some("detail".into())
        );
    }

    #[test]
    fn escapes_instance_segments_like_the_renderer() {
        assert_eq!(escape_instance_segment("a/b"), "a%2Fb");
        assert_eq!(escape_instance_segment("a%b"), "a%25b");
        assert_eq!(escape_instance_segment("a%b/c"), "a%25b%2Fc");
    }
}
