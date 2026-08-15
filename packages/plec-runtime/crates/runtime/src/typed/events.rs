use crate::dom::listeners::*;
use crate::runtime::lifecycle::*;

pub(crate) struct TypedListener {
    pub(crate) listener: Listener,
    pub(crate) target: usize,
    pub(crate) action: usize,
    pub(crate) loop_index: Option<usize>,
    pub(crate) row_key: Option<String>,
    pub(crate) generation: u64,
    /// Runtime-instance identity. This is deliberately not part of the
    /// executable artifact: graph definitions are immutable while listeners
    /// belong to concrete DOM instances.
    pub(crate) owner: TypedListenerOwner,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TypedListenerOwner {
    Static,
    Row {
        loop_index: usize,
        row_key: String,
        generation: u64,
    },
    Conditional {
        conditional: usize,
        generation: u64,
        row: Option<Box<TypedListenerOwner>>,
    },
}

impl TypedListenerOwner {
    /// Returns the keyed row that owns this listener, including listeners
    /// mounted inside a row-local conditional region.
    pub(crate) fn row_identity(&self) -> Option<(usize, &str, u64)> {
        match self {
            Self::Row { loop_index, row_key, generation } => {
                Some((*loop_index, row_key, *generation))
            }
            Self::Conditional { row: Some(row), .. } => row.row_identity(),
            Self::Static | Self::Conditional { row: None, .. } => None,
        }
    }

    pub(crate) fn belongs_to_row(&self, loop_index: usize, row_key: &str, generation: u64) -> bool {
        self.row_identity()
            .map(|(owner_loop, owner_key, owner_generation)| {
                owner_loop == loop_index && owner_key == row_key && owner_generation == generation
            })
            .unwrap_or(false)
    }
}

impl PlecRuntime {
    pub(crate) fn install_typed_event_listeners(&self) -> Result<(), JsValue> {
        let candidates = self.typed.borrow_mut().as_mut()
            .ok_or_else(|| JsValue::from_str("typed application missing"))?
            .listener_requests.drain(..).collect::<Vec<_>>();
        for request in candidates {
            let bindings = self.typed.borrow().as_ref().unwrap().app.events.iter()
                // A target may deliberately carry more than one declared
                // listener (for example click and keydown).  Requests are for
                // concrete DOM nodes, not for a single event definition.
                .filter(|binding| binding.target == request.target)
                .cloned().collect::<Vec<_>>();
            if bindings.is_empty() { continue; }
            for binding in bindings {
            let node = request.node.clone();
            let owner = request.owner.clone();
            let (loop_index, row_key, generation) = match &owner {
                TypedListenerOwner::Row { loop_index, row_key, generation } => (Some(*loop_index), Some(row_key.clone()), *generation),
                TypedListenerOwner::Conditional { row: Some(row), .. } => match row.as_ref() {
                    TypedListenerOwner::Row { loop_index, row_key, generation } => (Some(*loop_index), Some(row_key.clone()), *generation),
                    _ => (None, None, 0),
                },
                _ => (None, None, 0),
            };
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
                        && entry.listener.event_type == event_type
                        && entry.owner == owner
                })
            {
                continue;
            }
            let runtime = self.clone();
            let fields = binding.fields.clone();
            let action = binding.action;
            let captured_owner = owner.clone();
            let callback = Closure::wrap(Box::new(move |event: Event| {
                if let Err(error) = runtime.dispatch_typed_event(
                    action,
                    captured_owner.clone(),
                    &fields,
                    event,
                ) {
                    web_sys::console::error_1(&error);
                }
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
                    owner,
                });
            }
        }
        Ok(())
    }
}

impl PlecRuntime {
    pub(crate) fn dispatch_typed_event(
        &self,
        action: usize,
        owner: TypedListenerOwner,
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
                    Ok((
                        field.slot,
                        typed_event_field(
                            app.strings.get(field.name).ok_or_else(|| {
                                JsValue::from_str("event field handle out of range")
                            })?,
                            &event,
                            target.as_ref(),
                        )?,
                    ))
                })
                .collect::<Result<Vec<_>, JsValue>>()?
        };
        let row = if let Some((loop_index, key, generation)) = owner.row_identity() {
            let typed = self.typed.borrow();
            let Some(row) = typed
                .as_ref()
                .and_then(|typed| typed.loops.get(&loop_index))
                .and_then(|rows| rows.rows.get(key))
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
        let owner_is_live = {
            let typed = self.typed.borrow(); let typed = typed.as_ref().unwrap();
            match &owner {
                TypedListenerOwner::Static => true,
                TypedListenerOwner::Row { loop_index, row_key, generation } => typed.loops.get(loop_index).and_then(|rows| rows.rows.get(row_key)).map(|row| row.generation == *generation).unwrap_or(false),
                TypedListenerOwner::Conditional { conditional, generation, row: None } => typed.conditionals.get(conditional).map(|region| region.generation == *generation).unwrap_or(false),
                TypedListenerOwner::Conditional { conditional, generation, row: Some(row) } => match row.as_ref() { TypedListenerOwner::Row { loop_index, row_key, .. } => typed.loops.get(loop_index).and_then(|rows| rows.rows.get(row_key)).and_then(|entry| entry.conditionals.get(conditional)).map(|region| region.generation == *generation).unwrap_or(false), _ => false },
            }
        };
        if !owner_is_live { return Ok(()); }
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
) -> Result<RuntimeValue, JsValue> {
    match name {
        "type" => Ok(RuntimeValue::String(event.type_())),
        "value" => target
            .and_then(|element| element.clone().dyn_into::<HtmlInputElement>().ok())
            .map(|input| RuntimeValue::String(input.value()))
            .map_or(Ok(RuntimeValue::Null), Ok),
        "checked" => target
            .and_then(|element| element.clone().dyn_into::<HtmlInputElement>().ok())
            .map(|input| RuntimeValue::Bool(input.checked()))
            .map_or(Ok(RuntimeValue::Null), Ok),
        "rowKey" => target
            .and_then(|element| element.closest("[data-runtime-row-key]").ok().flatten())
            .and_then(|row| row.get_attribute("data-runtime-row-key"))
            .map(RuntimeValue::String)
            .map_or(Ok(RuntimeValue::Null), Ok),
        "key" => event
            .clone()
            .dyn_into::<KeyboardEvent>()
            .map(|key| RuntimeValue::String(key.key()))
            .map_or(Ok(RuntimeValue::Null), Ok),
        "button" => event
            .clone()
            .dyn_into::<MouseEvent>()
            .map(|mouse| RuntimeValue::Number(mouse.button() as f64))
            .map_or(Ok(RuntimeValue::Null), Ok),
        "metaKey" => Ok(RuntimeValue::Bool(
            event
                .clone()
                .dyn_into::<KeyboardEvent>()
                .map(|key| key.meta_key())
                .or_else(|event| event.dyn_into::<MouseEvent>().map(|mouse| mouse.meta_key()))
                .unwrap_or(false),
        )),
        "ctrlKey" => Ok(RuntimeValue::Bool(
            event
                .clone()
                .dyn_into::<KeyboardEvent>()
                .map(|key| key.ctrl_key())
                .or_else(|event| event.dyn_into::<MouseEvent>().map(|mouse| mouse.ctrl_key()))
                .unwrap_or(false),
        )),
        "shiftKey" => Ok(RuntimeValue::Bool(
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
        )),
        "altKey" => Ok(RuntimeValue::Bool(
            event
                .clone()
                .dyn_into::<KeyboardEvent>()
                .map(|key| key.alt_key())
                .or_else(|event| event.dyn_into::<MouseEvent>().map(|mouse| mouse.alt_key()))
                .unwrap_or(false),
        )),
        _ => Err(JsValue::from_str("unsupported typed event field")),
    }
}
