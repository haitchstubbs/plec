use crate::dom::platform::*;
use crate::runtime::lifecycle::*;
use crate::typed::runtime::*;
use crate::typed::vm::*;

#[derive(Clone)]
pub(crate) struct TypedPendingFetch {
    pub(crate) instance_id: String,
    pub(crate) continuation: TypedContinuationStack,
    pub(crate) success_pc: usize,
    pub(crate) failure_pc: usize,
    pub(crate) finally_pc: Option<usize>,
    pub(crate) finalizers: Vec<usize>,
    pub(crate) result_slot: usize,
    pub(crate) error_slot: usize,
    pub(crate) url: String,
    pub(crate) method: String,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Option<String>,
    pub(crate) decode: String,
    pub(crate) require_ok: bool,
    pub(crate) graph_generation: u64,
    pub(crate) request_id: u64,
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

impl PlecRuntime {
    pub(crate) fn start_typed_fetch(&self, mut pending: TypedPendingFetch) -> Result<(), JsValue> {
        let controller = AbortController::new()?;
        {
            let mut typed = self.typed.borrow_mut();
            let typed = typed
                .get_mut(&pending.instance_id)
                .ok_or_else(|| JsValue::from_str("typed application missing"))?;
            let Some(typed) = typed.runtime_for_generation_mut(pending.graph_generation) else {
                return Ok(());
            };
            typed.next_fetch_id += 1;
            pending.request_id = typed.next_fetch_id;
            typed
                .abort_controllers
                .insert(pending.request_id, controller.clone());
        }
        let init = RequestInit::new();
        init.set_method(&pending.method);
        init.set_signal(Some(&controller.signal()));
        let headers = web_sys::Headers::new()?;
        for (name, value) in &pending.headers {
            headers.set(name, value)?;
        }
        init.set_headers(&headers);
        if let Some(body) = &pending.body {
            init.set_body(&JsValue::from_str(body));
        }
        let request = Request::new_with_str_and_init(&pending.url, &init)?;
        // Start the browser request before yielding. Disposal can then abort an
        // in-flight promise even when it happens in the same event turn.
        let fetch = window()?.fetch_with_request(&request);
        let runtime = self.clone();
        spawn_local(async move {
            let result = match JsFuture::from(fetch).await {
                Ok(value) => match value.dyn_into::<Response>() {
                    Ok(response) if pending.require_ok && !response.ok() => {
                        Err(http_failure(response, &pending.url).await)
                    }
                    Ok(response) => match pending.decode.as_str() {
                        "empty" => match response.text() {
                            Ok(body) => JsFuture::from(body)
                                .await
                                .map(|_| RuntimeValue::Null)
                                .map_err(|error| {
                                    failure(
                                        "decode",
                                        error
                                            .as_string()
                                            .unwrap_or_else(|| "response body drain failed".into()),
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
                                    RuntimeValue::String(value.as_string().unwrap_or_default())
                                })
                                .map_err(|error| {
                                    failure(
                                        "decode",
                                        error.as_string().unwrap_or_else(|| {
                                            "response text decode failed".into()
                                        }),
                                        &pending.url,
                                    )
                                }),
                            Err(error) => Err(failure(
                                "decode",
                                error
                                    .as_string()
                                    .unwrap_or_else(|| "response text unavailable".into()),
                                &pending.url,
                            )),
                        },
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
                                    serde_wasm_bindgen::from_value(value).map_err(|error| {
                                        failure("decode", error.to_string(), &pending.url)
                                    })
                                }),
                            Err(error) => Err(failure(
                                "decode",
                                error
                                    .as_string()
                                    .unwrap_or_else(|| "response JSON unavailable".into()),
                                &pending.url,
                            )),
                        },
                    },
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

    pub(crate) fn complete_typed_fetch(
        &self,
        pending: TypedPendingFetch,
        result: Result<RuntimeValue, RuntimeValue>,
    ) -> Result<(), JsValue> {
        let route_loader = self
            .typed
            .borrow()
            .get(&pending.instance_id)
            .and_then(|instance| {
                instance
                    .runtime_for_generation(pending.graph_generation)?
                    .app
                    .actions
                    .get(pending.continuation.current.action)
                    .map(|action| action.route_loader)
            })
            .unwrap_or(false);
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
            typed.abort_controllers.remove(&pending.request_id);
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
                    typed.execute_action_at(
                        continuation.current.action,
                        finally_pc,
                        continuation.current.frame.clone(),
                        &continuation.current.event,
                        continuation.current.row.clone(),
                        None,
                        &mut metrics,
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
                let restore = {
                    let mut typed = self.typed.borrow_mut();
                    let instance = match typed.get_mut(&instance_id) {
                        Some(instance) => instance,
                        None => return Ok(()),
                    };
                    let Some(runtime) =
                        instance.runtime_for_generation_mut(pending.graph_generation)
                    else {
                        return Ok(());
                    };
                    runtime.abort_controllers.remove(&pending.request_id);
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
                    runtime.states[state] = value;
                    instance.loader_runtime.is_some()
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
    pub(crate) fn take_pending_fetches(&mut self) -> Vec<TypedPendingFetch> {
        std::mem::take(&mut self.pending_fetches)
    }
}
