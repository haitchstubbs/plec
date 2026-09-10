//! SSR execution and bootstrap snapshot construction.

pub(crate) mod render;
pub(crate) mod snapshot;

use std::collections::BTreeMap;

use plec_ir::{SsrSelectedBranch, ROOT_GRAPH_INSTANCE_ID};

use crate::{
    artifact::{ArtifactBundle, ComponentApplication, Route},
    http::RouteExecution,
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
    /// The element-tag policy this render serializes under (mirror of the
    /// CSR runtime's policy; forbidden tags stay rejected everywhere).
    pub tag_policy: plec_ir::sink::TagPolicy,
    /// Current `render_node` recursion depth (see
    /// `limits::MAX_SSR_RENDER_DEPTH`): the native render walk must fail
    /// closed before a deep (still acyclic) graph can overflow the stack.
    pub render_depth: usize,
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
            tag_policy: plec_ir::sink::TagPolicy::default(),
            render_depth: 0,
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
    pub child_graphs: Vec<ChildGraph>,
    pub gating: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ChildGraph {
    pub instance: String,
    pub graph_id: String,
}

pub(crate) struct RouteRender<'a> {
    route: &'a Route,
    graph: &'a ComponentApplication,
    graph_id: String,
    loader: Option<&'a plec_ir::SsrLoaderOutcome>,
    instance: String,
    child: Option<Box<RouteRender<'a>>>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum RenderError {
    #[error("root graph missing")]
    RootGraphMissing,
    #[error("route graph missing for {0}")]
    RouteGraphMissing(String),
    #[error("route {0} parent does not declare outlet {1}")]
    RouteOutletMissing(String, String),
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
    #[error("SSR render depth exceeds limit")]
    RenderDepthExceeded,
}

impl From<RenderError> for crate::ServerError {
    fn from(error: RenderError) -> Self {
        crate::ServerError::Other(error.to_string())
    }
}

/// SSR consumes exactly the executable component graph. It renders every
/// matched route graph through its declared parent outlet and records the
/// structural ownership the bootstrap snapshot transfers.
pub(crate) fn render_application(
    bundle: &ArtifactBundle,
    routes: &[RouteExecution<'_>],
    request: &RequestContext,
    tag_policy: &plec_ir::sink::TagPolicy,
    development: bool,
) -> Result<RenderedApplication, RenderError> {
    let root = bundle
        .graphs
        .iter()
        .find(|entry| entry.graph_id == bundle.manifest.root_graph_id)
        .map(|entry| &entry.graph)
        .ok_or(RenderError::RootGraphMissing)?;
    let route_tree = build_route_tree(bundle, routes, 0, ROOT_GRAPH_INSTANCE_ID)?;
    let child_graphs = route_tree.as_deref().map(route_graphs).unwrap_or_default();
    let mut state = RenderState {
        gate: Some(Gate {
            development,
            gated: Vec::new(),
        }),
        branches: BTreeMap::from([(ROOT_GRAPH_INSTANCE_ID.to_owned(), BTreeMap::new())]),
        loops: BTreeMap::from([(ROOT_GRAPH_INSTANCE_ID.to_owned(), BTreeMap::new())]),
        nested: BTreeMap::new(),
        tag_policy: tag_policy.clone(),
        render_depth: 0,
    };
    for child_graph in &child_graphs {
        state
            .branches
            .insert(child_graph.instance.clone(), BTreeMap::new());
        state
            .loops
            .insert(child_graph.instance.clone(), BTreeMap::new());
    }
    let scope = Scope {
        request,
        outlet: route_tree.as_deref(),
        path: "root".to_owned(),
        props: Vec::new(),
        component_props: std::collections::HashMap::new(),
        host_component_props: std::collections::HashMap::new(),
        states: Vec::new(),
        frame: Vec::new(),
        row: None,
        row_key: None,
        row_root: false,
        slot: None,
        loader_data: serde_json::Value::Null,
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
        child_graphs,
        gating: state.gate.map(|gate| gate.gated).unwrap_or_default(),
    })
}

fn build_route_tree<'a>(
    bundle: &'a ArtifactBundle,
    routes: &'a [RouteExecution<'a>],
    index: usize,
    parent_instance: &str,
) -> Result<Option<Box<RouteRender<'a>>>, RenderError> {
    let Some(execution) = routes.get(index) else {
        return Ok(None);
    };
    let route = execution.route_match.route;
    let graph_id = match execution.loader.as_ref().map(|loader| &loader.state) {
        Some(plec_ir::SsrLoaderState::Rejected { .. }) => {
            route.error_graph_id.as_deref().unwrap_or(&route.graph_id)
        }
        _ => &route.graph_id,
    };
    let graph = bundle
        .graphs
        .iter()
        .find(|entry| entry.graph_id == graph_id)
        .map(|entry| &entry.graph)
        .ok_or_else(|| RenderError::RouteGraphMissing(route.id.clone()))?;
    let instance = format!(
        "{}/outlet:{}",
        escape_instance_segment(parent_instance),
        escape_instance_segment(&route.outlet_id)
    );
    Ok(Some(Box::new(RouteRender {
        route,
        graph,
        graph_id: graph_id.to_owned(),
        loader: execution.loader.as_ref(),
        instance: instance.clone(),
        child: build_route_tree(bundle, routes, index + 1, &instance)?,
    })))
}

fn route_graphs(route: &RouteRender<'_>) -> Vec<ChildGraph> {
    let mut graphs = vec![ChildGraph {
        instance: route.instance.clone(),
        graph_id: route.graph_id.clone(),
    }];
    if let Some(child) = route.child.as_deref() {
        graphs.extend(route_graphs(child));
    }
    graphs
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
