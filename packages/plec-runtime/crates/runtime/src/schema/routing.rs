use serde::Deserialize;

use crate::schema::app::Action;

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
    pub outlet_id: String,
    #[serde(default)]
    pub loader: Option<Action>,
    #[serde(default)]
    pub loader_state_slot_id: Option<String>,
    #[serde(default)]
    pub loader_action: Option<usize>,
}

#[derive(Clone)]
pub struct RouterState {
    pub manifest: RouteManifest,
    pub root_instance_id: String,
}
