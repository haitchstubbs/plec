use std::collections::HashMap;

#[cfg(feature = "serde")]
use serde::Deserialize;
use wasm_bindgen::JsValue;

/// Decode the runtime's reduced manifest view directly from the normalized
/// browser value. This mirrors the Rust fields and Serde defaults above.
pub fn decode_runtime_manifest(value: &JsValue) -> Result<RouteManifest, JsValue> {
    use crate::js_decode as js;
    let mut object = js::ObjectDecoder::new(value)?;
    let version = object
        .optional("version")?
        .map(|value| js::u32(&value))
        .transpose()?;
    let root_graph_id = js::string(&object.get("rootGraphId")?)?;
    let root_not_found_graph_id = object
        .optional("rootNotFoundGraphId")?
        .map(|value| js::string(&value))
        .transpose()?;
    let routes = js::array(&object.get("routes")?)?
        .iter()
        .map(decode_runtime_route)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(RouteManifest {
        version,
        root_graph_id,
        root_not_found_graph_id,
        routes,
    })
}

fn decode_runtime_route(value: &JsValue) -> Result<RouteManifestEntry, JsValue> {
    use crate::js_decode as js;
    let mut object = js::ObjectDecoder::new(value)?;
    let id = js::string(&object.get("id")?)?;
    let parent_id = object
        .optional("parentId")?
        .map(|value| js::string(&value))
        .transpose()?;
    let path = js::string(&object.get("path")?)?;
    let graph_id = js::string(&object.get("graphId")?)?;
    let pending_graph_id = object
        .optional("pendingGraphId")?
        .map(|value| js::string(&value))
        .transpose()?;
    let error_graph_id = object
        .optional("errorGraphId")?
        .map(|value| js::string(&value))
        .transpose()?;
    let not_found_graph_id = object
        .optional("notFoundGraphId")?
        .map(|value| js::string(&value))
        .transpose()?;
    let outlet_id = js::string(&object.get("outletId")?)?;
    let loader_action = object
        .optional("loaderAction")?
        .map(|value| js::usize(&value))
        .transpose()?;
    let dependencies = object
        .optional("dependencies")?
        .map(|value| {
            js::array(&value)?
                .iter()
                .map(decode_identity_template)
                .collect()
        })
        .transpose()?
        .unwrap_or_default();
    let pending_mode = if object.has("pendingMode") {
        js::string(&object.get("pendingMode")?)?
    } else {
        default_pending_mode()
    };
    Ok(RouteManifestEntry {
        id,
        parent_id,
        path,
        graph_id,
        pending_graph_id,
        error_graph_id,
        not_found_graph_id,
        outlet_id,
        loader_action,
        dependencies,
        pending_mode,
    })
}

/// Decode the full IR manifest representation, including its revision and
/// metadata, before applying the authoritative `plec-ir` validator.
pub fn decode_ir_manifest(value: &JsValue) -> Result<plec_ir::RouteManifest, JsValue> {
    use crate::js_decode as js;
    let mut object = js::ObjectDecoder::new(value)?;
    let version = js::u32(&object.get("version")?)?;
    let revision = if object.has("revision") {
        js::string(&object.get("revision")?)?
    } else {
        String::new()
    };
    let root_graph_id = js::string(&object.get("rootGraphId")?)?;
    let root_not_found_graph_id = object
        .optional("rootNotFoundGraphId")?
        .map(|v| js::string(&v))
        .transpose()?;
    let routes = js::array(&object.get("routes")?)?
        .iter()
        .map(decode_ir_route)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(plec_ir::RouteManifest {
        version,
        revision,
        root_graph_id,
        root_not_found_graph_id,
        routes,
    })
}

fn decode_ir_route(value: &JsValue) -> Result<plec_ir::RouteManifestEntry, JsValue> {
    use crate::js_decode as js;
    let mut object = js::ObjectDecoder::new(value)?;
    let id = js::string(&object.get("id")?)?;
    let parent_id = object
        .optional("parentId")?
        .map(|v| js::string(&v))
        .transpose()?;
    let path = js::string(&object.get("path")?)?;
    let graph_id = js::string(&object.get("graphId")?)?;
    let pending_graph_id = object
        .optional("pendingGraphId")?
        .map(|v| js::string(&v))
        .transpose()?;
    let pending_mode = if object.has("pendingMode") {
        js::string(&object.get("pendingMode")?)?
    } else {
        "replace".into()
    };
    let error_graph_id = object
        .optional("errorGraphId")?
        .map(|v| js::string(&v))
        .transpose()?;
    let not_found_graph_id = object
        .optional("notFoundGraphId")?
        .map(|v| js::string(&v))
        .transpose()?;
    let loader_action = object
        .optional("loaderAction")?
        .map(|v| js::usize(&v))
        .transpose()?;
    let dependencies = object
        .optional("dependencies")?
        .map(|value| {
            js::array(&value)?
                .iter()
                .map(decode_ir_identity_template)
                .collect()
        })
        .transpose()?
        .unwrap_or_default();
    let outlet_id = js::string(&object.get("outletId")?)?;
    let meta = object.optional("meta")?.map(decode_metadata).transpose()?;
    Ok(plec_ir::RouteManifestEntry {
        id,
        parent_id,
        path,
        graph_id,
        pending_graph_id,
        pending_mode,
        error_graph_id,
        not_found_graph_id,
        loader_action,
        dependencies,
        outlet_id,
        meta,
    })
}

fn decode_identity_template(value: &JsValue) -> Result<plec_ir::ResourceIdentityTemplate, JsValue> {
    use crate::js_decode as js;
    let mut object = js::ObjectDecoder::new(value)?;
    let parts = js::array(&object.get("parts")?)?
        .iter()
        .map(|part| {
            let mut part_object = js::ObjectDecoder::new(part)?;
            let kind = js::string(&part_object.get("kind")?)?;
            let value = js::string(&part_object.get("value")?)?;
            match kind.as_str() {
                "static" => Ok(plec_ir::ResourceIdentityPart::Static(value)),
                "param" => Ok(plec_ir::ResourceIdentityPart::Param(value)),
                _ => Err(JsValue::from_str("invalid resource identity part")),
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(plec_ir::ResourceIdentityTemplate { parts })
}

fn decode_ir_identity_template(
    value: &JsValue,
) -> Result<plec_ir::ResourceIdentityTemplate, JsValue> {
    decode_identity_template(value)
}

fn decode_metadata(value: JsValue) -> Result<plec_ir::RouteMetadata, JsValue> {
    use crate::js_decode as js;
    let mut object = js::ObjectDecoder::new(&value)?;
    let title = object
        .optional("title")?
        .map(|v| js::string(&v))
        .transpose()?;
    let description = object
        .optional("description")?
        .map(|v| js::string(&v))
        .transpose()?;
    Ok(plec_ir::RouteMetadata { title, description })
}

#[derive(Clone)]
#[cfg_attr(feature = "serde", derive(Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct RouteManifest {
    #[cfg_attr(feature = "serde", serde(default))]
    pub version: Option<u32>,
    pub root_graph_id: String,
    #[cfg_attr(feature = "serde", serde(default))]
    pub root_not_found_graph_id: Option<String>,
    pub routes: Vec<RouteManifestEntry>,
}

#[derive(Clone)]
#[cfg_attr(feature = "serde", derive(Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct RouteManifestEntry {
    pub id: String,
    #[cfg_attr(feature = "serde", serde(default))]
    pub parent_id: Option<String>,
    pub path: String,
    pub graph_id: String,
    pub pending_graph_id: Option<String>,
    pub error_graph_id: Option<String>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub not_found_graph_id: Option<String>,
    pub outlet_id: String,
    pub loader_action: Option<usize>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub dependencies: Vec<plec_ir::ResourceIdentityTemplate>,
    #[cfg_attr(feature = "serde", serde(default = "default_pending_mode"))]
    pub pending_mode: String,
}

/// One entry in the ordered route branch selected for a pathname. Parameters
/// include values inherited from every matched parent.
#[derive(Clone)]
pub struct RouteMatch {
    pub route: RouteManifestEntry,
    pub params: HashMap<String, String>,
}

pub fn resolve_resource_identities(
    templates: &[plec_ir::ResourceIdentityTemplate],
    params: &HashMap<String, String>,
) -> Result<Vec<String>, String> {
    let mut identities = Vec::with_capacity(templates.len());
    for template in templates {
        let mut identity = String::new();
        for part in &template.parts {
            match part {
                plec_ir::ResourceIdentityPart::Static(value) => identity.push_str(value),
                plec_ir::ResourceIdentityPart::Param(name) => {
                    identity.push_str(params.get(name).ok_or_else(|| {
                        "resource identity references a missing route parameter".to_owned()
                    })?)
                }
            }
            if identity.len() > plec_ir::limits::MAX_RESOURCE_IDENTITY_BYTES {
                return Err("resolved resource identity exceeds 256 UTF-8 bytes".into());
            }
        }
        identities.push(identity);
    }
    Ok(identities)
}

/// Resolves a pathname against the compiled route tree. Browser navigation and
/// native SSR share this function so route semantics have one Rust authority.
pub fn match_route_chain(manifest: &RouteManifest, pathname: &str) -> Option<Vec<RouteMatch>> {
    let parts = pathname
        .trim_matches('/')
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    match_route_chain_from(manifest, None, &parts, 0, HashMap::new())
}

fn match_route_chain_from(
    manifest: &RouteManifest,
    parent: Option<&str>,
    parts: &[&str],
    offset: usize,
    params: HashMap<String, String>,
) -> Option<Vec<RouteMatch>> {
    let mut candidates = manifest
        .routes
        .iter()
        .filter_map(|route| {
            (route.parent_id.as_deref() == parent && !route.path.is_empty() && route.path != "*")
                .then(|| match_route_entry(route, parts, offset, &params))
                .flatten()
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|(static_segments, _, _)| std::cmp::Reverse(*static_segments));
    for (_, route, next_params) in candidates {
        let next_offset = offset + route_segments(&route.path).len();
        let mut branch = vec![RouteMatch {
            route: route.clone(),
            params: next_params.clone(),
        }];
        if next_offset < parts.len() {
            if let Some(mut child) =
                match_route_chain_from(manifest, Some(&route.id), parts, next_offset, next_params)
            {
                branch.append(&mut child);
                return Some(branch);
            }
        } else if let Some(mut child) =
            match_route_chain_from(manifest, Some(&route.id), parts, next_offset, next_params)
        {
            branch.append(&mut child);
            return Some(branch);
        } else {
            return Some(branch);
        }
    }
    if offset == parts.len() {
        if let Some(route) = manifest
            .routes
            .iter()
            .find(|route| route.parent_id.as_deref() == parent && route.path.is_empty())
        {
            let mut branch = vec![RouteMatch {
                route: route.clone(),
                params: params.clone(),
            }];
            if let Some(mut child) =
                match_route_chain_from(manifest, Some(&route.id), parts, offset, params)
            {
                branch.append(&mut child);
            }
            return Some(branch);
        }
    }
    manifest
        .routes
        .iter()
        .find(|route| route.parent_id.as_deref() == parent && route.path == "*")
        .map(|route| {
            vec![RouteMatch {
                route: route.clone(),
                params,
            }]
        })
}

fn match_route_entry(
    route: &RouteManifestEntry,
    parts: &[&str],
    offset: usize,
    params: &HashMap<String, String>,
) -> Option<(usize, RouteManifestEntry, HashMap<String, String>)> {
    let segments = route_segments(&route.path);
    if segments.len() > parts.len().saturating_sub(offset) {
        return None;
    }
    let mut params = params.clone();
    let mut static_segments = 0;
    for (index, segment) in segments.iter().enumerate() {
        let value = parts[offset + index];
        if let Some(name) = segment.strip_prefix('$') {
            params.insert(name.into(), decode_path_segment(value));
        } else if *segment == value {
            static_segments += 1;
        } else {
            return None;
        }
    }
    Some((static_segments, route.clone(), params))
}

fn route_segments(path: &str) -> Vec<&str> {
    path.trim_matches('/')
        .split('/')
        .filter(|part| !part.is_empty())
        .collect()
}

fn decode_path_segment(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2])) {
                output.push(high * 16 + low);
                index += 3;
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(output).unwrap_or_else(|_| value.into())
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(id: &str, parent_id: Option<&str>, path: &str) -> RouteManifestEntry {
        RouteManifestEntry {
            id: id.into(),
            parent_id: parent_id.map(str::to_owned),
            path: path.into(),
            graph_id: id.into(),
            pending_graph_id: None,
            error_graph_id: None,
            not_found_graph_id: None,
            outlet_id: "main".into(),
            loader_action: None,
            dependencies: vec![],
            pending_mode: "replace".into(),
        }
    }

    #[test]
    fn resource_identity_resolution_uses_decoded_route_params_verbatim() {
        let matched = match_route_chain(
            &RouteManifest {
                version: Some(3),
                root_graph_id: "root".into(),
                root_not_found_graph_id: None,
                routes: vec![RouteManifestEntry {
                    id: "project".into(),
                    parent_id: None,
                    path: "projects/$id".into(),
                    graph_id: "project".into(),
                    pending_graph_id: None,
                    error_graph_id: None,
                    not_found_graph_id: None,
                    outlet_id: "main".into(),
                    loader_action: Some(0),
                    dependencies: vec![plec_ir::ResourceIdentityTemplate {
                        parts: vec![
                            plec_ir::ResourceIdentityPart::Static("project:".into()),
                            plec_ir::ResourceIdentityPart::Param("id".into()),
                        ],
                    }],
                    pending_mode: "replace".into(),
                }],
            },
            "/projects/a%2Fb",
        )
        .unwrap();
        let identities =
            resolve_resource_identities(&matched[0].route.dependencies, &matched[0].params)
                .unwrap();
        assert_eq!(identities, vec!["project:a/b"]);
        assert!(resolve_resource_identities(
            &[plec_ir::ResourceIdentityTemplate {
                parts: vec![plec_ir::ResourceIdentityPart::Param("id".into())]
            }],
            &HashMap::from([("id".into(), "x".repeat(257))]),
        )
        .is_err());
    }

    #[test]
    fn matches_nested_routes_with_accumulated_params_and_static_precedence() {
        let manifest = RouteManifest {
            version: Some(3),
            root_graph_id: "root".into(),
            root_not_found_graph_id: None,
            routes: vec![
                route("projects", None, "projects"),
                route("project", Some("projects"), "$projectId"),
                route("new", Some("projects"), "new"),
                route("settings", Some("project"), "settings"),
                route("missing", None, "*"),
            ],
        };
        let matched = match_route_chain(&manifest, "/projects/a%20b/settings").unwrap();
        assert_eq!(
            matched
                .iter()
                .map(|matched| matched.route.id.as_str())
                .collect::<Vec<_>>(),
            ["projects", "project", "settings"]
        );
        assert_eq!(
            matched[2].params.get("projectId").map(String::as_str),
            Some("a b")
        );
        assert_eq!(
            match_route_chain(&manifest, "/projects/new").unwrap()[1]
                .route
                .id,
            "new"
        );
    }

    #[test]
    fn matches_pathless_descent_and_catch_all_with_lenient_segment_decoding() {
        let manifest = RouteManifest {
            version: Some(3),
            root_graph_id: "root".into(),
            root_not_found_graph_id: None,
            routes: vec![
                route("layout", None, ""),
                route("home", Some("layout"), ""),
                route("item", None, "$id"),
                route("missing", None, "*"),
            ],
        };
        assert_eq!(
            match_route_chain(&manifest, "/")
                .unwrap()
                .iter()
                .map(|matched| matched.route.id.as_str())
                .collect::<Vec<_>>(),
            ["layout", "home"]
        );
        assert_eq!(
            match_route_chain(&manifest, "/%ZZ").unwrap()[0]
                .params
                .get("id")
                .map(String::as_str),
            Some("%ZZ")
        );
        assert_eq!(
            match_route_chain(&manifest, "/a/b").unwrap()[0].route.id,
            "missing"
        );
    }
}

fn default_pending_mode() -> String {
    "replace".into()
}
