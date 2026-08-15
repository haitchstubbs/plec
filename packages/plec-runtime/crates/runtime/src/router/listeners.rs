use crate::dom::platform::*;
use crate::runtime::lifecycle::*;

pub(crate) struct RouterListener {
    pub(crate) target: EventTarget,
    pub(crate) event_type: String,
    pub(crate) callback: Closure<dyn FnMut(Event)>,
}

impl PlecRuntime {
    pub(crate) fn install_router_listeners(&self) -> Result<(), JsValue> {
        let document = document()?;
        let document_target: EventTarget = document.clone().into();
        let runtime: *const PlecRuntime = self;
        let click = Closure::wrap(Box::new(move |event: Event| {
            let Ok(mouse) = event.clone().dyn_into::<MouseEvent>() else {
                return;
            };
            if event.default_prevented()
                || mouse.button() != 0
                || mouse.meta_key()
                || mouse.ctrl_key()
                || mouse.shift_key()
                || mouse.alt_key()
            {
                return;
            }
            let Some(target) = event
                .target()
                .and_then(|target| target.dyn_into::<Element>().ok())
            else {
                return;
            };
            let Ok(Some(anchor)) = target.closest("a[href]") else {
                return;
            };
            if anchor.get_attribute("target").is_some() || anchor.has_attribute("download") {
                return;
            }
            let Some(href) = anchor.get_attribute("href") else {
                return;
            };
            if href.starts_with("//") {
                return;
            }
            let href = if href.starts_with('/') || href.starts_with('?') || href.starts_with('#') {
                href
            } else if let Ok(origin) = window().and_then(|window| window.location().origin()) {
                let Some(path) = href.strip_prefix(&origin) else { return };
                path.to_string()
            } else {
                return;
            };
            event.prevent_default();
            unsafe {
                if let Some(runtime) = runtime.as_ref() {
                    if let Some(root) = runtime.typed_root.borrow().clone() {
                        let _ = runtime.navigate_typed_route(&href, root, false, true);
                    } else { let _ = runtime.navigate_internal(&href, false); }
                }
            }
        }) as Box<dyn FnMut(Event)>);
        document_target
            .add_event_listener_with_callback("click", click.as_ref().unchecked_ref())?;
        self.router_listeners.borrow_mut().push(RouterListener {
            target: document_target,
            event_type: "click".into(),
            callback: click,
        });
        let window_target: EventTarget = window()?.into();
        let runtime: *const PlecRuntime = self;
        let popstate = Closure::wrap(Box::new(move |_event: Event| unsafe {
            if let Some(runtime) = runtime.as_ref() {
                if let Ok(location) = window().and_then(|window| {
                    let location = window.location();
                    Ok(format!("{}{}{}", location.pathname()?, location.search()?, location.hash()?))
                }) {
                    if let Some(root) = runtime.typed_root.borrow().clone() {
                        let _ = runtime.navigate_typed_route(&location, root, false, false);
                    } else { let _ = runtime.navigate_internal(&location, true); }
                }
            }
        }) as Box<dyn FnMut(Event)>);
        window_target
            .add_event_listener_with_callback("popstate", popstate.as_ref().unchecked_ref())?;
        self.router_listeners.borrow_mut().push(RouterListener {
            target: window_target,
            event_type: "popstate".into(),
            callback: popstate,
        });
        Ok(())
    }
}

impl PlecRuntime {
    pub(crate) fn dispose_router_listeners(&self) {
        for listener in self.router_listeners.borrow_mut().drain(..) {
            let _ = listener.target.remove_event_listener_with_callback(
                &listener.event_type,
                listener.callback.as_ref().unchecked_ref(),
            );
        }
    }
}
