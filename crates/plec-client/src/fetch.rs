use crate::prelude::*;
use crate::runtime::*;
use crate::vm::*;
use plec_dom::platform::*;
use web_sys::RequestCredentials;

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

fn fetch_origin(url: &str) -> Option<String> {
    let base = web_sys::window()?.location().href().ok()?;
    web_sys::Url::new_with_base(url, &base)
        .ok()
        .map(|url| url.origin())
}

impl RuntimeState {
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
    let body = match response.text().ok().map(JsFuture::from) {
        Some(body) => match body
            .await
            .ok()
            .and_then(|value| value.as_string())
            .filter(|text| !text.is_empty())
        {
            Some(text) if is_json => {
                serde_json::from_str(&text).unwrap_or(RuntimeValue::String(text))
            }
            Some(text) => RuntimeValue::String(text),
            None => RuntimeValue::Null,
        },
        None => RuntimeValue::Null,
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
                                "empty" => match response.text() {
                                    Ok(body) => JsFuture::from(body)
                                        .await
                                        .map(|_| RuntimeValue::Null)
                                        .map_err(|error| {
                                            failure(
                                                "decode",
                                                error.as_string().unwrap_or_else(|| {
                                                    "response body drain failed".into()
                                                }),
                                                &pending.url,
                                            )
                                        }),
                                    Err(error) => Err(failure(
                                        "decode",
                                        error
                                            .as_string()
                                            .unwrap_or_else(|| "response body unavailable".into()),
                                        &pending.url,
                                    )),
                                },
                                "text" => match response.text() {
                                    Ok(body) => JsFuture::from(body)
                                        .await
                                        .map(|value| {
                                            let text = value.as_string().unwrap_or_default();
                                            if text.len()
                                                > plec_ir::limits::MAX_FETCH_RESPONSE_BYTES
                                            {
                                                return Err(failure(
                                                    "decode",
                                                    format!(
                                                        "response body exceeds the {} byte limit",
                                                        plec_ir::limits::MAX_FETCH_RESPONSE_BYTES
                                                    ),
                                                    &pending.url,
                                                ));
                                            }
                                            Ok(RuntimeValue::String(text))
                                        })
                                        .map_err(|error| {
                                            failure(
                                                "decode",
                                                error.as_string().unwrap_or_else(|| {
                                                    "response text decode failed".into()
                                                }),
                                                &pending.url,
                                            )
                                        })
                                        .and_then(std::convert::identity),
                                    Err(error) => Err(failure(
                                        "decode",
                                        error
                                            .as_string()
                                            .unwrap_or_else(|| "response text unavailable".into()),
                                        &pending.url,
                                    )),
                                },
                                "responseJson" => {
                                    let ok = response.ok();
                                    let status = response.status();
                                    // 204/205 carry no body; `body` is null so
                                    // `{ok,status,body}` still reaches the action.
                                    // Drain via text() so the browser never flags the
                                    // request as abandoned mid-response.
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
                                        match response.json() {
                                            Ok(body) => JsFuture::from(body)
                                                .await
                                                .map_err(|error| {
                                                    failure(
                                                        "decode",
                                                        error.as_string().unwrap_or_else(|| {
                                                            "response JSON decode failed".into()
                                                        }),
                                                        &pending.url,
                                                    )
                                                })
                                                .and_then(|value| {
                                                    bounded_response_value(value, &pending.url)
                                                        .map_err(std::convert::identity)
                                                })
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
                                                }),
                                            Err(error) => Err(failure(
                                                "decode",
                                                error.as_string().unwrap_or_else(|| {
                                                    "response JSON unavailable".into()
                                                }),
                                                &pending.url,
                                            )),
                                        }
                                    }
                                }
                                _ => match response.json() {
                                    Ok(body) => JsFuture::from(body)
                                        .await
                                        .map_err(|error| {
                                            failure(
                                                "decode",
                                                error.as_string().unwrap_or_else(|| {
                                                    "response JSON decode failed".into()
                                                }),
                                                &pending.url,
                                            )
                                        })
                                        .and_then(|value| {
                                            bounded_response_value(value, &pending.url)
                                        }),
                                    Err(error) => Err(failure(
                                        "decode",
                                        error
                                            .as_string()
                                            .unwrap_or_else(|| "response JSON unavailable".into()),
                                        &pending.url,
                                    )),
                                },
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
                let bytes = value.json_body().map(|body| body.len()).map_err(|error| {
                    failure(
                        "decode",
                        error.as_string().unwrap_or_default(),
                        &pending.url,
                    )
                });
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
                // `responseJson` decodes actions receive an {ok,status,body}
                // envelope so they can branch on transport outcome themselves;
                // loader consumers asked for the payload alone, so export the
                // unwrapped body here.
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
                self.typed_host_inputs
                    .borrow_mut()
                    .insert("loaderData".into(), exported);
                let restore = {
                    let mut typed = self.typed.borrow_mut();
                    let instance = match typed.get_mut(&instance_id) {
                        Some(instance) => instance,
                        None => return Ok(()),
                    };
                    let restore = instance.loader_runtime.is_some();
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
                        // before the dependency refresh for loader-data
                        // initialisers to observe the outcome.
                        runtime.set_host_inputs(self.typed_host_inputs.borrow().clone())?;
                    }
                    runtime.states[state] = value;
                    runtime.refresh_state(state, &mut UpdateMetrics::default())?;
                    restore
                };
                if restore {
                    self.restore_typed_route_normal(&instance_id)?;
                }
                self.install_typed_event_listeners()?;
            }
        }
        Ok(())
    }
}

impl TypedRuntime {
    pub fn take_pending_fetches(&mut self) -> Vec<TypedPendingFetch> {
        std::mem::take(&mut self.pending_fetches)
    }
}
