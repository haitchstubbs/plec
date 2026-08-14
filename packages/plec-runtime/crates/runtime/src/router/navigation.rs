use crate::dom::platform::*;
use crate::runtime::lifecycle::*;
use crate::typed::runtime::*;

impl PlecRuntime {
    pub(crate) fn navigate_internal(&self, href: &str, replace: bool) -> Result<(), JsValue> {
        let state = self
            .router
            .borrow()
            .clone()
            .ok_or_else(|| JsValue::from_str("runtime has not been started"))?;
        let pathname = href.split(['?', '#']).next().unwrap_or(href);
        let route_path = pathname.trim_start_matches('/');
        let route = state
            .manifest
            .routes
            .iter()
            .find(|route| route.path == route_path)
            .or_else(|| state.manifest.routes.iter().find(|route| route.path == "*"))
            .cloned()
            .ok_or_else(|| JsValue::from_str("no compiled route for pathname"))?;
        let history = window()?.history()?;
        if replace {
            history.replace_state_with_url(&JsValue::NULL, "", Some(pathname))?;
        } else {
            history.push_state_with_url(&JsValue::NULL, "", Some(pathname))?;
        }
        self.refresh_navigation_state(pathname)?;
        let instance_id =
            self.replace_outlet_instance(state.root_instance_id, route.outlet_id, route.graph_id)?;
        if let Some(mut loader) = route.loader {
            if let Some(slot) = route.loader_state_slot_id {
                for operation in &mut loader.operations {
                    if operation.get("kind").and_then(Value::as_str) != Some("capability-request") {
                        continue;
                    }
                    let result = operation
                        .get("successResultName")
                        .and_then(Value::as_str)
                        .unwrap_or("result")
                        .to_string();
                    let success = operation
                        .as_object_mut()
                        .and_then(|operation| operation.get_mut("success"))
                        .and_then(Value::as_array_mut);
                    if let Some(success) = success {
                        success.push(serde_json::json!({
                            "kind": "set-state", "stateSlotId": slot,
                            "value": { "kind": "identifier", "name": result },
                        }));
                    }
                }
            }
            let app = self.app_for_instance(&instance_id)?;
            self.execute_action_operations(
                &instance_id,
                &app,
                &loader.operations,
                &HashMap::new(),
                None,
                &mut UpdateMetrics::default(),
                None,
            )?;
        }
        Ok(())
    }
}

impl PlecRuntime {
    pub(crate) fn refresh_navigation_state(&self, pathname: &str) -> Result<(), JsValue> {
        let links = document()?.query_selector_all("a[href]")?;
        for index in 0..links.length() {
            let Some(node) = links.item(index) else {
                continue;
            };
            let Ok(link) = node.dyn_into::<Element>() else {
                continue;
            };
            let href = link.get_attribute("href").unwrap_or_default();
            if href == pathname {
                link.set_attribute("aria-current", "page")?;
            } else {
                link.remove_attribute("aria-current")?;
            }
        }
        Ok(())
    }
}

impl PlecRuntime {
    pub(crate) fn navigate_typed_route(&self, href: &str, root: Element) -> Result<(), JsValue> {
        let manifest = self
            .typed_manifest
            .borrow()
            .clone()
            .ok_or_else(|| JsValue::from_str("typed router manifest missing"))?;
        let pathname = href.split('?').next().unwrap_or(href);
        let route = manifest
            .routes
            .iter()
            .find(|route| route.path == pathname)
            .or_else(|| manifest.routes.iter().find(|route| route.path == "*"));
        let (graph_id, loader_action) = match route {
            Some(route) => (&route.graph_id, route.loader_action),
            None => (&manifest.root_graph_id, None),
        };
        let app = self
            .typed_registry
            .borrow()
            .get(graph_id)
            .cloned()
            .ok_or_else(|| JsValue::from_str("typed route graph is not registered"))?;
        let typed = TypedRuntime::new(app)?;
        *self.typed.borrow_mut() = Some(typed);
        self.mount_typed(root)?;
        if let Some(action) = loader_action {
            #[cfg(not(feature = "fetch"))]
            return Err(JsValue::from_str("fetch capability is disabled"));
            #[cfg(feature = "fetch")]
            {
                let pending = {
                    let mut typed = self.typed.borrow_mut();
                    let typed = typed.as_mut().expect("typed runtime installed");
                    let mut metrics = UpdateMetrics::default();
                    typed.execute_action(action, &[], None, None, &mut metrics)?;
                    typed.take_pending_fetches()
                };
                for request in pending {
                    self.start_typed_fetch(request)?;
                }
            }
        }
        Ok(())
    }
}
