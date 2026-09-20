use crate::bindings::*;
use crate::cookie::*;
#[cfg(feature = "fetch")]
use crate::fetch::*;
use crate::prelude::*;
use crate::runtime::*;
use plec_action::{ActionError, ActionHost, Run, Suspension};
use plec_dom::platform::document;
use plec_eval::eval::*;
use plec_schema::typed::{TypedCapabilityRequest, TypedCookieRequest, TypedFetchRequest};
use wasm_bindgen::JsCast;
use web_sys::{Event, HtmlElement};

/// Fully evaluated browser fetch request. Transport remains in `fetch.rs`;
/// shared action execution only decides when capability suspension occurs.
#[cfg(feature = "fetch")]
#[derive(Clone, Debug)]
pub struct TypedLoaderFetchRequest {
    pub url: String,
    pub method: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    pub decode: String,
    pub require_ok: bool,
}

#[cfg(feature = "fetch")]
struct TypedLoaderHost<'a> {
    runtime: &'a mut TypedRuntime,
}

#[cfg(feature = "fetch")]
impl ActionHost for TypedLoaderHost<'_> {
    type Request = TypedLoaderFetchRequest;

    fn evaluate(
        &mut self,
        expression: usize,
        frame: &[RuntimeValue],
    ) -> Result<RuntimeValue, ActionError> {
        typed_eval_frame(
            &self.runtime.app,
            self.runtime.cookie_policy.borrow().as_ref(),
            expression,
            &self.runtime.states,
            None,
            0,
            frame,
            &[],
        )
        .map_err(|error| {
            ActionError(
                error
                    .as_string()
                    .unwrap_or_else(|| "action evaluation failed".into()),
            )
        })
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
        let url = typed_value_string(&self.evaluate(request.url, frame)?);
        if url.is_empty() {
            return Err(ActionError("fetch URL is empty".into()));
        }
        let headers = request
            .headers
            .iter()
            .map(|header| {
                let name = self
                    .runtime
                    .app
                    .strings
                    .get(header.name)
                    .cloned()
                    .ok_or_else(|| ActionError("header name handle out of range".into()))?;
                Ok((
                    name,
                    typed_value_string(&self.evaluate(header.value, frame)?),
                ))
            })
            .collect::<Result<Vec<_>, ActionError>>()?;
        let body = request
            .body
            .map(|expression| {
                let value = self.evaluate(expression, frame)?;
                match value {
                    RuntimeValue::String(value) => Ok(value),
                    value => value.json_body().map_err(ActionError),
                }
            })
            .transpose()?;
        Ok(TypedLoaderFetchRequest {
            url,
            method: request.method.clone(),
            headers,
            body,
            decode: request.decode.clone(),
            require_ok: request.require_ok,
        })
    }
}

/// Host-owned context for one browser action run. `event` and `row` are
/// constant for the lifetime of the run (set at entry, never mutated), so
/// they live here instead of per-frame action data and survive suspension.
#[derive(Clone, Default)]
pub struct ActionRunContext {
    pub event: Vec<RuntimeValue>,
    pub row: Option<HashMap<String, RuntimeValue>>,
}

/// A browser capability request suspended by the shared action machine.
#[derive(Clone, Debug)]
pub enum BrowserRequest {
    #[cfg(feature = "fetch")]
    Fetch(TypedLoaderFetchRequest),
    Cookie {
        request: TypedCookieRequest,
        value: Option<String>,
    },
}

/// One browser capability suspension, routed to its host transport.
pub(crate) enum PendingBrowserCapability {
    #[cfg(feature = "fetch")]
    Fetch(TypedPendingFetch),
    Cookie(TypedPendingCookie),
}

pub(crate) fn pending_browser_capability(
    instance_id: String,
    graph_generation: u64,
    route_loader: bool,
    suspension: Suspension<BrowserRequest>,
    context: ActionRunContext,
) -> PendingBrowserCapability {
    #[cfg(feature = "fetch")]
    let is_fetch = matches!(suspension.request, BrowserRequest::Fetch(_));
    #[cfg(feature = "fetch")]
    if is_fetch {
        return PendingBrowserCapability::Fetch(TypedPendingFetch {
            instance_id,
            suspension,
            context,
            graph_generation,
            request_id: 0,
            route_loader,
        });
    }
    PendingBrowserCapability::Cookie(TypedPendingCookie {
        instance_id,
        request_id: 0,
        suspension,
        context,
        graph_generation,
        route_loader,
    })
}

impl RuntimeState {
    pub(crate) fn start_browser_capability(
        &self,
        pending: PendingBrowserCapability,
    ) -> Result<(), JsValue> {
        match pending {
            #[cfg(feature = "fetch")]
            PendingBrowserCapability::Fetch(request) => self.start_typed_fetch(request),
            PendingBrowserCapability::Cookie(cookie) => self.start_typed_cookie(cookie),
        }
    }
}

/// Browser adapter for the shared action machine in `plec-action`. All
/// browser effects (DOM, state, callbacks, cookie declarations) live here;
/// frames, continuations, budgets, and suspension stay host-neutral.
pub(crate) struct BrowserActionHost<'a> {
    runtime: &'a mut TypedRuntime,
    context: ActionRunContext,
    /// Only valid during the synchronous run; resume paths pass `None`.
    native_event: Option<&'a Event>,
    metrics: &'a mut UpdateMetrics,
}

fn action_error(error: JsValue) -> ActionError {
    ActionError(
        error
            .as_string()
            .unwrap_or_else(|| "action effect failed".into()),
    )
}

impl ActionHost for BrowserActionHost<'_> {
    type Request = BrowserRequest;

    fn evaluate(
        &mut self,
        expression: usize,
        frame: &[RuntimeValue],
    ) -> Result<RuntimeValue, ActionError> {
        typed_eval_frame(
            &self.runtime.app,
            self.runtime.cookie_policy.borrow().as_ref(),
            expression,
            &self.runtime.states,
            self.context.row.as_ref(),
            0,
            frame,
            &self.context.event,
        )
        .map_err(action_error)
    }

    fn prepare_capability(
        &mut self,
        request: &TypedCapabilityRequest,
        frame: &[RuntimeValue],
    ) -> Result<BrowserRequest, ActionError> {
        match request {
            TypedCapabilityRequest::Fetch(request) => self.prepare_fetch_capability(request, frame),
            TypedCapabilityRequest::Cookie(request) => {
                self.prepare_cookie_capability(request, frame)
            }
        }
    }

    fn store_state(&mut self, state: usize, value: RuntimeValue) -> Result<(), ActionError> {
        if state >= self.runtime.states.len() {
            return Err(ActionError("state handle out of range".into()));
        }
        self.runtime.states[state] = value;
        self.runtime
            .refresh_state(state, self.metrics)
            .map_err(action_error)
    }

    fn mutation_start(
        &mut self,
        generation: usize,
        pending: usize,
        error: usize,
    ) -> Result<RuntimeValue, ActionError> {
        let current = match self.runtime.states.get(generation) {
            Some(RuntimeValue::Number(value)) => *value,
            Some(_) => return Err(ActionError("mutation generation is not numeric".into())),
            None => return Err(ActionError("mutation generation state out of range".into())),
        };
        let next = current + 1.0;
        if !next.is_finite() {
            return Err(ActionError("mutation invocation generation overflow".into()));
        }
        if pending >= self.runtime.states.len() || error >= self.runtime.states.len() {
            return Err(ActionError("mutation state handle out of range".into()));
        }
        self.runtime.states[generation] = RuntimeValue::Number(next);
        self.runtime.states[pending] = RuntimeValue::Bool(true);
        self.runtime.refresh_state(pending, self.metrics).map_err(action_error)?;
        self.runtime.states[error] = RuntimeValue::Null;
        self.runtime.refresh_state(error, self.metrics).map_err(action_error)?;
        Ok(RuntimeValue::Number(next))
    }

    fn mutation_publish(
        &mut self,
        generation: usize,
        pending: usize,
        error: usize,
        data: usize,
        invocation: RuntimeValue,
        value: RuntimeValue,
        success: bool,
    ) -> Result<(), ActionError> {
        let current = self
            .runtime
            .states
            .get(generation)
            .ok_or_else(|| ActionError("mutation generation state out of range".into()))?;
        if *current != invocation {
            return Ok(());
        }
        if [pending, error, data].iter().any(|state| *state >= self.runtime.states.len()) {
            return Err(ActionError("mutation state handle out of range".into()));
        }
        if success {
            self.runtime.states[data] = value;
            self.runtime.states[error] = RuntimeValue::Null;
        } else {
            self.runtime.states[error] = value;
        }
        self.runtime.states[pending] = RuntimeValue::Bool(false);
        for state in if success { vec![data, error, pending] } else { vec![error, pending] } {
            self.runtime.refresh_state(state, self.metrics).map_err(action_error)?;
        }
        Ok(())
    }

    fn store_ref(&mut self, reference: usize, value: RuntimeValue) -> Result<(), ActionError> {
        let slot = self
            .runtime
            .app
            .ref_values
            .get_mut(reference)
            .ok_or_else(|| ActionError("ref handle out of range".into()))?;
        *slot = value;
        Ok(())
    }

    fn capture_active_element(&mut self, reference: usize) -> Result<(), ActionError> {
        let slot = self
            .runtime
            .focus_refs
            .get_mut(reference)
            .ok_or_else(|| ActionError("focus ref handle out of range".into()))?;
        *slot = document().map_err(action_error)?.active_element();
        Ok(())
    }

    fn focus_host_ref(&mut self, reference: usize) -> Result<(), ActionError> {
        if let Some(node) = self
            .runtime
            .host_ref_nodes
            .get(reference)
            .and_then(Option::as_ref)
        {
            if let Some(element) = node.dyn_ref::<HtmlElement>() {
                let _ = element.focus();
            }
        }
        Ok(())
    }

    fn focus_ref(&mut self, reference: usize) -> Result<(), ActionError> {
        if let Some(element) = self
            .runtime
            .focus_refs
            .get(reference)
            .and_then(Option::as_ref)
        {
            if element.is_connected() {
                if let Some(element) = element.dyn_ref::<HtmlElement>() {
                    let _ = element.focus();
                }
            }
        }
        Ok(())
    }

    fn prevent_default(&mut self) -> Result<(), ActionError> {
        if let Some(event) = self.native_event {
            event.prevent_default();
        }
        Ok(())
    }

    fn call_prop(
        &mut self,
        prop: usize,
        arguments: Vec<RuntimeValue>,
        optional: bool,
    ) -> Result<(), ActionError> {
        let Some(mut callback) = self.runtime.callbacks.get(prop).and_then(Clone::clone) else {
            // A missing optional prop completes without error and control
            // simply falls through to the next instruction.
            if optional {
                return Ok(());
            }
            return Err(ActionError("callable component prop missing".into()));
        };
        callback.arguments = arguments;
        self.runtime.callback_requests.push(callback);
        Ok(())
    }

    fn mutate_collection(
        &mut self,
        input: usize,
        kind: &str,
        key: RuntimeValue,
        value: Option<RuntimeValue>,
    ) -> Result<(), ActionError> {
        let key = typed_value_string(&key);
        self.runtime
            .mutate_collection(input, kind, key, value, self.metrics)
            .map_err(action_error)
    }

    fn store_host_ref(&mut self, reference: usize) -> Result<(), ActionError> {
        let name = self
            .runtime
            .app
            .strings
            .get(reference)
            .cloned()
            .ok_or_else(|| ActionError("host ref handle out of range".into()))?;
        if let Some(active) = document().map_err(action_error)?.active_element() {
            self.runtime.host_refs.insert(name, active.into());
        } else {
            self.runtime.host_refs.remove(&name);
        }
        Ok(())
    }
}

impl BrowserActionHost<'_> {
    #[cfg(feature = "fetch")]
    fn prepare_fetch_capability(
        &mut self,
        request: &TypedFetchRequest,
        frame: &[RuntimeValue],
    ) -> Result<BrowserRequest, ActionError> {
        Ok(BrowserRequest::Fetch(self.prepare_fetch(request, frame)?))
    }

    #[cfg(not(feature = "fetch"))]
    fn prepare_fetch_capability(
        &mut self,
        _request: &TypedFetchRequest,
        _frame: &[RuntimeValue],
    ) -> Result<BrowserRequest, ActionError> {
        Err(ActionError("fetch capability is disabled".into()))
    }

    fn prepare_cookie_capability(
        &mut self,
        request: &TypedCookieRequest,
        frame: &[RuntimeValue],
    ) -> Result<BrowserRequest, ActionError> {
        // Name is validated here, before control crosses the host boundary.
        let name = self
            .runtime
            .app
            .strings
            .get(request.name)
            .cloned()
            .ok_or_else(|| ActionError("cookie name handle out of range".into()))?;
        let value = request
            .value
            .map(|expression| {
                self.evaluate(expression, frame)
                    .map(|value| typed_value_string(&value))
            })
            .transpose()?;
        let operation = request.operation.clone();
        if !self.runtime.app.capabilities.iter().any(|entry| {
            entry.kind == "cookie"
                && entry.name == name
                && entry.operations.iter().any(|allowed| allowed == &operation)
                && entry.path == request.path
                && entry.same_site == request.same_site
                && entry.secure == request.secure
                && entry
                    .expiry_modes
                    .iter()
                    .any(|mode| mode == &request.expiry)
        }) {
            return Err(ActionError("cookie request is not declared".into()));
        }
        Ok(BrowserRequest::Cookie {
            request: request.clone(),
            value,
        })
    }

    #[cfg(feature = "fetch")]
    fn prepare_fetch(
        &mut self,
        request: &TypedFetchRequest,
        frame: &[RuntimeValue],
    ) -> Result<TypedLoaderFetchRequest, ActionError> {
        let url = typed_value_string(&self.evaluate(request.url, frame)?);
        if url.is_empty() {
            return Err(ActionError("fetch URL is empty".into()));
        }
        let headers = request
            .headers
            .iter()
            .map(|header| {
                let name = self
                    .runtime
                    .app
                    .strings
                    .get(header.name)
                    .cloned()
                    .ok_or_else(|| ActionError("header name handle out of range".into()))?;
                Ok((
                    name,
                    typed_value_string(&self.evaluate(header.value, frame)?),
                ))
            })
            .collect::<Result<Vec<_>, ActionError>>()?;
        let body = request
            .body
            .map(|expression| {
                let value = self.evaluate(expression, frame)?;
                match value {
                    // JSON.stringify already produces a fetch-ready string.
                    // Encoding it again turns `{\"title\":\"Plec\"}` into a JSON
                    // string literal, which APIs correctly reject as a non-object body.
                    RuntimeValue::String(value) => Ok(value),
                    value => value.json_body().map_err(ActionError),
                }
            })
            .transpose()?;
        Ok(TypedLoaderFetchRequest {
            url,
            method: request.method.clone(),
            headers,
            body,
            decode: request.decode.clone(),
            require_ok: request.require_ok,
        })
    }
}

impl TypedRuntime {
    #[cfg(feature = "fetch")]
    pub(crate) fn start_shared_route_loader(
        &mut self,
        action: usize,
        frame: Vec<RuntimeValue>,
    ) -> Result<Run<TypedLoaderFetchRequest>, JsValue> {
        self.start_loader_run(action, frame)
            .map_err(|error| JsValue::from_str(&error.to_string()))
    }

    #[cfg(feature = "fetch")]
    pub(crate) fn start_loader_run(
        &mut self,
        action: usize,
        frame: Vec<RuntimeValue>,
    ) -> Result<Run<TypedLoaderFetchRequest>, ActionError> {
        let actions = self.app.actions.clone();
        let mut host = TypedLoaderHost { runtime: self };
        plec_action::start(&actions, action, frame, &mut host)
    }

    #[cfg(feature = "fetch")]
    pub(crate) fn resume_shared_route_loader(
        &mut self,
        suspension: Suspension<TypedLoaderFetchRequest>,
        result: Result<RuntimeValue, RuntimeValue>,
    ) -> Result<Run<TypedLoaderFetchRequest>, JsValue> {
        self.resume_loader_run(suspension, result)
            .map_err(|error| JsValue::from_str(&error.to_string()))
    }

    #[cfg(feature = "fetch")]
    pub(crate) fn resume_loader_run(
        &mut self,
        suspension: Suspension<TypedLoaderFetchRequest>,
        result: Result<RuntimeValue, RuntimeValue>,
    ) -> Result<Run<TypedLoaderFetchRequest>, ActionError> {
        let actions = self.app.actions.clone();
        let mut host = TypedLoaderHost { runtime: self };
        plec_action::resume(&actions, suspension, result, &mut host)
    }

    pub fn execute_action(
        &mut self,
        action: usize,
        event: &[RuntimeValue],
        row: Option<HashMap<String, RuntimeValue>>,
        native_event: Option<&Event>,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let slots = event.iter().cloned().enumerate().collect::<Vec<_>>();
        self.execute_action_with_frame(action, &slots, row, native_event, metrics)
    }

    pub fn execute_action_with_frame(
        &mut self,
        action: usize,
        event: &[(usize, RuntimeValue)],
        row: Option<HashMap<String, RuntimeValue>>,
        native_event: Option<&Event>,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let program = self
            .app
            .actions
            .get(action)
            .cloned()
            .ok_or_else(|| JsValue::from_str("action handle out of range"))?;
        let mut frame = vec![RuntimeValue::Null; program.frame_slots];
        for (slot, value) in event {
            if *slot >= frame.len() {
                return Err(JsValue::from_str("event frame slot out of range"));
            }
            frame[*slot] = value.clone();
        }
        let context = ActionRunContext {
            event: event.iter().map(|(_, value)| value.clone()).collect(),
            row,
        };
        let run = self
            .start_browser_action(action, frame, context.clone(), native_event, metrics)
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        self.queue_browser_suspension(action, run, context);
        Ok(())
    }

    /// Starts one browser action run on the shared machine. Errors keep the
    /// shared machine's exact strings (budgets, arity, depth).
    pub(crate) fn start_browser_action(
        &mut self,
        action: usize,
        frame: Vec<RuntimeValue>,
        context: ActionRunContext,
        native_event: Option<&Event>,
        metrics: &mut UpdateMetrics,
    ) -> Result<Run<BrowserRequest>, ActionError> {
        let actions = self.app.actions.clone();
        let mut host = BrowserActionHost {
            runtime: self,
            context,
            native_event,
            metrics,
        };
        plec_action::start(&actions, action, frame, &mut host)
    }

    /// Resumes a browser action suspension. The native event is gone by the
    /// time a capability transport completes, and metrics restart per resume.
    pub(crate) fn resume_browser_action(
        &mut self,
        suspension: Suspension<BrowserRequest>,
        result: Result<RuntimeValue, RuntimeValue>,
        context: ActionRunContext,
    ) -> Result<Run<BrowserRequest>, ActionError> {
        let actions = self.app.actions.clone();
        let mut metrics = UpdateMetrics::default();
        let mut host = BrowserActionHost {
            runtime: self,
            context,
            native_event: None,
            metrics: &mut metrics,
        };
        plec_action::resume(&actions, suspension, result, &mut host)
    }

    fn queue_browser_suspension(
        &mut self,
        action: usize,
        run: Run<BrowserRequest>,
        context: ActionRunContext,
    ) {
        let Run::Suspended(suspension) = run else {
            return;
        };
        let route_loader = self
            .app
            .actions
            .get(action)
            .map(|action| action.route_loader)
            .unwrap_or(false);
        match pending_browser_capability(
            String::new(),
            self.graph_generation,
            route_loader,
            suspension,
            context,
        ) {
            #[cfg(feature = "fetch")]
            PendingBrowserCapability::Fetch(request) => self.pending_fetches.push(request),
            PendingBrowserCapability::Cookie(cookie) => self.pending_cookies.push(cookie),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "fetch")]
    use plec_action::ActionOutcome;
    use plec_schema::typed::TypedApplication;
    use serde_json::json;

    fn call_frame_application() -> TypedApplication {
        serde_json::from_value(json!({
            "rootNode": 0,
            "strings": ["div"],
            "constants": [null, "success", "failure"],
            "nodes": [{"op": "element", "tag": 0, "parent": null}],
            "stateSlots": [{"initialExpression": 0, "frameSlot": 0}],
            "expressions": [
                {"instructions": [{"op": "constant", "constant": 0}, {"op": "return"}]},
                {"instructions": [{"op": "constant", "constant": 1}, {"op": "return"}]},
                {"instructions": [{"op": "constant", "constant": 2}, {"op": "return"}]}
            ],
            "actions": [
                {
                    "frameSlots": 4,
                    "instructions": [
                        {"op": "callFrame", "parameter": 1, "arguments": [], "successPc": 1, "failurePc": 4, "resultSlot": 2, "errorSlot": 3},
                        {"op": "evaluate", "expression": 1},
                        {"op": "storeState", "state": 0},
                        {"op": "return"},
                        {"op": "evaluate", "expression": 2},
                        {"op": "storeState", "state": 0},
                        {"op": "return"}
                    ]
                },
                {"instructions": [{"op": "return", "value": 1}]},
                {"instructions": [{"op": "return", "outcome": "failure", "value": 2}]}
            ]
        }))
        .unwrap()
    }

    #[cfg(feature = "fetch")]
    fn mutation_application() -> TypedApplication {
        serde_json::from_value(json!({
            "rootNode": 0,
            "strings": ["div"],
            "constants": ["/mutation", false, null, 0],
            "nodes": [{"op": "element", "tag": 0, "parent": null}],
            "stateSlots": [
                {"initialExpression": 1, "frameSlot": 0},
                {"initialExpression": 2, "frameSlot": 1},
                {"initialExpression": 2, "frameSlot": 2},
                {"initialExpression": 3, "frameSlot": 3}
            ],
            "expressions": [
                {"instructions": [{"op": "loadFrame", "slot": 0}, {"op": "return"}]},
                {"instructions": [{"op": "constant", "constant": 1}, {"op": "return"}]},
                {"instructions": [{"op": "constant", "constant": 2}, {"op": "return"}]},
                {"instructions": [{"op": "constant", "constant": 3}, {"op": "return"}]},
                {"instructions": [{"op": "constant", "constant": 0}, {"op": "return"}]},
                {"instructions": [{"op": "loadFrame", "slot": 1}, {"op": "return"}]},
                {"instructions": [{"op": "loadFrame", "slot": 2}, {"op": "return"}]}
            ],
            "actions": [
                {"frameSlots": 4, "parameterSlots": [0], "instructions": [
                    {"op": "mutationStart", "generation": 3, "pending": 0, "error": 1},
                    {"op": "storeFrame", "slot": 1},
                    {"op": "call", "action": 1, "arguments": [0], "successPc": 3, "failurePc": 5, "resultSlot": 2, "errorSlot": 3},
                    {"op": "mutationPublish", "generation": 3, "pending": 0, "error": 1, "data": 2, "invocationSlot": 1, "valueSlot": 2, "success": true},
                    {"op": "return", "value": 0},
                    {"op": "mutationPublish", "generation": 3, "pending": 0, "error": 1, "data": 2, "invocationSlot": 1, "valueSlot": 3, "success": false},
                    {"op": "return", "outcome": "failure", "value": 0}
                ]},
                {"frameSlots": 3, "parameterSlots": [0], "instructions": [
                    {"op": "capabilityRequest", "capability": "fetch", "request": {"url": 4, "method": "GET", "decode": "responseJson"}, "successPc": 1, "failurePc": 2, "resultSlot": 1, "errorSlot": 2},
                    {"op": "return", "value": 5},
                    {"op": "return", "outcome": "failure", "value": 6}
                ]}
            ]
        }))
        .unwrap()
    }

    #[test]
    fn call_frame_executes_action_handles_from_frame_slots_with_both_continuations() {
        let mut runtime = TypedRuntime::new(
            call_frame_application(),
            std::rc::Rc::new(std::cell::RefCell::new(None)),
        )
        .unwrap();
        let mut metrics = UpdateMetrics::default();

        runtime
            .execute_action_with_frame(
                0,
                &[(1, RuntimeValue::Number(1.0))],
                None,
                None,
                &mut metrics,
            )
            .unwrap();
        assert!(matches!(runtime.states[0], RuntimeValue::String(ref value) if value == "success"));

        runtime
            .execute_action_with_frame(
                0,
                &[(1, RuntimeValue::Number(2.0))],
                None,
                None,
                &mut metrics,
            )
            .unwrap();
        assert!(matches!(runtime.states[0], RuntimeValue::String(ref value) if value == "failure"));
    }

    #[test]
    #[cfg(feature = "fetch")]
    fn mutation_latest_success_cannot_be_overwritten_by_stale_completion() {
        let mut runtime = TypedRuntime::new(
            mutation_application(),
            std::rc::Rc::new(std::cell::RefCell::new(None)),
        )
        .unwrap();
        let context = ActionRunContext::default();
        let mut metrics = UpdateMetrics::default();
        let first = runtime
            .start_browser_action(
                0,
                vec![RuntimeValue::String("first".into()); 4],
                context.clone(),
                None,
                &mut metrics,
            )
            .unwrap();
        let Run::Suspended(first) = first else { panic!("expected first suspension") };
        let second = runtime
            .start_browser_action(
                0,
                vec![RuntimeValue::String("second".into()); 4],
                context.clone(),
                None,
                &mut metrics,
            )
            .unwrap();
        let Run::Suspended(second) = second else { panic!("expected second suspension") };

        let Run::Complete(ActionOutcome::Success(_)) = runtime
            .resume_browser_action(second, Ok(RuntimeValue::String("new".into())), context.clone())
            .unwrap()
        else {
            panic!("expected second completion");
        };
        assert_eq!(runtime.states[1], RuntimeValue::Null);
        assert_eq!(runtime.states[2], RuntimeValue::String("new".into()));
        assert_eq!(runtime.states[0], RuntimeValue::Bool(false));

        let Run::Complete(ActionOutcome::Success(_)) = runtime
            .resume_browser_action(first, Ok(RuntimeValue::String("old".into())), context)
            .unwrap()
        else {
            panic!("expected stale completion");
        };
        assert_eq!(runtime.states[2], RuntimeValue::String("new".into()));
    }

    #[test]
    #[cfg(feature = "fetch")]
    fn mutation_latest_failure_cannot_be_overwritten_by_stale_success() {
        let mut runtime = TypedRuntime::new(
            mutation_application(),
            std::rc::Rc::new(std::cell::RefCell::new(None)),
        )
        .unwrap();
        let context = ActionRunContext::default();
        let mut metrics = UpdateMetrics::default();
        let first = runtime
            .start_browser_action(
                0,
                vec![RuntimeValue::String("first".into()); 4],
                context.clone(),
                None,
                &mut metrics,
            )
            .unwrap();
        let Run::Suspended(first) = first else { panic!("expected first suspension") };
        let second = runtime
            .start_browser_action(
                0,
                vec![RuntimeValue::String("second".into()); 4],
                context.clone(),
                None,
                &mut metrics,
            )
            .unwrap();
        let Run::Suspended(second) = second else { panic!("expected second suspension") };

        let Run::Complete(ActionOutcome::Failure(_)) = runtime
            .resume_browser_action(second, Err(RuntimeValue::String("new error".into())), context.clone())
            .unwrap()
        else {
            panic!("expected second failure");
        };
        assert_eq!(runtime.states[1], RuntimeValue::String("new error".into()));
        assert_eq!(runtime.states[0], RuntimeValue::Bool(false));

        let Run::Complete(ActionOutcome::Success(_)) = runtime
            .resume_browser_action(first, Ok(RuntimeValue::String("old".into())), context)
            .unwrap()
        else {
            panic!("expected stale completion");
        };
        assert_eq!(runtime.states[1], RuntimeValue::String("new error".into()));
    }

    #[test]
    #[cfg(feature = "fetch")]
    fn mutation_stale_failure_cannot_overwrite_latest_success_state() {
        let mut runtime = TypedRuntime::new(
            mutation_application(),
            std::rc::Rc::new(std::cell::RefCell::new(None)),
        )
        .unwrap();
        let context = ActionRunContext::default();
        let mut metrics = UpdateMetrics::default();
        let first = runtime
            .start_browser_action(
                0,
                vec![RuntimeValue::String("first".into()); 4],
                context.clone(),
                None,
                &mut metrics,
            )
            .unwrap();
        let Run::Suspended(first) = first else {
            panic!("expected first suspension")
        };
        let second = runtime
            .start_browser_action(
                0,
                vec![RuntimeValue::String("second".into()); 4],
                context.clone(),
                None,
                &mut metrics,
            )
            .unwrap();
        let Run::Suspended(second) = second else {
            panic!("expected second suspension")
        };

        let Run::Complete(ActionOutcome::Success(_)) = runtime
            .resume_browser_action(
                second,
                Ok(RuntimeValue::String("new".into())),
                context.clone(),
            )
            .unwrap()
        else {
            panic!("expected second completion");
        };
        let Run::Complete(ActionOutcome::Failure(_error)) = runtime
            .resume_browser_action(
                first,
                Err(RuntimeValue::String("old error".into())),
                context,
            )
            .unwrap()
        else {
            panic!("expected stale first failure");
        };
        assert_eq!(runtime.states[1], RuntimeValue::Null);
        assert_eq!(runtime.states[2], RuntimeValue::String("new".into()));
        assert_eq!(runtime.states[0], RuntimeValue::Bool(false));
    }

    #[test]
    #[cfg(feature = "fetch")]
    fn mutation_sequential_invocations_advance_generation_and_settle_state() {
        let mut runtime = TypedRuntime::new(
            mutation_application(),
            std::rc::Rc::new(std::cell::RefCell::new(None)),
        )
        .unwrap();
        let context = ActionRunContext::default();
        let mut metrics = UpdateMetrics::default();

        let Run::Suspended(first) = runtime
            .start_browser_action(
                0,
                vec![RuntimeValue::String("first".into()); 4],
                context.clone(),
                None,
                &mut metrics,
            )
            .unwrap()
        else {
            panic!("expected first suspension");
        };
        assert_eq!(runtime.states[3], RuntimeValue::Number(1.0));
        assert_eq!(runtime.states[0], RuntimeValue::Bool(true));
        let Run::Complete(ActionOutcome::Success(_)) = runtime
            .resume_browser_action(
                first,
                Ok(RuntimeValue::String("one".into())),
                context.clone(),
            )
            .unwrap()
        else {
            panic!("expected first completion");
        };
        assert_eq!(runtime.states[3], RuntimeValue::Number(1.0));
        assert_eq!(runtime.states[0], RuntimeValue::Bool(false));
        assert_eq!(runtime.states[1], RuntimeValue::Null);
        assert_eq!(runtime.states[2], RuntimeValue::String("one".into()));

        let Run::Suspended(second) = runtime
            .start_browser_action(
                0,
                vec![RuntimeValue::String("second".into()); 4],
                context.clone(),
                None,
                &mut metrics,
            )
            .unwrap()
        else {
            panic!("expected second suspension");
        };
        assert_eq!(runtime.states[3], RuntimeValue::Number(2.0));
        assert_eq!(runtime.states[0], RuntimeValue::Bool(true));
        let Run::Complete(ActionOutcome::Success(_)) = runtime
            .resume_browser_action(second, Ok(RuntimeValue::String("two".into())), context)
            .unwrap()
        else {
            panic!("expected second completion");
        };
        assert_eq!(runtime.states[3], RuntimeValue::Number(2.0));
        assert_eq!(runtime.states[0], RuntimeValue::Bool(false));
        assert_eq!(runtime.states[1], RuntimeValue::Null);
        assert_eq!(runtime.states[2], RuntimeValue::String("two".into()));
    }

    #[test]
    #[cfg(feature = "fetch")]
    fn mutation_completion_after_graph_disposal_cannot_publish_state() {
        let runtime = TypedRuntime::new(
            mutation_application(),
            std::rc::Rc::new(std::cell::RefCell::new(None)),
        )
        .unwrap();
        let mut instance = TypedGraphInstance {
            parent_id: None,
            outlet_id: "outlet".into(),
            graph_id: "graph".into(),
            route_id: None,
            match_key: None,
            route_state: None,
            loader_data: None,
            loader_runtime: None,
            component_call: None,
            component_start: None,
            runtime,
        };
        let context = ActionRunContext::default();
        let mut metrics = UpdateMetrics::default();
        let suspended = instance
            .runtime
            .start_browser_action(
                0,
                vec![RuntimeValue::String("input".into()); 4],
                context,
                None,
                &mut metrics,
            )
            .unwrap();
        let Run::Suspended(_suspension) = suspended else { panic!("expected suspension") };
        assert_eq!(instance.runtime.states[0], RuntimeValue::Bool(true));
        // Capability completions capture the graph generation at suspension
        // time (fetch.rs/cookie.rs) and resolve the owning runtime through
        // `runtime_for_generation_mut` before resuming; disposal bumps the
        // generation, so the continuation holding `mutationPublish` must
        // never run.
        let suspended_generation = instance.runtime.graph_generation;
        instance.runtime.invalidate_fetches();
        assert!(instance
            .runtime_for_generation_mut(suspended_generation)
            .is_none());
        assert_eq!(instance.runtime.states[0], RuntimeValue::Bool(true));
        assert_eq!(instance.runtime.states[1], RuntimeValue::Null);
        assert_eq!(instance.runtime.states[2], RuntimeValue::Null);
    }

    #[test]
    #[cfg(feature = "fetch")]
    fn fetch_uses_json_stringify_results_as_raw_request_bodies() {
        let app: TypedApplication = serde_json::from_value(json!({
            "rootNode": 0,
            "strings": ["div", "title"],
            "constants": ["/todos", "Plec"],
            "nodes": [{"op": "element", "tag": 0, "parent": null}],
            "expressions": [
                {"instructions": [{"op": "constant", "constant": 0}, {"op": "return"}]},
                {"instructions": [
                    {"op": "constant", "constant": 1},
                    {"op": "makeRecord", "fields": [1]},
                    {"op": "string", "kind": "jsonStringify", "count": 1},
                    {"op": "return"}
                ]}
            ],
            "actions": [{
                "frameSlots": 2,
                "instructions": [
                    {"op": "capabilityRequest", "capability": "fetch", "request": {
                        "url": 0,
                        "method": "POST",
                        "body": 1,
                        "decode": "responseJson",
                        "requireOk": true
                    }, "successPc": 1, "failurePc": 2, "resultSlot": 0, "errorSlot": 1},
                    {"op": "return"},
                    {"op": "return", "outcome": "failure"}
                ]
            }]
        }))
        .unwrap();
        let mut runtime =
            TypedRuntime::new(app, std::rc::Rc::new(std::cell::RefCell::new(None))).unwrap();
        let mut metrics = UpdateMetrics::default();

        runtime
            .execute_action_with_frame(0, &[], None, None, &mut metrics)
            .unwrap();

        let first = runtime.take_pending_fetches();
        assert_eq!(first.len(), 1);
        let request = match &first[0].suspension.request {
            BrowserRequest::Fetch(request) => request,
            BrowserRequest::Cookie { .. } => panic!("expected fetch suspension"),
        };
        assert_eq!(request.url, "/todos");
        assert_eq!(request.method, "POST");
        assert_eq!(request.body.as_deref(), Some(r#"{"title":"Plec"}"#));
        assert_eq!(request.decode, "responseJson");
        assert!(request.require_ok);

        // A separate action run receives its own suspension and budget;
        // nothing from the first run leaks into the second.
        runtime
            .execute_action_with_frame(0, &[], None, None, &mut metrics)
            .unwrap();
        let second = runtime.take_pending_fetches();
        assert_eq!(second.len(), 1);
    }

    #[test]
    fn undeclared_cookie_request_is_rejected_before_suspension() {
        let app: TypedApplication = serde_json::from_value(json!({
            "rootNode": 0,
            "strings": ["div", "session"],
            "constants": [null],
            "nodes": [{"op": "element", "tag": 0, "parent": null}],
            "expressions": [
                {"instructions": [{"op": "constant", "constant": 0}, {"op": "return"}]}
            ],
            "actions": [{
                "frameSlots": 2,
                "instructions": [
                    {"op": "capabilityRequest", "capability": "cookie",
                     "request": {"operation": "get", "name": 1},
                     "successPc": 1, "failurePc": 1, "resultSlot": 0, "errorSlot": 1},
                    {"op": "return", "value": 0}
                ]
            }]
        }))
        .unwrap();
        let mut runtime =
            TypedRuntime::new(app, std::rc::Rc::new(std::cell::RefCell::new(None))).unwrap();
        let error = runtime
            .start_browser_action(
                0,
                vec![RuntimeValue::Null; 2],
                ActionRunContext::default(),
                None,
                &mut UpdateMetrics::default(),
            )
            .unwrap_err();
        assert_eq!(error.0, "cookie request is not declared");
        assert!(runtime.take_pending_cookies().is_empty());
    }

    #[cfg(feature = "fetch")]
    fn control_flow_application() -> TypedApplication {
        serde_json::from_value(json!({
            "rootNode": 0,
            "strings": ["div"],
            "constants": ["/api/data"],
            "nodes": [{"op": "element", "tag": 0, "parent": null}],
            "expressions": [
                {"instructions": [{"op": "constant", "constant": 0}, {"op": "return"}]},
                {"instructions": [{"op": "loadFrame", "slot": 0}, {"op": "return"}]},
                {"instructions": [{"op": "loadFrame", "slot": 1}, {"op": "return"}]},
                {"instructions": [{"op": "loadFrame", "slot": 2}, {"op": "return"}]},
                {"instructions": [{"op": "loadFrame", "slot": 3}, {"op": "return"}]}
            ],
            "actions": [
                {
                    "frameSlots": 4,
                    "instructions": [
                        {"op": "call", "action": 1, "arguments": [], "successPc": 1, "failurePc": 3, "resultSlot": 2, "errorSlot": 3},
                        {"op": "return", "value": 3},
                        {"op": "return"},
                        {"op": "return", "outcome": "failure", "value": 4}
                    ]
                },
                {
                    "frameSlots": 2,
                    "instructions": [
                        {"op": "capabilityRequest", "capability": "fetch",
                         "request": {"url": 0, "method": "GET", "decode": "responseJson"},
                         "successPc": 1, "failurePc": 2, "finallyPc": 3,
                         "resultSlot": 0, "errorSlot": 1},
                        {"op": "return", "value": 1},
                        {"op": "return", "outcome": "failure", "value": 2},
                        {"op": "return"}
                    ]
                }
            ]
        }))
        .unwrap()
    }

    #[cfg(feature = "fetch")]
    fn assert_same_fetch_suspension(
        browser: &Suspension<BrowserRequest>,
        loader: &Suspension<TypedLoaderFetchRequest>,
    ) {
        let request = match &browser.request {
            BrowserRequest::Fetch(request) => request,
            BrowserRequest::Cookie { .. } => panic!("expected fetch suspension"),
        };
        assert_eq!(request.url, loader.request.url);
        assert_eq!(request.method, loader.request.method);
        assert_eq!(request.headers, loader.request.headers);
        assert_eq!(request.body, loader.request.body);
        assert_eq!(request.decode, loader.request.decode);
        assert_eq!(request.require_ok, loader.request.require_ok);
        assert_eq!(browser.success_pc, loader.success_pc);
        assert_eq!(browser.failure_pc, loader.failure_pc);
        assert_eq!(browser.finally_pc, loader.finally_pc);
        assert_eq!(browser.result_slot, loader.result_slot);
        assert_eq!(browser.error_slot, loader.error_slot);
    }

    /// Runs one representative action program (local call with success and
    /// failure continuations, a fetch suspension with a deferred finalizer)
    /// through both browser and loader hosts and asserts the machines and
    /// hosts agree on every observable suspension field and terminal outcome.
    #[cfg(feature = "fetch")]
    fn resume_parity(result: Result<RuntimeValue, RuntimeValue>) -> ActionOutcome {
        let mut browser_runtime = TypedRuntime::new(
            control_flow_application(),
            std::rc::Rc::new(std::cell::RefCell::new(None)),
        )
        .unwrap();
        let mut loader_runtime = TypedRuntime::new(
            control_flow_application(),
            std::rc::Rc::new(std::cell::RefCell::new(None)),
        )
        .unwrap();

        let Run::Suspended(browser) = browser_runtime
            .start_browser_action(
                0,
                vec![RuntimeValue::Null; 4],
                ActionRunContext::default(),
                None,
                &mut UpdateMetrics::default(),
            )
            .unwrap()
        else {
            panic!("expected browser suspension");
        };
        let Run::Suspended(loader) = loader_runtime
            .start_loader_run(0, vec![RuntimeValue::Null; 4])
            .unwrap()
        else {
            panic!("expected loader suspension");
        };
        assert_same_fetch_suspension(&browser, &loader);

        let Run::Complete(browser_outcome) = browser_runtime
            .resume_browser_action(browser, result.clone(), ActionRunContext::default())
            .unwrap()
        else {
            panic!("expected browser completion");
        };
        let Run::Complete(loader_outcome) =
            loader_runtime.resume_loader_run(loader, result).unwrap()
        else {
            panic!("expected loader completion");
        };
        assert_eq!(browser_outcome, loader_outcome);
        browser_outcome
    }

    #[test]
    #[cfg(feature = "fetch")]
    fn browser_and_loader_hosts_agree_on_fetch_suspensions_and_outcomes() {
        let success = resume_parity(Ok(RuntimeValue::String("loaded".into())));
        assert!(matches!(
            &success,
            ActionOutcome::Success(RuntimeValue::String(value)) if value == "loaded"
        ));
        let failure = resume_parity(Err(RuntimeValue::String("denied".into())));
        assert!(matches!(
            &failure,
            ActionOutcome::Failure(RuntimeValue::String(value)) if value == "denied"
        ));
    }

    #[test]
    #[cfg(feature = "fetch")]
    fn budget_exhaustion_error_matches_between_hosts() {
        let app: TypedApplication = serde_json::from_value(json!({
            "rootNode": 0,
            "strings": ["div"],
            "constants": [],
            "nodes": [{"op": "element", "tag": 0, "parent": null}],
            "expressions": [],
            "actions": [{"frameSlots": 0, "instructions": [{"op": "jump", "target": 0}]}]
        }))
        .unwrap();
        let mut browser_runtime =
            TypedRuntime::new(app.clone(), std::rc::Rc::new(std::cell::RefCell::new(None)))
                .unwrap();
        let mut loader_runtime =
            TypedRuntime::new(app, std::rc::Rc::new(std::cell::RefCell::new(None))).unwrap();
        let browser_error = browser_runtime
            .start_browser_action(
                0,
                Vec::new(),
                ActionRunContext::default(),
                None,
                &mut UpdateMetrics::default(),
            )
            .unwrap_err();
        let loader_error = loader_runtime.start_loader_run(0, Vec::new()).unwrap_err();
        assert_eq!(browser_error.0, loader_error.0);
        assert_eq!(browser_error.0, "action execution budget exceeded");
    }

    #[test]
    #[cfg(feature = "fetch")]
    fn fetch_count_limit_error_matches_between_hosts() {
        let limit = plec_ir::limits::MAX_FETCHES_PER_ACTION;
        let mut instructions: Vec<_> = (0..=limit)
            .map(|index| {
                json!({
                    "op": "capabilityRequest", "capability": "fetch",
                    "request": {"url": 0, "method": "GET", "decode": "empty"},
                    "successPc": index + 1, "failurePc": index + 1,
                    "resultSlot": 0, "errorSlot": 1
                })
            })
            .collect();
        instructions.push(json!({"op": "return", "value": 0}));
        let app: TypedApplication = serde_json::from_value(json!({
            "rootNode": 0,
            "strings": ["div"],
            "constants": ["/api/data"],
            "nodes": [{"op": "element", "tag": 0, "parent": null}],
            "expressions": [
                {"instructions": [{"op": "constant", "constant": 0}, {"op": "return"}]}
            ],
            "actions": [{"frameSlots": 2, "instructions": instructions}]
        }))
        .unwrap();
        let mut browser_runtime =
            TypedRuntime::new(app.clone(), std::rc::Rc::new(std::cell::RefCell::new(None)))
                .unwrap();
        let mut loader_runtime =
            TypedRuntime::new(app, std::rc::Rc::new(std::cell::RefCell::new(None))).unwrap();

        let Run::Suspended(mut browser) = browser_runtime
            .start_browser_action(
                0,
                vec![RuntimeValue::Null; 2],
                ActionRunContext::default(),
                None,
                &mut UpdateMetrics::default(),
            )
            .unwrap()
        else {
            panic!("expected first suspension");
        };
        let browser_error = loop {
            match browser_runtime.resume_browser_action(
                browser,
                Ok(RuntimeValue::Null),
                ActionRunContext::default(),
            ) {
                Ok(Run::Suspended(next)) => browser = next,
                Ok(Run::Complete(_)) => panic!("expected the fetch limit to reject the run"),
                Err(error) => break error,
            }
        };
        let Run::Suspended(mut loader) = loader_runtime
            .start_loader_run(0, vec![RuntimeValue::Null; 2])
            .unwrap()
        else {
            panic!("expected first suspension");
        };
        let loader_error = loop {
            match loader_runtime.resume_loader_run(loader, Ok(RuntimeValue::Null)) {
                Ok(Run::Suspended(next)) => loader = next,
                Ok(Run::Complete(_)) => panic!("expected the fetch limit to reject the run"),
                Err(error) => break error,
            }
        };
        assert_eq!(browser_error.0, loader_error.0);
        assert_eq!(
            browser_error.0,
            format!("fetch count exceeds the {limit} per-action limit")
        );
    }

    #[test]
    #[cfg(feature = "fetch")]
    fn action_budget_spans_capability_suspensions() {
        // Two half-budget loops around one fetch: the run's single fuel
        // budget carries across the suspension, so the second loop exhausts
        // it after resume instead of starting from a fresh per-resume budget.
        let iterations = 180_000.0;
        let app: TypedApplication = serde_json::from_value(json!({
            "rootNode": 0,
            "strings": ["div"],
            "constants": [1.0, "/api/data"],
            "nodes": [{"op": "element", "tag": 0, "parent": null}],
            "expressions": [
                {"instructions": [{"op": "loadFrame", "slot": 0}, {"op": "constant", "constant": 0}, {"op": "binary", "kind": "subtract"}, {"op": "return"}]},
                {"instructions": [{"op": "loadFrame", "slot": 0}, {"op": "return"}]},
                {"instructions": [{"op": "loadFrame", "slot": 1}, {"op": "constant", "constant": 0}, {"op": "binary", "kind": "subtract"}, {"op": "return"}]},
                {"instructions": [{"op": "loadFrame", "slot": 1}, {"op": "return"}]},
                {"instructions": [{"op": "constant", "constant": 1}, {"op": "return"}]}
            ],
            "actions": [{
                "frameSlots": 4,
                "instructions": [
                    {"op": "evaluate", "expression": 0},
                    {"op": "storeFrame", "slot": 0},
                    {"op": "evaluate", "expression": 1},
                    {"op": "jumpIfFalse", "target": 5},
                    {"op": "jump", "target": 0},
                    {"op": "capabilityRequest", "capability": "fetch",
                     "request": {"url": 4, "method": "GET", "decode": "empty"},
                     "successPc": 6, "failurePc": 12, "resultSlot": 2, "errorSlot": 3},
                    {"op": "evaluate", "expression": 2},
                    {"op": "storeFrame", "slot": 1},
                    {"op": "evaluate", "expression": 3},
                    {"op": "jumpIfFalse", "target": 11},
                    {"op": "jump", "target": 6},
                    {"op": "return", "value": 2},
                    {"op": "return", "outcome": "failure", "value": 3}
                ]
            }]
        }))
        .unwrap();
        let mut runtime =
            TypedRuntime::new(app, std::rc::Rc::new(std::cell::RefCell::new(None))).unwrap();
        let Run::Suspended(suspension) = runtime
            .start_browser_action(
                0,
                vec![
                    RuntimeValue::Number(iterations),
                    RuntimeValue::Number(iterations),
                    RuntimeValue::Null,
                    RuntimeValue::Null,
                ],
                ActionRunContext::default(),
                None,
                &mut UpdateMetrics::default(),
            )
            .unwrap()
        else {
            panic!("expected suspension");
        };
        let error = runtime
            .resume_browser_action(
                suspension,
                Ok(RuntimeValue::Null),
                ActionRunContext::default(),
            )
            .unwrap_err();
        assert_eq!(error.0, "action execution budget exceeded");
    }

    // Error-path budget tests (action loop, self-requeuing reaction) live in
    // crates/plec-runtime/tests/untrusted_input_limits.rs: host binaries
    // cannot touch JsValue without tripping wasm-bindgen's non-wasm stubs.
}

impl TypedRuntime {
    pub fn mutate_collection(
        &mut self,
        input: usize,
        kind: &str,
        key: String,
        value: Option<RuntimeValue>,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let next = value.as_ref().and_then(|value| value.record().cloned());
        let collection = self.collections.entry(input).or_default();
        match kind {
            "append" => {
                let value = next
                    .ok_or_else(|| JsValue::from_str("collection append value must be a record"))?;
                if collection.rows.contains_key(&key) {
                    return Err(JsValue::from_str("collection append key already exists"));
                }
                collection.order.push(key.clone());
                collection.rows.insert(key, value);
            }
            "keyedReplace" => {
                let value = next.ok_or_else(|| {
                    JsValue::from_str("collection replace value must be a record")
                })?;
                if !collection.rows.contains_key(&key) {
                    return Err(JsValue::from_str("collection replace key missing"));
                }
                collection.rows.insert(key, value);
            }
            "keyedRemove" => {
                if value.is_some() {
                    return Err(JsValue::from_str("collection remove forbids a value"));
                }
                if collection.rows.remove(&key).is_none() {
                    return Err(JsValue::from_str("collection remove key missing"));
                }
                collection.order.retain(|entry| entry != &key);
            }
            _ => return Err(JsValue::from_str("unknown collection mutation kind")),
        }
        self.invalidate_collection(input, metrics)
    }
}

impl TypedRuntime {
    pub fn invalidate_collection(
        &mut self,
        input: usize,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let snapshot = self.collections.get(&input).cloned().unwrap_or_default();
        let targets = self
            .app
            .loops
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| (entry.input == Some(input)).then_some(index))
            .collect::<Vec<_>>();
        for loop_index in targets {
            if snapshot.order.len() > plec_ir::limits::MAX_LOOP_ROWS {
                return Err(JsValue::from_str("LOOP_ROW_LIMIT_EXCEEDED"));
            }
            let parent = self.parent_for_loop(loop_index)?;
            let projection = snapshot
                .order
                .iter()
                .filter_map(|key| {
                    snapshot
                        .rows
                        .get(key)
                        .cloned()
                        .map(|row| (key.clone(), row))
                })
                .collect();
            self.reconcile_loop(loop_index, &parent, projection, metrics)?;
        }
        Ok(())
    }
}

impl TypedRuntime {
    pub fn refresh_state(
        &mut self,
        state: usize,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let targets = self
            .app
            .dependency_edges
            .iter()
            .filter_map(|edge| {
                (edge.source.kind == "state" && edge.source.handle == state)
                    .then(|| (edge.target.kind.clone(), edge.target.handle))
            })
            .collect::<Vec<_>>();
        let mut row_updates = Vec::new();
        for (_, handle) in targets.iter().filter(|(kind, _)| kind == "conditional") {
            if self.conditionals.contains_key(handle) {
                self.reconcile_static_conditional(*handle, metrics)?;
            }
        }
        if targets.iter().any(|(kind, _)| kind == "conditional") {
            row_updates.extend(
                self.loops
                    .iter()
                    .flat_map(|(loop_index, rows)| {
                        rows.rows.keys().cloned().map(move |key| (*loop_index, key))
                    })
                    .collect::<Vec<_>>(),
            );
        }
        for target in targets
            .iter()
            .filter_map(|(kind, handle)| match kind.as_str() {
                "binding" => self.app.bindings.get(*handle).map(|binding| binding.target),
                "propProgram" => self
                    .app
                    .prop_programs
                    .get(*handle)
                    .map(|program| program.target),
                // Component-call targets have no direct DOM write here; the row
                // scan below queues their prop refreshes through the owner row.
                "component" => Some(*handle),
                _ => None,
            })
        {
            row_updates.extend(self.loops.iter().flat_map(|(loop_index, rows)| {
                rows.rows.iter().filter_map(move |(key, row)| {
                    (row.nodes.contains_key(&target)
                        || row
                            .conditionals
                            .values()
                            .any(|region| region.nodes.contains_key(&target)))
                    .then(|| (*loop_index, key.clone()))
                })
            }));
        }
        row_updates.sort();
        row_updates.dedup();
        for (loop_index, key) in row_updates {
            if let Some(values) = self
                .loops
                .get(&loop_index)
                .and_then(|rows| rows.rows.get(&key))
                .map(|row| row.values.clone())
            {
                self.update_typed_row(loop_index, &key, values, Some(state), metrics)?;
            }
        }
        for (kind, handle) in targets {
            if kind == "conditional" {
                continue;
            }
            if kind == "binding" {
                if let (Some(binding), Some(node)) = (
                    self.app.bindings.get(handle).cloned(),
                    self.nodes.get(&self.app.bindings[handle].target).cloned(),
                ) {
                    typed_apply_binding(
                        &self.app,
                        self.cookie_policy.borrow().as_ref(),
                        &binding,
                        &node,
                        &self.states,
                        None,
                        0,
                    )?;
                    metrics.dom_operations += 1;
                    metrics.bindings_touched += 1;
                }
            } else if kind == "propProgram" {
                if let Some(program) = self.app.prop_programs.get(handle).cloned() {
                    if let Some(node) = self.nodes.get(&program.target).cloned() {
                        for write in program.writes {
                            let value = match write.expression {
                                Some(expression) => typed_eval(
                                    &self.app,
                                    self.cookie_policy.borrow().as_ref(),
                                    expression,
                                    &self.states,
                                    None,
                                    0,
                                )?,
                                None => write
                                    .constant
                                    .and_then(|index| self.app.constants.get(index))
                                    .cloned()
                                    .unwrap_or_default(),
                            };
                            if write.spread {
                                typed_apply_spread(&self.app, &write.kind, &node, value)?;
                            } else {
                                typed_apply_value(
                                    &self.app,
                                    &write.kind,
                                    write.name,
                                    &node,
                                    value,
                                )?;
                            }
                            metrics.dom_operations += 1;
                        }
                    }
                }
            } else if kind == "loop" {
                let parent = self.parent_for_loop(handle)?;
                self.render_loop(handle, &parent)?;
            }
        }
        // ponytail: scans mounted calls; index component dependency edges if profiles require it.
        self.queue_static_component_refreshes()?;
        self.queue_reactions_from("state", state);
        self.drain_reactions(metrics)?;
        Ok(())
    }

    fn queue_reactions_from(&mut self, kind: &str, handle: usize) {
        for edge in &self.app.dependency_edges {
            if edge.source.kind == kind
                && edge.source.handle == handle
                && edge.target.kind == "reaction"
                && !self.pending_reactions.contains(&edge.target.handle)
            {
                self.pending_reactions.push(edge.target.handle);
            }
        }
    }

    fn drain_reactions(&mut self, metrics: &mut UpdateMetrics) -> Result<(), JsValue> {
        // A reaction that (transitively) re-queues itself would otherwise
        // drain forever. The step budget bounds total work per drain and the
        // depth bound rejects the recursive drain chains a refresh_state
        // triggers from inside a reaction.
        if self.reaction_drain_depth >= plec_ir::limits::MAX_REACTION_DRAIN_DEPTH {
            return Err(JsValue::from_str("reaction drain depth exceeds limit"));
        }
        self.reaction_drain_depth += 1;
        let result = self.drain_reactions_bounded(metrics);
        self.reaction_drain_depth -= 1;
        result
    }

    fn drain_reactions_bounded(&mut self, metrics: &mut UpdateMetrics) -> Result<(), JsValue> {
        let mut budget = plec_ir::limits::MAX_REACTION_STEPS;
        while let Some(reaction) = self.pending_reactions.first().copied() {
            if budget == 0 {
                return Err(JsValue::from_str("reaction execution budget exceeded"));
            }
            budget -= 1;
            self.pending_reactions.remove(0);
            let reaction_def = self
                .app
                .reactions
                .get(reaction)
                .cloned()
                .ok_or_else(|| JsValue::from_str("reaction handle out of range"))?;
            if reaction_def
                .dependencies
                .iter()
                .any(|dependency| *dependency >= self.app.expressions.len())
            {
                return Err(JsValue::from_str("reaction dependency out of range"));
            }
            if let Some(cleanup) = self
                .reaction_cleanups
                .get_mut(reaction)
                .and_then(Option::take)
            {
                self.execute_action(cleanup, &[], None, None, metrics)?;
            }
            self.execute_action(reaction_def.action, &[], None, None, metrics)?;
            if let Some(slot) = self.reaction_cleanups.get_mut(reaction) {
                *slot = reaction_def.cleanup_action;
            }
        }
        Ok(())
    }

    pub fn refresh_prop(
        &mut self,
        prop: usize,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let targets = self
            .app
            .dependency_edges
            .iter()
            .filter_map(|edge| {
                (edge.source.kind == "prop" && edge.source.handle == prop)
                    .then(|| (edge.target.kind.clone(), edge.target.handle))
            })
            .collect::<Vec<_>>();
        for (_, handle) in targets.iter().filter(|(kind, _)| kind == "conditional") {
            if self.conditionals.contains_key(handle) {
                self.reconcile_static_conditional(*handle, metrics)?;
            }
        }
        for (kind, handle) in targets {
            match kind.as_str() {
                "binding" => {
                    if let (Some(binding), Some(node)) = (
                        self.app.bindings.get(handle).cloned(),
                        self.app
                            .bindings
                            .get(handle)
                            .and_then(|binding| self.nodes.get(&binding.target))
                            .cloned(),
                    ) {
                        typed_apply_binding(
                            &self.app,
                            self.cookie_policy.borrow().as_ref(),
                            &binding,
                            &node,
                            &self.states,
                            None,
                            0,
                        )?;
                        metrics.dom_operations += 1;
                        metrics.bindings_touched += 1;
                    }
                }
                "propProgram" => {
                    if let Some(program) = self.app.prop_programs.get(handle).cloned() {
                        if let Some(node) = self.nodes.get(&program.target).cloned() {
                            for write in program.writes {
                                let value = write
                                    .expression
                                    .map(|expression| {
                                        typed_eval(
                                            &self.app,
                                            self.cookie_policy.borrow().as_ref(),
                                            expression,
                                            &self.states,
                                            None,
                                            0,
                                        )
                                    })
                                    .transpose()?
                                    .or_else(|| {
                                        write
                                            .constant
                                            .and_then(|index| self.app.constants.get(index))
                                            .cloned()
                                    })
                                    .unwrap_or_default();
                                if write.spread {
                                    typed_apply_spread(&self.app, &write.kind, &node, value)?;
                                } else {
                                    typed_apply_value(
                                        &self.app,
                                        &write.kind,
                                        write.name,
                                        &node,
                                        value,
                                    )?;
                                }
                                metrics.dom_operations += 1;
                            }
                        }
                    }
                }
                "loop" => {
                    let parent = self.parent_for_loop(handle)?;
                    self.render_loop(handle, &parent)?;
                }
                _ => {}
            }
        }
        self.queue_static_component_refreshes()?;
        self.queue_reactions_from("prop", prop);
        self.drain_reactions(metrics)?;
        Ok(())
    }
}
