use crate::dom::listeners::*;
use crate::runtime::lifecycle::*;
use crate::schema::typed::TypedActionInstruction;
use crate::typed::runtime::TypedRoutePhase;

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
pub(crate) struct TypedGlobalListenerHandle {
    pub(crate) target: EventTarget,
    pub(crate) event_type: String,
    pub(crate) callback: Closure<dyn FnMut(Event)>,
}

impl PlecRuntime {
    pub(crate) fn install_typed_global_listeners(&self) -> Result<(), JsValue> {
        let candidates = self
            .typed
            .borrow()
            .iter()
            .map(|(id, instance)| (id.clone(), instance.runtime.app.listeners.clone()))
            .collect::<Vec<_>>();
        for (instance_id, listeners) in candidates {
            for listener in listeners {
                let target: EventTarget = match listener.source.as_str() {
                    "window" => web_sys::window()
                        .ok_or_else(|| JsValue::from_str("window unavailable"))?
                        .into(),
                    "document" => web_sys::window()
                        .ok_or_else(|| JsValue::from_str("window unavailable"))?
                        .document()
                        .ok_or_else(|| JsValue::from_str("document unavailable"))?
                        .into(),
                    _ => continue,
                };
                let event_type = self
                    .typed
                    .borrow()
                    .get(&instance_id)
                    .and_then(|value| value.runtime.app.strings.get(listener.event))
                    .cloned()
                    .ok_or_else(|| JsValue::from_str("listener event out of range"))?;
                let already = self.typed.borrow().get(&instance_id).is_some_and(|value| {
                    value
                        .runtime
                        .global_listeners
                        .iter()
                        .any(|entry| entry.event_type == event_type)
                });
                if already {
                    continue;
                }
                let runtime = self.clone();
                let id = instance_id.clone();
                let action = listener.action;
                let callback = Closure::wrap(Box::new(move |event: Event| {
                    let value = RuntimeValue::Record(
                        [
                            (
                                "key".into(),
                                event
                                    .dyn_ref::<KeyboardEvent>()
                                    .map(|e| RuntimeValue::String(e.key()))
                                    .unwrap_or(RuntimeValue::Null),
                            ),
                            (
                                "metaKey".into(),
                                RuntimeValue::Bool(
                                    event
                                        .dyn_ref::<KeyboardEvent>()
                                        .map(|e| e.meta_key())
                                        .unwrap_or(false),
                                ),
                            ),
                            (
                                "ctrlKey".into(),
                                RuntimeValue::Bool(
                                    event
                                        .dyn_ref::<KeyboardEvent>()
                                        .map(|e| e.ctrl_key())
                                        .unwrap_or(false),
                                ),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    if let Ok(mut typed) = runtime.typed.try_borrow_mut() {
                        if let Some(instance) = typed.get_mut(&id) {
                            let _ = instance.runtime.execute_action_with_frame(
                                action,
                                &[(0, value)],
                                None,
                                Some(&event),
                                &mut UpdateMetrics::default(),
                            );
                        }
                    }
                }) as Box<dyn FnMut(Event)>);
                target.add_event_listener_with_callback(
                    &event_type,
                    callback.as_ref().unchecked_ref(),
                )?;
                if let Some(instance) = self.typed.borrow_mut().get_mut(&instance_id) {
                    instance
                        .runtime
                        .global_listeners
                        .push(TypedGlobalListenerHandle {
                            target,
                            event_type,
                            callback,
                        });
                }
            }
        }
        Ok(())
    }
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
            Self::Row {
                loop_index,
                row_key,
                generation,
            } => Some((*loop_index, row_key, *generation)),
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

    /// Ownership is runtime-instance state. This keeps the liveness decision
    /// independent from DOM objects so dispatch and tests share one model.
    pub(crate) fn is_live_with(
        &self,
        row_is_live: &impl Fn(usize, &str, u64) -> bool,
        conditional_is_live: &impl Fn(usize, u64, Option<&TypedListenerOwner>) -> bool,
    ) -> bool {
        match self {
            Self::Static => true,
            Self::Row {
                loop_index,
                row_key,
                generation,
            } => row_is_live(*loop_index, row_key, *generation),
            Self::Conditional {
                conditional,
                generation,
                row,
            } => {
                row.as_deref()
                    .map(|owner| owner.is_live_with(row_is_live, conditional_is_live))
                    .unwrap_or(true)
                    && conditional_is_live(*conditional, *generation, row.as_deref())
            }
        }
    }

    pub(crate) fn matches_disposal(&self, owner: &TypedListenerOwner) -> bool {
        self == owner
    }
}

impl PlecRuntime {
    pub(crate) fn install_typed_event_listeners(&self) -> Result<(), JsValue> {
        let Ok(mut typed) = self.typed.try_borrow_mut() else {
            return Ok(());
        };
        let candidates = typed
            .iter_mut()
            .map(|(id, instance)| {
                (
                    id.clone(),
                    instance
                        .runtime
                        .listener_requests
                        .drain(..)
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
        drop(typed);
        for (instance_id, request) in candidates.into_iter().flat_map(|(id, requests)| {
            requests
                .into_iter()
                .map(move |request| (id.clone(), request))
        }) {
            let bindings = {
                let typed = self.typed.borrow();
                typed
                    .get(&instance_id)
                    .unwrap()
                    .runtime
                    .app
                    .events
                    .iter()
                    // A target may deliberately carry more than one declared
                    // listener (for example click and keydown).  Requests are for
                    // concrete DOM nodes, not for a single event definition.
                    .filter(|binding| binding.target == request.target)
                    .cloned()
                    .collect::<Vec<_>>()
            };
            if bindings.is_empty() {
                continue;
            }
            for binding in bindings {
                let node = request.node.clone();
                let owner = request.owner.clone();
                let (loop_index, row_key, generation) = match &owner {
                    TypedListenerOwner::Row {
                        loop_index,
                        row_key,
                        generation,
                    } => (Some(*loop_index), Some(row_key.clone()), *generation),
                    TypedListenerOwner::Conditional { row: Some(row), .. } => match row.as_ref() {
                        TypedListenerOwner::Row {
                            loop_index,
                            row_key,
                            generation,
                        } => (Some(*loop_index), Some(row_key.clone()), *generation),
                        _ => (None, None, 0),
                    },
                    _ => (None, None, 0),
                };
                let element = node
                    .dyn_into::<Element>()
                    .map_err(|_| JsValue::from_str("event target is not an element"))?;
                let event_type = {
                    let typed = self.typed.borrow();
                    typed
                        .get(&instance_id)
                        .unwrap()
                        .runtime
                        .app
                        .strings
                        .get(binding.event_type)
                        .cloned()
                        .ok_or_else(|| JsValue::from_str("event type handle out of range"))?
                };
                let installed = {
                    let typed = self.typed.borrow();
                    typed
                        .get(&instance_id)
                        .unwrap()
                        .runtime
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
                };
                if installed {
                    continue;
                }
                let runtime = self.clone();
                let captured_instance_id = instance_id.clone();
                let fields = binding.fields.clone();
                let action = binding.action;
                let captured_owner = owner.clone();
                let callback = Closure::wrap(Box::new(move |event: Event| {
                    if let Err(error) = runtime.dispatch_typed_event(
                        &captured_instance_id,
                        action,
                        captured_owner.clone(),
                        &fields,
                        event,
                    ) {
                        web_sys::console::error_1(&error);
                    }
                }) as Box<dyn FnMut(Event)>);
                element.add_event_listener_with_callback(
                    &event_type,
                    callback.as_ref().unchecked_ref(),
                )?;
                self.typed
                    .borrow_mut()
                    .get_mut(&instance_id)
                    .unwrap()
                    .runtime
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
        instance_id: &str,
        action: usize,
        owner: TypedListenerOwner,
        fields: &[TypedEventField],
        event: Event,
    ) -> Result<(), JsValue> {
        let current_target = event
            .current_target()
            .and_then(|value| value.dyn_into::<Element>().ok());
        let target = event
            .target()
            .and_then(|value| value.dyn_into::<Element>().ok());
        let values = {
            let Ok(typed) = self.typed.try_borrow() else {
                return Ok(());
            };
            let app = &typed
                .get(instance_id)
                .ok_or_else(|| JsValue::from_str("typed application missing"))?
                .runtime
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
                            current_target.as_ref(),
                        )?,
                    ))
                })
                .collect::<Result<Vec<_>, JsValue>>()?
        };
        let row = if let Some((loop_index, key, generation)) = owner.row_identity() {
            let Ok(typed) = self.typed.try_borrow() else {
                return Ok(());
            };
            let Some(row) = typed
                .get(instance_id)
                .and_then(|typed| typed.runtime.loops.get(&loop_index))
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
            let Ok(typed) = self.typed.try_borrow() else {
                return Ok(());
            };
            let typed = &typed.get(instance_id).unwrap().runtime;
            owner.is_live_with(
                &|loop_index, row_key, generation| {
                    typed
                        .loops
                        .get(&loop_index)
                        .and_then(|rows| rows.rows.get(row_key))
                        .map(|row| row.generation == generation)
                        .unwrap_or(false)
                },
                &|conditional, generation, row| match row.and_then(TypedListenerOwner::row_identity)
                {
                    Some((loop_index, row_key, _)) => typed
                        .loops
                        .get(&loop_index)
                        .and_then(|rows| rows.rows.get(row_key))
                        .and_then(|entry| entry.conditionals.get(&conditional))
                        .map(|region| region.generation == generation)
                        .unwrap_or(false),
                    None => typed
                        .conditionals
                        .get(&conditional)
                        .map(|region| region.generation == generation)
                        .unwrap_or(false),
                },
            )
        };
        if !owner_is_live {
            return Ok(());
        }
        // A child component can disappear with its keyed row or conditional
        // parent. Its callback may still be invoked on a detached test node;
        // detached component roots are never live execution targets.
        let component_detached = self
            .typed
            .try_borrow()
            .ok()
            .map(|typed| {
                typed
                    .get(instance_id)
                    .and_then(|instance| {
                        instance.runtime.nodes.get(&instance.runtime.app.root_node)
                    })
                    .map(|node| node.parent_node().is_none())
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        if component_detached {
            return Ok(());
        }
        let route_retry = self
            .typed
            .try_borrow()
            .ok()
            .and_then(|typed| {
                typed.get(instance_id).and_then(|instance| {
                    let app = &instance.runtime.app;
                    let in_error_phase = instance
                        .route_state
                        .as_ref()
                        .is_some_and(|state| state.phase == TypedRoutePhase::Error);
                    app.actions.get(action).map(|action| {
                        action.route_retry
                            || (in_error_phase
                                && action.instructions.iter().any(|instruction| {
                                    let prop = match instruction {
                                        TypedActionInstruction::CallProp { prop, .. }
                                        | TypedActionInstruction::CallPropOptional {
                                            prop, ..
                                        } => *prop,
                                        _ => return false,
                                    };
                                    app.parameters.get(prop).is_some_and(|parameter| {
                                        parameter.callable
                                            && app
                                                .strings
                                                .get(parameter.name)
                                                .is_some_and(|name| name == "retry")
                                    })
                                }))
                    })
                })
            })
            .unwrap_or(false);
        if route_retry {
            return self.retry_typed_route(instance_id);
        }
        let (pending, cookies) = {
            let Ok(mut typed) = self.typed.try_borrow_mut() else {
                return Ok(());
            };
            let mut metrics = UpdateMetrics::default();
            typed
                .get_mut(instance_id)
                .unwrap()
                .runtime
                .execute_action_with_frame(action, &values, row, Some(&event), &mut metrics)?;
            let runtime = &mut typed.get_mut(instance_id).unwrap().runtime;
            (
                runtime.take_pending_fetches(),
                runtime.take_pending_cookies(),
            )
        };
        self.dispatch_typed_callbacks(instance_id)?;
        let has_pending_fetch = !pending.is_empty();
        #[cfg(feature = "fetch")]
        for mut request in pending {
            request.instance_id = instance_id.into();
            request.graph_generation = self
                .typed
                .borrow()
                .get(instance_id)
                .unwrap()
                .runtime
                .graph_generation;
            self.start_typed_fetch(request)?;
        }
        for mut request in cookies {
            request.instance_id = instance_id.into();
            self.start_typed_cookie(request)?;
        }
        self.flush_component_work()?;
        #[cfg(not(feature = "fetch"))]
        if !pending.is_empty() {
            return Err(JsValue::from_str("fetch capability is disabled"));
        }
        let install_listeners = self
            .typed
            .borrow()
            .get(instance_id)
            .map(|typed| !typed.runtime.listener_requests.is_empty())
            .unwrap_or(false);
        if install_listeners && !has_pending_fetch && event.type_() != "keydown" {
            self.install_typed_event_listeners()?;
        }
        Ok(())
    }

    fn dispatch_typed_callbacks(&self, instance_id: &str) -> Result<(), JsValue> {
        let callbacks = self
            .typed
            .borrow_mut()
            .get_mut(instance_id)
            .map(|instance| std::mem::take(&mut instance.runtime.callback_requests))
            .unwrap_or_default();
        #[cfg(feature = "fetch")]
        let mut pending_fetches = Vec::new();
        for callback in callbacks {
            let Some(mut typed) = self.typed.try_borrow_mut().ok() else {
                continue;
            };
            let Some(parent) = typed.get_mut(&callback.parent_id) else {
                continue;
            };
            let action = parent
                .runtime
                .app
                .actions
                .get(callback.action)
                .ok_or_else(|| JsValue::from_str("callback action out of range"))?;
            if action.parameter_slots.len() != callback.arguments.len() {
                return Err(JsValue::from_str("callback action arity mismatch"));
            }
            let frame = action
                .parameter_slots
                .iter()
                .copied()
                .zip(callback.arguments)
                .collect::<Vec<_>>();
            let mut metrics = UpdateMetrics::default();
            parent.runtime.execute_action_with_frame(
                callback.action,
                &frame,
                callback.row,
                None,
                &mut metrics,
            )?;
            #[cfg(feature = "fetch")]
            pending_fetches.extend(
                parent
                    .runtime
                    .take_pending_fetches()
                    .into_iter()
                    .map(|request| (callback.parent_id.clone(), request)),
            );
        }
        #[cfg(feature = "fetch")]
        for (parent_id, mut request) in pending_fetches {
            request.instance_id = parent_id;
            self.start_typed_fetch(request)?;
        }
        self.flush_component_work()?;
        self.install_typed_event_listeners()?;
        Ok(())
    }
}

pub(crate) fn typed_event_field(
    name: &str,
    event: &Event,
    target: Option<&Element>,
    current_target: Option<&Element>,
) -> Result<RuntimeValue, JsValue> {
    match name {
        "event" => Ok(RuntimeValue::Record(std::collections::HashMap::from([
            ("target".into(), typed_event_element(target)),
            ("currentTarget".into(), typed_event_element(current_target)),
            (
                "key".into(),
                event
                    .clone()
                    .dyn_into::<KeyboardEvent>()
                    .map(|key| RuntimeValue::String(key.key()))
                    .unwrap_or(RuntimeValue::Null),
            ),
            ("type".into(), RuntimeValue::String(event.type_())),
        ]))),
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

fn typed_event_element(element: Option<&Element>) -> RuntimeValue {
    let Some(element) = element else {
        return RuntimeValue::Null;
    };
    let value = element
        .clone()
        .dyn_into::<HtmlInputElement>()
        .ok()
        .map(|input| {
            RuntimeValue::Record(std::collections::HashMap::from([
                ("value".into(), RuntimeValue::String(input.value())),
                ("checked".into(), RuntimeValue::Bool(input.checked())),
            ]))
        })
        .unwrap_or(RuntimeValue::Record(std::collections::HashMap::new()));
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(loop_index: usize, key: &str, generation: u64) -> TypedListenerOwner {
        TypedListenerOwner::Row {
            loop_index,
            row_key: key.into(),
            generation,
        }
    }

    #[test]
    fn listener_owner_equality_and_disposal_are_exact() {
        let owner = TypedListenerOwner::Conditional {
            conditional: 4,
            generation: 9,
            row: Some(Box::new(row(1, "a", 7))),
        };
        assert!(owner.matches_disposal(&owner));
        assert!(!owner.matches_disposal(&TypedListenerOwner::Conditional {
            conditional: 4,
            generation: 10,
            row: Some(Box::new(row(1, "a", 7))),
        }));
        assert!(owner.belongs_to_row(1, "a", 7));
        assert!(!owner.belongs_to_row(1, "a", 8));
    }

    #[test]
    fn listener_owner_liveness_requires_current_row_and_conditional_generations() {
        let owner = TypedListenerOwner::Conditional {
            conditional: 3,
            generation: 11,
            row: Some(Box::new(row(2, "row", 5))),
        };
        let current_row =
            |loop_index, key: &str, generation| (loop_index, key, generation) == (2, "row", 5);
        let current_conditional = |conditional, generation, row: Option<&TypedListenerOwner>| {
            conditional == 3 && generation == 11 && row.is_some()
        };
        assert!(owner.is_live_with(&current_row, &current_conditional));
        assert!(!owner.is_live_with(&|_, _, _| false, &current_conditional));
        assert!(!owner.is_live_with(&current_row, &|_, _, _| false));
        assert!(TypedListenerOwner::Static.is_live_with(&|_, _, _| false, &|_, _, _| false));
    }

    #[test]
    fn event_record_preserves_distinct_missing_dom_sources() {
        assert_eq!(typed_event_element(None), RuntimeValue::Null);
    }
}
