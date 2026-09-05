//! Cookie document access and the active cookie policy.
//!
//! The policy store lives here (not in the runtime state) because
//! `read_sync_cookie` is called from the expression VM during evaluation,
//! outside any runtime call frame; `set_cookie_policy` on the runtime
//! facade publishes into it.

use crate::platform::document;
use plec_schema::delta::RuntimeValue;
use serde::Deserialize;
use std::cell::RefCell;
use std::collections::HashMap;
use wasm_bindgen::{JsCast, JsValue};

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CookiePolicy {
    pub operations: Vec<String>,
    #[serde(default)]
    pub path: Option<String>,
}

thread_local! {
    static ACTIVE_COOKIE_POLICY: RefCell<Option<HashMap<String, CookiePolicy>>> = RefCell::new(None);
}

/// Publishes the runtime's cookie policy so synchronous cookie reads
/// evaluate against the same gate as asynchronous cookie operations.
pub fn set_active_cookie_policy(policy: Option<HashMap<String, CookiePolicy>>) {
    ACTIVE_COOKIE_POLICY.with(|active| *active.borrow_mut() = policy);
}

pub fn read_sync_cookie(name: &str) -> Result<RuntimeValue, JsValue> {
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
