//! Cookie document access and the synchronous `getSync` capability gate.
//!
//! The gate is a pure function of the calling runtime's host-owned policy:
//! `read_sync_cookie` takes the policy explicitly, because the expression VM
//! may evaluate on behalf of any `PlecRuntime` on this thread. Policy state
//! lives on `RuntimeState` (plec-client); the runtime's evaluation entry
//! points thread it into the VM. There is no process-global policy store.

use crate::platform::document;
use plec_schema::delta::RuntimeValue;
use serde::Deserialize;
use std::collections::HashMap;
use wasm_bindgen::{JsCast, JsValue};

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CookiePolicy {
    pub operations: Vec<String>,
    #[serde(default)]
    pub path: Option<String>,
}

/// Map from cookie name to the host grant for that name, as configured on a
/// single `PlecRuntime` via `set_cookie_policy`.
pub type CookiePolicyMap = HashMap<String, CookiePolicy>;

pub fn read_sync_cookie(
    policy: Option<&CookiePolicyMap>,
    name: &str,
) -> Result<RuntimeValue, JsValue> {
    // Default-deny: only a host-owned policy entry on the calling runtime
    // that explicitly lists `getSync` grants a synchronous read. An absent
    // policy, a missing entry, or an unlisted operation all deny;
    // artifact-declared capabilities never grant authority on their own.
    let allowed = policy
        .and_then(|entries| entries.get(name))
        .is_some_and(|entry| entry.operations.iter().any(|operation| operation == "getSync"));
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
