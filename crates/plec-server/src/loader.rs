//! Executes the narrow server loader subset: the compiler lowers a route
//! loader to one `routeLoader` action whose only capability is a static-URL
//! GET fetch with JSON decoding, so running exactly that program gives SSR
//! the same outcome the browser loader would observe.

use plec_ir::{
    limits::MAX_FETCH_RESPONSE_BYTES, SsrLoaderOutcome, SsrLoaderState, SsrSnapshotValue,
};

use crate::{
    artifact::{ActionProgram, ArtifactBundle, JsonValue, Route},
    request::RequestContext,
    ssr, ServerError,
};

/// The decoded `capabilityRequest::Fetch` payload a route loader carries.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct FetchRequest {
    #[serde(default)]
    url: usize,
    #[serde(default)]
    method: String,
    #[serde(default)]
    decode: String,
    #[serde(default)]
    require_ok: bool,
}

/// One executed route loader. The shape is `SsrLoaderOutcome` itself, so the
/// snapshot and its validation share the `plec-ir` contract by construction.
pub(crate) async fn execute_route_loader(
    bundle: &ArtifactBundle,
    route: &Route,
    context: &RequestContext,
    client: &reqwest::Client,
) -> Result<Option<SsrLoaderOutcome>, ServerError> {
    let Some(action) = route.loader_action else {
        return Ok(None);
    };
    let graph = bundle
        .graphs
        .iter()
        .find(|entry| entry.graph_id == route.graph_id)
        .map(|entry| &entry.graph);
    let component = graph.and_then(|graph| graph.components.get(graph.root_component));
    let program = component.and_then(|component| component.actions.get(action));
    let fetch_request = program
        .filter(|program| program.route_loader)
        .and_then(find_fetch_request);
    let (Some(component), Some(fetch_request)) = (component, fetch_request) else {
        return Err(ServerError::message(format!(
            "route loader action is invalid for {}",
            route.id
        )));
    };

    let raw_url = ssr::evaluate(
        component,
        fetch_request.url,
        &ssr::loader_scope(context),
        &mut ssr::RenderState::bare(),
    );
    let base: reqwest::Url = context
        .url
        .parse()
        .map_err(|_| ServerError::message("request url is not a valid fetch base"))?;
    let url = reqwest::Url::options()
        .base_url(Some(&base))
        .parse(&js_string(&raw_url))
        .map_err(|error| ServerError::message(format!("invalid loader url: {error}")))?;

    let rejected = |message: String| SsrLoaderOutcome {
        graph_id: route.graph_id.clone(),
        action,
        state: SsrLoaderState::Rejected { message },
    };

    // Loader failures are total outcomes, not server errors: a rejected
    // loader renders the route's error phase and transfers the rejection in
    // the snapshot. Only an invalid loader program above fails the request.
    let response = match client
        .request(
            if fetch_request.method.is_empty() {
                reqwest::Method::GET
            } else {
                reqwest::Method::from_bytes(fetch_request.method.as_bytes()).map_err(|error| {
                    ServerError::message(format!("invalid loader method: {error}"))
                })?
            },
            url.clone(),
        )
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            return Ok(Some(rejected(format!(
                "fetch {} failed: {error}",
                url.path()
            ))));
        }
    };
    if fetch_request.require_ok && !response.status().is_success() {
        return Ok(Some(rejected(format!(
            "fetch {} failed with status {}",
            url.path(),
            response.status().as_u16()
        ))));
    }
    if fetch_request.decode != "responseJson" {
        return Ok(Some(rejected(format!(
            "unsupported route loader decode {}",
            fetch_request.decode
        ))));
    }
    // Byte ceilings for untrusted loader responses (mirrors the Rust decode
    // limits; see docs/security-limits.md). The declared length fails fast;
    // the buffered length catches lying or absent declarations.
    if response
        .content_length()
        .is_some_and(|declared| declared > MAX_FETCH_RESPONSE_BYTES as u64)
    {
        return Ok(Some(rejected(
            "loader response exceeds byte limit".to_owned(),
        )));
    }
    let text = match response.text().await {
        Ok(text) => text,
        Err(error) => {
            return Ok(Some(rejected(format!(
                "fetch {} failed: {error}",
                url.path()
            ))));
        }
    };
    if text.len() > MAX_FETCH_RESPONSE_BYTES {
        return Ok(Some(rejected(
            "loader response exceeds byte limit".to_owned(),
        )));
    }
    let value: JsonValue = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(error) => {
            return Ok(Some(rejected(format!(
                "fetch {} failed: {error}",
                url.path()
            ))));
        }
    };
    Ok(Some(SsrLoaderOutcome {
        graph_id: route.graph_id.clone(),
        action,
        state: SsrLoaderState::Resolved {
            value: to_snapshot_value(value),
        },
    }))
}

fn find_fetch_request(program: &ActionProgram) -> Option<FetchRequest> {
    program
        .instructions
        .iter()
        .filter_map(|instruction| instruction.as_object())
        .find(|instruction| {
            instruction.get("op").and_then(|op| op.as_str()) == Some("capabilityRequest")
                && instruction
                    .get("capability")
                    .and_then(|capability| capability.as_str())
                    == Some("fetch")
        })
        .and_then(|instruction| instruction.get("request"))
        .and_then(|request| serde_json::from_value(request.clone()).ok())
}

/// Converts the evaluated transport value into the strict snapshot value.
/// JSON cannot carry non-finite numbers, so `as_f64` cannot lose here.
fn to_snapshot_value(value: JsonValue) -> SsrSnapshotValue {
    match value {
        JsonValue::Null => SsrSnapshotValue::Null,
        JsonValue::Bool(value) => SsrSnapshotValue::Bool(value),
        JsonValue::Number(value) => {
            SsrSnapshotValue::Number(value.as_f64().expect("json numbers are finite"))
        }
        JsonValue::String(value) => SsrSnapshotValue::String(value),
        JsonValue::Array(values) => {
            SsrSnapshotValue::Array(values.into_iter().map(to_snapshot_value).collect())
        }
        JsonValue::Object(fields) => SsrSnapshotValue::Record(
            fields
                .into_iter()
                .map(|(name, value)| (name, to_snapshot_value(value)))
                .collect(),
        ),
    }
}

/// The reverse of `to_snapshot_value`: loader outcomes transfer through the
/// strict snapshot type and feed back into SSR evaluation as transport
/// values (`loadHost loaderData`).
pub(crate) fn snapshot_value_to_json(value: &SsrSnapshotValue) -> JsonValue {
    match value {
        SsrSnapshotValue::Null => JsonValue::Null,
        SsrSnapshotValue::Bool(value) => JsonValue::Bool(*value),
        SsrSnapshotValue::Number(value) => serde_json::Number::from_f64(*value)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        SsrSnapshotValue::String(value) => JsonValue::String(value.clone()),
        SsrSnapshotValue::Array(values) => {
            JsonValue::Array(values.iter().map(snapshot_value_to_json).collect())
        }
        SsrSnapshotValue::Record(fields) => JsonValue::Object(
            fields
                .iter()
                .map(|(name, value)| (name.clone(), snapshot_value_to_json(value)))
                .collect(),
        ),
    }
}

/// `String(value)` coercion for the loader URL, mirroring the TS host's
/// plain JavaScript `String()` (not the DOM-sink `typed_value_string`).
fn js_string(value: &JsonValue) -> String {
    match value {
        JsonValue::String(value) => value.clone(),
        JsonValue::Null => "null".to_owned(),
        JsonValue::Bool(value) => value.to_string(),
        JsonValue::Number(value) => value
            .as_f64()
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NaN".to_owned()),
        JsonValue::Array(values) => values.iter().map(js_string).collect::<Vec<_>>().join(","),
        JsonValue::Object(_) => "[object Object]".to_owned(),
    }
}
