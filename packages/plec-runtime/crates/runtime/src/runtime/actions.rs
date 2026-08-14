use crate::eval::{expression::*, value::*};
use crate::runtime::lifecycle::*;

#[wasm_bindgen::prelude::wasm_bindgen]
impl PlecRuntime {
    pub fn apply_action(&self, action_id: String, event: JsValue) -> Result<JsValue, JsValue> {
        let instance_id = self.legacy_instance_id()?;
        self.apply_action_for(&instance_id, action_id, event)
    }
}

#[wasm_bindgen::prelude::wasm_bindgen]
impl PlecRuntime {
    pub fn apply_instance_action(
        &self,
        instance_id: String,
        action_id: String,
        event: JsValue,
    ) -> Result<JsValue, JsValue> {
        self.apply_action_for(&instance_id, action_id, event)
    }
}

#[wasm_bindgen::prelude::wasm_bindgen]
impl PlecRuntime {
    pub(crate) fn apply_action_for(
        &self,
        instance_id: &str,
        action_id: String,
        event: JsValue,
    ) -> Result<JsValue, JsValue> {
        let app = self.app_for_instance(&instance_id)?;
        let action = app
            .actions
            .iter()
            .find(|action| action.id == action_id)
            .ok_or_else(|| JsValue::from_str("unknown action"))?;
        let mut metrics = UpdateMetrics::default();
        let event = serde_wasm_bindgen::from_value(event).unwrap_or_default();
        self.execute_action_operations(
            &instance_id,
            &app,
            &action.operations,
            &event,
            None,
            &mut metrics,
            None,
        )?;
        self.refresh_state_bindings(&instance_id, &app, &mut metrics)?;
        serde_wasm_bindgen::to_value(&metrics).map_err(error)
    }
}

impl PlecRuntime {
    pub(crate) fn execute_action_operations(
        &self,
        instance_id: &str,
        app: &Application,
        operations: &[Value],
        event: &HashMap<String, Value>,
        native_event: Option<&Event>,
        metrics: &mut UpdateMetrics,
        continuation_scope: Option<&HashMap<String, Value>>,
    ) -> Result<(), JsValue> {
        for operation in operations {
            match operation.get("kind").and_then(Value::as_str) {
                Some("set-state") => {
                    let Some(slot) = operation.get("stateSlotId").and_then(Value::as_str) else {
                        continue;
                    };
                    let scope = self.action_scope(instance_id, app, event, continuation_scope)?;
                    let value = operation
                        .get("value")
                        .map(|expression| evaluate(expression, &scope))
                        .unwrap_or(Value::Null);
                    self.instances
                        .borrow_mut()
                        .get_mut(instance_id)
                        .ok_or_else(|| JsValue::from_str("unknown graph instance"))?
                        .local_state
                        .insert(slot.into(), value);
                }
                Some("if") => {
                    let test = operation
                        .get("test")
                        .map(|expression| {
                            self.action_scope(instance_id, app, event, continuation_scope)
                                .map(|scope| evaluate(expression, &scope))
                        })
                        .transpose()?
                        .unwrap_or(Value::Bool(false));
                    let branch = if truthy(&test) {
                        "consequent"
                    } else {
                        "alternate"
                    };
                    let nested = operation
                        .get(branch)
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    self.execute_action_operations(
                        instance_id,
                        app,
                        &nested,
                        event,
                        native_event,
                        metrics,
                        continuation_scope,
                    )?;
                }
                Some("prevent-default") => {
                    if let Some(event) = native_event {
                        event.prevent_default();
                    }
                }
                Some("return") => return Ok(()),
                Some("capability-request") => {
                    if operation.get("capability").and_then(Value::as_str) != Some("network.fetch")
                    {
                        return Err(JsValue::from_str("unsupported capability"));
                    }
                    self.start_fetch(
                        instance_id,
                        app.clone(),
                        operation.clone(),
                        event.clone(),
                        continuation_scope.cloned().unwrap_or_default(),
                    )?;
                }
                Some("invoke-action-ref") => {
                    let id = operation
                        .get("actionId")
                        .and_then(Value::as_str)
                        .ok_or_else(|| JsValue::from_str("action reference missing id"))?;
                    let referenced = app
                        .actions
                        .iter()
                        .find(|action| action.id == id)
                        .ok_or_else(|| JsValue::from_str("unknown action reference"))?;
                    self.execute_action_operations(
                        instance_id,
                        app,
                        &referenced.operations,
                        event,
                        native_event,
                        metrics,
                        continuation_scope,
                    )?;
                }
                Some(other) => {
                    return Err(JsValue::from_str(&format!(
                        "unsupported action operation: {other}"
                    )))
                }
                None => return Err(JsValue::from_str("invalid action operation")),
            }
        }
        Ok(())
    }
}

impl PlecRuntime {
    pub(crate) fn start_fetch(
        &self,
        instance_id: &str,
        app: Application,
        operation: Value,
        event: HashMap<String, Value>,
        inherited_scope: HashMap<String, Value>,
    ) -> Result<(), JsValue> {
        let scope = self.action_scope(instance_id, &app, &event, Some(&inherited_scope))?;
        let request = operation
            .get("request")
            .ok_or_else(|| JsValue::from_str("fetch request missing"))?;
        let url = evaluate(request.get("url").unwrap_or(&Value::Null), &scope)
            .as_str()
            .unwrap_or_default()
            .to_string();
        if url.is_empty() {
            return Err(JsValue::from_str("fetch URL is empty"));
        }
        let controller = AbortController::new()?;
        let init = RequestInit::new();
        init.set_method(
            request
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or("GET"),
        );
        init.set_signal(Some(&controller.signal()));
        let headers = web_sys::Headers::new()?;
        if let Some(values) = request.get("headers").and_then(Value::as_object) {
            for (name, expression) in values {
                let value = evaluate(expression, &scope);
                headers.set(name, value.as_str().unwrap_or_default())?;
            }
        }
        init.set_headers(&headers);
        if let Some(body) = request.get("jsonBody") {
            let body = serde_json::to_string(&evaluate(body, &scope)).map_err(error)?;
            init.set_body(&JsValue::from_str(&body));
        }
        let fetch_request = Request::new_with_str_and_init(&url, &init)?;
        let (epoch, request_id) = {
            let mut instances = self.instances.borrow_mut();
            let instance = instances
                .get_mut(instance_id)
                .ok_or_else(|| JsValue::from_str("unknown graph instance"))?;
            instance.next_request_id += 1;
            let request_id = instance.next_request_id;
            instance.abort_controllers.insert(request_id, controller);
            (instance.continuation_epoch, request_id)
        };
        let runtime = self.clone();
        let instance_id = instance_id.to_string();
        spawn_local(async move {
            let response = async {
                let window =
                    web_sys::window().ok_or_else(|| JsValue::from_str("window unavailable"))?;
                let response: Response = JsFuture::from(window.fetch_with_request(&fetch_request))
                    .await?
                    .dyn_into()?;
                let status = response.status();
                if operation
                    .get("request")
                    .and_then(|r| r.get("requireOk"))
                    .and_then(Value::as_bool)
                    .unwrap_or(true)
                    && !response.ok()
                {
                    return Err(JsValue::from_str(&format!("request failed ({status})")));
                }
                let decode = operation
                    .get("request")
                    .and_then(|r| r.get("decode"))
                    .and_then(Value::as_str)
                    .unwrap_or("json");
                let result = match decode {
                    "empty" => Value::Null,
                    "text" => Value::String(
                        JsFuture::from(response.text()?)
                            .await?
                            .as_string()
                            .unwrap_or_default(),
                    ),
                    _ => serde_wasm_bindgen::from_value(JsFuture::from(response.json()?).await?)
                        .map_err(error)?,
                };
                Ok::<_, JsValue>((result, status))
            }
            .await;
            let _ = runtime.complete_fetch(
                &instance_id,
                epoch,
                request_id,
                app,
                operation,
                event,
                inherited_scope,
                response,
            );
        });
        Ok(())
    }
}

impl PlecRuntime {
    pub(crate) fn complete_fetch(
        &self,
        instance_id: &str,
        epoch: u64,
        request_id: u64,
        app: Application,
        operation: Value,
        event: HashMap<String, Value>,
        inherited_scope: HashMap<String, Value>,
        response: Result<(Value, u16), JsValue>,
    ) -> Result<(), JsValue> {
        {
            let mut instances = self.instances.borrow_mut();
            let Some(instance) = instances.get_mut(instance_id) else {
                return Ok(());
            };
            if instance.continuation_epoch != epoch {
                return Ok(());
            }
            instance.abort_controllers.remove(&request_id);
        }
        let mut metrics = UpdateMetrics::default();
        let mut scope = inherited_scope;
        let (branch, name, value) = match response {
            Ok((result, _)) => (
                "success",
                operation
                    .get("successResultName")
                    .and_then(Value::as_str)
                    .unwrap_or("result"),
                result,
            ),
            Err(reason) => (
                "failure",
                operation
                    .get("failureErrorName")
                    .and_then(Value::as_str)
                    .unwrap_or("error"),
                serde_json::json!({"message": reason.as_string().unwrap_or_else(|| "network request failed".into())}),
            ),
        };
        scope.insert(name.into(), value);
        let operations = operation
            .get(branch)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        self.execute_action_operations(
            instance_id,
            &app,
            &operations,
            &event,
            None,
            &mut metrics,
            Some(&scope),
        )?;
        let finally = operation
            .get("finally")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        self.execute_action_operations(
            instance_id,
            &app,
            &finally,
            &event,
            None,
            &mut metrics,
            Some(&scope),
        )?;
        self.refresh_state_bindings(instance_id, &app, &mut metrics)
    }
}

impl PlecRuntime {
    pub(crate) fn action_scope(
        &self,
        instance_id: &str,
        app: &Application,
        event: &HashMap<String, Value>,
        continuation_scope: Option<&HashMap<String, Value>>,
    ) -> Result<HashMap<String, Value>, JsValue> {
        let mut scope = self.state_scope(instance_id, app)?;
        scope.insert(
            "event".into(),
            Value::Object(event.clone().into_iter().collect()),
        );
        if let Some(values) = continuation_scope {
            scope.extend(values.clone());
        }
        Ok(scope)
    }
}
