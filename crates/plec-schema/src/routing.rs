use serde::Deserialize;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteManifest {
    #[serde(default)]
    pub version: Option<u32>,
    pub root_graph_id: String,
    pub routes: Vec<RouteManifestEntry>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteManifestEntry {
    pub id: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    pub path: String,
    pub graph_id: String,
    pub pending_graph_id: Option<String>,
    pub error_graph_id: Option<String>,
    pub outlet_id: String,
    pub loader_action: Option<usize>,
    #[serde(default = "default_pending_mode")]
    pub pending_mode: String,
}

fn default_pending_mode() -> String {
    "replace".into()
}
