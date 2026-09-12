//! Server route-loader host for shared typed action execution.

use plec_action::{
    charge_response_bytes, resume, start, ActionError, ActionHost, ActionOutcome, Run,
};
use plec_ir::{
    limits::MAX_FETCH_RESPONSE_BYTES, SsrLoaderOutcome, SsrLoaderState, SsrSnapshotValue,
};
use plec_schema::{delta::RuntimeValue, typed::TypedCapabilityRequest};

use crate::{
    artifact::{ArtifactBundle, Component, JsonValue, Route},
    request::RequestContext,
    ssr, ServerError,
};

#[derive(Clone)]
struct LoaderFetch {
    url: String,
    method: String,
    headers: Vec<(String, String)>,
    body: Option<String>,
    decode: String,
    require_ok: bool,
}

struct LoaderHost<'a> {
    component: &'a Component,
    context: &'a RequestContext,
    states: Vec<JsonValue>,
}

impl<'a> LoaderHost<'a> {
    fn new(component: &'a Component, context: &'a RequestContext) -> Self {
        let mut states = Vec::with_capacity(component.state_slots.len());
        for state in &component.state_slots {
            let scope = ssr::Scope {
                states: states.clone(),
                ..ssr::loader_scope(context)
            };
            states.push(ssr::evaluate(
                component,
                state.initial_expression,
                &scope,
                &mut ssr::RenderState::bare(),
            ));
        }
        Self {
            component,
            context,
            states,
        }
    }
}

impl ActionHost for LoaderHost<'_> {
    type Request = LoaderFetch;

    fn evaluate(
        &mut self,
        expression: usize,
        frame: &[RuntimeValue],
    ) -> Result<RuntimeValue, ActionError> {
        let scope = ssr::Scope {
            frame: frame.iter().cloned().map(runtime_to_json).collect(),
            states: self.states.clone(),
            ..ssr::loader_scope(self.context)
        };
        Ok(json_to_runtime(ssr::evaluate(
            self.component,
            expression,
            &scope,
            &mut ssr::RenderState::bare(),
        )))
    }

    fn prepare_capability(
        &mut self,
        request: &TypedCapabilityRequest,
        frame: &[RuntimeValue],
    ) -> Result<Self::Request, ActionError> {
        let TypedCapabilityRequest::Fetch(request) = request else {
            return Err(ActionError(
                "unsupported route loader capability: cookie".into(),
            ));
        };
        let url = value_string(&self.evaluate(request.url, frame)?);
        if url.is_empty() {
            return Err(ActionError("fetch URL is empty".into()));
        }
        let headers = request
            .headers
            .iter()
            .map(|header| {
                let name = self
                    .component
                    .strings
                    .get(header.name)
                    .cloned()
                    .ok_or_else(|| ActionError("header name handle out of range".into()))?;
                Ok((name, value_string(&self.evaluate(header.value, frame)?)))
            })
            .collect::<Result<Vec<_>, ActionError>>()?;
        let body = request
            .body
            .map(|expression| {
                let value = self.evaluate(expression, frame)?;
                match value {
                    RuntimeValue::String(value) => Ok(value),
                    value => serde_json::to_string(&value).map_err(|error| {
                        ActionError(format!("fetch body encoding failed: {error}"))
                    }),
                }
            })
            .transpose()?;
        Ok(LoaderFetch {
            url,
            method: request.method.clone(),
            headers,
            body,
            decode: request.decode.clone(),
            require_ok: request.require_ok,
        })
    }
}

/// One executed route loader. The snapshot sees only its terminal action
/// outcome: `responseJson` remains an internal capability envelope.
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
    let Some(component) = component else {
        return Err(ServerError::message(format!(
            "route loader action is invalid for {}",
            route.id
        )));
    };
    let program = component
        .actions
        .get(action)
        .filter(|program| program.route_loader);
    let Some(program) = program else {
        return Err(ServerError::message(format!(
            "route loader action is invalid for {}",
            route.id
        )));
    };
    if program.instructions.is_empty() {
        return Err(ServerError::message(format!(
            "route loader action is invalid for {}",
            route.id
        )));
    }
    let mut host = LoaderHost::new(component, context);
    let mut run = start(
        &component.actions,
        action,
        vec![RuntimeValue::Null; program.frame_slots],
        &mut host,
    )
    .map_err(loader_program_error)?;
    loop {
        match run {
            Run::Complete(outcome) => return Ok(Some(loader_outcome(route, action, outcome))),
            Run::Suspended(mut suspension) => {
                let result = execute_fetch(&suspension.request, context, client).await;
                if let Ok((value, _)) = &result {
                    // Browser accounting measures the decoded runtime value
                    // after envelope creation. Keep SSR's action-wide budget
                    // on that same representation; raw stream limits remain
                    // enforced in `read_bounded_stream` below.
                    let bytes = serde_json::to_vec(value).map_or(0, |value| value.len());
                    if let Err(error) = charge_response_bytes(&mut suspension, bytes) {
                        run = resume(
                            &component.actions,
                            suspension,
                            Err(loader_failure(error.0)),
                            &mut host,
                        )
                        .map_err(loader_program_error)?;
                        continue;
                    }
                }
                run = resume(
                    &component.actions,
                    suspension,
                    result.map(|(value, _)| value),
                    &mut host,
                )
                .map_err(loader_program_error)?;
            }
        }
    }
}

fn loader_program_error(error: ActionError) -> ServerError {
    ServerError::message(format!("route loader action is invalid: {error}"))
}

fn loader_outcome(route: &Route, action: usize, outcome: ActionOutcome) -> SsrLoaderOutcome {
    let state = match outcome {
        ActionOutcome::Success(value) => SsrLoaderState::Resolved {
            value: to_snapshot_value(unwrap_response_body(value)),
        },
        ActionOutcome::Failure(error) => SsrLoaderState::Rejected {
            message: failure_message(&error),
        },
    };
    SsrLoaderOutcome {
        graph_id: route.graph_id.clone(),
        action,
        state,
    }
}

async fn execute_fetch(
    fetch: &LoaderFetch,
    context: &RequestContext,
    client: &reqwest::Client,
) -> Result<(RuntimeValue, usize), RuntimeValue> {
    if fetch.decode != "responseJson" {
        return Err(loader_failure(format!(
            "unsupported route loader decode {}",
            fetch.decode
        )));
    }
    let base: reqwest::Url = context
        .url
        .parse()
        .map_err(|_| loader_failure("request url is not a valid fetch base"))?;
    let url = reqwest::Url::options()
        .base_url(Some(&base))
        .parse(&fetch.url)
        .map_err(|error| loader_failure(format!("invalid loader url: {error}")))?;
    let method = if fetch.method.is_empty() {
        reqwest::Method::GET
    } else {
        reqwest::Method::from_bytes(fetch.method.as_bytes())
            .map_err(|error| loader_failure(format!("invalid loader method: {error}")))?
    };
    let mut request = client.request(method, url.clone());
    for (name, value) in &fetch.headers {
        request = request.header(name, value);
    }
    if let Some(body) = &fetch.body {
        request = request.body(body.clone());
    }
    let response = request
        .send()
        .await
        .map_err(|error| loader_failure(format!("fetch {} failed: {error}", url.path())))?;
    if fetch.require_ok && !response.status().is_success() {
        return Err(loader_failure(format!(
            "fetch {} failed with status {}",
            url.path(),
            response.status().as_u16()
        )));
    }
    if response
        .content_length()
        .is_some_and(|declared| declared > MAX_FETCH_RESPONSE_BYTES as u64)
    {
        return Err(loader_failure("loader response exceeds byte limit"));
    }
    let ok = response.status().is_success();
    let status = response.status().as_u16();
    let bytes = read_bounded_stream(response, url.path())
        .await
        .map_err(loader_failure)?;
    let body = if status == 204 || status == 205 {
        RuntimeValue::Null
    } else {
        let value: JsonValue = serde_json::from_slice(&bytes)
            .map_err(|error| loader_failure(format!("fetch {} failed: {error}", url.path())))?;
        json_to_runtime(value)
    };
    Ok((
        RuntimeValue::Record(std::collections::HashMap::from([
            ("ok".into(), RuntimeValue::Bool(ok)),
            ("status".into(), RuntimeValue::Number(status as f64)),
            ("body".into(), body),
        ])),
        bytes.len(),
    ))
}

/// Reads under a hard ceiling before body buffering.
async fn read_bounded_stream(response: reqwest::Response, path: &str) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut stream = std::pin::pin!(response.bytes_stream());
    while let Some(chunk) = futures_util::StreamExt::next(&mut stream).await {
        let chunk = chunk.map_err(|error| format!("fetch {path} failed: {error}"))?;
        if bytes.len() + chunk.len() > MAX_FETCH_RESPONSE_BYTES {
            return Err("loader response exceeds byte limit".to_owned());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn unwrap_response_body(value: RuntimeValue) -> RuntimeValue {
    match &value {
        RuntimeValue::Record(record)
            if record.len() == 3
                && record.contains_key("ok")
                && record.contains_key("status")
                && record.contains_key("body") =>
        {
            record.get("body").cloned().unwrap_or(value)
        }
        _ => value,
    }
}

fn loader_failure(message: impl Into<String>) -> RuntimeValue {
    RuntimeValue::Record(std::collections::HashMap::from([(
        "message".into(),
        RuntimeValue::String(message.into()),
    )]))
}

fn failure_message(value: &RuntimeValue) -> String {
    value
        .record()
        .and_then(|record| record.get("message"))
        .map(value_string)
        .unwrap_or_else(|| value_string(value))
}

fn to_snapshot_value(value: RuntimeValue) -> SsrSnapshotValue {
    match value {
        RuntimeValue::Null => SsrSnapshotValue::Null,
        RuntimeValue::Bool(value) => SsrSnapshotValue::Bool(value),
        RuntimeValue::Number(value) => SsrSnapshotValue::Number(value),
        RuntimeValue::String(value) => SsrSnapshotValue::String(value),
        RuntimeValue::Array(values) => {
            SsrSnapshotValue::Array(values.into_iter().map(to_snapshot_value).collect())
        }
        RuntimeValue::Record(fields) => SsrSnapshotValue::Record(
            fields
                .into_iter()
                .map(|(name, value)| (name, to_snapshot_value(value)))
                .collect(),
        ),
    }
}

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

fn json_to_runtime(value: JsonValue) -> RuntimeValue {
    match value {
        JsonValue::Null => RuntimeValue::Null,
        JsonValue::Bool(value) => RuntimeValue::Bool(value),
        JsonValue::Number(value) => RuntimeValue::Number(value.as_f64().unwrap_or(0.0)),
        JsonValue::String(value) => RuntimeValue::String(value),
        JsonValue::Array(values) => {
            RuntimeValue::Array(values.into_iter().map(json_to_runtime).collect())
        }
        JsonValue::Object(values) => RuntimeValue::Record(
            values
                .into_iter()
                .map(|(name, value)| (name, json_to_runtime(value)))
                .collect(),
        ),
    }
}

fn runtime_to_json(value: RuntimeValue) -> JsonValue {
    match value {
        RuntimeValue::Null => JsonValue::Null,
        RuntimeValue::Bool(value) => JsonValue::Bool(value),
        RuntimeValue::Number(value) => serde_json::Number::from_f64(value)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        RuntimeValue::String(value) => JsonValue::String(value),
        RuntimeValue::Array(values) => {
            JsonValue::Array(values.into_iter().map(runtime_to_json).collect())
        }
        RuntimeValue::Record(values) => JsonValue::Object(
            values
                .into_iter()
                .map(|(name, value)| (name, runtime_to_json(value)))
                .collect(),
        ),
    }
}

fn value_string(value: &RuntimeValue) -> String {
    match value {
        RuntimeValue::String(value) => value.clone(),
        RuntimeValue::Null => String::new(),
        RuntimeValue::Bool(value) => value.to_string(),
        RuntimeValue::Number(value) => value.to_string(),
        RuntimeValue::Array(_) | RuntimeValue::Record(_) => {
            serde_json::to_string(value).unwrap_or_default()
        }
    }
}
