use crate::runtime::lifecycle::*;
use crate::schema::typed::TypedCookieRequest;
use crate::typed::runtime::*;
use crate::typed::vm::*;
use crate::dom::platform::window;

#[derive(Clone)]
pub(crate) struct TypedPendingCookie {
    pub(crate) instance_id: String,
    pub(crate) request_id: u64,
    pub(crate) continuation: TypedContinuationStack,
    pub(crate) success_pc: usize,
    pub(crate) failure_pc: usize,
    pub(crate) finally_pc: Option<usize>,
    pub(crate) result_slot: usize,
    pub(crate) error_slot: usize,
    pub(crate) request: TypedCookieRequest,
    pub(crate) value: Option<String>,
    pub(crate) graph_generation: u64,
}

impl TypedRuntime {
    pub(crate) fn take_pending_cookies(&mut self) -> Vec<TypedPendingCookie> { std::mem::take(&mut self.pending_cookies) }
}

impl PlecRuntime {
    pub(crate) fn start_typed_cookie(&self, mut pending: TypedPendingCookie) -> Result<(), JsValue> {
        {
            let mut typed = self.typed.borrow_mut();
            let runtime = &mut typed.get_mut(&pending.instance_id).ok_or_else(|| JsValue::from_str("typed application missing"))?.runtime;
            if runtime.graph_generation != pending.graph_generation { return Ok(()); }
            runtime.next_cookie_id += 1;
            pending.request_id = runtime.next_cookie_id;
        }
        let name = self.typed.borrow().get(&pending.instance_id).and_then(|typed| typed.runtime.app.strings.get(pending.request.name)).cloned().ok_or_else(|| JsValue::from_str("cookie name handle out of range"))?;
        let detail = serde_wasm_bindgen::to_value(&serde_json::json!({ "instanceId": pending.instance_id, "requestId": pending.request_id, "operation": pending.request.operation, "name": name, "value": pending.value, "path": pending.request.path, "sameSite": pending.request.same_site, "secure": pending.request.secure, "expiry": pending.request.expiry, "maxAge": pending.request.max_age })).map_err(error)?;
        let init = web_sys::CustomEventInit::new(); init.set_detail(&detail);
        self.typed.borrow_mut().get_mut(&pending.instance_id).ok_or_else(|| JsValue::from_str("typed application missing"))?.runtime.pending_cookies.push(pending);
        window()?.dispatch_event(&web_sys::CustomEvent::new_with_event_init_dict("plec:cookie-request", &init)?.into())?;
        Ok(())
    }

    pub(crate) fn complete_typed_cookie(&self, instance_id: String, request_id: u64, result: Result<RuntimeValue, RuntimeValue>) -> Result<(), JsValue> {
        let pending = self.typed.borrow_mut().get_mut(&instance_id).and_then(|typed| {
            let index = typed.runtime.pending_cookies.iter().position(|pending| pending.request_id == request_id)?;
            Some(typed.runtime.pending_cookies.remove(index))
        });
        let Some(pending) = pending else { return Ok(()); };
        let more = {
            let mut typed = self.typed.borrow_mut(); let runtime = &mut typed.get_mut(&instance_id).unwrap().runtime;
            if runtime.graph_generation != pending.graph_generation || runtime.root.is_none() { return Ok(()); }
            let mut continuation = pending.continuation;
            let pc = match result { Ok(value) => { continuation.current.frame[pending.result_slot] = value; pending.success_pc }, Err(error) => { continuation.current.frame[pending.error_slot] = error; pending.failure_pc } };
            continuation.current.pc = pc;
            runtime.execute_continuation(continuation.clone(), None, &mut UpdateMetrics::default())?;
            let mut more = runtime.take_pending_cookies();
            if more.is_empty() {
                if let Some(finally_pc) = pending.finally_pc {
                    runtime.execute_action_at(continuation.current.action, finally_pc, continuation.current.frame, &continuation.current.event, continuation.current.row, None, &mut UpdateMetrics::default())?;
                    more.extend(runtime.take_pending_cookies());
                }
            }
            more
        };
        for mut next in more { next.instance_id = instance_id.clone(); self.start_typed_cookie(next)?; }
        self.install_typed_event_listeners()?;
        Ok(())
    }
}

#[wasm_bindgen::prelude::wasm_bindgen]
impl PlecRuntime {
    pub fn complete_cookie_request(&self, instance_id: String, request_id: u64, value: JsValue, failure: Option<String>) -> Result<(), JsValue> {
        let result = match failure { Some(message) => Err(RuntimeValue::Record(HashMap::from([("kind".into(), RuntimeValue::String("cookie".into())), ("message".into(), RuntimeValue::String(message))]))), None => Ok(serde_wasm_bindgen::from_value(value).map_err(error)?), };
        self.complete_typed_cookie(instance_id, request_id, result)
    }
}
