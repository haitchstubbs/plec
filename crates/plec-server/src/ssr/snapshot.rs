//! The v2 bootstrap payload, constructed directly from the `plec-ir` SSR
//! snapshot types. Because the snapshot schema is typed here rather than
//! hand-shaped JSON, the server cannot drift from the contract the WASM
//! runtime imports and validates.

use std::collections::BTreeMap;

use plec_ir::{
    PlecSsrSnapshot, SsrBranchSelection, SsrGraphStructure, SsrLoaderState, SsrLoopRows,
    SsrPublicState, SsrRouteInstance, SsrRoutePhase, SsrSelectedBranch, SsrStructure,
    ROOT_GRAPH_INSTANCE_ID, SSR_SNAPSHOT_VERSION,
};
use serde::Serialize;

use crate::{
    artifact::ArtifactBundle, http::RouteMatch, request::RequestContext, ssr::RenderedApplication,
};

/// The bootstrap wrapper the browser adoption gate reads: `{ version,
/// snapshot }`. It carries the same schema version as the snapshot itself.
const BOOTSTRAP_WRAPPER_VERSION: u32 = SSR_SNAPSHOT_VERSION;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BootstrapPayload {
    version: u32,
    snapshot: PlecSsrSnapshot,
}

/// The payload to embed in the document, or `None` when no route matched:
/// without a matched route there is nothing to resume, so no bootstrap is
/// emitted and the browser mounts fresh.
pub(crate) fn bootstrap_payload(
    bundle: &ArtifactBundle,
    route_match: Option<&RouteMatch<'_>>,
    context: &RequestContext,
    loader: Option<&plec_ir::SsrLoaderOutcome>,
    rendered: &RenderedApplication,
) -> Option<BootstrapPayload> {
    let route_match = route_match?;
    // A rejected loader rendered the error phase; the browser resumes that
    // phase from the snapshot instead of refetching on first paint.
    let phase = if matches!(
        loader.map(|loader| &loader.state),
        Some(SsrLoaderState::Rejected { .. })
    ) {
        SsrRoutePhase::Error
    } else {
        SsrRoutePhase::Active
    };
    let mut graphs = BTreeMap::new();
    graphs.insert(
        ROOT_GRAPH_INSTANCE_ID.to_owned(),
        graph_structure(
            ROOT_GRAPH_INSTANCE_ID,
            &bundle.manifest.root_graph_id,
            rendered,
        ),
    );
    if let Some(child) = &rendered.child_graph {
        graphs.insert(
            child.instance.clone(),
            graph_structure(&child.instance, &child.graph_id, rendered),
        );
    }
    // Nested component records are the ownership cause for component
    // instances below the graph root, keyed by the exact marker path the
    // client adopter derives. Deterministic order: sorted by path (BTreeMap),
    // node handle within a record. Records without observed structure or a
    // compiled id carry nothing adoptable and are omitted.
    let nested: BTreeMap<String, SsrGraphStructure> = rendered
        .nested
        .iter()
        .filter_map(|(path, record)| {
            let graph_id = record.graph_id.as_ref()?;
            let branches = branch_records(&record.branches);
            let loops = loop_records(&record.loops);
            if branches.is_empty() && loops.is_empty() {
                return None;
            }
            Some((
                path.clone(),
                SsrGraphStructure {
                    graph_id: graph_id.clone(),
                    branches,
                    loops,
                },
            ))
        })
        .collect();
    Some(BootstrapPayload {
        version: BOOTSTRAP_WRAPPER_VERSION,
        snapshot: PlecSsrSnapshot {
            version: SSR_SNAPSHOT_VERSION,
            revision: bundle.manifest.revision.clone(),
            routes: vec![SsrRouteInstance {
                route_id: route_match.route.id.clone(),
                params: route_match
                    .params
                    .iter()
                    .map(|(name, value)| (name.clone(), value.clone()))
                    .collect(),
                phase,
            }],
            // Public request state: the location is public by contract and
            // cookies are gated at the render boundary. `exports` holds
            // explicitly public values only (PublicExport /
            // validate_public_export in crates/plec-ir); loader outcomes
            // transfer through `loaders` instead.
            public: SsrPublicState {
                location: format!("{}{}", context.pathname, super::url_search(&context.url)),
                exports: BTreeMap::new(),
            },
            loaders: loader
                .cloned()
                .map(|loader| vec![loader])
                .unwrap_or_default(),
            structure: SsrStructure { graphs, nested },
        },
    })
}

/// Branch records are the structural ownership cause for conditional
/// adoption: `branches[nodeHandle] = which side the server instantiated`,
/// ordered by node handle exactly as snapshot validation requires.
fn branch_records(branches: &BTreeMap<usize, SsrSelectedBranch>) -> Vec<SsrBranchSelection> {
    branches
        .iter()
        .map(|(&node, &selected)| SsrBranchSelection { node, selected })
        .collect()
}

fn loop_records(loops: &BTreeMap<usize, Vec<String>>) -> Vec<SsrLoopRows> {
    loops
        .iter()
        .map(|(&node, keys)| SsrLoopRows {
            node,
            keys: keys.clone(),
        })
        .collect()
}

fn graph_structure(
    instance: &str,
    graph_id: &str,
    rendered: &RenderedApplication,
) -> SsrGraphStructure {
    SsrGraphStructure {
        graph_id: graph_id.to_owned(),
        branches: rendered
            .branches
            .get(instance)
            .map(branch_records)
            .unwrap_or_default(),
        loops: rendered
            .loops
            .get(instance)
            .map(loop_records)
            .unwrap_or_default(),
    }
}
