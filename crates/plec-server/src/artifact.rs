//! Deserialization mirror of the compiled route artifact.
//!
//! `plec-ir` owns the canonical artifact schema, but its graph types are
//! serialize-only (`&'static str` fields cannot derive `Deserialize` without
//! rippling `String` through the whole compiler). The runtime already solves
//! this with its own WASM deserialization mirror; this module is the native
//! server's equivalent, mirroring exactly the subset SSR and the loader
//! executor consume. Field names are frozen by `plec-ir`'s `Serialize`
//! output, so drift here breaks loudly in tests rather than silently.

use std::path::Path;

use serde::Deserialize;

use crate::ServerError;

/// A JSON-transportable runtime value. Server-side evaluation deliberately
/// runs over the transport representation so every value it produces is
/// directly embeddable in the snapshot without a second conversion.
pub type JsonValue = serde_json::Value;

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactBundle {
    #[serde(default)]
    pub manifest: Manifest,
    #[serde(default)]
    pub graphs: Vec<GraphEntry>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    #[serde(default)]
    pub revision: String,
    #[serde(default)]
    pub root_graph_id: String,
    #[serde(default)]
    pub routes: Vec<Route>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub graph_id: String,
    #[serde(default)]
    pub outlet_id: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub pending_graph_id: Option<String>,
    #[serde(default)]
    pub error_graph_id: Option<String>,
    #[serde(default)]
    pub loader_action: Option<usize>,
    #[serde(default)]
    pub meta: Option<plec_ir::RouteMetadata>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphEntry {
    #[serde(default)]
    pub graph_id: String,
    #[serde(default)]
    pub graph: ComponentApplication,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentApplication {
    #[serde(default)]
    pub root_component: usize,
    #[serde(default)]
    pub components: Vec<Component>,
}

/// Lets the canonical snapshot validator (`PlecSsrSnapshot::validate`) treat
/// the decoded server mirror exactly like a compile-time application, so
/// tests can prove emitted snapshots pass the contract the WASM runtime
/// enforces. Structure records reference compiled component ids, so
/// resolution walks every graph's component table — instance records name
/// the graph's root component, nested records any component below it.
impl plec_ir::SsrStructureApplication for ArtifactBundle {
    fn structure_graph(&self, graph_id: &str) -> Option<&dyn plec_ir::SsrStructureGraph> {
        self.graphs
            .iter()
            .find_map(|entry| entry.graph.structure_graph(graph_id))
    }
}

impl plec_ir::SsrStructureApplication for ComponentApplication {
    fn structure_graph(&self, graph_id: &str) -> Option<&dyn plec_ir::SsrStructureGraph> {
        let component = self
            .components
            .iter()
            .find(|component| component.id.as_deref() == Some(graph_id))?;
        Some(component)
    }
}

impl plec_ir::SsrStructureGraph for Component {
    fn structure_node(&self, handle: usize) -> Option<plec_ir::SsrStructureNode> {
        Some(match self.nodes.get(handle)? {
            Node::Conditional { alternate, .. } => plec_ir::SsrStructureNode::Conditional {
                has_alternate: alternate.is_some(),
            },
            Node::Loop { .. } => plec_ir::SsrStructureNode::Loop,
            _ => plec_ir::SsrStructureNode::Other,
        })
    }
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Component {
    /// The compiled component id (`module#Name`); nested structure records
    /// reference it so snapshot validation resolves the right node table.
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub root_node: usize,
    #[serde(default)]
    pub strings: Vec<String>,
    #[serde(default)]
    pub constants: Vec<JsonValue>,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub texts: Vec<Text>,
    #[serde(default)]
    pub bindings: Vec<Binding>,
    #[serde(default)]
    pub prop_programs: Vec<PropProgram>,
    #[serde(default)]
    pub host_slots: Vec<HostSlot>,
    #[serde(default)]
    pub state_slots: Vec<StateSlot>,
    #[serde(default)]
    pub parameters: Vec<Parameter>,
    #[serde(default)]
    pub expressions: Vec<ExpressionProgram>,
    /// Loader-action subset only: the server executes route loaders, never
    /// general actions (see `loader::execute_route_loader`).
    #[serde(default)]
    pub actions: Vec<ActionProgram>,
    #[serde(default)]
    pub loops: Vec<Loop>,
    #[serde(default)]
    pub route_outlets: Vec<RouteOutlet>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum Node {
    Element {
        #[serde(default)]
        tag: usize,
        #[serde(default)]
        children: Vec<usize>,
    },
    Text {
        #[serde(default)]
        text: usize,
    },
    Conditional {
        #[serde(default)]
        test: usize,
        #[serde(default)]
        consequent: usize,
        #[serde(default)]
        alternate: Option<usize>,
    },
    Loop {
        #[serde(default)]
        r#loop: usize,
    },
    Component {
        #[serde(default)]
        component: usize,
        #[serde(default)]
        props: Vec<ComponentProp>,
        #[serde(default)]
        children: Vec<usize>,
    },
    /// A component whose graph definition comes from a component-valued prop.
    DynamicComponent {
        #[serde(default)]
        prop: usize,
        #[serde(default)]
        props: Vec<ComponentProp>,
        #[serde(default)]
        children: Vec<usize>,
    },
    /// Insertion range for the implicit `children` prop.
    Slot,
    /// Unknown ops render as nothing, mirroring the TS host's fallthrough.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ComponentProp {
    Value {
        name: usize,
        expression: usize,
    },
    /// Callable props never fire during SSR; they are skipped.
    Callable {
        #[serde(default)]
        name: usize,
        #[serde(default)]
        action: usize,
    },
    Component {
        name: usize,
        #[serde(default)]
        component: usize,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Text {
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub binding: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Binding {
    #[serde(default)]
    pub target: usize,
    #[serde(default)]
    pub sink: String,
    #[serde(default)]
    pub name: Option<usize>,
    #[serde(default)]
    pub expression: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PropProgram {
    #[serde(default)]
    pub target: usize,
    #[serde(default)]
    pub writes: Vec<PropWrite>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PropWrite {
    #[serde(default)]
    pub name: Option<usize>,
    #[serde(default)]
    pub constant: Option<usize>,
    #[serde(default)]
    pub expression: Option<usize>,
    #[serde(default)]
    pub spread: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostSlot {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub name: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateSlot {
    #[serde(default)]
    pub initial_expression: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameter {
    #[serde(default)]
    pub name: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpressionProgram {
    #[serde(default)]
    pub instructions: Vec<ExpressionInstruction>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum ExpressionInstruction {
    Constant {
        #[serde(default)]
        constant: usize,
    },
    LoadState {
        #[serde(default)]
        state: usize,
    },
    LoadFrame {
        #[serde(default)]
        slot: usize,
    },
    LoadProp {
        #[serde(default)]
        prop: usize,
    },
    LoadHost {
        #[serde(default)]
        host: usize,
    },
    LoadRowRecord,
    LoadRowField {
        #[serde(default)]
        field: usize,
    },
    Field {
        #[serde(default)]
        field: usize,
    },
    Unary {
        #[serde(default)]
        kind: String,
    },
    Binary {
        #[serde(default)]
        kind: String,
    },
    MakeArray {
        #[serde(default)]
        count: usize,
        #[serde(default)]
        spreads: Vec<bool>,
    },
    MakeRecord {
        #[serde(default)]
        fields: Vec<usize>,
        #[serde(default)]
        spreads: Vec<bool>,
    },
    Jump {
        target: usize,
    },
    JumpIfFalse {
        target: usize,
    },
    Return,
    /// Unknown instructions are skipped without stack effect, mirroring the
    /// TS host's switch fallthrough. SSR-path expressions never contain them.
    #[serde(other)]
    Unknown,
}

/// The server never runs general action programs; instructions stay as raw
/// JSON so the loader executor can locate the single `fetch` capability
/// request the compiler lowers route loaders into.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionProgram {
    #[serde(default)]
    pub route_loader: bool,
    #[serde(default)]
    pub instructions: Vec<JsonValue>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Loop {
    #[serde(default)]
    pub source_expression: usize,
    #[serde(default)]
    pub key_expression: usize,
    #[serde(default)]
    pub item_slot: usize,
    #[serde(default)]
    pub row_template: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteOutlet {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub node: usize,
}

/// Reads the application artifact under the documented decode ceiling. Unlike
/// the TS host, the read itself is bounded (`take`), so an oversized artifact
/// never gets buffered before rejection.
pub async fn read_bounded(path: &Path) -> Result<ArtifactBundle, ServerError> {
    use plec_ir::limits::MAX_ARTIFACT_JSON_BYTES;
    use tokio::io::AsyncReadExt;

    let file = tokio::fs::File::open(path).await.map_err(|error| {
        ServerError::message(format!("cannot read application artifact: {error}"))
    })?;
    let mut bytes = Vec::new();
    file.take(MAX_ARTIFACT_JSON_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() > MAX_ARTIFACT_JSON_BYTES {
        return Err(ServerError::ArtifactTooLarge);
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| ServerError::message(format!("invalid application artifact: {error}")))
}
