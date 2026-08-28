use crate::dom::platform::document;
use crate::runtime::lifecycle::*;
use crate::schema::typed::TypedCookieRequest;
use crate::typed::runtime::*;
use crate::typed::vm::*;
use serde::Deserialize;
use std::cell::RefCell;
use wasm_bindgen::JsCast;

thread_local! {
    static ACTIVE_COOKIE_POLICY: RefCell<Option<HashMap<String, CookiePolicy>>> = RefCell::new(None);
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CookiePolicy {
    pub(crate) operations: Vec<String>,
    #[serde(default)]
    pub(crate) path: Option<String>,
}

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
    pub(crate) fn take_pending_cookies(&mut self) -> Vec<TypedPendingCookie> {
        std::mem::take(&mut self.pending_cookies)
    }
}

impl PlecRuntime {
    pub(crate) fn start_typed_cookie(
        &self,
        mut pending: TypedPendingCookie,
    ) -> Result<(), JsValue> {
        {
            let mut typed = self.typed.borrow_mut();
            let runtime = &mut typed
                .get_mut(&pending.instance_id)
                .ok_or_else(|| JsValue::from_str("typed application missing"))?
                .runtime;
            if runtime.graph_generation != pending.graph_generation {
                return Ok(());
            }
            runtime.next_cookie_id += 1;
            pending.request_id = runtime.next_cookie_id;
        }
        let name = self
            .typed
            .borrow()
            .get(&pending.instance_id)
            .and_then(|typed| typed.runtime.app.strings.get(pending.request.name))
            .cloned()
            .ok_or_else(|| JsValue::from_str("cookie name handle out of range"))?;
        let instance_id = pending.instance_id.clone();
        let request_id = pending.request_id;
        let result = self.execute_cookie(&name, &pending.request, pending.value.as_deref());
        self.typed
            .borrow_mut()
            .get_mut(&pending.instance_id)
            .ok_or_else(|| JsValue::from_str("typed application missing"))?
            .runtime
            .pending_cookies
            .push(pending);
        self.complete_typed_cookie(instance_id, request_id, result)
    }

    fn execute_cookie(
        &self,
        name: &str,
        request: &TypedCookieRequest,
        value: Option<&str>,
    ) -> Result<RuntimeValue, RuntimeValue> {
        if let Some(policy) = self
            .cookie_policy
            .borrow()
            .as_ref()
            .and_then(|entries| entries.get(name))
        {
            if !policy
                .operations
                .iter()
                .any(|operation| operation == &request.operation)
            {
                return Err(cookie_error("cookie operation denied by runtime policy"));
            }
            if policy
                .path
                .as_deref()
                .is_some_and(|path| path != request.path)
            {
                return Err(cookie_error("cookie path denied by runtime policy"));
            }
        }
        let document: web_sys::HtmlDocument = document()
            .map_err(|error| {
                cookie_error(
                    &error
                        .as_string()
                        .unwrap_or_else(|| "cookie document unavailable".into()),
                )
            })?
            .dyn_into()
            .map_err(|_| cookie_error("HTML document unavailable"))?;
        let encoded_name = js_sys::encode_uri_component(name)
            .as_string()
            .ok_or_else(|| cookie_error("cookie name encoding failed"))?;
        if request.operation == "get" {
            let prefix = format!("{encoded_name}=");
            let value = document
                .cookie()
                .map_err(|error| {
                    cookie_error(
                        &error
                            .as_string()
                            .unwrap_or_else(|| "cookie read failed".into()),
                    )
                })?
                .split(';')
                .map(str::trim)
                .find_map(|entry| entry.strip_prefix(&prefix))
                .map(|encoded| {
                    js_sys::decode_uri_component(encoded)
                        .unwrap_or_else(|_| encoded.into())
                        .as_string()
                        .unwrap_or_default()
                });
            return Ok(value
                .map(RuntimeValue::String)
                .unwrap_or(RuntimeValue::Null));
        }
        let encoded_value = js_sys::encode_uri_component(if request.operation == "delete" {
            ""
        } else {
            value.unwrap_or("")
        })
        .as_string()
        .ok_or_else(|| cookie_error("cookie value encoding failed"))?;
        let mut attributes = vec![format!("path={}", request.path)];
        if request.expiry == "maxAge" {
            attributes.push(format!("max-age={}", request.max_age.unwrap_or(0)));
        }
        if let Some(same_site) = &request.same_site {
            attributes.push(format!("samesite={same_site}"));
        }
        if request.secure == Some(true) {
            attributes.push("secure".into());
        }
        document
            .set_cookie(&format!(
                "{encoded_name}={encoded_value}; {}",
                attributes.join("; ")
            ))
            .map_err(|error| {
                cookie_error(
                    &error
                        .as_string()
                        .unwrap_or_else(|| "cookie write failed".into()),
                )
            })?;
        Ok(RuntimeValue::Null)
    }

    pub(crate) fn complete_typed_cookie(
        &self,
        instance_id: String,
        request_id: u64,
        result: Result<RuntimeValue, RuntimeValue>,
    ) -> Result<(), JsValue> {
        let pending = self
            .typed
            .borrow_mut()
            .get_mut(&instance_id)
            .and_then(|typed| {
                let index = typed
                    .runtime
                    .pending_cookies
                    .iter()
                    .position(|pending| pending.request_id == request_id)?;
                Some(typed.runtime.pending_cookies.remove(index))
            });
        let Some(pending) = pending else {
            return Ok(());
        };
        let more = {
            let mut typed = self.typed.borrow_mut();
            let runtime = &mut typed.get_mut(&instance_id).unwrap().runtime;
            if runtime.graph_generation != pending.graph_generation || runtime.root.is_none() {
                return Ok(());
            }
            let mut continuation = pending.continuation;
            let pc = match result {
                Ok(value) => {
                    continuation.current.frame[pending.result_slot] = value;
                    pending.success_pc
                }
                Err(error) => {
                    continuation.current.frame[pending.error_slot] = error;
                    pending.failure_pc
                }
            };
            continuation.current.pc = pc;
            runtime.execute_continuation(
                continuation.clone(),
                None,
                &mut UpdateMetrics::default(),
            )?;
            let mut more = runtime.take_pending_cookies();
            if more.is_empty() {
                if let Some(finally_pc) = pending.finally_pc {
                    runtime.execute_action_at(
                        continuation.current.action,
                        finally_pc,
                        continuation.current.frame,
                        &continuation.current.event,
                        continuation.current.row,
                        None,
                        &mut UpdateMetrics::default(),
                    )?;
                    more.extend(runtime.take_pending_cookies());
                }
            }
            more
        };
        for mut next in more {
            next.instance_id = instance_id.clone();
            self.start_typed_cookie(next)?;
        }
        self.install_typed_event_listeners()?;
        Ok(())
    }
}

fn cookie_error(message: &str) -> RuntimeValue {
    RuntimeValue::Record(HashMap::from([
        ("kind".into(), RuntimeValue::String("cookie".into())),
        ("message".into(), RuntimeValue::String(message.into())),
    ]))
}

pub(crate) fn read_sync_cookie(name: &str) -> Result<RuntimeValue, JsValue> {
    let allowed = ACTIVE_COOKIE_POLICY.with(|policy| {
        policy
            .borrow()
            .as_ref()
            .and_then(|entries| entries.get(name))
            .map(|entry| {
                entry
                    .operations
                    .iter()
                    .any(|operation| operation == "getSync")
            })
            .unwrap_or(true)
    });
    if !allowed {
        return Err(JsValue::from_str(
            "cookie operation denied by runtime policy",
        ));
    }
    let document: web_sys::HtmlDocument = document()?
        .dyn_into()
        .map_err(|_| JsValue::from_str("HTML document unavailable"))?;
    let encoded_name = js_sys::encode_uri_component(name)
        .as_string()
        .ok_or_else(|| JsValue::from_str("cookie name encoding failed"))?;
    let prefix = format!("{encoded_name}=");
    Ok(document
        .cookie()?
        .split(';')
        .map(str::trim)
        .find_map(|entry| entry.strip_prefix(&prefix))
        .map(|encoded| {
            js_sys::decode_uri_component(encoded)
                .unwrap_or_else(|_| encoded.into())
                .as_string()
                .unwrap_or_default()
        })
        .map(RuntimeValue::String)
        .unwrap_or(RuntimeValue::Null))
}

#[wasm_bindgen::prelude::wasm_bindgen]
impl PlecRuntime {
    pub fn set_cookie_policy(&self, policy: JsValue) -> Result<(), JsValue> {
        let policy: Option<HashMap<String, CookiePolicy>> =
            serde_wasm_bindgen::from_value(policy).map_err(error)?;
        *self.cookie_policy.borrow_mut() = policy.clone();
        ACTIVE_COOKIE_POLICY.with(|active| *active.borrow_mut() = policy);
        Ok(())
    }
}
