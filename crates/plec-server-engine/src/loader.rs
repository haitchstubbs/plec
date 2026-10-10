//! Host-independent route-loader execution for compiled applications.

use plec_action::{
    charge_response_bytes, resume, start, ActionError, ActionHost, ActionOutcome, Run,
};
use plec_ir::{limits::MAX_FETCH_RESPONSE_BYTES, SsrLoaderOutcome, SsrLoaderState};
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
            frame: frame
                .iter()
                .cloned()
                .map(RuntimeValue::into_json_value)
                .collect(),
            states: self.states.clone(),
            ..ssr::loader_scope(self.context)
        };
        Ok(RuntimeValue::from_json_value(ssr::evaluate(
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
        let url = self.evaluate(request.url, frame)?.dom_string();
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
                Ok((name, self.evaluate(header.value, frame)?.dom_string()))
            })
            .collect::<Result<Vec<_>, ActionError>>()?;
        let body = request
            .body
            .map(|expression| {
                let value = self.evaluate(expression, frame)?;
                match value {
                    RuntimeValue::String(value) => Ok(value),
                    value => value.json_body().map_err(|error| {
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
pub enum LoaderExecution {
    None,
    Snapshot(SsrLoaderOutcome),
    Redirect { location: String, replace: bool },
    NotFound,
}

pub async fn execute_route_loader(
    bundle: &ArtifactBundle,
    route: &Route,
    context: &RequestContext,
    params: &[(String, String)],
    client: &reqwest::Client,
) -> Result<LoaderExecution, ServerError> {
    let Some(action) = route.loader_action else {
        return Ok(LoaderExecution::None);
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
    let mut frame = vec![RuntimeValue::Null; program.frame_slots];
    // Compiled loaders declare the shared parameter contract: slot 0 carries
    // matched route params, slot 1 the request location (slot 1 stays null
    // until loader grammar exposes it; the slot contract is fixed).
    if !program.parameter_slots.is_empty() {
        frame[PARAMS_SLOT] = RuntimeValue::Record(
            params
                .iter()
                .map(|(key, value)| (key.clone(), RuntimeValue::String(value.clone())))
                .collect(),
        );
        if let Some(location) = frame.get_mut(LOCATION_SLOT) {
            *location = RuntimeValue::Record(std::collections::HashMap::from([
                (
                    "pathname".into(),
                    RuntimeValue::String(context.pathname.clone()),
                ),
                (
                    "search".into(),
                    RuntimeValue::String(ssr::url_search(&context.url)),
                ),
                ("hash".into(), RuntimeValue::String(String::new())),
            ]));
        }
    }
    let mut run =
        start(&component.actions, action, frame, &mut host).map_err(loader_program_error)?;
    loop {
        match run {
            Run::Complete(outcome) => return Ok(loader_execution(route, action, program, outcome)),
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

const PARAMS_SLOT: usize = 0;
const LOCATION_SLOT: usize = 1;

fn loader_program_error(error: ActionError) -> ServerError {
    ServerError::message(format!("route loader action is invalid: {error}"))
}

fn loader_execution(
    route: &Route,
    action: usize,
    program: &plec_schema::typed::TypedAction,
    outcome: ActionOutcome,
) -> LoaderExecution {
    match outcome {
        ActionOutcome::Success(value) => {
            // Body-decoding loaders export the body directly; legacy loaders
            // export the transport envelope and unwrap at this boundary.
            let value = if program.loader_decode_body {
                value
            } else {
                unwrap_response_body(value)
            };
            LoaderExecution::Snapshot(SsrLoaderOutcome {
                graph_id: route.graph_id.clone(),
                action,
                state: SsrLoaderState::Resolved {
                    value: value.into_ssr_snapshot(),
                },
            })
        }
        ActionOutcome::Failure(error) => LoaderExecution::Snapshot(SsrLoaderOutcome {
            graph_id: route.graph_id.clone(),
            action,
            state: SsrLoaderState::Rejected {
                failure: plec_schema::public_route_loader_failure(&error),
            },
        }),
        ActionOutcome::Redirect { location, replace } => {
            LoaderExecution::Redirect { location, replace }
        }
        ActionOutcome::NotFound => LoaderExecution::NotFound,
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
    let response = request.send().await.map_err(|_| {
        loader_failure_record("network", "network request failed", Some(&fetch.url))
    })?;
    if fetch.require_ok && !response.status().is_success() {
        let status = response.status().as_u16();
        let status_text = plec_ir::canonical_status_text(status).to_owned();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        let bytes = read_public_failure_body(response).await.unwrap_or_default();
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let body = if bytes.is_empty() {
            RuntimeValue::Null
        } else if content_type == "application/json"
            || (content_type.starts_with("application/") && content_type.ends_with("+json"))
        {
            serde_json::from_slice::<JsonValue>(&bytes)
                .map(RuntimeValue::from_json_value)
                .unwrap_or_else(|_| RuntimeValue::String(text))
        } else {
            RuntimeValue::String(text)
        };
        let mut failure = match loader_failure_record(
            "http",
            &format!("request failed ({status})"),
            Some(&fetch.url),
        ) {
            RuntimeValue::Record(record) => record,
            _ => unreachable!(),
        };
        failure.insert("status".into(), RuntimeValue::Number(status as f64));
        failure.insert("statusText".into(), RuntimeValue::String(status_text));
        failure.insert("body".into(), body);
        return Err(RuntimeValue::Record(failure));
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
        let value: JsonValue = serde_json::from_slice(&bytes).map_err(|_| {
            loader_failure_record("decode", "response JSON decode failed", Some(&fetch.url))
        })?;
        RuntimeValue::from_json_value(value)
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

/// Supplementary public failure data is deliberately much smaller than normal
/// fetch responses so it can always fit safely inside the SSR snapshot.
async fn read_public_failure_body(response: reqwest::Response) -> Result<Vec<u8>, String> {
    const LIMIT: usize = plec_ir::PublicRouteLoaderFailure::MAX_BODY_BYTES;
    let mut bytes = Vec::new();
    let mut stream = std::pin::pin!(response.bytes_stream());
    while let Some(chunk) = futures_util::StreamExt::next(&mut stream).await {
        let chunk = chunk.map_err(|error| error.to_string())?;
        if bytes.len() + chunk.len() > LIMIT {
            return Err("public failure body exceeds limit".into());
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

fn loader_failure_record(kind: &str, message: &str, url: Option<&str>) -> RuntimeValue {
    let mut record = std::collections::HashMap::from([
        ("kind".into(), RuntimeValue::String(kind.into())),
        ("message".into(), RuntimeValue::String(message.into())),
    ]);
    if let Some(url) = url {
        record.insert("url".into(), RuntimeValue::String(url.into()));
    }
    RuntimeValue::Record(record)
}

pub fn snapshot_value_to_json(value: &plec_ir::SsrSnapshotValue) -> JsonValue {
    RuntimeValue::from_ssr_snapshot(value).into_json_value()
}
