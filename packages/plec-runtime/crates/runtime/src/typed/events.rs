use crate::dom::listeners::*;
use crate::runtime::lifecycle::*;

pub(crate) struct TypedListener {
    pub(crate) listener: Listener,
    pub(crate) target: usize,
    pub(crate) action: usize,
    pub(crate) loop_index: Option<usize>,
    pub(crate) row_key: Option<String>,
    pub(crate) generation: u64,
}

impl PlecRuntime {
    pub(crate) fn install_typed_event_listeners(&self) -> Result<(), JsValue> {
        let candidates = {
            let typed = self.typed.borrow();
            let typed = typed
                .as_ref()
                .ok_or_else(|| JsValue::from_str("typed application missing"))?;
            let mut result = Vec::new();
            for binding in &typed.app.events {
                if let Some(node) = typed.nodes.get(&binding.target) {
                    result.push((binding.clone(), node.clone(), None, None, 0));
                }
                if let Some(loop_index) = binding.r#loop {
                    if let Some(rows) = typed.loops.get(&loop_index) {
                        for (key, row) in &rows.rows {
                            if let Some(node) = row.nodes.get(&binding.target) {
                                result.push((
                                    binding.clone(),
                                    node.clone(),
                                    Some(loop_index),
                                    Some(key.clone()),
                                    row.generation,
                                ));
                            }
                        }
                    }
                }
            }
            result
        };
        for (binding, node, loop_index, row_key, generation) in candidates {
            let element = node
                .dyn_into::<Element>()
                .map_err(|_| JsValue::from_str("event target is not an element"))?;
            let event_type = self
                .typed
                .borrow()
                .as_ref()
                .unwrap()
                .app
                .strings
                .get(binding.event_type)
                .cloned()
                .ok_or_else(|| JsValue::from_str("event type handle out of range"))?;
            if self
                .typed
                .borrow()
                .as_ref()
                .unwrap()
                .listeners
                .iter()
                .any(|entry| {
                    entry.target == binding.target
                        && entry.action == binding.action
                        && entry.loop_index == loop_index
                        && entry.row_key == row_key
                        && entry.generation == generation
                })
            {
                continue;
            }
            let runtime = self.clone();
            let fields = binding.fields.clone();
            let action = binding.action;
            let captured_loop = loop_index;
            let captured_key = row_key.clone();
            let callback = Closure::wrap(Box::new(move |event: Event| {
                let _ = runtime.dispatch_typed_event(
                    action,
                    captured_loop,
                    captured_key.clone(),
                    generation,
                    &fields,
                    event,
                );
            }) as Box<dyn FnMut(Event)>);
            element
                .add_event_listener_with_callback(&event_type, callback.as_ref().unchecked_ref())?;
            self.typed
                .borrow_mut()
                .as_mut()
                .unwrap()
                .listeners
                .push(TypedListener {
                    listener: Listener {
                        element,
                        event_type,
                        callback,
                    },
                    target: binding.target,
                    action,
                    loop_index,
                    row_key,
                    generation,
                });
        }
        Ok(())
    }
}

impl PlecRuntime {
    pub(crate) fn dispatch_typed_event(
        &self,
        action: usize,
        loop_index: Option<usize>,
        row_key: Option<String>,
        generation: u64,
        fields: &[TypedEventField],
        event: Event,
    ) -> Result<(), JsValue> {
        let target = event
            .current_target()
            .or_else(|| event.target())
            .and_then(|value| value.dyn_into::<Element>().ok());
        let values = {
            let typed = self.typed.borrow();
            let app = &typed
                .as_ref()
                .ok_or_else(|| JsValue::from_str("typed application missing"))?
                .app;
            fields
                .iter()
                .map(|field| {
                    (
                        field.slot,
                        typed_event_field(
                            app.strings
                                .get(field.name)
                                .map(String::as_str)
                                .unwrap_or(""),
                            &event,
                            target.as_ref(),
                        ),
                    )
                })
                .collect::<Vec<_>>()
        };
        let row = if let (Some(loop_index), Some(key)) = (loop_index, row_key) {
            let typed = self.typed.borrow();
            let Some(row) = typed
                .as_ref()
                .and_then(|typed| typed.loops.get(&loop_index))
                .and_then(|rows| rows.rows.get(&key))
            else {
                return Ok(());
            };
            if row.generation != generation {
                return Ok(());
            }
            Some(row.values.clone())
        } else {
            None
        };
        let pending = {
            let mut typed = self.typed.borrow_mut();
            let mut metrics = UpdateMetrics::default();
            typed.as_mut().unwrap().execute_action_with_frame(
                action,
                &values,
                row,
                Some(&event),
                &mut metrics,
            )?;
            typed.as_mut().unwrap().take_pending_fetches()
        };
        #[cfg(feature = "fetch")]
        for request in pending {
            self.start_typed_fetch(request)?;
        }
        #[cfg(not(feature = "fetch"))]
        if !pending.is_empty() {
            return Err(JsValue::from_str("fetch capability is disabled"));
        }
        self.install_typed_event_listeners()?;
        Ok(())
    }
}

pub(crate) fn typed_event_field(
    name: &str,
    event: &Event,
    target: Option<&Element>,
) -> RuntimeValue {
    match name {
        "type" => RuntimeValue::String(event.type_()),
        "value" => target
            .and_then(|element| element.clone().dyn_into::<HtmlInputElement>().ok())
            .map(|input| RuntimeValue::String(input.value()))
            .unwrap_or(RuntimeValue::Null),
        "checked" => target
            .and_then(|element| element.clone().dyn_into::<HtmlInputElement>().ok())
            .map(|input| RuntimeValue::Bool(input.checked()))
            .unwrap_or(RuntimeValue::Null),
        "rowKey" => target
            .and_then(|element| element.closest("[data-runtime-row-key]").ok().flatten())
            .and_then(|row| row.get_attribute("data-runtime-row-key"))
            .map(RuntimeValue::String)
            .unwrap_or(RuntimeValue::Null),
        "key" => event
            .clone()
            .dyn_into::<KeyboardEvent>()
            .map(|key| RuntimeValue::String(key.key()))
            .unwrap_or(RuntimeValue::Null),
        "button" => event
            .clone()
            .dyn_into::<MouseEvent>()
            .map(|mouse| RuntimeValue::Number(mouse.button() as f64))
            .unwrap_or(RuntimeValue::Null),
        "metaKey" => RuntimeValue::Bool(
            event
                .clone()
                .dyn_into::<KeyboardEvent>()
                .map(|key| key.meta_key())
                .or_else(|event| event.dyn_into::<MouseEvent>().map(|mouse| mouse.meta_key()))
                .unwrap_or(false),
        ),
        "ctrlKey" => RuntimeValue::Bool(
            event
                .clone()
                .dyn_into::<KeyboardEvent>()
                .map(|key| key.ctrl_key())
                .or_else(|event| event.dyn_into::<MouseEvent>().map(|mouse| mouse.ctrl_key()))
                .unwrap_or(false),
        ),
        "shiftKey" => RuntimeValue::Bool(
            event
                .clone()
                .dyn_into::<KeyboardEvent>()
                .map(|key| key.shift_key())
                .or_else(|event| {
                    event
                        .dyn_into::<MouseEvent>()
                        .map(|mouse| mouse.shift_key())
                })
                .unwrap_or(false),
        ),
        "altKey" => RuntimeValue::Bool(
            event
                .clone()
                .dyn_into::<KeyboardEvent>()
                .map(|key| key.alt_key())
                .or_else(|event| event.dyn_into::<MouseEvent>().map(|mouse| mouse.alt_key()))
                .unwrap_or(false),
        ),
        _ => RuntimeValue::Null,
    }
}
