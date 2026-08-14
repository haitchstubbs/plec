use crate::dom::platform::*;
use crate::runtime::lifecycle::*;
use crate::typed::runtime::*;

#[derive(Clone)]
pub(crate) struct TypedPendingFetch {
    pub(crate) action: usize,
    pub(crate) success_pc: usize,
    pub(crate) failure_pc: usize,
    pub(crate) finally_pc: Option<usize>,
    pub(crate) finalizers: Vec<usize>,
    pub(crate) result_slot: usize,
    pub(crate) error_slot: usize,
    pub(crate) frame: Vec<RuntimeValue>,
    pub(crate) event: Vec<RuntimeValue>,
    pub(crate) row: Option<HashMap<String, RuntimeValue>>,
    pub(crate) url: String,
    pub(crate) method: String,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Option<String>,
    pub(crate) decode: String,
    pub(crate) require_ok: bool,
}

impl PlecRuntime {
    #[cfg(feature = "fetch")]
    pub(crate) fn start_typed_fetch(&self, pending: TypedPendingFetch) -> Result<(), JsValue> {
        let init = RequestInit::new();
        init.set_method(&pending.method);
        let headers = web_sys::Headers::new()?;
        for (name, value) in &pending.headers {
            headers.set(name, value)?;
        }
        init.set_headers(&headers);
        if let Some(body) = &pending.body {
            init.set_body(&JsValue::from_str(body));
        }
        let request = Request::new_with_str_and_init(&pending.url, &init)?;
        let runtime = self.clone();
        spawn_local(async move {
            let result = async {
                let response: Response = JsFuture::from(window()?.fetch_with_request(&request))
                    .await?
                    .dyn_into()?;
                if pending.require_ok && !response.ok() {
                    return Err(JsValue::from_str(&format!(
                        "request failed ({})",
                        response.status()
                    )));
                }
                match pending.decode.as_str() {
                    "empty" => Ok(RuntimeValue::Null),
                    "text" => Ok(RuntimeValue::String(
                        JsFuture::from(response.text()?)
                            .await?
                            .as_string()
                            .unwrap_or_default(),
                    )),
                    _ => serde_wasm_bindgen::from_value(JsFuture::from(response.json()?).await?)
                        .map_err(error),
                }
            }
            .await;
            let _ = runtime.complete_typed_fetch(pending, result);
        });
        Ok(())
    }
}

impl PlecRuntime {
    #[cfg(feature = "fetch")]
    pub(crate) fn complete_typed_fetch(
        &self,
        pending: TypedPendingFetch,
        result: Result<RuntimeValue, JsValue>,
    ) -> Result<(), JsValue> {
        let more = {
            let mut typed = self.typed.borrow_mut();
            let Some(typed) = typed.as_mut() else {
                return Ok(());
            };
            if typed.root.is_none() {
                return Ok(());
            }
            let mut frame = pending.frame;
            let pc = match result {
                Ok(value) => {
                    if pending.result_slot >= frame.len() {
                        return Err(JsValue::from_str("result frame slot out of range"));
                    }
                    frame[pending.result_slot] = value.clone();
                    if let Some(state) = typed.app.actions.get(pending.action).and_then(|action| {
                        action
                            .route_loader
                            .then_some(action.loader_result_state)
                            .flatten()
                    }) {
                        if state >= typed.states.len() {
                            return Err(JsValue::from_str("loader state handle out of range"));
                        }
                        typed.states[state] = value;
                        let mut loader_metrics = UpdateMetrics::default();
                        typed.refresh_state(state, &mut loader_metrics)?;
                    }
                    pending.success_pc
                }
                Err(error) => {
                    if pending.error_slot >= frame.len() {
                        return Err(JsValue::from_str("error frame slot out of range"));
                    }
                    frame[pending.error_slot] = RuntimeValue::Record(HashMap::from([(
                        "message".into(),
                        RuntimeValue::String(
                            error
                                .as_string()
                                .unwrap_or_else(|| "network request failed".into()),
                        ),
                    )]));
                    pending.failure_pc
                }
            };
            let mut metrics = UpdateMetrics::default();
            typed.execute_action_at(
                pending.action,
                pc,
                frame.clone(),
                &pending.event,
                pending.row.clone(),
                None,
                &mut metrics,
            )?;
            let mut requests = typed.take_pending_fetches();
            if requests.is_empty() {
                let mut finalizers = pending.finalizers.clone();
                if let Some(finally_pc) = pending.finally_pc {
                    finalizers.push(finally_pc);
                }
                for finally_pc in finalizers.into_iter().rev() {
                    typed.execute_action_at(
                        pending.action,
                        finally_pc,
                        frame.clone(),
                        &pending.event,
                        pending.row.clone(),
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
        for request in more {
            self.start_typed_fetch(request)?;
        }
        Ok(())
    }
}

impl TypedRuntime {
    pub(crate) fn take_pending_fetches(&mut self) -> Vec<TypedPendingFetch> {
        std::mem::take(&mut self.pending_fetches)
    }
}
