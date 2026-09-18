use crate::prelude::*;
use crate::runtime::*;
use crate::vm::*;
use plec_action::{charge_response_bytes, ActionOutcome, Run, Suspension};
use plec_dom::platform::*;
use web_sys::{ReadableStreamDefaultReader, RequestCredentials, TextDecoder};

#[derive(Clone)]
pub struct TypedPendingFetch {
    pub instance_id: String,
    pub continuation: TypedContinuationStack,
    pub success_pc: usize,
    pub failure_pc: usize,
    pub finally_pc: Option<usize>,
    pub finalizers: Vec<usize>,
    pub result_slot: usize,
    pub error_slot: usize,
    pub url: String,
    pub method: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    pub decode: String,
    pub require_ok: bool,
    pub graph_generation: u64,
    pub request_id: u64,
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
        // Host-policy gate before any browser resource is touched. A denial
        // completes the action through its failure path without a network
        // request, exactly like any other fetch failure.
        let credentials =
            match self.authorize_fetch(&pending.url, &pending.method, &pending.headers) {
                Ok(credentials) => credentials,
                Err(message) => {
                    let url = pending.url.clone();
                    return self
                        .complete_typed_fetch(pending, Err(failure("policy", message, &url)));
                }
            };
        let controller = AbortController::new()?;
        let init = RequestInit::new();
        init.set_method(&pending.method);
        init.set_signal(Some(&controller.signal()));
        // The credentials mode is host-owned: artifact code cannot widen it.
        init.set_credentials(if credentials {
            RequestCredentials::Include
        } else {
            RequestCredentials::Omit
        });
        let headers = web_sys::Headers::new()?;
        for (name, value) in &pending.headers {
            headers.set(name, value)?;
        }
        init.set_headers(&headers);
        if let Some(body) = &pending.body {
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
        let request = browser.fetch_with_str_and_init(&pending.url, &init);
        let runtime = self.clone();
        spawn_local(async move {
            let result = match JsFuture::from(request).await {
                Ok(value) => match value.dyn_into::<Response>() {
                    Ok(response) if pending.require_ok && !response.ok() => {
                        Err(http_failure(response, &pending.url).await)
                    }
                    Ok(response) => {
                        if let Some(rejected) = declared_length_failure(&response, &pending.url) {
                            Err(rejected)
                        } else {
                            match pending.decode.as_str() {
                                // Every decode below streams the body through
                                // `bounded_body_bytes`, so the byte ceiling is
                                // enforced before any whole-body buffering.
                                "empty" => bounded_body_bytes(&response, &pending.url)
                                    .await
                                    .map(|_| RuntimeValue::Null),
                                "text" => bounded_body_bytes(&response, &pending.url)
                                    .await
                                    .and_then(|bytes| {
                                        decode_body_text(bytes, &pending.url)
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
                                        bounded_body_bytes(&response, &pending.url)
                                            .await
                                            .and_then(|bytes| decode_body_text(bytes, &pending.url))
                                            .and_then(|text| parse_body_json(&text, &pending.url))
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
                                _ => bounded_body_bytes(&response, &pending.url)
                                    .await
                                    .and_then(|bytes| decode_body_text(bytes, &pending.url))
                                    .and_then(|text| parse_body_json(&text, &pending.url)),
                            }
                        }
                    }
                    Err(error) => Err(js_failure(error, &pending.url)),
                },
                Err(error) => Err(js_failure(error, &pending.url)),
            };
            if let Err(error) = runtime.complete_typed_fetch(pending, result) {
                web_sys::console::error_1(&error);
            }
        });
        Ok(())
    }

    pub fn complete_typed_fetch(
        &self,
        pending: TypedPendingFetch,
        result: Result<RuntimeValue, RuntimeValue>,
    ) -> Result<(), JsValue> {
        let result = match result {
            Ok(value) => {
                let bytes = value
                    .json_body()
                    .map(|body| body.len())
                    .map_err(|error| failure("decode", error, &pending.url));
                match bytes.and_then(|bytes| {
                    pending
                        .continuation
                        .fetch_accounting
                        .borrow_mut()
                        .charge_response_bytes(bytes)
                        .map_err(|error| {
                            failure("limit", error.as_string().unwrap_or_default(), &pending.url)
                        })
                }) {
                    Ok(()) => Ok(value),
                    Err(error) => Err(error),
                }
            }
            Err(error) => Err(error),
        };
        let route_loader = {
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
            runtime
                .app
                .actions
                .get(pending.continuation.current.action)
                .map(|action| action.route_loader)
                .unwrap_or(false)
        };
        if route_loader {
            return self.complete_typed_route_loader(pending, result);
        }
        let instance_id = pending.instance_id.clone();
        let more = {
            let mut typed = self.typed.borrow_mut();
            let Some(typed) = typed.get_mut(&pending.instance_id) else {
                return Ok(());
            };
            let Some(typed) = typed.runtime_for_generation_mut(pending.graph_generation) else {
                return Ok(());
            };
            if typed.root.is_none() {
                return Ok(());
            }
            let mut continuation = pending.continuation;
            let frame = &mut continuation.current.frame;
            let pc = match result {
                Ok(value) => {
                    if pending.result_slot >= frame.len() {
                        return Err(JsValue::from_str("result frame slot out of range"));
                    }
                    frame[pending.result_slot] = value.clone();
                    if let Some(state) = typed
                        .app
                        .actions
                        .get(continuation.current.action)
                        .and_then(|action| {
                            action
                                .route_loader
                                .then_some(action.loader_result_state)
                                .flatten()
                        })
                    {
                        if state >= typed.states.len() {
                            return Err(JsValue::from_str("loader state handle out of range"));
                        }
                        typed.states[state] = value;
                        typed.refresh_state(state, &mut UpdateMetrics::default())?;
                    }
                    pending.success_pc
                }
                Err(error) => {
                    if pending.error_slot >= frame.len() {
                        return Err(JsValue::from_str("error frame slot out of range"));
                    }
                    frame[pending.error_slot] = error;
                    pending.failure_pc
                }
            };
            let mut metrics = UpdateMetrics::default();
            continuation.current.pc = pc;
            typed.execute_continuation(continuation.clone(), None, &mut metrics)?;
            let mut requests = typed.take_pending_fetches();
            if requests.is_empty() {
                let mut finalizers = pending.finalizers.clone();
                if let Some(finally_pc) = pending.finally_pc {
                    finalizers.push(finally_pc);
                }
                for finally_pc in finalizers.into_iter().rev() {
                    typed.execute_action_at_with_fetch_accounting(
                        continuation.current.action,
                        finally_pc,
                        continuation.current.frame.clone(),
                        &continuation.current.event,
                        continuation.current.row.clone(),
                        None,
                        &mut metrics,
                        continuation.fetch_accounting.clone(),
                    )?;
                    requests.extend(typed.take_pending_fetches());
                }
            } else {
                for request in &mut requests {
                    request
                        .finalizers
                        .extend(pending.finalizers.iter().copied());
                    if let Some(finally_pc) = pending.finally_pc {
                        request.finalizers.push(finally_pc);
                    }
                }
            }
            requests
        };
        for mut request in more {
            request.instance_id = instance_id.clone();
            self.start_typed_fetch(request)?;
        }
        self.mount_component_requests()?;
        // Continuation state writes queue row component refreshes; without this
        // drain the child components keep their pre-fetch props forever.
        self.flush_component_work()?;
        self.install_typed_event_listeners()?;
        Ok(())
    }

    fn complete_typed_route_loader(
        &self,
        pending: TypedPendingFetch,
        result: Result<RuntimeValue, RuntimeValue>,
    ) -> Result<(), JsValue> {
        let instance_id = pending.instance_id.clone();
        match result {
            Err(error) => {
                if self.typed.borrow().contains_key(&instance_id) {
                    self.show_typed_route_error(&instance_id, error)?;
                    self.install_typed_event_listeners()?;
                }
            }
            Ok(value) => {
                // Route loader data is an explicit host input to the normal
                // route graph. It never crosses the VM as a browser Response.
                // `responseJson` decodes receive an {ok,status,body}
                // envelope so they can branch on transport outcome themselves;
                // loader consumers asked for the payload alone, so export the
                // unwrapped body here. The exported payload is also the
                // loader result state value: SSR resolves the same outcome
                // (loader.rs `execute_route_loader`) as the bare body, so a
                // slot that is both `loaderResultState` and
                // `loadHost("loaderData")`-initialised must never observe the
                // transport envelope.
                let exported = match &value {
                    RuntimeValue::Record(record)
                        if record.len() == 3
                            && record.contains_key("ok")
                            && record.contains_key("status")
                            && record.contains_key("body") =>
                    {
                        record.get("body").cloned().unwrap_or_else(|| value.clone())
                    }
                    _ => value.clone(),
                };
                let restore = {
                    let mut typed = self.typed.borrow_mut();
                    let instance = match typed.get_mut(&instance_id) {
                        Some(instance) => instance,
                        None => return Ok(()),
                    };
                    let restore = instance.loader_runtime.is_some();
                    instance.loader_data = Some(exported.clone());
                    let host_inputs = self.typed_host_inputs_for(instance.loader_data.as_ref());
                    let Some(runtime) =
                        instance.runtime_for_generation_mut(pending.graph_generation)
                    else {
                        return Ok(());
                    };
                    let state = runtime
                        .app
                        .actions
                        .get(pending.continuation.current.action)
                        .and_then(|action| action.loader_result_state)
                        .ok_or_else(|| {
                            JsValue::from_str("typed route loader result state missing")
                        })?;
                    if state >= runtime.states.len() {
                        return Err(JsValue::from_str("loader state handle out of range"));
                    }
                    if !restore {
                        // Without a preserved loader runtime there is no
                        // restore path: the fetch completed against the live
                        // normal graph, so its host inputs must be re-applied
                        // before the loader data becomes visible.
                        //
                        // Re-application alone only rewrites values in
                        // place: bindings compiled against host slots (the
                        // `useLoaderData` shape reads
                        // `loadHost("loaderData")` directly and carries no
                        // dependency edges) keep their pre-fetch DOM until
                        // the static bindings are re-applied, mirroring how
                        // location host inputs refresh on navigation.
                        runtime.set_host_inputs(host_inputs)?;
                        runtime.apply_static_bindings()?;
                        runtime.queue_static_component_refreshes()?;
                    }
                    runtime.states[state] = exported;
                    runtime.refresh_state(state, &mut UpdateMetrics::default())?;
                    restore
                };
                if restore {
                    self.restore_typed_route_normal(&instance_id)?;
                }
                // Re-applied bindings can queue component refreshes; without
                // this drain child components keep their pre-fetch props,
                // mirroring the plain fetch completion path.
                self.flush_component_work()?;
                self.install_typed_event_listeners()?;
            }
        }
        Ok(())
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
