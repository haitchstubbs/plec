use crate::prelude::*;
use crate::runtime::*;
use crate::vm::*;
use plec_action::{Run, Suspension};
use plec_dom::cookie::CookiePolicy;
use plec_dom::platform::document;
use plec_schema::typed::TypedCookieRequest;
use wasm_bindgen::JsCast;

#[derive(Clone)]
pub struct TypedPendingCookie {
    pub instance_id: String,
    pub request_id: u64,
    pub suspension: Suspension<BrowserRequest>,
    pub context: ActionRunContext,
    pub graph_generation: u64,
    /// Entry action of the suspended run carried the route-loader flag; a
    /// later fetch suspension in the same run must complete through the
    /// shared loader pipeline.
    pub route_loader: bool,
}

impl TypedRuntime {
    pub fn take_pending_cookies(&mut self) -> Vec<TypedPendingCookie> {
        std::mem::take(&mut self.pending_cookies)
    }
}

impl RuntimeState {
    pub fn start_typed_cookie(&self, mut pending: TypedPendingCookie) -> Result<(), JsValue> {
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
        let (request, value) = match &pending.suspension.request {
            BrowserRequest::Cookie { request, value } => (request.clone(), value.clone()),
            #[cfg(feature = "fetch")]
            BrowserRequest::Fetch(_) => {
                return Err(JsValue::from_str(
                    "fetch suspension routed through cookie transport",
                ));
            }
        };
        let name = self
            .typed
            .borrow()
            .get(&pending.instance_id)
            .and_then(|typed| typed.runtime.app.strings.get(request.name))
            .cloned()
            .ok_or_else(|| JsValue::from_str("cookie name handle out of range"))?;
        let instance_id = pending.instance_id.clone();
        let request_id = pending.request_id;
        let result = self.execute_cookie(&name, &request, value.as_deref());
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
        // Default-deny: the host-owned policy is the only grant source. A
        // missing policy or a missing entry for this name denies the
        // operation even when the artifact declares a matching capability.
        let policy = {
            let entries = self.cookie_policy.borrow();
            entries
                .as_ref()
                .and_then(|entries| entries.get(name))
                .cloned()
        };
        let Some(policy) = policy else {
            return Err(cookie_error("cookie name denied by runtime policy"));
        };
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

    pub fn complete_typed_cookie(
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
        let graph_generation = pending.graph_generation;
        let route_loader = pending.route_loader;
        let run = {
            let mut typed = self.typed.borrow_mut();
            let runtime = &mut typed
                .get_mut(&instance_id)
                .ok_or_else(|| JsValue::from_str("typed application missing"))?
                .runtime;
            if runtime.graph_generation != graph_generation || runtime.root.is_none() {
                return Ok(());
            }
            runtime
                .resume_browser_action(pending.suspension, result, pending.context.clone())
                .map_err(|error| JsValue::from_str(&error.to_string()))?
        };
        if let Run::Suspended(suspension) = run {
            let next = pending_browser_capability(
                instance_id,
                graph_generation,
                route_loader,
                suspension,
                pending.context,
            );
            self.start_browser_capability(next)?;
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

impl RuntimeState {
    /// Stores the host-owned cookie capability policy on this runtime only.
    /// Both asynchronous cookie operations and synchronous `getSync` reads
    /// gate on this per-runtime store; no cross-runtime global exists.
    pub fn set_cookie_policy(&self, policy: JsValue) -> Result<(), JsValue> {
        let policy: Option<HashMap<String, CookiePolicy>> =
            serde_wasm_bindgen::from_value(policy).map_err(error)?;
        *self.cookie_policy.borrow_mut() = policy;
        Ok(())
    }
}
