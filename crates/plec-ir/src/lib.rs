use serde::{Deserialize, Serialize};

pub mod limits;
pub mod sink;

pub const VERSION: &str = "0.10";
// The component graph schema is still in its 0.10 development window.  Keep
// additions in this contract until it is deliberately released.
pub const COMPONENT_VERSION: &str = "0.10";

/// Execution ownership and public exposure are intentionally separate. A
/// value may be serializable while still being server-only (for example a
/// session identifier); only an explicit PublicExport may cross the boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExecutionOwner {
    Shared,
    Server,
    Client,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicExport {
    pub name: String,
    pub source_owner: ExecutionOwner,
    pub value_is_serializable: bool,
    pub explicitly_public: bool,
}

pub fn validate_public_export(export: &PublicExport) -> Result<(), &'static str> {
    if export.source_owner == ExecutionOwner::Client {
        return Err("client values cannot be server exports");
    }
    if !export.value_is_serializable {
        return Err("public export must be serializable");
    }
    if !export.explicitly_public {
        return Err("server value requires an explicit public export boundary");
    }
    Ok(())
}

/// A separately-versioned component application.  Component definitions keep
/// their local node/state handles; call nodes connect those local programs.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentApplication {
    pub version: &'static str,
    pub root_component: usize,
    pub components: Vec<ExecutableComponent>,
}

/// The router is deliberately a separate artifact: graphs keep local runtime
/// handles while this manifest owns the links between route instances.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteManifest {
    pub version: u32,
    #[serde(default)]
    pub revision: String,
    pub root_graph_id: String,
    pub routes: Vec<RouteManifestEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteManifestEntry {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub path: String,
    pub graph_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_graph_id: Option<String>,
    #[serde(
        skip_serializing_if = "is_replace_pending_mode",
        default = "default_pending_mode"
    )]
    pub pending_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_graph_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loader_action: Option<usize>,
    pub outlet_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<RouteMetadata>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RouteMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl RouteManifest {
    pub fn validate(&self) -> Result<(), String> {
        use crate::limits::MAX_MANIFEST_ROUTES;
        if self.version != 3 {
            return Err("unsupported route manifest version".into());
        }
        if self.root_graph_id.is_empty() {
            return Err("route manifest root graph id is required".into());
        }
        if self.routes.len() > MAX_MANIFEST_ROUTES {
            return Err("route manifest exceeds the route count limit".into());
        }
        Ok(())
    }
}

fn is_replace_pending_mode(value: &String) -> bool {
    value == "replace"
}

fn default_pending_mode() -> String {
    "replace".into()
}

// ---------------------------------------------------------------------------
// SSR execution snapshot (v1)
//
// The snapshot is the typed transfer half of the SSR contract: the server
// emits HTML plus one `PlecSsrSnapshot`, and the browser/WASM runtime imports
// that snapshot as its initial execution state before adopting the
// server-rendered DOM.
//
// Design decisions frozen here:
//
// 1. **Schema home and format.** These types live in `plec-ir` (Rust-owned);
//    serde is camelCase to match `RouteManifest`. The native server produces
//    JSON conforming to this schema and the WASM runtime is the
//    strict consumer. There is deliberately no Zod or TypeScript schema for
//    the snapshot — a second schema would drift from this authority.
// 2. **Canonical encodings.** No new id spaces are introduced. Route chain
//    entries reference manifest route ids (`"{module}#{local}"`). Loader
//    outcomes are derived from `RouteManifestEntry { graph_id, loader_action }`
//    (see `loader_ref`). Conditional branch ownership is encoded against the
//    node handle with `consequent | alternate | none`; selecting `alternate`
//    is only valid when that conditional defines one. Loop keys are the
//    strings produced by the runtime's `typed_value_string` canonicalization
//    (string as-is, null → `""`, bool → `"true"`/`"false"`, number → its
//    Rust `Display` form, array/record → compact JSON); non-string keys are
//    unrepresentable in this schema, so producers must canonicalize first.
//    Graph instance keys use the runtime `graph_instance_id` grammar
//    (`root/outlet:main`, `{parent}/outlet:{outlet}[/key:{key}]` with `%2F`
//    and `%25` escaping) so snapshot keys and runtime instance ids coincide.
// 3. **Version + validation policy.** The snapshot has its own version
//    constant, independent of the route manifest and artifact versions.
//    `validate` fails closed on: unknown versions, revision mismatch with the
//    manifest, malformed graph-instance paths, unknown route/graph/node
//    references, broken route chains, undeclared route params, duplicate or
//    unordered structural entries, duplicate loop keys, non-finite numbers,
//    and any export failing `validate_public_export`.
// 4. **Public vs server-only taxonomy.** The snapshot may carry only
//    explicitly public exports (`validate_public_export` enforces
//    `explicitly_public` + `source_owner != Client` + `value_is_serializable`).
//    Raw headers, cookies, sessions, DB handles, closures, and continuations
//    are server-only and can never be represented as exports.
// 5. **Transfer-vs-recompute scope.** The snapshot transfers identity and
//    causes that cannot safely be reconstructed: route chain identity, public
//    execution exports, loader outcomes, and structural ownership (selected
//    branches, keyed loop rows). Everything deterministic is recomputed
//    client-side: pure expressions, derived bindings, dependency edges, and
//    static deterministic state. The snapshot never carries component-local
//    VM state.
// ---------------------------------------------------------------------------

/// Snapshot schema version, independent of `RouteManifest.version` and the
/// artifact version. Version 2 added `SsrStructure.nested`: recorded branch
/// and loop state for nested component instances, keyed by marker path, so
/// adoption no longer infers nested branch selections from DOM shape and no
/// longer fails on nested component loops.
pub const SSR_SNAPSHOT_VERSION: u32 = 2;

/// The graph instance the route chain renders into. Mirrors the runtime's
/// `graph_instance_id(None, "main", None)` convention.
pub const ROOT_GRAPH_INSTANCE_ID: &str = "root/outlet:main";

/// The server-rendered execution state for one request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlecSsrSnapshot {
    #[serde(default)]
    pub version: u32,
    /// Must equal the `RouteManifest` revision (the freshness handshake).
    #[serde(default)]
    pub revision: String,
    /// The matched route chain, outermost first.
    pub routes: Vec<SsrRouteInstance>,
    /// Request-scoped public state.
    pub public: SsrPublicState,
    /// Outcomes of route loaders executed server-side, keyed by manifest
    /// entry (`graph_id` + `loader_action`).
    #[serde(default)]
    pub loaders: Vec<SsrLoaderOutcome>,
    /// Structural ownership per mounted graph instance.
    pub structure: SsrStructure,
}

/// The compiled-graph facts snapshot structure validation needs.  A trait so
/// the compile-time `ComponentApplication` and the WASM runtime's
/// deserialization mirror can both act as the validation reference without
/// plec-ir owning the runtime's schema.
pub trait SsrStructureApplication {
    /// The component graph with this id, if the application defines it.
    fn structure_graph(&self, graph_id: &str) -> Option<&dyn SsrStructureGraph>;
}

/// One component graph as seen by snapshot structure validation.
pub trait SsrStructureGraph {
    /// The structure-relevant kind of one node handle, `None` when the handle
    /// is out of range.
    fn structure_node(&self, handle: usize) -> Option<SsrStructureNode>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SsrStructureNode {
    /// A conditional node; `has_alternate` gates `SsrSelectedBranch::Alternate`.
    Conditional {
        has_alternate: bool,
    },
    Loop,
    Other,
}

/// The compile-time artifacts a snapshot must be validated against.
#[derive(Clone, Copy)]
pub struct SsrSnapshotReferences<'a> {
    pub manifest: &'a RouteManifest,
    pub application: &'a dyn SsrStructureApplication,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SsrRouteInstance {
    /// A `RouteManifestEntry.id`.
    pub route_id: String,
    /// Matched `$param` values accumulated through this entry's route branch.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub params: std::collections::BTreeMap<String, String>,
    /// Which phase graph the instance rendered.
    #[serde(default)]
    pub phase: SsrRoutePhase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SsrRoutePhase {
    #[default]
    Active,
    Pending,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SsrPublicState {
    /// Absolute request path the snapshot was produced for.
    pub location: String,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub exports: std::collections::BTreeMap<String, SsrPublicExport>,
}

/// One value that crossed the server boundary through an explicit public
/// export. The declaration is carried alongside the value so consumers (and
/// `validate`) can re-check the boundary at import time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SsrPublicExport {
    pub value: SsrSnapshotValue,
    pub declaration: PublicExport,
}

/// A JSON-transportable value. Deliberately separate from the graph `Value`
/// enum, which is serialize-only; the snapshot needs strict round-tripping.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SsrSnapshotValue {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<SsrSnapshotValue>),
    Record(std::collections::BTreeMap<String, SsrSnapshotValue>),
}

/// Canonical loader reference derived from a `RouteManifestEntry`:
/// `"{graph_id}#action:{n}"`. Graph ids may contain `#`, so parse refs from
/// the right.
pub fn loader_ref(graph_id: &str, action: usize) -> String {
    format!("{graph_id}#action:{action}")
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SsrLoaderOutcome {
    /// The `RouteManifestEntry.graph_id` that owns the loader.
    pub graph_id: String,
    /// The entry's `loader_action` index.
    pub action: usize,
    pub state: SsrLoaderState,
}

/// Loader outcomes are total: a loader either resolved a value or was
/// rejected with a message. There is no "unresolved" state to represent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SsrLoaderState {
    Resolved { value: SsrSnapshotValue },
    Rejected { message: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SsrStructure {
    /// Structural ownership per graph instance, keyed by instance id. The
    /// map key grammar keeps snapshot keys identical to runtime instance ids.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub graphs: std::collections::BTreeMap<String, SsrGraphStructure>,
    /// Structural ownership for nested component instances, keyed by the
    /// component's marker path (`{instance marker path}/component:{handle}`,
    /// row-scoped below loops). Since the 2 snapshot these records are the
    /// ownership cause for nested component adoption; the DOM-shape inference
    /// fallback only serves legacy producers.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub nested: std::collections::BTreeMap<String, SsrGraphStructure>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SsrGraphStructure {
    /// The compiled component graph mounted at this instance.
    pub graph_id: String,
    /// Selected conditional branches, ordered by node handle.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub branches: Vec<SsrBranchSelection>,
    /// Claimed loop rows, ordered by node handle.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loops: Vec<SsrLoopRows>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SsrBranchSelection {
    /// Handle of the `Node::Conditional` within `graph_id`'s component.
    pub node: usize,
    pub selected: SsrSelectedBranch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SsrSelectedBranch {
    Consequent,
    Alternate,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SsrLoopRows {
    /// Handle of the `Node::Loop` within `graph_id`'s component.
    pub node: usize,
    /// Row keys in render order, canonicalized with `typed_value_string`.
    pub keys: Vec<String>,
}

impl SsrStructureApplication for ComponentApplication {
    fn structure_graph(&self, graph_id: &str) -> Option<&dyn SsrStructureGraph> {
        let component = self
            .components
            .iter()
            .find(|component| component.id == graph_id)?;
        Some(component)
    }
}

impl SsrStructureGraph for ExecutableComponent {
    fn structure_node(&self, handle: usize) -> Option<SsrStructureNode> {
        Some(match self.nodes.get(handle)? {
            Node::Conditional { alternate, .. } => SsrStructureNode::Conditional {
                has_alternate: alternate.is_some(),
            },
            Node::Loop { .. } => SsrStructureNode::Loop,
            _ => SsrStructureNode::Other,
        })
    }
}

impl PlecSsrSnapshot {
    /// Validates the snapshot against the artifacts it claims to describe.
    /// Every check fails closed; a snapshot that cannot be fully validated
    /// must send the browser to a client remount, never to a guess.
    pub fn validate(&self, references: &SsrSnapshotReferences<'_>) -> Result<(), String> {
        use crate::limits::{MAX_SNAPSHOT_ENTRIES, MAX_SNAPSHOT_LOOP_KEYS};
        if self.version != SSR_SNAPSHOT_VERSION {
            return Err("unsupported ssr snapshot version".into());
        }
        if self.revision.is_empty() {
            return Err("ssr snapshot revision is required".into());
        }
        if self.revision != references.manifest.revision {
            return Err("ssr snapshot revision does not match the route manifest".into());
        }
        if self.routes.len() > MAX_SNAPSHOT_ENTRIES
            || self.loaders.len() > MAX_SNAPSHOT_ENTRIES
            || self.structure.graphs.len() > MAX_SNAPSHOT_ENTRIES
            || self.structure.nested.len() > MAX_SNAPSHOT_ENTRIES
            || self.public.exports.len() > MAX_SNAPSHOT_ENTRIES
        {
            return Err("ssr snapshot entry count exceeds limit".into());
        }
        for graph in self.structure.graphs.values() {
            if graph.branches.len() > MAX_SNAPSHOT_ENTRIES {
                return Err("ssr snapshot branch count exceeds limit".into());
            }
            for loop_rows in &graph.loops {
                if loop_rows.keys.len() > MAX_SNAPSHOT_LOOP_KEYS {
                    return Err("ssr snapshot loop key count exceeds limit".into());
                }
            }
        }
        for graph in self.structure.nested.values() {
            if graph.branches.len() > MAX_SNAPSHOT_ENTRIES {
                return Err("ssr snapshot branch count exceeds limit".into());
            }
            for loop_rows in &graph.loops {
                if loop_rows.keys.len() > MAX_SNAPSHOT_LOOP_KEYS {
                    return Err("ssr snapshot loop key count exceeds limit".into());
                }
            }
        }
        self.validate_route_chain(references.manifest)?;
        self.validate_public_state()?;
        self.validate_loaders(references.manifest)?;
        self.validate_structure(references.application)
    }

    fn validate_route_chain(&self, manifest: &RouteManifest) -> Result<(), String> {
        if self.routes.is_empty() {
            return Err("ssr snapshot route chain is required".into());
        }
        let mut declared_params = std::collections::BTreeSet::new();
        for (index, instance) in self.routes.iter().enumerate() {
            if instance.route_id.is_empty() {
                return Err("ssr snapshot route instance id is required".into());
            }
            let entry = manifest
                .routes
                .iter()
                .find(|route| route.id == instance.route_id)
                .ok_or_else(|| format!("unknown ssr snapshot route {}", instance.route_id))?;
            if index > 0 {
                let parent_id = self.routes[index - 1].route_id.as_str();
                if entry.parent_id.as_deref() != Some(parent_id) {
                    return Err(format!(
                        "ssr snapshot route chain breaks at {}",
                        instance.route_id
                    ));
                }
            }
            declared_params.extend(route_path_param_names(&entry.path));
            for (name, value) in &instance.params {
                if name.is_empty() {
                    return Err("ssr snapshot route param name is required".into());
                }
                if !declared_params.contains(name.as_str()) {
                    return Err(format!(
                        "route param {name} is not declared by {}",
                        instance.route_id
                    ));
                }
                if value.is_empty() {
                    return Err(format!("route param {name} value is required"));
                }
            }
        }
        Ok(())
    }

    fn validate_public_state(&self) -> Result<(), String> {
        if !self.public.location.starts_with('/') {
            return Err("ssr snapshot location must be an absolute path".into());
        }
        for (name, export) in &self.public.exports {
            if name.is_empty() {
                return Err("ssr snapshot export name is required".into());
            }
            if export.declaration.name != *name {
                return Err(format!(
                    "public export declaration name does not match {name}"
                ));
            }
            validate_public_export(&export.declaration)
                .map_err(|error| format!("public export {name} rejected: {error}"))?;
            if ensure_value_is_finite(&export.value).is_err() {
                return Err(format!("public export {name} contains a non-finite number"));
            }
            if let Err(error) = ensure_value_is_bounded(&export.value) {
                return Err(format!("public export {name} {error}"));
            }
        }
        Ok(())
    }

    fn validate_loaders(&self, manifest: &RouteManifest) -> Result<(), String> {
        let mut seen = std::collections::BTreeSet::new();
        for outcome in &self.loaders {
            let reference = loader_ref(&outcome.graph_id, outcome.action);
            if outcome.graph_id.is_empty() {
                return Err("ssr snapshot loader graph id is required".into());
            }
            let known = manifest.routes.iter().any(|route| {
                route.graph_id == outcome.graph_id && route.loader_action == Some(outcome.action)
            });
            if !known {
                return Err(format!("unknown ssr snapshot loader {reference}"));
            }
            if !seen.insert((outcome.graph_id.as_str(), outcome.action)) {
                return Err(format!("duplicate ssr snapshot loader {reference}"));
            }
            match &outcome.state {
                SsrLoaderState::Resolved { value } => {
                    if ensure_value_is_finite(value).is_err() {
                        return Err(format!("loader {reference} resolved a non-finite number"));
                    }
                    if let Err(error) = ensure_value_is_bounded(value) {
                        return Err(format!("loader {reference} {error}"));
                    }
                }
                SsrLoaderState::Rejected { message } => {
                    if message.is_empty() {
                        return Err(format!("loader {reference} rejection requires a message"));
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_structure(&self, application: &dyn SsrStructureApplication) -> Result<(), String> {
        if !self.structure.graphs.contains_key(ROOT_GRAPH_INSTANCE_ID) {
            return Err("ssr snapshot structure requires the root graph instance".into());
        }
        for (instance_id, structure) in &self.structure.graphs {
            validate_graph_instance_path(instance_id)
                .map_err(|error| format!("malformed graph instance path {instance_id}: {error}"))?;
            let (parent, _, _) = split_graph_instance_path(instance_id)
                .map_err(|error| format!("malformed graph instance path {instance_id}: {error}"))?;
            if let Some(parent) = parent {
                // Nested instance ids embed their parent segment escaped
                // (`graph_instance_id` escapes `/` as `%2F` and `%` as
                // `%25`), so the claimed parent may be stored under its
                // decoded form: `root%2Foutlet:main/outlet:main` chains to
                // the root instance keyed `root/outlet:main`.
                let decoded = parent.replace("%2F", "/").replace("%25", "%");
                if !self.structure.graphs.contains_key(parent)
                    && !self.structure.graphs.contains_key(&decoded)
                {
                    return Err(format!(
                        "graph instance {instance_id} references unclaimed parent {parent}"
                    ));
                }
            }
            let component = application
                .structure_graph(&structure.graph_id)
                .ok_or_else(|| format!("unknown ssr snapshot graph {}", structure.graph_id))?;
            validate_branch_selections(component, instance_id, &structure.branches)?;
            validate_loop_rows(component, instance_id, &structure.loops)?;
        }
        for (path, structure) in &self.structure.nested {
            validate_nested_component_path(path)
                .map_err(|error| format!("unknown nested component path {path}: {error}"))?;
            let component = application
                .structure_graph(&structure.graph_id)
                .ok_or_else(|| format!("unknown ssr snapshot graph {}", structure.graph_id))?;
            validate_branch_selections(component, path, &structure.branches)?;
            validate_loop_rows(component, path, &structure.loops)?;
        }
        Ok(())
    }
}

fn route_path_param_names(path: &str) -> Vec<&str> {
    path.split('/')
        .filter_map(|segment| segment.strip_prefix('$'))
        .collect()
}

fn ensure_value_is_finite(value: &SsrSnapshotValue) -> Result<(), &'static str> {
    match value {
        SsrSnapshotValue::Number(number) if !number.is_finite() => {
            Err("snapshot numbers must be finite to survive the JSON transport")
        }
        SsrSnapshotValue::Array(values) => values.iter().try_for_each(ensure_value_is_finite),
        SsrSnapshotValue::Record(fields) => fields.values().try_for_each(ensure_value_is_finite),
        _ => Ok(()),
    }
}

/// Bounds an untrusted snapshot value tree before adoption seeds it into
/// host inputs. The parser's depth guard already limits recursion during
/// decode; this rejects shapes beyond the documented runtime-value limits
/// before they drive further allocation.
fn ensure_value_is_bounded(value: &SsrSnapshotValue) -> Result<(), &'static str> {
    use crate::limits::{MAX_VALUE_DEPTH, MAX_VALUE_NODES};
    let mut stack: Vec<(&SsrSnapshotValue, usize)> = vec![(value, 0)];
    let mut nodes = 0usize;
    while let Some((value, depth)) = stack.pop() {
        if depth > MAX_VALUE_DEPTH {
            return Err("exceeds the snapshot value depth limit");
        }
        nodes += 1;
        if nodes > MAX_VALUE_NODES {
            return Err("exceeds the snapshot value size limit");
        }
        match value {
            SsrSnapshotValue::Array(values) => {
                stack.extend(values.iter().map(|value| (value, depth + 1)));
            }
            SsrSnapshotValue::Record(fields) => {
                nodes += fields.len();
                if nodes > MAX_VALUE_NODES {
                    return Err("exceeds the snapshot value size limit");
                }
                stack.extend(fields.values().map(|value| (value, depth + 1)));
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_branch_selections(
    graph: &dyn SsrStructureGraph,
    instance_id: &str,
    branches: &[SsrBranchSelection],
) -> Result<(), String> {
    let mut previous: Option<usize> = None;
    for branch in branches {
        if previous.is_some_and(|previous| branch.node <= previous) {
            return Err(format!(
                "branch selections for {instance_id} must be ordered by node handle without duplicates"
            ));
        }
        previous = Some(branch.node);
        let has_alternate = match graph.structure_node(branch.node) {
            Some(SsrStructureNode::Conditional { has_alternate }) => has_alternate,
            Some(_) => {
                return Err(format!(
                    "branch selection for {instance_id} names node {} which is not a conditional",
                    branch.node
                ))
            }
            None => {
                return Err(format!(
                    "branch selection for {instance_id} references unknown node handle {}",
                    branch.node
                ))
            }
        };
        if branch.selected == SsrSelectedBranch::Alternate && !has_alternate {
            return Err(format!(
                "branch selection for {instance_id} names an alternate that node {} does not define",
                branch.node
            ));
        }
    }
    Ok(())
}

fn validate_loop_rows(
    graph: &dyn SsrStructureGraph,
    instance_id: &str,
    loops: &[SsrLoopRows],
) -> Result<(), String> {
    let mut previous: Option<usize> = None;
    for loop_rows in loops {
        if previous.is_some_and(|previous| loop_rows.node <= previous) {
            return Err(format!(
                "loop records for {instance_id} must be ordered by node handle without duplicates"
            ));
        }
        previous = Some(loop_rows.node);
        match graph.structure_node(loop_rows.node) {
            Some(SsrStructureNode::Loop) => {}
            Some(_) => {
                return Err(format!(
                    "loop record for {instance_id} names node {} which is not a loop",
                    loop_rows.node
                ))
            }
            None => {
                return Err(format!(
                    "loop record for {instance_id} references unknown node handle {}",
                    loop_rows.node
                ))
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        for key in &loop_rows.keys {
            if !seen.insert(key.as_str()) {
                return Err(format!(
                    "loop at {instance_id} node {} has duplicate key {key}",
                    loop_rows.node
                ));
            }
        }
    }
    Ok(())
}

/// Validates one graph instance path against the runtime's ownership grammar:
/// `root`, `root/outlet:main`, or `{parent}/outlet:{outlet}[/key:{key}]`
/// where `outlet` and `key` escape `/` as `%2F` and `%` as `%25`. Marker
/// paths (`{instance}/component:{i}/node:{j}`) compose from a valid instance
/// prefix.
pub fn validate_graph_instance_path(path: &str) -> Result<(), String> {
    split_graph_instance_path(path).map(|_| ())
}

fn split_graph_instance_path(path: &str) -> Result<(Option<&str>, &str, Option<&str>), String> {
    if path.is_empty() {
        return Err("graph instance path is empty".into());
    }
    let (base, key) = match path.rfind("/key:") {
        Some(index) => {
            let key = &path[index + "/key:".len()..];
            if key.is_empty() {
                return Err("graph instance key segment is empty".into());
            }
            if key.contains('/') {
                return Err("graph instance key segment must escape '/'".into());
            }
            (&path[..index], Some(key))
        }
        None => (path, None),
    };
    let outlet_at = base
        .rfind("/outlet:")
        .ok_or_else(|| "graph instance path requires an /outlet: segment".to_string())?;
    let outlet = &base[outlet_at + "/outlet:".len()..];
    if outlet.is_empty() {
        return Err("graph instance outlet segment is empty".into());
    }
    if outlet.contains('/') {
        return Err("graph instance outlet segment must escape '/'".into());
    }
    let parent = &base[..outlet_at];
    if parent.is_empty() {
        return Err("graph instance path requires a parent".into());
    }
    let parent = if parent == "root" { None } else { Some(parent) };
    Ok((parent, outlet, key))
}

/// Validates one nested component marker path: `root` followed by
/// `component:{handle}`, `loop:{handle}`, and escaped `key:`/`outlet:`
/// segments, terminating at a `component:` segment. The runtime composes
/// these paths from the same segment grammar the server renderer emits, so a
/// snapshot key outside the grammar cannot address any component instance.
fn validate_nested_component_path(path: &str) -> Result<(), String> {
    let segments: Vec<&str> = path.split('/').collect();
    if segments.first() != Some(&"root") {
        return Err("path must start at the root marker scope".into());
    }
    if segments.len() < 2 {
        return Err("path requires a component segment".into());
    }
    for segment in &segments[1..] {
        if let Some(handle) = segment
            .strip_prefix("component:")
            .or_else(|| segment.strip_prefix("loop:"))
        {
            if handle.is_empty() || !handle.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(format!("segment {segment} requires a node handle"));
            }
        } else if let Some(value) = segment
            .strip_prefix("key:")
            .or_else(|| segment.strip_prefix("outlet:"))
        {
            if value.is_empty() {
                return Err(format!("segment {segment} is empty"));
            }
            validate_escaped_segment(value)
                .map_err(|error| format!("segment {segment} {error}"))?;
        } else {
            return Err(format!("unknown segment {segment}"));
        }
    }
    if !segments.last().unwrap().starts_with("component:") {
        return Err("path must end at a component segment".into());
    }
    Ok(())
}

/// `graph_instance_id` escapes `%` as `%25` and `/` as `%2F`; a raw escape
/// sequence outside that grammar would decode ambiguously at claim time.
fn validate_escaped_segment(value: &str) -> Result<(), String> {
    let mut chars = value.chars();
    while let Some(character) = chars.next() {
        if character == '%' {
            match (chars.next(), chars.next()) {
                (Some('2'), Some('5')) | (Some('2'), Some('F')) => {}
                _ => return Err("must escape '%' as %25 and '/' as %2F".into()),
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutableComponent {
    pub id: String,
    pub root_node: usize,
    pub strings: Vec<String>,
    pub constants: Vec<Value>,
    pub nodes: Vec<Node>,
    pub texts: Vec<Text>,
    pub bindings: Vec<Binding>,
    pub prop_programs: Vec<PropProgram>,
    pub events: Vec<Event>,
    pub inputs: Vec<Input>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub host_slots: Vec<HostSlot>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub capabilities: Vec<CookieCapability>,
    pub state_slots: Vec<StateSlot>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub ref_slots: Vec<RefSlot>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub host_refs: Vec<HostRef>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub reactions: Vec<Reaction>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub listeners: Vec<Listener>,
    pub parameters: Vec<ComponentParameter>,
    pub expressions: Vec<ExpressionProgram>,
    pub actions: Vec<ActionProgram>,
    pub loops: Vec<Loop>,
    pub dependency_edges: Vec<DependencyEdge>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub route_outlets: Vec<RouteOutlet>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ComponentParameter {
    pub name: usize,
    pub callable: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub component: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ComponentProp {
    Value {
        name: usize,
        expression: usize,
    },
    Callable {
        name: usize,
        action: usize,
    },
    Component {
        name: usize,
        component: usize,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        host: Option<HostComponentTarget>,
    },
}

/// Stable identity for a registered external renderer component.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostComponentTarget {
    pub provider: String,
    pub component: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutableApplication {
    pub version: &'static str,
    pub root_node: usize,
    pub strings: Vec<String>,
    pub constants: Vec<Value>,
    pub nodes: Vec<Node>,
    pub texts: Vec<Text>,
    pub bindings: Vec<Binding>,
    pub prop_programs: Vec<PropProgram>,
    pub events: Vec<Event>,
    pub inputs: Vec<Input>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub host_slots: Vec<HostSlot>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub capabilities: Vec<CookieCapability>,
    pub state_slots: Vec<StateSlot>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub ref_slots: Vec<RefSlot>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub host_refs: Vec<HostRef>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub reactions: Vec<Reaction>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub listeners: Vec<Listener>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub parameters: Vec<ComponentParameter>,
    pub expressions: Vec<ExpressionProgram>,
    pub actions: Vec<ActionProgram>,
    pub loops: Vec<Loop>,
    pub dependency_edges: Vec<DependencyEdge>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub route_outlets: Vec<RouteOutlet>,
}

impl Default for ExecutableApplication {
    fn default() -> Self {
        Self {
            version: VERSION,
            root_node: 0,
            strings: vec![],
            constants: vec![],
            nodes: vec![],
            texts: vec![],
            bindings: vec![],
            prop_programs: vec![],
            events: vec![],
            inputs: vec![],
            host_slots: vec![],
            capabilities: vec![],
            state_slots: vec![],
            ref_slots: vec![],
            host_refs: vec![],
            reactions: vec![],
            listeners: vec![],
            parameters: vec![],
            expressions: vec![],
            actions: vec![],
            loops: vec![],
            dependency_edges: vec![],
            route_outlets: vec![],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    Record(std::collections::BTreeMap<String, Value>),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum Node {
    Element {
        tag: usize,
        #[serde(skip_serializing_if = "is_html_namespace")]
        namespace: &'static str,
        parent: Option<usize>,
        children: Vec<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        host_ref: Option<usize>,
    },
    Text {
        text: usize,
        parent: Option<usize>,
    },
    Conditional {
        test: usize,
        parent: Option<usize>,
        consequent: usize,
        alternate: Option<usize>,
    },
    Loop {
        r#loop: usize,
        parent: Option<usize>,
    },
    Component {
        component: usize,
        parent: Option<usize>,
        props: Vec<ComponentProp>,
        #[serde(skip_serializing_if = "Vec::is_empty", default)]
        children: Vec<usize>,
    },
    /// A component whose graph definition comes from a component-valued prop.
    DynamicComponent {
        prop: usize,
        parent: Option<usize>,
        props: Vec<ComponentProp>,
        #[serde(skip_serializing_if = "Vec::is_empty", default)]
        children: Vec<usize>,
    },
    /// A provider-owned DOM subtree. The runtime owns only the boundary.
    HostComponent {
        provider: String,
        component: String,
        parent: Option<usize>,
        props: Vec<ComponentProp>,
    },
    /// Insertion range for the implicit `children` prop. The content belongs
    /// to the caller, not the component definition which declares this node.
    Slot {
        parent: Option<usize>,
    },
}

fn is_html_namespace(value: &&'static str) -> bool {
    *value == "html"
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Text {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binding: Option<usize>,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Binding {
    pub target: usize,
    pub sink: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<usize>,
    pub expression: usize,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PropProgram {
    pub target: usize,
    pub writes: Vec<PropWrite>,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PropWrite {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<usize>,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub constant: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expression: Option<usize>,
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub spread: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Event {
    pub target: usize,
    #[serde(rename = "type")]
    pub event_type: usize,
    pub action: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#loop: Option<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub fields: Vec<EventField>,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Input {
    pub name: usize,
    pub kind: &'static str,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostSlot {
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<usize>,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CookieCapability {
    pub kind: &'static str,
    pub name: String,
    pub operations: Vec<&'static str>,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub same_site: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secure: Option<bool>,
    pub expiry_modes: Vec<&'static str>,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EventField {
    pub name: usize,
    pub slot: usize,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateSlot {
    pub initial_expression: usize,
    pub frame_slot: usize,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefSlot {
    pub initial_expression: usize,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HostRef {}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Reaction {
    pub dependencies: Vec<usize>,
    pub action: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cleanup_action: Option<usize>,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listener {
    pub source: &'static str,
    pub event: usize,
    pub action: usize,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExpressionProgram {
    pub instructions: Vec<ExpressionInstruction>,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum ExpressionInstruction {
    Constant {
        constant: usize,
    },
    LoadState {
        state: usize,
    },
    LoadRef {
        reference: usize,
    },
    LoadProp {
        prop: usize,
    },
    LoadFrame {
        slot: usize,
    },
    LoadHost {
        host: usize,
    },
    /// The whole serializable loop row. This is distinct from a field read so
    /// computed row access and row spreads retain their normal value-graph
    /// semantics.
    LoadRowRecord,
    LoadRowField {
        field: usize,
    },
    Field {
        field: usize,
    },
    Index,
    Unary {
        kind: &'static str,
    },
    Binary {
        kind: &'static str,
    },
    String {
        kind: &'static str,
        count: usize,
    },
    MakeArray {
        count: usize,
        #[serde(skip_serializing_if = "Vec::is_empty", default)]
        spreads: Vec<bool>,
    },
    MakeRecord {
        fields: Vec<usize>,
        #[serde(skip_serializing_if = "Vec::is_empty", default)]
        spreads: Vec<bool>,
    },
    OmitFields {
        fields: Vec<usize>,
    },
    Map {
        mapper: usize,
        item_slot: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        index_slot: Option<usize>,
    },
    Filter {
        predicate: usize,
        item_slot: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        index_slot: Option<usize>,
    },
    Jump {
        target: usize,
    },
    JumpIfFalse {
        target: usize,
    },
    Return,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionProgram {
    #[serde(skip_serializing_if = "is_zero", default)]
    pub frame_slots: usize,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub parameter_slots: Vec<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loader_result_state: Option<usize>,
    #[serde(skip_serializing_if = "is_false", default)]
    pub route_loader: bool,
    pub instructions: Vec<ActionInstruction>,
}

fn is_zero(value: &usize) -> bool {
    *value == 0
}

fn is_false(value: &bool) -> bool {
    !*value
}
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum ActionInstruction {
    Evaluate {
        expression: usize,
    },
    StoreState {
        state: usize,
    },
    MutationStart {
        generation: usize,
        pending: usize,
        error: usize,
    },
    MutationPublish {
        generation: usize,
        pending: usize,
        error: usize,
        data: usize,
        #[serde(rename = "invocationSlot")]
        invocation_slot: usize,
        #[serde(rename = "valueSlot")]
        value_slot: usize,
        success: bool,
    },
    StoreFrame {
        slot: usize,
    },
    StoreRef {
        reference: usize,
    },
    CaptureActiveElement {
        reference: usize,
    },
    FocusHostRef {
        reference: usize,
    },
    FocusRef {
        reference: usize,
    },
    PreventDefault,
    CallProp {
        prop: usize,
        #[serde(skip_serializing_if = "Vec::is_empty", default)]
        arguments: Vec<usize>,
    },
    CallPropOptional {
        prop: usize,
        #[serde(skip_serializing_if = "Vec::is_empty", default)]
        arguments: Vec<usize>,
    },
    CollectionMutation {
        input: usize,
        kind: &'static str,
        key: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        value: Option<usize>,
    },
    CapabilityRequest {
        #[serde(flatten)]
        request: CapabilityRequest,
        #[serde(rename = "successPc")]
        success_pc: usize,
        #[serde(rename = "failurePc")]
        failure_pc: usize,
        #[serde(rename = "finallyPc", skip_serializing_if = "Option::is_none")]
        finally_pc: Option<usize>,
        #[serde(rename = "resultSlot")]
        result_slot: usize,
        #[serde(rename = "errorSlot")]
        error_slot: usize,
    },
    Call {
        action: usize,
        #[serde(skip_serializing_if = "Vec::is_empty", default)]
        arguments: Vec<usize>,
        #[serde(rename = "successPc", skip_serializing_if = "Option::is_none")]
        success_pc: Option<usize>,
        #[serde(rename = "failurePc", skip_serializing_if = "Option::is_none")]
        failure_pc: Option<usize>,
        #[serde(rename = "resultSlot", skip_serializing_if = "Option::is_none")]
        result_slot: Option<usize>,
        #[serde(rename = "errorSlot", skip_serializing_if = "Option::is_none")]
        error_slot: Option<usize>,
    },
    CallFrame {
        parameter: usize,
        #[serde(skip_serializing_if = "Vec::is_empty", default)]
        arguments: Vec<usize>,
        #[serde(rename = "successPc", skip_serializing_if = "Option::is_none")]
        success_pc: Option<usize>,
        #[serde(rename = "failurePc", skip_serializing_if = "Option::is_none")]
        failure_pc: Option<usize>,
        #[serde(rename = "resultSlot", skip_serializing_if = "Option::is_none")]
        result_slot: Option<usize>,
        #[serde(rename = "errorSlot", skip_serializing_if = "Option::is_none")]
        error_slot: Option<usize>,
    },
    Jump {
        target: usize,
    },
    JumpIfFalse {
        target: usize,
    },
    Return {
        #[serde(skip_serializing_if = "is_success", default)]
        outcome: ReturnOutcome,
        #[serde(skip_serializing_if = "Option::is_none")]
        value: Option<usize>,
    },
}

fn is_success(value: &ReturnOutcome) -> bool {
    matches!(value, ReturnOutcome::Success)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(Default)]
pub enum ReturnOutcome {
    #[default]
    Success,
    Failure,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "capability", content = "request", rename_all = "camelCase")]
pub enum CapabilityRequest {
    Fetch {
        url: usize,
        method: &'static str,
        #[serde(skip_serializing_if = "Vec::is_empty", default)]
        headers: Vec<FetchHeader>,
        #[serde(skip_serializing_if = "Option::is_none")]
        body: Option<usize>,
        decode: &'static str,
        #[serde(rename = "requireOk")]
        require_ok: bool,
    },
    Cookie {
        operation: &'static str,
        name: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        value: Option<usize>,
        path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        same_site: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        secure: Option<bool>,
        expiry: &'static str,
        #[serde(rename = "maxAge", skip_serializing_if = "Option::is_none")]
        max_age: Option<i64>,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FetchHeader {
    pub name: usize,
    pub value: usize,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Loop {
    pub source_expression: usize,
    pub key_expression: usize,
    pub item_slot: usize,
    pub row_template: usize,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub dependency_slots: Vec<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<usize>,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DependencyEdge {
    pub source: DependencyEndpoint,
    pub target: DependencyEndpoint,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DependencyEndpoint {
    pub kind: &'static str,
    pub handle: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#loop: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RouteOutlet {
    pub id: String,
    pub node: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutation_publish_serializes_slot_fields_in_camel_case() {
        let value = serde_json::to_value(ActionInstruction::MutationPublish {
            generation: 0,
            pending: 1,
            error: 2,
            data: 3,
            invocation_slot: 4,
            value_slot: 5,
            success: true,
        })
        .unwrap();
        let object = value.as_object().unwrap();
        assert!(object.contains_key("invocationSlot"));
        assert!(object.contains_key("valueSlot"));
        assert!(!object.contains_key("invocation_slot"));
        assert!(!object.contains_key("value_slot"));
    }

    #[test]
    fn route_manifest_round_trips_the_active_transport_schema() {
        let manifest = RouteManifest {
            version: 3,
            revision: "test".into(),
            root_graph_id: "root".into(),
            routes: vec![RouteManifestEntry {
                id: "todos".into(),
                parent_id: None,
                path: "/todos".into(),
                graph_id: "todos-graph".into(),
                pending_graph_id: None,
                pending_mode: "replace".into(),
                error_graph_id: None,
                loader_action: None,
                outlet_id: "main".into(),
                meta: None,
            }],
        };
        let json = serde_json::to_value(&manifest).unwrap();
        assert_eq!(
            serde_json::from_value::<RouteManifest>(json).unwrap(),
            manifest
        );
    }

    #[test]
    fn serializable_server_values_still_require_explicit_public_exposure() {
        let hidden = PublicExport {
            name: "session".into(),
            source_owner: ExecutionOwner::Server,
            value_is_serializable: true,
            explicitly_public: false,
        };
        assert_eq!(
            validate_public_export(&hidden),
            Err("server value requires an explicit public export boundary")
        );
        let public = PublicExport {
            explicitly_public: true,
            ..hidden
        };
        assert!(validate_public_export(&public).is_ok());
    }

    #[test]
    fn route_manifest_rejects_a_non_v3_version() {
        let value =
            serde_json::json!({"version":2,"revision":"x","rootGraphId":"root","routes":[]});
        assert!(serde_json::from_value::<RouteManifest>(value)
            .unwrap()
            .validate()
            .is_err());
    }

    fn test_manifest() -> RouteManifest {
        RouteManifest {
            version: 3,
            revision: "rev-1".into(),
            root_graph_id: "app#Index".into(),
            routes: vec![
                RouteManifestEntry {
                    id: "app#Index".into(),
                    parent_id: None,
                    path: "".into(),
                    graph_id: "app#Index".into(),
                    pending_graph_id: None,
                    pending_mode: "replace".into(),
                    error_graph_id: None,
                    loader_action: None,
                    outlet_id: "main".into(),
                    meta: None,
                },
                RouteManifestEntry {
                    id: "app#Todo".into(),
                    parent_id: Some("app#Index".into()),
                    path: "todos/$todoId".into(),
                    graph_id: "app#Todo".into(),
                    pending_graph_id: None,
                    pending_mode: "replace".into(),
                    error_graph_id: None,
                    loader_action: Some(0),
                    outlet_id: "main".into(),
                    meta: None,
                },
            ],
        }
    }

    fn executable_component(id: &str, nodes: Vec<Node>) -> ExecutableComponent {
        ExecutableComponent {
            id: id.into(),
            root_node: 0,
            strings: vec![],
            constants: vec![],
            nodes,
            texts: vec![],
            bindings: vec![],
            prop_programs: vec![],
            events: vec![],
            inputs: vec![],
            host_slots: vec![],
            capabilities: vec![],
            state_slots: vec![],
            ref_slots: vec![],
            host_refs: vec![],
            reactions: vec![],
            listeners: vec![],
            parameters: vec![],
            expressions: vec![],
            actions: vec![],
            loops: vec![],
            dependency_edges: vec![],
            route_outlets: vec![],
        }
    }

    fn test_application(with_alternate: bool) -> ComponentApplication {
        ComponentApplication {
            version: COMPONENT_VERSION,
            root_component: 0,
            components: vec![
                executable_component("app#Index", vec![]),
                executable_component(
                    "app#Todo",
                    vec![
                        Node::Conditional {
                            test: 0,
                            parent: None,
                            consequent: 1,
                            alternate: with_alternate.then_some(2),
                        },
                        Node::Element {
                            tag: 0,
                            namespace: "html",
                            parent: None,
                            children: vec![],
                            host_ref: None,
                        },
                        Node::Loop {
                            r#loop: 0,
                            parent: None,
                        },
                    ],
                ),
            ],
        }
    }

    fn valid_snapshot() -> PlecSsrSnapshot {
        PlecSsrSnapshot {
            version: SSR_SNAPSHOT_VERSION,
            revision: "rev-1".into(),
            routes: vec![
                SsrRouteInstance {
                    route_id: "app#Index".into(),
                    params: Default::default(),
                    phase: SsrRoutePhase::Active,
                },
                SsrRouteInstance {
                    route_id: "app#Todo".into(),
                    params: [("todoId".into(), "42".into())].into_iter().collect(),
                    phase: SsrRoutePhase::Active,
                },
            ],
            public: SsrPublicState {
                location: "/todos/42".into(),
                exports: [(
                    "todoCount".to_string(),
                    SsrPublicExport {
                        value: SsrSnapshotValue::Number(2.0),
                        declaration: PublicExport {
                            name: "todoCount".into(),
                            source_owner: ExecutionOwner::Server,
                            value_is_serializable: true,
                            explicitly_public: true,
                        },
                    },
                )]
                .into_iter()
                .collect(),
            },
            loaders: vec![SsrLoaderOutcome {
                graph_id: "app#Todo".into(),
                action: 0,
                state: SsrLoaderState::Resolved {
                    value: SsrSnapshotValue::String("todo".into()),
                },
            }],
            structure: SsrStructure {
                graphs: [
                    (
                        ROOT_GRAPH_INSTANCE_ID.to_string(),
                        SsrGraphStructure {
                            graph_id: "app#Index".into(),
                            branches: vec![],
                            loops: vec![],
                        },
                    ),
                    (
                        "root/outlet:main/outlet:main/key:todos".to_string(),
                        SsrGraphStructure {
                            graph_id: "app#Todo".into(),
                            branches: vec![SsrBranchSelection {
                                node: 0,
                                selected: SsrSelectedBranch::Consequent,
                            }],
                            loops: vec![SsrLoopRows {
                                node: 2,
                                keys: vec!["t1".into(), "t2".into()],
                            }],
                        },
                    ),
                ]
                .into_iter()
                .collect(),
                nested: [(
                    "root/outlet:main/outlet:main/key:todos/component:1".to_string(),
                    SsrGraphStructure {
                        graph_id: "app#Todo".into(),
                        branches: vec![SsrBranchSelection {
                            node: 0,
                            selected: SsrSelectedBranch::Alternate,
                        }],
                        loops: vec![SsrLoopRows {
                            node: 2,
                            keys: vec!["t3".into()],
                        }],
                    },
                )]
                .into_iter()
                .collect(),
            },
        }
    }

    fn snapshot_error(mutate: impl FnOnce(&mut PlecSsrSnapshot)) -> String {
        let mut snapshot = valid_snapshot();
        mutate(&mut snapshot);
        let manifest = test_manifest();
        let application = test_application(true);
        snapshot
            .validate(&SsrSnapshotReferences {
                manifest: &manifest,
                application: &application,
            })
            .expect_err("snapshot should fail validation")
    }

    #[test]
    fn ssr_snapshot_round_trips_with_camel_case_fields() {
        let snapshot = valid_snapshot();
        let json = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(json["version"], SSR_SNAPSHOT_VERSION);
        assert_eq!(json["routes"][1]["routeId"], "app#Todo");
        assert_eq!(json["routes"][1]["params"]["todoId"], "42");
        assert_eq!(json["public"]["location"], "/todos/42");
        assert_eq!(
            json["public"]["exports"]["todoCount"]["declaration"]["explicitlyPublic"],
            true
        );
        assert_eq!(json["loaders"][0]["state"]["kind"], "resolved");
        assert_eq!(
            json["structure"]["graphs"]["root/outlet:main"]["graphId"],
            "app#Index"
        );
        assert_eq!(
            serde_json::from_value::<PlecSsrSnapshot>(json).unwrap(),
            snapshot
        );
    }

    #[test]
    fn ssr_snapshot_rejects_a_wrong_version() {
        assert_eq!(
            snapshot_error(|snapshot| snapshot.version = SSR_SNAPSHOT_VERSION + 1),
            "unsupported ssr snapshot version"
        );
        // The v1 snapshot predates nested component records; it must keep
        // failing closed rather than adopting with inferred state.
        assert_eq!(
            snapshot_error(|snapshot| snapshot.version = 1),
            "unsupported ssr snapshot version"
        );
    }

    #[test]
    fn ssr_snapshot_rejects_a_missing_version() {
        let mut json = serde_json::to_value(valid_snapshot()).unwrap();
        json.as_object_mut().unwrap().remove("version");
        let snapshot = serde_json::from_value::<PlecSsrSnapshot>(json).unwrap();
        assert_eq!(
            snapshot
                .validate(&SsrSnapshotReferences {
                    manifest: &test_manifest(),
                    application: &test_application(true),
                })
                .unwrap_err(),
            "unsupported ssr snapshot version"
        );
    }

    #[test]
    fn ssr_snapshot_rejects_a_revision_mismatch() {
        assert_eq!(
            snapshot_error(|snapshot| snapshot.revision = "rev-2".into()),
            "ssr snapshot revision does not match the route manifest"
        );
    }

    #[test]
    fn ssr_snapshot_rejects_loop_keys_beyond_limit() {
        let keys = vec!["k".to_string(); limits::MAX_SNAPSHOT_LOOP_KEYS + 1];
        assert_eq!(
            snapshot_error(|snapshot| {
                snapshot
                    .structure
                    .graphs
                    .get_mut("root/outlet:main/outlet:main/key:todos")
                    .unwrap()
                    .loops[0]
                    .keys = keys
            }),
            "ssr snapshot loop key count exceeds limit"
        );
    }

    #[test]
    fn ssr_snapshot_rejects_deep_public_export_values() {
        let mut value = SsrSnapshotValue::Null;
        for _ in 0..(limits::MAX_VALUE_DEPTH + 8) {
            value = SsrSnapshotValue::Array(vec![value]);
        }
        assert_eq!(
            snapshot_error(|snapshot| {
                snapshot.public.exports.get_mut("todoCount").unwrap().value = value
            }),
            "public export todoCount exceeds the snapshot value depth limit"
        );
    }

    #[test]
    fn ssr_snapshot_rejects_oversized_loader_values() {
        let mut value = SsrSnapshotValue::Null;
        for _ in 0..(limits::MAX_VALUE_DEPTH + 4) {
            value = SsrSnapshotValue::Array(vec![value]);
        }
        assert_eq!(
            snapshot_error(
                |snapshot| snapshot.loaders[0].state = SsrLoaderState::Resolved { value }
            ),
            "loader app#Todo#action:0 exceeds the snapshot value depth limit"
        );
    }

    #[test]
    fn route_manifest_rejects_routes_beyond_limit() {
        let mut manifest = test_manifest();
        manifest.routes = (0..limits::MAX_MANIFEST_ROUTES + 1)
            .map(|index| RouteManifestEntry {
                id: format!("app#Route{index}"),
                parent_id: None,
                path: format!("route-{index}"),
                graph_id: format!("app#Graph{index}"),
                pending_graph_id: None,
                pending_mode: "replace".into(),
                error_graph_id: None,
                loader_action: None,
                outlet_id: "main".into(),
                meta: None,
            })
            .collect();
        assert_eq!(
            manifest.validate().unwrap_err(),
            "route manifest exceeds the route count limit"
        );
    }

    #[test]
    fn ssr_snapshot_rejects_an_empty_route_chain() {
        assert_eq!(
            snapshot_error(|snapshot| snapshot.routes = vec![]),
            "ssr snapshot route chain is required"
        );
    }

    #[test]
    fn ssr_snapshot_rejects_an_unknown_route_id() {
        assert_eq!(
            snapshot_error(|snapshot| snapshot.routes[1].route_id = "app#Ghost".into()),
            "unknown ssr snapshot route app#Ghost"
        );
    }

    #[test]
    fn ssr_snapshot_rejects_a_broken_route_chain() {
        let error = snapshot_error(|snapshot| snapshot.routes.swap(0, 1));
        assert_eq!(error, "ssr snapshot route chain breaks at app#Index");
    }

    #[test]
    fn ssr_snapshot_rejects_undeclared_route_params() {
        assert_eq!(
            snapshot_error(|snapshot| {
                snapshot.routes[1]
                    .params
                    .insert("userId".into(), "7".into());
            }),
            "route param userId is not declared by app#Todo"
        );
    }

    #[test]
    fn ssr_snapshot_accepts_params_inherited_from_parent_routes() {
        let mut manifest = test_manifest();
        manifest.routes[0].path = "$ownerId".into();
        let mut snapshot = valid_snapshot();
        snapshot.routes[0]
            .params
            .insert("ownerId".into(), "7".into());
        snapshot.routes[1]
            .params
            .insert("ownerId".into(), "7".into());
        snapshot
            .validate(&SsrSnapshotReferences {
                manifest: &manifest,
                application: &test_application(true),
            })
            .expect("nested route snapshots retain parent params");
    }

    #[test]
    fn ssr_snapshot_rejects_malformed_graph_instance_paths() {
        for path in [
            "root",
            "root/outlet:",
            "root/outlet:main/outlet:a/b",
            "root/outlet:main/key:",
            "outlet:main",
        ] {
            assert!(
                validate_graph_instance_path(path).is_err(),
                "expected {path} to be malformed"
            );
        }
        assert_eq!(
            snapshot_error(|snapshot| {
                snapshot.structure.graphs.insert(
                    "root".into(),
                    SsrGraphStructure {
                        graph_id: "app#Index".into(),
                        branches: vec![],
                        loops: vec![],
                    },
                );
            }),
            "malformed graph instance path root: graph instance path requires an /outlet: segment"
        );
    }

    #[test]
    fn graph_instance_paths_follow_the_runtime_grammar() {
        assert!(validate_graph_instance_path(ROOT_GRAPH_INSTANCE_ID).is_ok());
        assert!(validate_graph_instance_path("root/outlet:main/outlet:main/key:todos").is_ok());
        assert!(
            validate_graph_instance_path("root%2Foutlet:main/outlet:rows/key:todo%2F1").is_ok()
        );
    }

    #[test]
    fn nested_component_paths_follow_the_marker_grammar() {
        for path in [
            "root/component:1",
            "root/component:1/component:1",
            "root/outlet:main/component:0",
            "root/component:1/loop:2/key:todo%2F1/component:0",
            "root/component:1/loop:2/key:100%25/component:0",
        ] {
            assert!(
                validate_nested_component_path(path).is_ok(),
                "expected {path} to be a valid nested component path"
            );
        }
        for path in [
            "",
            "root",
            "base/component:0",
            "root/row/component:0",
            "root/component:",
            "root/component:x",
            "root/loop:1",
            "root/component:1/key:",
            "root/component:1/key:a%2Fb/x:0/component:0",
            "root/component:1/key:a/b/component:0",
            "root/component:1/key:50%/component:0",
        ] {
            assert!(
                validate_nested_component_path(path).is_err(),
                "expected {path} to be malformed"
            );
        }
    }

    #[test]
    fn ssr_snapshot_validates_nested_component_records() {
        let snapshot = valid_snapshot();
        let manifest = test_manifest();
        let application = test_application(true);
        assert!(snapshot
            .validate(&SsrSnapshotReferences {
                manifest: &manifest,
                application: &application,
            })
            .is_ok());
    }

    #[test]
    fn ssr_snapshot_rejects_malformed_nested_component_paths() {
        assert_eq!(
            snapshot_error(|snapshot| {
                snapshot.structure.nested.insert(
                    "root/wrong:0".into(),
                    SsrGraphStructure {
                        graph_id: "app#Todo".into(),
                        branches: vec![],
                        loops: vec![],
                    },
                );
            }),
            "unknown nested component path root/wrong:0: unknown segment wrong:0"
        );
    }

    #[test]
    fn ssr_snapshot_rejects_unknown_nested_component_graphs() {
        assert_eq!(
            snapshot_error(|snapshot| {
                let record = snapshot
                    .structure
                    .nested
                    .get_mut("root/outlet:main/outlet:main/key:todos/component:1")
                    .unwrap();
                record.graph_id = "app#Ghost".into();
            }),
            "unknown ssr snapshot graph app#Ghost"
        );
    }

    #[test]
    fn ssr_snapshot_rejects_unknown_nested_branch_handles() {
        assert_eq!(
            snapshot_error(|snapshot| {
                let record = snapshot
                    .structure
                    .nested
                    .get_mut("root/outlet:main/outlet:main/key:todos/component:1")
                    .unwrap();
                record.branches = vec![SsrBranchSelection {
                    node: 9,
                    selected: SsrSelectedBranch::Consequent,
                }];
            }),
            "branch selection for root/outlet:main/outlet:main/key:todos/component:1 references unknown node handle 9"
        );
    }

    #[test]
    fn ssr_snapshot_rejects_duplicate_nested_loop_keys() {
        assert_eq!(
            snapshot_error(|snapshot| {
                let record = snapshot
                    .structure
                    .nested
                    .get_mut("root/outlet:main/outlet:main/key:todos/component:1")
                    .unwrap();
                record.loops = vec![SsrLoopRows {
                    node: 2,
                    keys: vec!["t3".into(), "t3".into()],
                }];
            }),
            "loop at root/outlet:main/outlet:main/key:todos/component:1 node 2 has duplicate key t3"
        );
    }

    #[test]
    fn ssr_snapshot_round_trips_nested_records() {
        let json = serde_json::to_value(valid_snapshot()).unwrap();
        assert_eq!(
            json["structure"]["nested"]["root/outlet:main/outlet:main/key:todos/component:1"]
                ["graphId"],
            "app#Todo"
        );
        assert_eq!(
            serde_json::from_value::<PlecSsrSnapshot>(json).unwrap(),
            valid_snapshot()
        );
    }

    #[test]
    fn ssr_snapshot_rejects_a_missing_root_graph_instance() {
        assert_eq!(
            snapshot_error(|snapshot| {
                snapshot.structure.graphs.remove(ROOT_GRAPH_INSTANCE_ID);
            }),
            "ssr snapshot structure requires the root graph instance"
        );
    }

    #[test]
    fn ssr_snapshot_rejects_an_unclaimed_parent_instance() {
        assert_eq!(
            snapshot_error(|snapshot| {
                snapshot.structure.graphs.insert(
                    "root/outlet:side/outlet:rows".into(),
                    SsrGraphStructure {
                        graph_id: "app#Index".into(),
                        branches: vec![],
                        loops: vec![],
                    },
                );
            }),
            "graph instance root/outlet:side/outlet:rows references unclaimed parent root/outlet:side"
        );
    }

    #[test]
    fn ssr_snapshot_accepts_an_escaped_parent_instance_segment() {
        // Runtime instance ids embed their parent segment escaped
        // (`root%2Foutlet:main/outlet:main`); the claimed parent is the
        // same instance stored under its decoded key.
        let mut snapshot = valid_snapshot();
        let child = snapshot
            .structure
            .graphs
            .remove("root/outlet:main/outlet:main/key:todos")
            .unwrap();
        snapshot
            .structure
            .graphs
            .insert("root%2Foutlet:main/outlet:main/key:todos".into(), child);
        let manifest = test_manifest();
        let application = test_application(true);
        assert!(
            snapshot
                .validate(&SsrSnapshotReferences {
                    manifest: &manifest,
                    application: &application,
                })
                .is_ok(),
            "escaped parent segments must chain to their decoded instance key"
        );
    }

    #[test]
    fn ssr_snapshot_rejects_unknown_graph_ids() {
        assert_eq!(
            snapshot_error(|snapshot| {
                snapshot
                    .structure
                    .graphs
                    .get_mut("root/outlet:main/outlet:main/key:todos")
                    .unwrap()
                    .graph_id = "app#Ghost".into();
            }),
            "unknown ssr snapshot graph app#Ghost"
        );
    }

    #[test]
    fn ssr_snapshot_rejects_branch_selections_that_are_not_conditionals() {
        assert_eq!(
            snapshot_error(|snapshot| {
                snapshot
                    .structure
                    .graphs
                    .get_mut("root/outlet:main/outlet:main/key:todos")
                    .unwrap()
                    .branches = vec![SsrBranchSelection {
                    node: 1,
                    selected: SsrSelectedBranch::Consequent,
                }];
            }),
            "branch selection for root/outlet:main/outlet:main/key:todos names node 1 which is not a conditional"
        );
    }

    #[test]
    fn ssr_snapshot_rejects_unknown_branch_node_handles() {
        assert_eq!(
            snapshot_error(|snapshot| {
                snapshot
                    .structure
                    .graphs
                    .get_mut("root/outlet:main/outlet:main/key:todos")
                    .unwrap()
                    .branches = vec![SsrBranchSelection {
                    node: 9,
                    selected: SsrSelectedBranch::Consequent,
                }];
            }),
            "branch selection for root/outlet:main/outlet:main/key:todos references unknown node handle 9"
        );
    }

    #[test]
    fn ssr_snapshot_rejects_alternate_selection_without_an_alternate() {
        let mut snapshot = valid_snapshot();
        snapshot
            .structure
            .graphs
            .get_mut("root/outlet:main/outlet:main/key:todos")
            .unwrap()
            .branches[0]
            .selected = SsrSelectedBranch::Alternate;
        let manifest = test_manifest();
        let application = test_application(false);
        assert_eq!(
            snapshot
                .validate(&SsrSnapshotReferences {
                    manifest: &manifest,
                    application: &application,
                })
                .unwrap_err(),
            "branch selection for root/outlet:main/outlet:main/key:todos names an alternate that node 0 does not define"
        );
    }

    #[test]
    fn ssr_snapshot_rejects_duplicate_and_unordered_branch_selections() {
        assert!(snapshot_error(|snapshot| {
            snapshot
                .structure
                .graphs
                .get_mut("root/outlet:main/outlet:main/key:todos")
                .unwrap()
                .branches = vec![
                SsrBranchSelection {
                    node: 0,
                    selected: SsrSelectedBranch::Consequent,
                },
                SsrBranchSelection {
                    node: 0,
                    selected: SsrSelectedBranch::None,
                },
            ];
        })
        .contains("must be ordered by node handle without duplicates"));
    }

    #[test]
    fn ssr_snapshot_rejects_loop_records_that_are_not_loops() {
        assert_eq!(
            snapshot_error(|snapshot| {
                snapshot
                    .structure
                    .graphs
                    .get_mut("root/outlet:main/outlet:main/key:todos")
                    .unwrap()
                    .loops = vec![SsrLoopRows {
                    node: 0,
                    keys: vec![],
                }];
            }),
            "loop record for root/outlet:main/outlet:main/key:todos names node 0 which is not a loop"
        );
    }

    #[test]
    fn ssr_snapshot_rejects_duplicate_loop_keys() {
        assert_eq!(
            snapshot_error(|snapshot| {
                snapshot
                    .structure
                    .graphs
                    .get_mut("root/outlet:main/outlet:main/key:todos")
                    .unwrap()
                    .loops = vec![SsrLoopRows {
                    node: 2,
                    keys: vec!["t1".into(), "t1".into()],
                }];
            }),
            "loop at root/outlet:main/outlet:main/key:todos node 2 has duplicate key t1"
        );
    }

    #[test]
    fn ssr_snapshot_rejects_unknown_loader_references() {
        assert_eq!(
            snapshot_error(|snapshot| snapshot.loaders[0].graph_id = "app#Ghost".into()),
            "unknown ssr snapshot loader app#Ghost#action:0"
        );
        assert_eq!(
            snapshot_error(|snapshot| snapshot.loaders[0].action = 5),
            "unknown ssr snapshot loader app#Todo#action:5"
        );
        assert_eq!(
            snapshot_error(|snapshot| snapshot.loaders[0].graph_id = "app#Index".into()),
            "unknown ssr snapshot loader app#Index#action:0"
        );
    }

    #[test]
    fn ssr_snapshot_rejects_duplicate_loader_outcomes() {
        assert_eq!(
            snapshot_error(|snapshot| snapshot.loaders.push(snapshot.loaders[0].clone())),
            "duplicate ssr snapshot loader app#Todo#action:0"
        );
    }

    #[test]
    fn ssr_snapshot_rejects_loader_rejections_without_messages() {
        assert_eq!(
            snapshot_error(|snapshot| snapshot.loaders[0].state =
                SsrLoaderState::Rejected { message: "".into() }),
            "loader app#Todo#action:0 rejection requires a message"
        );
    }

    #[test]
    fn ssr_snapshot_rejects_exports_that_fail_the_public_boundary() {
        assert_eq!(
            snapshot_error(|snapshot| {
                snapshot
                    .public
                    .exports
                    .get_mut("todoCount")
                    .unwrap()
                    .declaration
                    .source_owner = ExecutionOwner::Client;
            }),
            "public export todoCount rejected: client values cannot be server exports"
        );
        assert_eq!(
            snapshot_error(|snapshot| {
                snapshot
                    .public
                    .exports
                    .get_mut("todoCount")
                    .unwrap()
                    .declaration
                    .value_is_serializable = false;
            }),
            "public export todoCount rejected: public export must be serializable"
        );
        // A cookie- or header-derived value (serializable, server-owned) still
        // requires the explicit boundary.
        assert_eq!(
            snapshot_error(|snapshot| {
                snapshot.public.exports.get_mut("todoCount").unwrap().declaration.explicitly_public = false;
            }),
            "public export todoCount rejected: server value requires an explicit public export boundary"
        );
        assert_eq!(
            snapshot_error(|snapshot| {
                snapshot
                    .public
                    .exports
                    .get_mut("todoCount")
                    .unwrap()
                    .declaration
                    .name = "other".into();
            }),
            "public export declaration name does not match todoCount"
        );
    }

    #[test]
    fn ssr_snapshot_rejects_non_finite_numbers() {
        assert_eq!(
            snapshot_error(|snapshot| {
                snapshot.public.exports.get_mut("todoCount").unwrap().value =
                    SsrSnapshotValue::Number(f64::NAN);
            }),
            "public export todoCount contains a non-finite number"
        );
    }

    #[test]
    fn ssr_snapshot_rejects_relative_locations() {
        assert_eq!(
            snapshot_error(|snapshot| snapshot.public.location = "todos/42".into()),
            "ssr snapshot location must be an absolute path"
        );
    }

    #[test]
    fn ssr_snapshot_rejects_unknown_fields() {
        let mut json = serde_json::to_value(valid_snapshot()).unwrap();
        json.as_object_mut()
            .unwrap()
            .insert("extra".into(), 1.into());
        assert!(serde_json::from_value::<PlecSsrSnapshot>(json).is_err());
    }

    #[test]
    fn loader_refs_derive_from_the_manifest_entry() {
        assert_eq!(loader_ref("app#Todo", 0), "app#Todo#action:0");
        assert_eq!(
            loader_ref("app#Layout#Home", 12),
            "app#Layout#Home#action:12"
        );
    }
}
