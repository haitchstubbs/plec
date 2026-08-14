use crate::runtime::lifecycle::*;

pub(crate) fn document() -> Result<Document, JsValue> {
    web_sys::window()
        .ok_or_else(|| JsValue::from_str("window unavailable"))?
        .document()
        .ok_or_else(|| JsValue::from_str("document unavailable"))
}

pub(crate) fn window() -> Result<web_sys::Window, JsValue> {
    web_sys::window().ok_or_else(|| JsValue::from_str("window unavailable"))
}

pub(crate) fn now() -> f64 {
    web_sys::window()
        .and_then(|window| window.performance())
        .map(|performance| performance.now())
        .unwrap_or(0.0)
}
