//! Handwritten decoder for the Rust-authoritative SSR snapshot schema.
//! Snapshot structs remain defined/validated by `plec-ir`; this module is the
//! browser's direct `JsValue` projection into those exact types.

use crate::js_decode as js;
use js_sys::Object;
use std::collections::BTreeMap;
use wasm_bindgen::{JsCast, JsValue};

fn required<T>(
    o: &mut js::ObjectDecoder<'_>,
    name: &str,
    f: impl FnOnce(&JsValue) -> Result<T, JsValue>,
) -> Result<T, JsValue> {
    f(&o.get(name)?)
}
fn optional<T>(
    o: &mut js::ObjectDecoder<'_>,
    name: &str,
    f: impl FnOnce(&JsValue) -> Result<T, JsValue>,
) -> Result<Option<T>, JsValue> {
    o.optional(name)?.map(|v| f(&v)).transpose()
}
fn array<T>(v: &JsValue, f: impl Fn(&JsValue) -> Result<T, JsValue>) -> Result<Vec<T>, JsValue> {
    js::array(v)?.iter().map(|item| f(&item)).collect()
}
fn string_map<T>(
    v: &JsValue,
    f: impl Fn(&JsValue) -> Result<T, JsValue>,
) -> Result<BTreeMap<String, T>, JsValue> {
    if !v.is_object() || v.is_null() || v.is_instance_of::<js_sys::Array>() {
        return Err(js::type_error("map"));
    }
    let keys = Object::keys(v.unchecked_ref::<Object>());
    let mut out = BTreeMap::new();
    for key in keys.iter() {
        let name = js::string(&key)?;
        let value = js_sys::Reflect::get(v, &key)?;
        out.insert(name, f(&value)?);
    }
    Ok(out)
}

pub fn decode_snapshot(v: &JsValue) -> Result<plec_ir::PlecSsrSnapshot, JsValue> {
    let mut o = js::ObjectDecoder::new(v)?;
    let version = if o.has("version") {
        js::u32(&o.get("version")?)?
    } else {
        0
    };
    let revision = if o.has("revision") {
        js::string(&o.get("revision")?)?
    } else {
        String::new()
    };
    let routes = required(&mut o, "routes", |v| array(v, decode_route_instance))?;
    let public = required(&mut o, "public", decode_public_state)?;
    let loaders = if o.has("loaders") {
        array(&o.get("loaders")?, decode_loader_outcome)?
    } else {
        Vec::new()
    };
    let structure = required(&mut o, "structure", decode_structure)?;
    o.reject_unknown()?;
    Ok(plec_ir::PlecSsrSnapshot {
        version,
        revision,
        routes,
        public,
        loaders,
        structure,
    })
}

fn decode_route_instance(v: &JsValue) -> Result<plec_ir::SsrRouteInstance, JsValue> {
    let mut o = js::ObjectDecoder::new(v)?;
    let route_id = required(&mut o, "routeId", js::string)?;
    let params = if o.has("params") {
        string_map(&o.get("params")?, js::string)?
    } else {
        BTreeMap::new()
    };
    let phase = if o.has("phase") {
        decode_phase(&o.get("phase")?)?
    } else {
        plec_ir::SsrRoutePhase::Active
    };
    o.reject_unknown()?;
    Ok(plec_ir::SsrRouteInstance {
        route_id,
        params,
        phase,
    })
}
fn decode_phase(v: &JsValue) -> Result<plec_ir::SsrRoutePhase, JsValue> {
    match js::string(v)?.as_str() {
        "active" => Ok(plec_ir::SsrRoutePhase::Active),
        "pending" => Ok(plec_ir::SsrRoutePhase::Pending),
        "error" => Ok(plec_ir::SsrRoutePhase::Error),
        "notFound" => Ok(plec_ir::SsrRoutePhase::NotFound),
        _ => Err(JsValue::from_str("unknown variant for enum SsrRoutePhase")),
    }
}
fn decode_public_state(v: &JsValue) -> Result<plec_ir::SsrPublicState, JsValue> {
    let mut o = js::ObjectDecoder::new(v)?;
    let location = required(&mut o, "location", js::string)?;
    let exports = if o.has("exports") {
        string_map(&o.get("exports")?, decode_public_export)?
    } else {
        BTreeMap::new()
    };
    o.reject_unknown()?;
    Ok(plec_ir::SsrPublicState { location, exports })
}
fn decode_public_export(v: &JsValue) -> Result<plec_ir::SsrPublicExport, JsValue> {
    let mut o = js::ObjectDecoder::new(v)?;
    let value = required(&mut o, "value", decode_snapshot_value)?;
    let declaration = required(&mut o, "declaration", decode_public_export_declaration)?;
    o.reject_unknown()?;
    Ok(plec_ir::SsrPublicExport { value, declaration })
}
fn decode_public_export_declaration(v: &JsValue) -> Result<plec_ir::PublicExport, JsValue> {
    let mut o = js::ObjectDecoder::new(v)?;
    let name = required(&mut o, "name", js::string)?;
    let source_owner = match js::string(&o.get("sourceOwner")?)?.as_str() {
        "shared" => plec_ir::ExecutionOwner::Shared,
        "server" => plec_ir::ExecutionOwner::Server,
        "client" => plec_ir::ExecutionOwner::Client,
        _ => return Err(JsValue::from_str("unknown variant for enum ExecutionOwner")),
    };
    let value_is_serializable = required(&mut o, "valueIsSerializable", js::boolean)?;
    let explicitly_public = required(&mut o, "explicitlyPublic", js::boolean)?;
    Ok(plec_ir::PublicExport {
        name,
        source_owner,
        value_is_serializable,
        explicitly_public,
    })
}

fn decode_snapshot_value(v: &JsValue) -> Result<plec_ir::SsrSnapshotValue, JsValue> {
    if v.is_null() || v.is_undefined() {
        return Ok(plec_ir::SsrSnapshotValue::Null);
    }
    if let Some(value) = v.as_bool() {
        return Ok(plec_ir::SsrSnapshotValue::Bool(value));
    }
    if let Some(value) = v.as_f64() {
        return Ok(plec_ir::SsrSnapshotValue::Number(value));
    }
    if let Some(value) = v.as_string() {
        return Ok(plec_ir::SsrSnapshotValue::String(value));
    }
    if v.is_instance_of::<js_sys::Array>() {
        return Ok(plec_ir::SsrSnapshotValue::Array(array(
            v,
            decode_snapshot_value,
        )?));
    }
    if v.is_object() {
        return Ok(plec_ir::SsrSnapshotValue::Record(string_map(
            v,
            decode_snapshot_value,
        )?));
    }
    Err(js::type_error("JSON value"))
}

fn decode_loader_outcome(v: &JsValue) -> Result<plec_ir::SsrLoaderOutcome, JsValue> {
    let mut o = js::ObjectDecoder::new(v)?;
    let graph_id = required(&mut o, "graphId", js::string)?;
    let action = required(&mut o, "action", js::usize)?;
    let state = required(&mut o, "state", decode_loader_state)?;
    o.reject_unknown()?;
    Ok(plec_ir::SsrLoaderOutcome {
        graph_id,
        action,
        state,
    })
}
fn decode_loader_state(v: &JsValue) -> Result<plec_ir::SsrLoaderState, JsValue> {
    let mut o = js::ObjectDecoder::new(v)?;
    let kind = required(&mut o, "kind", js::string)?;
    match kind.as_str() {
        "resolved" => {
            let value = required(&mut o, "value", decode_snapshot_value)?;
            Ok(plec_ir::SsrLoaderState::Resolved { value })
        }
        "rejected" => {
            let failure = match optional(&mut o, "failure", decode_public_failure) {
                Ok(Some(failure)) if failure.validate().is_ok() => failure,
                _ => plec_ir::PublicRouteLoaderFailure::generic(),
            };
            Ok(plec_ir::SsrLoaderState::Rejected { failure })
        }
        "notFound" => Ok(plec_ir::SsrLoaderState::NotFound),
        _ => Err(JsValue::from_str("unknown variant for enum SsrLoaderState")),
    }
}
fn decode_public_failure(v: &JsValue) -> Result<plec_ir::PublicRouteLoaderFailure, JsValue> {
    let mut o = js::ObjectDecoder::new(v)?;
    let kind = match js::string(&o.get("kind")?)?.as_str() {
        "http" => plec_ir::PublicRouteLoaderFailureKind::Http,
        "network" => plec_ir::PublicRouteLoaderFailureKind::Network,
        "abort" => plec_ir::PublicRouteLoaderFailureKind::Abort,
        "decode" => plec_ir::PublicRouteLoaderFailureKind::Decode,
        "runtime" => plec_ir::PublicRouteLoaderFailureKind::Runtime,
        _ => {
            return Err(JsValue::from_str(
                "unknown variant for enum PublicRouteLoaderFailureKind",
            ))
        }
    };
    let message = js::string(&o.get("message")?)?;
    let status = optional(&mut o, "status", |v| {
        let n = js::u32(v)?;
        u16::try_from(n).map_err(|_| js::type_error("u16"))
    })?;
    let status_text = optional(&mut o, "statusText", js::string)?;
    let body = optional(&mut o, "body", decode_snapshot_value)?;
    let url = optional(&mut o, "url", js::string)?;
    o.reject_unknown()?;
    Ok(plec_ir::PublicRouteLoaderFailure {
        kind,
        message,
        status,
        status_text,
        body,
        url,
    })
}

fn decode_structure(v: &JsValue) -> Result<plec_ir::SsrStructure, JsValue> {
    let mut o = js::ObjectDecoder::new(v)?;
    let graphs = if o.has("graphs") {
        string_map(&o.get("graphs")?, decode_graph_structure)?
    } else {
        BTreeMap::new()
    };
    let nested = if o.has("nested") {
        string_map(&o.get("nested")?, decode_graph_structure)?
    } else {
        BTreeMap::new()
    };
    o.reject_unknown()?;
    Ok(plec_ir::SsrStructure { graphs, nested })
}
fn decode_graph_structure(v: &JsValue) -> Result<plec_ir::SsrGraphStructure, JsValue> {
    let mut o = js::ObjectDecoder::new(v)?;
    let graph_id = required(&mut o, "graphId", js::string)?;
    let branches = if o.has("branches") {
        array(&o.get("branches")?, decode_branch)?
    } else {
        Vec::new()
    };
    let loops = if o.has("loops") {
        array(&o.get("loops")?, decode_loop_rows)?
    } else {
        Vec::new()
    };
    o.reject_unknown()?;
    Ok(plec_ir::SsrGraphStructure {
        graph_id,
        branches,
        loops,
    })
}
fn decode_branch(v: &JsValue) -> Result<plec_ir::SsrBranchSelection, JsValue> {
    let mut o = js::ObjectDecoder::new(v)?;
    let node = required(&mut o, "node", js::usize)?;
    let selected = match js::string(&o.get("selected")?)?.as_str() {
        "consequent" => plec_ir::SsrSelectedBranch::Consequent,
        "alternate" => plec_ir::SsrSelectedBranch::Alternate,
        "none" => plec_ir::SsrSelectedBranch::None,
        _ => {
            return Err(JsValue::from_str(
                "unknown variant for enum SsrSelectedBranch",
            ))
        }
    };
    o.reject_unknown()?;
    Ok(plec_ir::SsrBranchSelection { node, selected })
}
fn decode_loop_rows(v: &JsValue) -> Result<plec_ir::SsrLoopRows, JsValue> {
    let mut o = js::ObjectDecoder::new(v)?;
    let node = required(&mut o, "node", js::usize)?;
    let keys = required(&mut o, "keys", |v| array(v, js::string))?;
    o.reject_unknown()?;
    Ok(plec_ir::SsrLoopRows { node, keys })
}
