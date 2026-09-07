//! SSR execution and bootstrap snapshot construction.

pub(crate) mod render;
pub(crate) mod snapshot;

use std::collections::BTreeMap;

use plec_ir::{SsrSelectedBranch, ROOT_GRAPH_INSTANCE_ID};

use crate::{
    artifact::{ArtifactBundle, Route},
    request::RequestContext,
};

pub(crate) use render::{escape_attribute, escape_html, evaluate, Scope};
pub(crate) use snapshot::bootstrap_payload;

/// Server-only boundary enforcement during one document render. Request
/// cookies can never satisfy `validate_public_export` (not explicitly public,
/// server-owned private state), so the render evaluates them as absent and
/// records each gate for the development-only diagnostic header.
pub(crate) struct Gate {
    pub development: bool,
    pub gated: Vec<String>,
}

/// Recorded execution state for one nested component instance, addressed by
/// its marker path (`SsrStructure.nested` in crates/plec-ir). The compiled
/// component id is the graph reference snapshot validation resolves.
#[derive(Debug, Default, Clone)]
pub(crate) struct NestedRecord {
    pub graph_id: Option<String>,
    /// Selected conditional branches, ordered by node handle.
    pub branches: BTreeMap<usize, SsrSelectedBranch>,
    /// Claimed loop rows, ordered by node handle.
    pub loops: BTreeMap<usize, Vec<String>>,
}

/// Mutable state accumulated across one document render: branch/loop
/// ownership per graph instance, nested component records, and the
/// development gate. `BTreeMap` keeps every snapshot record deterministic by
/// construction (ordered by node handle, ordered by marker path).
pub(crate) struct RenderState {
    pub gate: Option<Gate>,
    pub branches: BTreeMap<String, BTreeMap<usize, SsrSelectedBranch>>,
    pub loops: BTreeMap<String, BTreeMap<usize, Vec<String>>>,
    pub nested: BTreeMap<String, NestedRecord>,
}

impl RenderState {
    /// The record-free state a route loader evaluates in: no gate, no
    /// structural records.
    pub(crate) fn bare() -> Self {
        Self {
            gate: None,
            branches: BTreeMap::new(),
            loops: BTreeMap::new(),
            nested: BTreeMap::new(),
        }
    }
}

/// One completed document render: markup, the structural ownership records
/// the snapshot embeds, the route child graph it rendered, and the gated
/// host loads observed.
pub(crate) struct RenderedApplication {
    pub body: String,
    pub branches: BTreeMap<String, BTreeMap<usize, SsrSelectedBranch>>,
    pub loops: BTreeMap<String, BTreeMap<usize, Vec<String>>>,
    pub nested: BTreeMap<String, NestedRecord>,
    pub child_graph: Option<ChildGraph>,
    pub gating: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ChildGraph {
    pub instance: String,
    pub graph_id: String,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum RenderError {
    #[error("root graph missing")]
    RootGraphMissing,
    #[error("missing component {0}")]
    MissingComponent(usize),
    #[error("missing node {0}:{1}")]
    MissingNode(usize, usize),
    #[error("missing text {0}:{1}")]
    MissingText(usize, usize),
    #[error("missing binding {0}:{1}")]
    MissingBinding(usize, usize),
    #[error("missing string {0}:{1}")]
    MissingString(usize, usize),
    #[error("missing loop {0}:{1}")]
    MissingLoop(usize, usize),
    #[error("LOOP_SOURCE_NOT_ARRAY")]
    LoopSourceNotArray,
    #[error("LOOP_ROW_NOT_OBJECT")]
    LoopRowNotObject,
    #[error("DUPLICATE_LOOP_KEY:{0}")]
    DuplicateLoopKey(String),
    #[error("component {0} loop at node {1} exceeds the snapshot loop key limit")]
    LoopKeyLimit(usize, usize),
    #[error("RESERVED_ATTRIBUTE:{0}")]
    ReservedAttribute(String),
    #[error("UNSAFE_ATTRIBUTE:{0}")]
    UnsafeAttribute(String),
    #[error("UNSAFE_URL_ATTRIBUTE:{0}")]
    UnsafeUrlAttribute(String),
    #[error("UNSAFE_TAG:{0}")]
    UnsafeTag(String),
    #[error("SSR dynamic component is unavailable at {0}:{1}")]
    DynamicComponentUnavailable(String, usize),
}

impl From<RenderError> for crate::ServerError {
    fn from(error: RenderError) -> Self {
        crate::ServerError::Other(error.to_string())
    }
}

/// SSR consumes exactly the executable component graph. It renders the root
/// graph, the matched route's child graph into the root outlet (or the
/// route's error graph when a loader rejected), and records the structural
/// ownership the bootstrap snapshot transfers.
pub(crate) fn render_application(
    bundle: &ArtifactBundle,
    route: Option<&Route>,
    request: &RequestContext,
    loader: Option<&plec_ir::SsrLoaderOutcome>,
    development: bool,
) -> Result<RenderedApplication, RenderError> {
    let root = bundle
        .graphs
        .iter()
        .find(|entry| entry.graph_id == bundle.manifest.root_graph_id)
        .map(|entry| &entry.graph)
        .ok_or(RenderError::RootGraphMissing)?;
    // A rejected loader rendered the route's error phase, so the outlet child
    // is the error graph the browser will resume into.
    let route_graph_id = match (loader, route) {
        (Some(loader), Some(route))
            if matches!(loader.state, plec_ir::SsrLoaderState::Rejected { .. }) =>
        {
            route.error_graph_id.as_ref().or(Some(&route.graph_id))
        }
        (_, Some(route)) => Some(&route.graph_id),
        (_, None) => None,
    };
    let child = route_graph_id.and_then(|graph_id| {
        bundle
            .graphs
            .iter()
            .find(|entry| &entry.graph_id == graph_id)
            .map(|entry| &entry.graph)
    });
    // Instance ids mirror graph_instance_id (crates/plec-runtime): the root
    // graph always mounts at `root/outlet:main`; a route child composes its
    // instance from the escaped parent instance and the outlet id. Branch
    // records are keyed by these ids so the adopter finds its ownership cause
    // verbatim.
    let child_graph = child.zip(route).map(|(_, route)| ChildGraph {
        instance: format!(
            "{}/outlet:{}",
            escape_instance_segment(ROOT_GRAPH_INSTANCE_ID),
            escape_instance_segment(&route.outlet_id)
        ),
        graph_id: route_graph_id
            .expect("child graph implies a graph id")
            .clone(),
    });
    let mut state = RenderState {
        gate: Some(Gate {
            development,
            gated: Vec::new(),
        }),
        branches: BTreeMap::from([(ROOT_GRAPH_INSTANCE_ID.to_owned(), BTreeMap::new())]),
        loops: BTreeMap::from([(ROOT_GRAPH_INSTANCE_ID.to_owned(), BTreeMap::new())]),
        nested: BTreeMap::new(),
    };
    if let Some(child_graph) = &child_graph {
        state
            .branches
            .insert(child_graph.instance.clone(), BTreeMap::new());
        state
            .loops
            .insert(child_graph.instance.clone(), BTreeMap::new());
    }
    let scope = Scope {
        request,
        outlet: child,
        path: "root".to_owned(),
        props: Vec::new(),
        component_props: std::collections::HashMap::new(),
        states: Vec::new(),
        frame: Vec::new(),
        row: None,
        row_key: None,
        row_root: false,
        slot: None,
        loader_data: match loader {
            Some(plec_ir::SsrLoaderOutcome {
                state: plec_ir::SsrLoaderState::Resolved { value },
                ..
            }) => crate::loader::snapshot_value_to_json(value),
            _ => serde_json::Value::Null,
        },
        instance: ROOT_GRAPH_INSTANCE_ID.to_owned(),
        root_component: root.root_component,
        nested_key: None,
        nested_graph_id: None,
    };
    let body = render::render_component(root, root.root_component, &scope, &mut state)?;
    Ok(RenderedApplication {
        body,
        branches: state.branches,
        loops: state.loops,
        nested: state.nested,
        child_graph,
        gating: state.gate.map(|gate| gate.gated).unwrap_or_default(),
    })
}

/// The bare render state a route loader evaluates in: no gate, no records.
pub(crate) fn loader_scope(context: &RequestContext) -> Scope<'_> {
    Scope::for_loader(context)
}

/// One instance-id segment (`graph_instance_id` escapes `/` as `%2F` and
/// `%` as `%25`), so nested snapshot keys match runtime instance ids.
pub(crate) fn escape_instance_segment(value: &str) -> String {
    value.replace('%', "%25").replace('/', "%2F")
}

/// The `URL.search` component of a request URL (`""` when absent), shared by
/// the public-state location and the `location` host load.
pub(crate) fn url_search(url: &str) -> String {
    url.split_once('?')
        .map(|(_, search)| format!("?{search}"))
        .unwrap_or_default()
}
