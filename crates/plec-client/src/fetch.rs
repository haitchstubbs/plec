use crate::prelude::*;
use crate::runtime::*;
use crate::vm::*;
use plec_action::{charge_response_bytes, ActionOutcome, Run, Suspension};
use plec_dom::platform::*;
use web_sys::{ReadableStreamDefaultReader, RequestCredentials, TextDecoder};

#[derive(Clone)]
pub struct TypedPendingFetch {
    pub instance_id: String,
    pub suspension: Suspension<BrowserRequest>,
    pub context: ActionRunContext,
    pub graph_generation: u64,
    pub request_id: u64,
    /// Entry action of the suspended run carried the route-loader flag; a
    /// completed route-loader browser action finishes through the shared
    /// loader pipeline instead of the browser completion tail.
    pub route_loader: bool,
}

#[derive(Clone)]
pub struct TypedPendingLoaderFetch {
    pub instance_id: String,
    pub suspension: Suspension<TypedLoaderFetchRequest>,
    pub graph_generation: u64,
    pub request_id: u64,
}

fn fetch_origin(url: &str) -> Option<String> {
    let base = web_sys::window()?.location().href().ok()?;
    web_sys::Url::new_with_base(url, &base)
        .ok()
        .map(|url| url.origin())
}

impl RuntimeState {
    pub fn start_typed_loader_fetch(
        &self,
        mut pending: TypedPendingLoaderFetch,
    ) -> Result<(), JsValue> {
        let request = pending.suspension.request.clone();
        let credentials =
            match self.authorize_fetch(&request.url, &request.method, &request.headers) {
                Ok(credentials) => credentials,
                Err(message) => {
                    return self.complete_typed_loader_fetch(
                        pending,
                        Err(failure("policy", message, &request.url)),
                    );
                }
            };
        let controller = AbortController::new()?;
        let init = RequestInit::new();
        init.set_method(&request.method);
        init.set_signal(Some(&controller.signal()));
        init.set_credentials(if credentials {
            RequestCredentials::Include
        } else {
            RequestCredentials::Omit
        });
        let headers = web_sys::Headers::new()?;
        for (name, value) in &request.headers {
            headers.set(name, value)?;
        }
        init.set_headers(&headers);
        if let Some(body) = &request.body {
            init.set_body(&JsValue::from_str(body));
        }
        let browser = window()?;
        {
            let mut typed = self.typed.borrow_mut();
            let instance = typed
                .get_mut(&pending.instance_id)
                .ok_or_else(|| JsValue::from_str("typed application missing"))?;
            let Some(runtime) = instance.runtime_for_generation_mut(pending.graph_generation)
            else {
                return Ok(());
            };
            self.region_tracker.acquire_fetch()?;
            runtime.next_fetch_id += 1;
            pending.request_id = runtime.next_fetch_id;
            runtime
                .abort_controllers
                .insert(pending.request_id, controller.clone());
        }
        let browser_request = browser.fetch_with_str_and_init(&request.url, &init);
        let runtime = self.clone();
        spawn_local(async move {
            let result = match JsFuture::from(browser_request).await {
                Ok(value) => match value.dyn_into::<Response>() {
                    Ok(response) if request.require_ok && !response.ok() => {
                        Err(http_failure(response, &request.url).await)
                    }
                    Ok(response) => {
                        if let Some(rejected) = declared_length_failure(&response, &request.url) {
                            Err(rejected)
                        } else {
                            match request.decode.as_str() {
                                "empty" => bounded_body_bytes(&response, &request.url)
                                    .await
                                    .map(|_| RuntimeValue::Null),
                                "text" => bounded_body_bytes(&response, &request.url)
                                    .await
                                    .and_then(|bytes| {
                                        decode_body_text(bytes, &request.url)
                                            .map(RuntimeValue::String)
                                    }),
                                "responseJson" => {
                                    let ok = response.ok();
                                    let status = response.status();
                                    if status == 204 || status == 205 {
                                        if let Ok(empty) = response.text() {
                                            let _ = JsFuture::from(empty).await;
                                        }
                                        Ok(RuntimeValue::Record(std::collections::HashMap::from([
                                            ("ok".into(), RuntimeValue::Bool(ok)),
                                            ("status".into(), RuntimeValue::Number(status as f64)),
                                            ("body".into(), RuntimeValue::Null),
                                        ])))
                                    } else {
                                        bounded_body_bytes(&response, &request.url)
                                            .await
                                            .and_then(|bytes| decode_body_text(bytes, &request.url))
                                            .and_then(|text| parse_body_json(&text, &request.url))
                                            .map(|body| {
                                                RuntimeValue::Record(
                                                    std::collections::HashMap::from([
                                                        ("ok".into(), RuntimeValue::Bool(ok)),
                                                        (
                                                            "status".into(),
                                                            RuntimeValue::Number(status as f64),
                                                        ),
                                                        ("body".into(), body),
                                                    ]),
                                                )
                                            })
                                    }
                                }
                                _ => bounded_body_bytes(&response, &request.url)
                                    .await
                                    .and_then(|bytes| decode_body_text(bytes, &request.url))
                                    .and_then(|text| parse_body_json(&text, &request.url)),
                            }
                        }
                    }
                    Err(error) => Err(js_failure(error, &request.url)),
                },
                Err(error) => Err(js_failure(error, &request.url)),
            };
            if let Err(error) = runtime.complete_typed_loader_fetch(pending, result) {
                web_sys::console::error_1(&error);
            }
        });
        Ok(())
    }

    fn complete_typed_loader_fetch(
        &self,
        mut pending: TypedPendingLoaderFetch,
        result: Result<RuntimeValue, RuntimeValue>,
    ) -> Result<(), JsValue> {
        let result = match result {
            Ok(value) => {
                let bytes = value
                    .json_body()
                    .map(|body| body.len())
                    .map_err(|error| failure("decode", error, &pending.suspension.request.url));
                match bytes.and_then(|bytes| {
                    charge_response_bytes(&mut pending.suspension, bytes).map_err(|error| {
                        failure("limit", error.to_string(), &pending.suspension.request.url)
                    })
                }) {
                    Ok(()) => Ok(value),
                    Err(error) => Err(error),
                }
            }
            Err(error) => Err(error),
        };
        let run = {
            let mut typed = self.typed.borrow_mut();
            let Some(instance) = typed.get_mut(&pending.instance_id) else {
                return Ok(());
            };
            let Some(runtime) = instance.runtime_for_generation_mut(pending.graph_generation)
            else {
                return Ok(());
            };
            if runtime
                .abort_controllers
                .remove(&pending.request_id)
                .is_some()
            {
                self.region_tracker.release_fetch();
            }
            runtime.resume_shared_route_loader(pending.suspension, result)?
        };
        match run {
            Run::Suspended(suspension) => self.start_typed_loader_fetch(TypedPendingLoaderFetch {
                instance_id: pending.instance_id,
                suspension,
                graph_generation: pending.graph_generation,
                request_id: 0,
            }),
            Run::Complete(outcome) => self.complete_shared_route_loader(
                &pending.instance_id,
                pending.graph_generation,
                outcome,
            ),
        }
    }

    /// Authorizes a queued fetch against the host policy. Returns whether the
    /// request may carry credentials, or a denial message. No grant exists:
    /// deny, regardless of what the artifact declares.
    fn authorize_fetch(
        &self,
        url: &str,
        method: &str,
        headers: &[(String, String)],
    ) -> Result<bool, String> {
        let grants = self.fetch_policy.borrow();
        let grants = grants
            .as_ref()
            .ok_or_else(|| "fetch origin denied by runtime policy".to_string())?;
        let origin =
            fetch_origin(url).ok_or_else(|| "fetch origin denied by runtime policy".to_string())?;
        let grant = grants
            .iter()
            .find(|grant| grant.origin.eq_ignore_ascii_case(&origin))
            .ok_or_else(|| "fetch origin denied by runtime policy".to_string())?;
        if !grant
            .methods
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(method))
        {
            return Err("fetch method denied by runtime policy".into());
        }
        for (name, _) in headers {
            if !grant
                .headers
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(name))
            {
                return Err("fetch header denied by runtime policy".into());
            }
        }
        Ok(grant.credentials)
    }
}

fn failure(kind: &str, message: String, url: &str) -> RuntimeValue {
    RuntimeValue::Record(HashMap::from([
        ("kind".into(), RuntimeValue::String(kind.into())),
        ("message".into(), RuntimeValue::String(message)),
        ("url".into(), RuntimeValue::String(url.into())),
    ]))
}

async fn http_failure(response: Response, url: &str) -> RuntimeValue {
    let status = response.status();
    let status_text = response.status_text();
    let is_json = response
        .headers()
        .get("content-type")
        .ok()
        .flatten()
        .and_then(|value| {
            value
                .split(';')
                .next()
                .map(str::trim)
                .map(str::to_ascii_lowercase)
        })
        .map(|value| {
            value == "application/json"
                || (value.starts_with("application/") && value.ends_with("+json"))
        })
        .unwrap_or(false);
    let body = match bounded_body_bytes(&response, url).await {
        Ok(bytes) if bytes.is_empty() => RuntimeValue::Null,
        Ok(bytes) => decode_body_text(bytes, url)
            .ok()
            .filter(|text| !text.is_empty())
            .map(|text| {
                if is_json {
                    serde_json::from_str(&text).unwrap_or(RuntimeValue::String(text))
                } else {
                    RuntimeValue::String(text)
                }
            })
            .unwrap_or(RuntimeValue::Null),
        // A rejected or unreadable error body stays `Null`: the failure the
        // action observes is the HTTP status, never the supplementary body.
        Err(_) => RuntimeValue::Null,
    };
    let mut record = match failure("http", format!("request failed ({status})"), url) {
        RuntimeValue::Record(record) => record,
        _ => unreachable!(),
    };
    record.insert("status".into(), RuntimeValue::Number(status.into()));
    record.insert("statusText".into(), RuntimeValue::String(status_text));
    record.insert("body".into(), body);
    RuntimeValue::Record(record)
}

fn js_failure(error: JsValue, url: &str) -> RuntimeValue {
    let name = js_sys::Reflect::get(&error, &JsValue::from_str("name"))
        .ok()
        .and_then(|value| value.as_string());
    failure(
        if name.as_deref() == Some("AbortError") {
            "abort"
        } else {
            "network"
        },
        error
            .as_string()
            .unwrap_or_else(|| "network request failed".into()),
        url,
    )
}

/// Rejects responses that declare a body beyond the documented fetch
/// budget before any body bytes are read into the tab.
fn declared_length_failure(response: &Response, url: &str) -> Option<RuntimeValue> {
    let length = response.headers().get("content-length").ok().flatten()?;
    let bytes: usize = length.parse().ok()?;
    if bytes > plec_ir::limits::MAX_FETCH_RESPONSE_BYTES {
        return Some(failure(
            "decode",
            format!(
                "response body exceeds the {} byte limit",
                plec_ir::limits::MAX_FETCH_RESPONSE_BYTES
            ),
            url,
        ));
    }
    None
}

fn body_limit_failure(url: &str) -> RuntimeValue {
    failure(
        "decode",
        format!(
            "response body exceeds the {} byte limit",
            plec_ir::limits::MAX_FETCH_RESPONSE_BYTES
        ),
        url,
    )
}

fn body_decode_failure(url: &str, message: &str) -> RuntimeValue {
    failure("decode", message.to_owned(), url)
}

/// Streams a response body under a hard byte ceiling. Each chunk is
/// accounted before accumulation and the stream is cancelled the moment the
/// budget is exceeded, so an absent or forged `content-length` cannot
/// buffer the whole body before rejection; `declared_length_failure` above
/// stays only as the declared-length fast path.
async fn bounded_body_bytes(response: &Response, url: &str) -> Result<Vec<u8>, RuntimeValue> {
    let limit = plec_ir::limits::MAX_FETCH_RESPONSE_BYTES;
    let stream = response
        .body()
        .ok_or_else(|| body_decode_failure(url, "response body unavailable"))?;
    let reader: ReadableStreamDefaultReader = stream
        .get_reader()
        .dyn_into()
        .map_err(|_| body_decode_failure(url, "response body unavailable"))?;
    let mut bytes: Vec<u8> = Vec::new();
    loop {
        let result = JsFuture::from(reader.read()).await.map_err(|error| {
            body_decode_failure(
                url,
                &error
                    .as_string()
                    .unwrap_or_else(|| "response body read failed".into()),
            )
        })?;
        let done = js_sys::Reflect::get(&result, &JsValue::from_str("done"))
            .ok()
            .and_then(|done| done.as_bool())
            .unwrap_or(true);
        if done {
            return Ok(bytes);
        }
        let chunk: js_sys::Uint8Array = js_sys::Reflect::get(&result, &JsValue::from_str("value"))
            .map_err(|_| body_decode_failure(url, "response body chunk was not bytes"))?
            .dyn_into()
            .map_err(|_| body_decode_failure(url, "response body chunk was not bytes"))?;
        let chunk = chunk.to_vec();
        if bytes.len() + chunk.len() > limit {
            // Release the stream so the connection is not left half-read.
            let _ = JsFuture::from(reader.cancel()).await;
            return Err(body_limit_failure(url));
        }
        bytes.extend_from_slice(&chunk);
    }
}

/// Decodes bounded body bytes as UTF-8 with replacement, matching the
/// browser `response.text()` semantics the previous whole-body read used.
fn decode_body_text(mut bytes: Vec<u8>, url: &str) -> Result<String, RuntimeValue> {
    TextDecoder::new_with_label("utf-8")
        .and_then(|decoder| decoder.decode_with_u8_array(&mut bytes))
        .map_err(|_| body_decode_failure(url, "response text decode failed"))
}

/// Parses bounded body text as JSON through the engine's native
/// `JSON.parse` (no WASM recursion) before the bounded runtime-value walk.
fn parse_body_json(text: &str, url: &str) -> Result<RuntimeValue, RuntimeValue> {
    match js_sys::JSON::parse(text) {
        Ok(value) => bounded_response_value(value, url),
        Err(_) => Err(body_decode_failure(url, "response JSON decode failed")),
    }
}

/// Bounds a response body decoded through the JS engine: the value is
/// stringified (native stack), size-checked, then parsed by `serde_json`'s
/// depth-guarded parser instead of `serde_wasm_bindgen`'s unbounded walk.
fn bounded_response_value(value: JsValue, url: &str) -> Result<RuntimeValue, RuntimeValue> {
    decode_bounded_json::<RuntimeValue>(
        &value,
        plec_ir::limits::MAX_FETCH_RESPONSE_BYTES,
        "response body",
    )
    .map_err(|message| failure("decode", message.as_string().unwrap_or_default(), url))
}

impl RuntimeState {
    pub fn start_typed_fetch(&self, mut pending: TypedPendingFetch) -> Result<(), JsValue> {
        let request = match pending.suspension.request.clone() {
            BrowserRequest::Fetch(request) => request,
            BrowserRequest::Cookie { .. } => {
                return Err(JsValue::from_str(
                    "cookie suspension routed through fetch transport",
                ));
            }
        };
        // Host-policy gate before any browser resource is touched. A denial
        // completes the action through its failure path without a network
        // request, exactly like any other fetch failure.
        let credentials =
            match self.authorize_fetch(&request.url, &request.method, &request.headers) {
                Ok(credentials) => credentials,
                Err(message) => {
                    let url = request.url.clone();
                    return self
                        .complete_typed_fetch(pending, Err(failure("policy", message, &url)));
                }
            };
        let controller = AbortController::new()?;
        let init = RequestInit::new();
        init.set_method(&request.method);
        init.set_signal(Some(&controller.signal()));
        // The credentials mode is host-owned: artifact code cannot widen it.
        init.set_credentials(if credentials {
            RequestCredentials::Include
        } else {
            RequestCredentials::Omit
        });
        let headers = web_sys::Headers::new()?;
        for (name, value) in &request.headers {
            headers.set(name, value)?;
        }
        init.set_headers(&headers);
        if let Some(body) = &request.body {
            init.set_body(&JsValue::from_str(body));
        }
        let browser = window()?;
        {
            let mut typed = self.typed.borrow_mut();
            let typed = typed
                .get_mut(&pending.instance_id)
                .ok_or_else(|| JsValue::from_str("typed application missing"))?;
            let Some(typed) = typed.runtime_for_generation_mut(pending.graph_generation) else {
                return Ok(());
            };
            self.region_tracker.acquire_fetch()?;
            typed.next_fetch_id += 1;
            pending.request_id = typed.next_fetch_id;
            typed
                .abort_controllers
                .insert(pending.request_id, controller.clone());
        }
        // Start the browser request before yielding. Disposal can then abort an
        // in-flight promise even when it happens in the same event turn.
        let browser_request = browser.fetch_with_str_and_init(&request.url, &init);
        let runtime = self.clone();
        spawn_local(async move {
            let result = match JsFuture::from(browser_request).await {
                Ok(value) => match value.dyn_into::<Response>() {
                    Ok(response) if request.require_ok && !response.ok() => {
                        Err(http_failure(response, &request.url).await)
                    }
                    Ok(response) => {
                        if let Some(rejected) = declared_length_failure(&response, &request.url) {
                            Err(rejected)
                        } else {
                            match request.decode.as_str() {
                                // Every decode below streams the body through
                                // `bounded_body_bytes`, so the byte ceiling is
                                // enforced before any whole-body buffering.
                                "empty" => bounded_body_bytes(&response, &request.url)
                                    .await
                                    .map(|_| RuntimeValue::Null),
                                "text" => bounded_body_bytes(&response, &request.url)
                                    .await
                                    .and_then(|bytes| {
                                        decode_body_text(bytes, &request.url)
                                            .map(RuntimeValue::String)
                                    }),
                                "responseJson" => {
                                    let ok = response.ok();
                                    let status = response.status();
                                    // 204/205 carry no body; `body` is null so
                                    // `{ok,status,body}` still reaches the action.
                                    // Their (empty) body needs no ceiling.
                                    if status == 204 || status == 205 {
                                        if let Ok(empty) = response.text() {
                                            let _ = JsFuture::from(empty).await;
                                        }
                                        Ok(RuntimeValue::Record(std::collections::HashMap::from([
                                            ("ok".into(), RuntimeValue::Bool(ok)),
                                            ("status".into(), RuntimeValue::Number(status as f64)),
                                            ("body".into(), RuntimeValue::Null),
                                        ])))
                                    } else {
                                        bounded_body_bytes(&response, &request.url)
                                            .await
                                            .and_then(|bytes| decode_body_text(bytes, &request.url))
                                            .and_then(|text| parse_body_json(&text, &request.url))
                                            .map(|body| {
                                                RuntimeValue::Record(
                                                    std::collections::HashMap::from([
                                                        ("ok".into(), RuntimeValue::Bool(ok)),
                                                        (
                                                            "status".into(),
                                                            RuntimeValue::Number(status as f64),
                                                        ),
                                                        ("body".into(), body),
                                                    ]),
                                                )
                                            })
                                    }
                                }
                                _ => bounded_body_bytes(&response, &request.url)
                                    .await
                                    .and_then(|bytes| decode_body_text(bytes, &request.url))
                                    .and_then(|text| parse_body_json(&text, &request.url)),
                            }
                        }
                    }
                    Err(error) => Err(js_failure(error, &request.url)),
                },
                Err(error) => Err(js_failure(error, &request.url)),
            };
            if let Err(error) = runtime.complete_typed_fetch(pending, result) {
                web_sys::console::error_1(&error);
            }
        });
        Ok(())
    }

    pub fn complete_typed_fetch(
        &self,
        mut pending: TypedPendingFetch,
        result: Result<RuntimeValue, RuntimeValue>,
    ) -> Result<(), JsValue> {
        let url = match &pending.suspension.request {
            BrowserRequest::Fetch(request) => request.url.clone(),
            BrowserRequest::Cookie { .. } => String::new(),
        };
        let result = match result {
            Ok(value) => {
                let bytes = value
                    .json_body()
                    .map(|body| body.len())
                    .map_err(|error| failure("decode", error, &url));
                match bytes.and_then(|bytes| {
                    charge_response_bytes(&mut pending.suspension, bytes)
                        .map_err(|error| failure("limit", error.to_string(), &url))
                }) {
                    Ok(()) => Ok(value),
                    Err(error) => Err(error),
                }
            }
            Err(error) => Err(error),
        };
        let graph_generation = pending.graph_generation;
        let route_loader = pending.route_loader;
        let run = {
            let mut typed = self.typed.borrow_mut();
            let Some(instance) = typed.get_mut(&pending.instance_id) else {
                return Ok(());
            };
            let Some(runtime) = instance.runtime_for_generation_mut(graph_generation) else {
                return Ok(());
            };
            if runtime
                .abort_controllers
                .remove(&pending.request_id)
                .is_some()
            {
                self.region_tracker.release_fetch();
            }
            if runtime.root.is_none() {
                return Ok(());
            }
            runtime
                .resume_browser_action(pending.suspension, result, pending.context.clone())
                .map_err(|error| JsValue::from_str(&error.to_string()))?
        };
        let instance_id = pending.instance_id.clone();
        match run {
            Run::Suspended(suspension) => {
                let next = pending_browser_capability(
                    instance_id,
                    graph_generation,
                    route_loader,
                    suspension,
                    pending.context,
                );
                self.start_browser_capability(next)?;
                self.mount_component_requests()?;
                self.flush_component_work()?;
                self.install_typed_event_listeners()
            }
            Run::Complete(outcome) => {
                if route_loader {
                    // Route-loader browser actions complete through the same
                    // shared-loader pipeline (exported-body unwrapping, loader
                    // result state, route restore) as route-initiated loaders.
                    return self.complete_shared_route_loader(
                        &instance_id,
                        graph_generation,
                        outcome,
                    );
                }
                self.mount_component_requests()?;
                // Continuation state writes queue row component refreshes; without this
                // drain the child components keep their pre-fetch props forever.
                self.flush_component_work()?;
                self.install_typed_event_listeners()?;
                Ok(())
            }
        }
    }

    pub(crate) fn complete_shared_route_loader(
        &self,
        instance_id: &str,
        graph_generation: u64,
        outcome: ActionOutcome,
    ) -> Result<(), JsValue> {
        let ActionOutcome::Success(value) = outcome else {
            let ActionOutcome::Failure(error) = outcome else {
                unreachable!();
            };
            if self.typed.borrow().contains_key(instance_id) {
                self.show_typed_route_error(instance_id, error)?;
                self.install_typed_event_listeners()?;
            }
            return Ok(());
        };
        let exported = match &value {
            RuntimeValue::Record(record)
                if record.len() == 3
                    && record.contains_key("ok")
                    && record.contains_key("status")
                    && record.contains_key("body") =>
            {
                record.get("body").cloned().unwrap_or(value)
            }
            _ => value,
        };
        let restore = {
            let mut typed = self.typed.borrow_mut();
            let instance = typed
                .get_mut(instance_id)
                .ok_or_else(|| JsValue::from_str("typed route instance missing"))?;
            let restore = instance.loader_runtime.is_some();
            instance.loader_data = Some(exported.clone());
            let host_inputs = self.typed_host_inputs_for(instance.loader_data.as_ref());
            let runtime = instance
                .runtime_for_generation_mut(graph_generation)
                .ok_or_else(|| JsValue::from_str("typed route loader runtime is stale"))?;
            let state = runtime
                .app
                .actions
                .iter()
                .find(|action| action.route_loader)
                .and_then(|action| action.loader_result_state)
                .ok_or_else(|| JsValue::from_str("typed route loader result state missing"))?;
            if state >= runtime.states.len() {
                return Err(JsValue::from_str("loader state handle out of range"));
            }
            if !restore {
                runtime.set_host_inputs(host_inputs)?;
                runtime.apply_static_bindings()?;
                runtime.queue_static_component_refreshes()?;
            }
            runtime.states[state] = exported;
            runtime.refresh_state(state, &mut UpdateMetrics::default())?;
            restore
        };
        if restore {
            self.restore_typed_route_normal(instance_id)?;
        }
        self.flush_component_work()?;
        self.install_typed_event_listeners()?;
        Ok(())
    }
}

impl TypedRuntime {
    pub fn take_pending_fetches(&mut self) -> Vec<TypedPendingFetch> {
        std::mem::take(&mut self.pending_fetches)
    }
}
