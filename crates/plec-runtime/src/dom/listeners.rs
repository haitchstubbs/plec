use crate::runtime::lifecycle::*;

pub(crate) struct Listener {
    pub(crate) element: Element,
    pub(crate) event_type: String,
    pub(crate) callback: Closure<dyn FnMut(Event)>,
}

impl PlecRuntime {
    pub(crate) fn install_event_listeners(
        &self,
        instance_id: &str,
        app: &Application,
    ) -> Result<(), JsValue> {
        self.remove_event_listeners(instance_id)?;
        let nodes = self
            .instances
            .borrow()
            .get(instance_id)
            .ok_or_else(|| JsValue::from_str("unknown graph instance"))?
            .dom_nodes
            .clone();
        self.install_event_listeners_for_nodes(instance_id, app, &nodes)
    }
}

impl PlecRuntime {
    pub(crate) fn install_event_listeners_for_nodes(
        &self,
        instance_id: &str,
        app: &Application,
        nodes: &HashMap<String, Node>,
    ) -> Result<(), JsValue> {
        let mut new_listeners = Vec::new();
        for binding in &app.events {
            let Some(node) = nodes.get(&binding.target_id) else {
                continue;
            };
            let Ok(element) = node.clone().dyn_into::<Element>() else {
                continue;
            };
            let action_id = binding.action_id.clone();
            let loop_id = binding.loop_id.clone();
            let instance_id = instance_id.to_owned();
            let runtime: *const PlecRuntime = self;
            let callback = Closure::wrap(Box::new(move |event: Event| {
                // The runtime instance outlives registered listeners; dispose removes each
                // listener before its instance can be released.
                unsafe {
                    if let Some(runtime) = runtime.as_ref() {
                        let _ = runtime.dispatch_dom_event(
                            &instance_id,
                            action_id.clone(),
                            loop_id.clone(),
                            event,
                        );
                    }
                }
            }) as Box<dyn FnMut(Event)>);
            element.add_event_listener_with_callback(
                &binding.event_type,
                callback.as_ref().unchecked_ref(),
            )?;
            new_listeners.push(Listener {
                element,
                event_type: binding.event_type.clone(),
                callback,
            });
        }
        let mut instances = self.instances.borrow_mut();
        let instance = instances
            .get_mut(instance_id)
            .ok_or_else(|| JsValue::from_str("unknown graph instance"))?;
        instance.active_listeners += new_listeners.len();
        instance.listeners.extend(new_listeners);
        Ok(())
    }
}

impl PlecRuntime {
    pub(crate) fn remove_event_listeners(&self, instance_id: &str) -> Result<(), JsValue> {
        let listeners = std::mem::take(
            &mut self
                .instances
                .borrow_mut()
                .get_mut(instance_id)
                .ok_or_else(|| JsValue::from_str("unknown graph instance"))?
                .listeners,
        );
        for listener in listeners {
            let _ = listener.element.remove_event_listener_with_callback(
                &listener.event_type,
                listener.callback.as_ref().unchecked_ref(),
            );
        }
        Ok(())
    }
}

impl PlecRuntime {
    pub(crate) fn rebuild_event_listeners(
        &self,
        instance_id: &str,
        app: &Application,
    ) -> Result<(), JsValue> {
        self.remove_event_listeners(instance_id)?;
        let mounted = self
            .instances
            .borrow()
            .get(instance_id)
            .ok_or_else(|| JsValue::from_str("unknown graph instance"))?
            .dom_nodes
            .clone();
        self.install_event_listeners_for_nodes(instance_id, app, &mounted)?;
        let row_nodes = self
            .instances
            .borrow()
            .get(instance_id)
            .ok_or_else(|| JsValue::from_str("unknown graph instance"))?
            .rows
            .values()
            .flat_map(|rows| rows.values().map(|row| row.nodes.clone()))
            .collect::<Vec<_>>();
        for nodes in row_nodes {
            self.install_event_listeners_for_nodes(instance_id, app, &nodes)?;
        }
        Ok(())
    }
}

impl PlecRuntime {
    pub(crate) fn dispatch_dom_event(
        &self,
        instance_id: &str,
        action_id: String,
        loop_id: Option<String>,
        event: Event,
    ) -> Result<(), JsValue> {
        let mut value = HashMap::new();
        value.insert("type".into(), Value::String(event.type_()));
        let target = event
            .current_target()
            .or_else(|| event.target())
            .and_then(|target| target.dyn_into::<Element>().ok());
        if let Some(target) = target {
            if let Ok(input) = target.clone().dyn_into::<HtmlInputElement>() {
                value.insert("value".into(), Value::String(input.value()));
                value.insert("checked".into(), Value::Bool(input.checked()));
            }
            if let Ok(Some(row)) = target.closest("[data-runtime-row-key]") {
                if let Some(key) = row.get_attribute("data-runtime-row-key") {
                    value.insert("rowKey".into(), Value::String(key));
                }
            }
        }
        if let Ok(keyboard) = event.clone().dyn_into::<KeyboardEvent>() {
            value.insert("key".into(), Value::String(keyboard.key()));
            value.insert("metaKey".into(), Value::Bool(keyboard.meta_key()));
            value.insert("ctrlKey".into(), Value::Bool(keyboard.ctrl_key()));
            value.insert("shiftKey".into(), Value::Bool(keyboard.shift_key()));
            value.insert("altKey".into(), Value::Bool(keyboard.alt_key()));
        } else if let Ok(mouse) = event.clone().dyn_into::<MouseEvent>() {
            value.insert("button".into(), Value::Number(mouse.button().into()));
            value.insert("metaKey".into(), Value::Bool(mouse.meta_key()));
            value.insert("ctrlKey".into(), Value::Bool(mouse.ctrl_key()));
            value.insert("shiftKey".into(), Value::Bool(mouse.shift_key()));
            value.insert("altKey".into(), Value::Bool(mouse.alt_key()));
        }
        // Action programs use the finite event record from the IR, not a DOM
        // Event. Preserve the common React shape so `event.currentTarget.value`
        // and `.checked` lower without a JavaScript compatibility callback.
        value.insert(
            "currentTarget".into(),
            Value::Object(value.clone().into_iter().collect()),
        );
        let app = self.app_for_instance(instance_id)?;
        let action = app
            .actions
            .iter()
            .find(|action| action.id == action_id)
            .ok_or_else(|| JsValue::from_str("unknown action"))?;
        let mut metrics = UpdateMetrics::default();
        let row_scope = loop_id
            .as_deref()
            .and_then(|loop_id| {
                self.row_scope_for_event(instance_id, &app, loop_id, value.get("rowKey"))
            })
            .unwrap_or_default();
        self.execute_action_operations(
            instance_id,
            &app,
            &action.operations,
            &value,
            Some(&event),
            &mut metrics,
            Some(&row_scope),
        )?;
        self.refresh_state_bindings(instance_id, &app, &mut metrics)
    }
}
