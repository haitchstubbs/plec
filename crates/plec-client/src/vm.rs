use crate::bindings::*;
use crate::cookie::*;
#[cfg(feature = "fetch")]
use crate::fetch::*;
use crate::prelude::*;
use crate::runtime::*;
use plec_dom::platform::document;
use plec_eval::eval::*;
use plec_schema::typed::TypedCapabilityRequest;
use wasm_bindgen::JsCast;
use web_sys::HtmlElement;

/// Result-slot sentinel for tail-call continuations: a tail call discards
/// the callee result, so no caller frame slot receives it.
const TAIL_CALL_NO_SLOT: usize = usize::MAX;

#[derive(Clone)]
pub struct TypedActionFrame {
    pub action: usize,
    pub pc: usize,
    pub stack: Vec<RuntimeValue>,
    pub frame: Vec<RuntimeValue>,
    pub event: Vec<RuntimeValue>,
    pub row: Option<HashMap<String, RuntimeValue>>,
}

#[derive(Clone)]
pub struct TypedCallerContinuation {
    pub frame: TypedActionFrame,
    pub success_pc: usize,
    pub failure_pc: usize,
    pub result_slot: usize,
    pub error_slot: usize,
}

#[derive(Clone)]
pub struct TypedContinuationStack {
    pub current: TypedActionFrame,
    pub callers: Vec<TypedCallerContinuation>,
    pub fetch_accounting: Rc<RefCell<ActionFetchAccounting>>,
}

#[derive(Default)]
pub struct ActionFetchAccounting {
    #[cfg(feature = "fetch")]
    fetches: usize,
    #[cfg(feature = "fetch")]
    bytes: usize,
}

impl ActionFetchAccounting {
    #[cfg(feature = "fetch")]
    fn reserve_fetch(&mut self) -> Result<(), JsValue> {
        if self.fetches >= plec_ir::limits::MAX_FETCHES_PER_ACTION {
            return Err(JsValue::from_str(&format!(
                "fetch count exceeds the {} per-action limit",
                plec_ir::limits::MAX_FETCHES_PER_ACTION
            )));
        }
        self.fetches += 1;
        Ok(())
    }

    #[cfg(feature = "fetch")]
    pub(crate) fn charge_response_bytes(&mut self, bytes: usize) -> Result<(), JsValue> {
        let total = self.bytes.checked_add(bytes).ok_or_else(|| {
            JsValue::from_str("fetch response bytes overflow the per-action accounting")
        })?;
        if total > plec_ir::limits::MAX_FETCH_BYTES_PER_ACTION {
            return Err(JsValue::from_str(&format!(
                "fetch response bytes exceed the {} per-action limit",
                plec_ir::limits::MAX_FETCH_BYTES_PER_ACTION
            )));
        }
        self.bytes = total;
        Ok(())
    }
}

impl TypedRuntime {
    pub fn execute_action(
        &mut self,
        action: usize,
        event: &[RuntimeValue],
        row: Option<HashMap<String, RuntimeValue>>,
        native_event: Option<&Event>,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let slots = event.iter().cloned().enumerate().collect::<Vec<_>>();
        self.execute_action_with_frame(action, &slots, row, native_event, metrics)
    }
}

impl TypedRuntime {
    pub fn execute_action_with_frame(
        &mut self,
        action: usize,
        event: &[(usize, RuntimeValue)],
        row: Option<HashMap<String, RuntimeValue>>,
        native_event: Option<&Event>,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let program = self
            .app
            .actions
            .get(action)
            .cloned()
            .ok_or_else(|| JsValue::from_str("action handle out of range"))?;
        let mut frame = vec![RuntimeValue::Null; program.frame_slots];
        for (slot, value) in event {
            if *slot >= frame.len() {
                return Err(JsValue::from_str("event frame slot out of range"));
            }
            frame[*slot] = value.clone();
        }
        let event_values = event
            .iter()
            .map(|(_, value)| value.clone())
            .collect::<Vec<_>>();
        self.execute_action_at(action, 0, frame, &event_values, row, native_event, metrics)
    }
}

impl TypedRuntime {
    pub fn execute_action_at(
        &mut self,
        action: usize,
        pc: usize,
        frame: Vec<RuntimeValue>,
        event: &[RuntimeValue],
        row: Option<HashMap<String, RuntimeValue>>,
        native_event: Option<&Event>,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        self.execute_continuation(
            TypedContinuationStack {
                current: TypedActionFrame {
                    action,
                    pc,
                    stack: Vec::new(),
                    frame,
                    event: event.to_vec(),
                    row,
                },
                callers: Vec::new(),
                fetch_accounting: Rc::new(RefCell::new(ActionFetchAccounting::default())),
            },
            native_event,
            metrics,
        )
    }

    pub(crate) fn execute_action_at_with_fetch_accounting(
        &mut self,
        action: usize,
        pc: usize,
        frame: Vec<RuntimeValue>,
        event: &[RuntimeValue],
        row: Option<HashMap<String, RuntimeValue>>,
        native_event: Option<&Event>,
        metrics: &mut UpdateMetrics,
        fetch_accounting: Rc<RefCell<ActionFetchAccounting>>,
    ) -> Result<(), JsValue> {
        self.execute_continuation(
            TypedContinuationStack {
                current: TypedActionFrame {
                    action,
                    pc,
                    stack: Vec::new(),
                    frame,
                    event: event.to_vec(),
                    row,
                },
                callers: Vec::new(),
                fetch_accounting,
            },
            native_event,
            metrics,
        )
    }

    pub fn execute_continuation(
        &mut self,
        mut continuation: TypedContinuationStack,
        native_event: Option<&Event>,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        // One shared step budget across every frame of this run: crafted
        // backward jumps or mutually recursive actions exhaust a documented
        // budget instead of pinning the event loop.
        let mut fuel = plec_ir::limits::MAX_ACTION_STEPS;
        'run: loop {
            let action = continuation.current.action;
            let mut pc = continuation.current.pc;
            let mut frame = continuation.current.frame.clone();
            let event = continuation.current.event.clone();
            let row = continuation.current.row.clone();
            let program = self
                .app
                .actions
                .get(action)
                .cloned()
                .ok_or_else(|| JsValue::from_str("action handle out of range"))?;
            let mut stack = continuation.current.stack.clone();
            while let Some(instruction) = program.instructions.get(pc).cloned() {
                if fuel == 0 {
                    return Err(JsValue::from_str("action execution budget exceeded"));
                }
                fuel -= 1;
                match instruction {
                    TypedActionInstruction::Evaluate { expression } => {
                        stack.push(typed_eval_frame(
                            &self.app,
                            expression,
                            &self.states,
                            row.as_ref(),
                            0,
                            &frame,
                            &event,
                        )?)
                    }
                    TypedActionInstruction::StoreState { state } => {
                        let value = stack.pop().ok_or_else(|| {
                            JsValue::from_str("action stack underflow: storeState")
                        })?;
                        if state >= self.states.len() {
                            return Err(JsValue::from_str("state handle out of range"));
                        }
                        self.states[state] = value;
                        self.refresh_state(state, metrics)?;
                    }
                    TypedActionInstruction::StoreFrame { slot } => {
                        let value = stack.pop().ok_or_else(|| {
                            JsValue::from_str("action stack underflow: storeFrame")
                        })?;
                        if slot >= frame.len() {
                            return Err(JsValue::from_str("action frame slot out of range"));
                        }
                        frame[slot] = value;
                    }
                    TypedActionInstruction::StoreRef { reference } => {
                        let value = stack
                            .pop()
                            .ok_or_else(|| JsValue::from_str("action stack underflow: storeRef"))?;
                        let slot = self
                            .app
                            .ref_values
                            .get_mut(reference)
                            .ok_or_else(|| JsValue::from_str("ref handle out of range"))?;
                        *slot = value;
                    }
                    TypedActionInstruction::CaptureActiveElement { reference } => {
                        let slot = self
                            .focus_refs
                            .get_mut(reference)
                            .ok_or_else(|| JsValue::from_str("focus ref handle out of range"))?;
                        *slot = document()?.active_element();
                    }
                    TypedActionInstruction::FocusHostRef { reference } => {
                        if let Some(node) =
                            self.host_ref_nodes.get(reference).and_then(Option::as_ref)
                        {
                            if let Some(element) = node.dyn_ref::<HtmlElement>() {
                                let _ = element.focus();
                            }
                        }
                    }
                    TypedActionInstruction::FocusRef { reference } => {
                        if let Some(element) =
                            self.focus_refs.get(reference).and_then(Option::as_ref)
                        {
                            if element.is_connected() {
                                if let Some(element) = element.dyn_ref::<HtmlElement>() {
                                    let _ = element.focus();
                                }
                            }
                        }
                    }
                    TypedActionInstruction::PreventDefault => {
                        if let Some(event) = native_event {
                            event.prevent_default();
                        }
                    }
                    TypedActionInstruction::CallProp { prop, arguments } => {
                        let mut callback = self
                            .callbacks
                            .get(prop)
                            .and_then(Clone::clone)
                            .ok_or_else(|| JsValue::from_str("callable component prop missing"))?;
                        callback.arguments = arguments
                            .into_iter()
                            .map(|expression| {
                                typed_eval_frame(
                                    &self.app,
                                    expression,
                                    &self.states,
                                    row.as_ref(),
                                    0,
                                    &frame,
                                    &event,
                                )
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        self.callback_requests.push(callback);
                    }
                    TypedActionInstruction::CallPropOptional { prop, arguments } => {
                        let Some(mut callback) = self.callbacks.get(prop).and_then(Clone::clone)
                        else {
                            pc += 1;
                            continue;
                        };
                        callback.arguments = arguments
                            .into_iter()
                            .map(|expression| {
                                typed_eval_frame(
                                    &self.app,
                                    expression,
                                    &self.states,
                                    row.as_ref(),
                                    0,
                                    &frame,
                                    &event,
                                )
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        self.callback_requests.push(callback);
                    }
                    TypedActionInstruction::StoreHostRef { r#ref } => {
                        let name = self
                            .app
                            .strings
                            .get(r#ref)
                            .cloned()
                            .ok_or_else(|| JsValue::from_str("host ref handle out of range"))?;
                        if let Some(active) = document()?.active_element() {
                            self.host_refs.insert(name, active.into());
                        } else {
                            self.host_refs.remove(&name);
                        }
                    }
                    TypedActionInstruction::Jump { target } => {
                        pc = target;
                        continue;
                    }
                    TypedActionInstruction::JumpIfFalse { target } => {
                        if !typed_truthy(&stack.pop().ok_or_else(|| {
                            JsValue::from_str("action stack underflow: jumpIfFalse")
                        })?) {
                            pc = target;
                            continue;
                        }
                    }
                    TypedActionInstruction::Call {
                        action: target,
                        arguments,
                        success_pc,
                        failure_pc,
                        result_slot,
                        error_slot,
                    } => {
                        let target_program = self
                            .app
                            .actions
                            .get(target)
                            .cloned()
                            .ok_or_else(|| JsValue::from_str("action handle out of range"))?;
                        if arguments.len() != target_program.parameter_slots.len() {
                            return Err(JsValue::from_str("action call arity mismatch"));
                        }
                        let mut child = vec![RuntimeValue::Null; target_program.frame_slots];
                        for (expression, slot) in
                            arguments.into_iter().zip(target_program.parameter_slots)
                        {
                            child[slot] = typed_eval_frame(
                                &self.app,
                                expression,
                                &self.states,
                                row.as_ref(),
                                0,
                                &frame,
                                &event,
                            )?;
                        }
                        match (success_pc, failure_pc, result_slot, error_slot) {
                            (
                                Some(success_pc),
                                Some(failure_pc),
                                Some(result_slot),
                                Some(error_slot),
                            ) => {
                                if continuation.callers.len() >= plec_ir::limits::MAX_CALL_DEPTH {
                                    return Err(JsValue::from_str(
                                        "action call depth exceeds limit",
                                    ));
                                }
                                continuation.callers.push(TypedCallerContinuation {
                                    frame: TypedActionFrame {
                                        action,
                                        pc: pc + 1,
                                        stack: stack.clone(),
                                        frame: frame.clone(),
                                        event: event.clone(),
                                        row: row.clone(),
                                    },
                                    success_pc,
                                    failure_pc,
                                    result_slot,
                                    error_slot,
                                });
                                continuation.current = TypedActionFrame {
                                    action: target,
                                    pc: 0,
                                    stack: Vec::new(),
                                    frame: child,
                                    event: event.clone(),
                                    row: row.clone(),
                                };
                                continue 'run;
                            }
                            (None, None, None, None) => {
                                // Tail call: reuse the continuation machinery
                                // instead of native recursion, so one shared
                                // step budget and one call-depth bound cover
                                // self-referential tail calls too.
                                if continuation.callers.len()
                                    >= plec_ir::limits::MAX_CALL_DEPTH
                                {
                                    return Err(JsValue::from_str(
                                        "action call depth exceeds limit",
                                    ));
                                }
                                continuation.callers.push(TypedCallerContinuation {
                                    frame: TypedActionFrame {
                                        action,
                                        pc: pc + 1,
                                        stack,
                                        frame,
                                        event: event.clone(),
                                        row: row.clone(),
                                    },
                                    success_pc: pc + 1,
                                    failure_pc: pc + 1,
                                    result_slot: TAIL_CALL_NO_SLOT,
                                    error_slot: TAIL_CALL_NO_SLOT,
                                });
                                continuation.current = TypedActionFrame {
                                    action: target,
                                    pc: 0,
                                    stack: Vec::new(),
                                    frame: child,
                                    event,
                                    row,
                                };
                                continue 'run;
                            }
                            _ => return Err(JsValue::from_str("partial action call continuation")),
                        }
                    }
                    TypedActionInstruction::CallFrame {
                        parameter,
                        arguments,
                        success_pc,
                        failure_pc,
                        result_slot,
                        error_slot,
                    } => {
                        let RuntimeValue::Number(target) =
                            frame.get(parameter).cloned().unwrap_or(RuntimeValue::Null)
                        else {
                            return Err(JsValue::from_str(
                                "callFrame parameter must be an action handle",
                            ));
                        };
                        let target = target as usize;
                        let target_program =
                            self.app.actions.get(target).cloned().ok_or_else(|| {
                                JsValue::from_str("callFrame action handle out of range")
                            })?;
                        if arguments.len() != target_program.parameter_slots.len() {
                            return Err(JsValue::from_str("callFrame action arity mismatch"));
                        }
                        let mut child = vec![RuntimeValue::Null; target_program.frame_slots];
                        for (expression, slot) in
                            arguments.into_iter().zip(target_program.parameter_slots)
                        {
                            child[slot] = typed_eval_frame(
                                &self.app,
                                expression,
                                &self.states,
                                row.as_ref(),
                                0,
                                &frame,
                                &event,
                            )?;
                        }
                        match (success_pc, failure_pc, result_slot, error_slot) {
                            (
                                Some(success_pc),
                                Some(failure_pc),
                                Some(result_slot),
                                Some(error_slot),
                            ) => {
                                if continuation.callers.len() >= plec_ir::limits::MAX_CALL_DEPTH {
                                    return Err(JsValue::from_str(
                                        "action call depth exceeds limit",
                                    ));
                                }
                                continuation.callers.push(TypedCallerContinuation {
                                    frame: TypedActionFrame {
                                        action,
                                        pc: pc + 1,
                                        stack: stack.clone(),
                                        frame: frame.clone(),
                                        event: event.clone(),
                                        row: row.clone(),
                                    },
                                    success_pc,
                                    failure_pc,
                                    result_slot,
                                    error_slot,
                                });
                                continuation.current = TypedActionFrame {
                                    action: target,
                                    pc: 0,
                                    stack: Vec::new(),
                                    frame: child,
                                    event: event.clone(),
                                    row: row.clone(),
                                };
                                continue 'run;
                            }
                            (None, None, None, None) => {
                                // Tail call: same continuation-based bound as
                                // the `Call` arm above.
                                if continuation.callers.len()
                                    >= plec_ir::limits::MAX_CALL_DEPTH
                                {
                                    return Err(JsValue::from_str(
                                        "action call depth exceeds limit",
                                    ));
                                }
                                continuation.callers.push(TypedCallerContinuation {
                                    frame: TypedActionFrame {
                                        action,
                                        pc: pc + 1,
                                        stack,
                                        frame,
                                        event: event.clone(),
                                        row: row.clone(),
                                    },
                                    success_pc: pc + 1,
                                    failure_pc: pc + 1,
                                    result_slot: TAIL_CALL_NO_SLOT,
                                    error_slot: TAIL_CALL_NO_SLOT,
                                });
                                continuation.current = TypedActionFrame {
                                    action: target,
                                    pc: 0,
                                    stack: Vec::new(),
                                    frame: child,
                                    event,
                                    row,
                                };
                                continue 'run;
                            }
                            _ => {
                                return Err(JsValue::from_str(
                                    "partial action callFrame continuation",
                                ))
                            }
                        }
                    }
                    TypedActionInstruction::CollectionMutation {
                        input,
                        kind,
                        key,
                        value,
                    } => {
                        let key = typed_value_string(&typed_eval_frame(
                            &self.app,
                            key,
                            &self.states,
                            row.as_ref(),
                            0,
                            &frame,
                            &event,
                        )?);
                        let value = value
                            .map(|program| {
                                typed_eval_frame(
                                    &self.app,
                                    program,
                                    &self.states,
                                    row.as_ref(),
                                    0,
                                    &frame,
                                    &event,
                                )
                            })
                            .transpose()?;
                        self.mutate_collection(input, &kind, key, value, metrics)?;
                    }
                    TypedActionInstruction::CapabilityRequest {
                        request,
                        success_pc,
                        failure_pc,
                        finally_pc,
                        result_slot,
                        error_slot,
                    } => {
                        if let TypedCapabilityRequest::Cookie(request) = request {
                            let name =
                                self.app.strings.get(request.name).cloned().ok_or_else(|| {
                                    JsValue::from_str("cookie name handle out of range")
                                })?;
                            let value = request
                                .value
                                .map(|expression| {
                                    typed_eval_frame(
                                        &self.app,
                                        expression,
                                        &self.states,
                                        row.as_ref(),
                                        0,
                                        &frame,
                                        &event,
                                    )
                                    .map(|value| typed_value_string(&value))
                                })
                                .transpose()?;
                            let operation = request.operation.clone();
                            // Name is validated here, before control crosses the host boundary.
                            if !self.app.capabilities.iter().any(|entry| {
                                entry.kind == "cookie"
                                    && entry.name == name
                                    && entry.operations.iter().any(|allowed| allowed == &operation)
                                    && entry.path == request.path
                                    && entry.same_site == request.same_site
                                    && entry.secure == request.secure
                                    && entry
                                        .expiry_modes
                                        .iter()
                                        .any(|mode| mode == &request.expiry)
                            }) {
                                return Err(JsValue::from_str("cookie request is not declared"));
                            }
                            continuation.current = TypedActionFrame {
                                action,
                                pc,
                                stack,
                                frame,
                                event,
                                row,
                            };
                            self.pending_cookies.push(TypedPendingCookie {
                                instance_id: String::new(),
                                request_id: 0,
                                continuation,
                                success_pc,
                                failure_pc,
                                finally_pc,
                                result_slot,
                                error_slot,
                                request,
                                value,
                                graph_generation: self.graph_generation,
                            });
                            return Ok(());
                        }
                        #[cfg(not(feature = "fetch"))]
                        return Err(JsValue::from_str("fetch capability is disabled"));
                        #[cfg(feature = "fetch")]
                        {
                            let TypedCapabilityRequest::Fetch(request) = request else {
                                return Err(JsValue::from_str("unsupported typed capability"));
                            };
                            let url = typed_value_string(&typed_eval_frame(
                                &self.app,
                                request.url,
                                &self.states,
                                row.as_ref(),
                                0,
                                &frame,
                                &event,
                            )?);
                            if url.is_empty() {
                                return Err(JsValue::from_str("fetch URL is empty"));
                            }
                            let headers = request
                                .headers
                                .iter()
                                .map(|header| {
                                    Ok((
                                        self.app.strings.get(header.name).cloned().ok_or_else(
                                            || JsValue::from_str("header name handle out of range"),
                                        )?,
                                        typed_value_string(&typed_eval_frame(
                                            &self.app,
                                            header.value,
                                            &self.states,
                                            row.as_ref(),
                                            0,
                                            &frame,
                                            &event,
                                        )?),
                                    ))
                                })
                                .collect::<Result<Vec<_>, JsValue>>()?;
                            let body = request
                                .body
                                .map(|expression| {
                                    typed_eval_frame(
                                        &self.app,
                                        expression,
                                        &self.states,
                                        row.as_ref(),
                                        0,
                                        &frame,
                                        &event,
                                    )
                                    .and_then(|value| {
                                        match value {
                                            // JSON.stringify already produces a fetch-ready string.
                                            // Encoding it again turns `{\"title\":\"Plec\"}` into a JSON
                                            // string literal, which APIs correctly reject as a non-object body.
                                            RuntimeValue::String(value) => Ok(value),
                                            value => value.json_body(),
                                        }
                                    })
                                })
                                .transpose()?;
                            continuation.fetch_accounting.borrow_mut().reserve_fetch()?;
                            continuation.current = TypedActionFrame {
                                action,
                                pc,
                                stack,
                                frame,
                                event,
                                row,
                            };
                            self.pending_fetches.push(TypedPendingFetch {
                                instance_id: String::new(),
                                continuation,
                                success_pc,
                                failure_pc,
                                finally_pc,
                                finalizers: Vec::new(),
                                result_slot,
                                error_slot,
                                url,
                                method: request.method,
                                headers,
                                body,
                                decode: request.decode,
                                require_ok: request.require_ok,
                                graph_generation: self.graph_generation,
                                request_id: 0,
                            });
                            return Ok(());
                        }
                    }
                    TypedActionInstruction::Return { outcome, value } => {
                        let value = value
                            .map(|expression| {
                                typed_eval_frame(
                                    &self.app,
                                    expression,
                                    &self.states,
                                    row.as_ref(),
                                    0,
                                    &frame,
                                    &event,
                                )
                            })
                            .transpose()?
                            .unwrap_or(RuntimeValue::Null);
                        if let Some(caller) = continuation.callers.pop() {
                            let mut caller_frame = caller.frame;
                            let (slot, next_pc) = match outcome {
                                plec_schema::typed::TypedReturnOutcome::Success => {
                                    (caller.result_slot, caller.success_pc)
                                }
                                plec_schema::typed::TypedReturnOutcome::Failure => {
                                    (caller.error_slot, caller.failure_pc)
                                }
                            };
                            // Tail-call continuations discard the result, so
                            // there is no caller frame slot to receive it.
                            if slot != TAIL_CALL_NO_SLOT {
                                if slot >= caller_frame.frame.len() {
                                    return Err(JsValue::from_str(
                                        "caller continuation slot out of range",
                                    ));
                                }
                                caller_frame.frame[slot] = value;
                                let other = if slot == caller.result_slot {
                                    caller.error_slot
                                } else {
                                    caller.result_slot
                                };
                                if other >= caller_frame.frame.len() {
                                    return Err(JsValue::from_str(
                                        "caller continuation slot out of range",
                                    ));
                                }
                                caller_frame.frame[other] = RuntimeValue::Null;
                            }
                            caller_frame.pc = next_pc;
                            continuation.current = caller_frame;
                            continue 'run;
                        }
                        return Ok(());
                    }
                }
                pc += 1;
                continuation.current = TypedActionFrame {
                    action,
                    pc,
                    stack: stack.clone(),
                    frame: frame.clone(),
                    event: event.clone(),
                    row: row.clone(),
                };
            }
            if let Some(caller) = continuation.callers.pop() {
                let mut caller_frame = caller.frame;
                if caller.result_slot == TAIL_CALL_NO_SLOT {
                    caller_frame.pc = caller.success_pc;
                    continuation.current = caller_frame;
                    continue 'run;
                }
                if caller.result_slot >= caller_frame.frame.len()
                    || caller.error_slot >= caller_frame.frame.len()
                {
                    return Err(JsValue::from_str("caller continuation slot out of range"));
                }
                caller_frame.frame[caller.result_slot] = RuntimeValue::Null;
                caller_frame.frame[caller.error_slot] = RuntimeValue::Null;
                caller_frame.pc = caller.success_pc;
                continuation.current = caller_frame;
                continue 'run;
            }
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plec_schema::typed::TypedApplication;
    use serde_json::json;

    fn call_frame_application() -> TypedApplication {
        serde_json::from_value(json!({
            "rootNode": 0,
            "strings": ["div"],
            "constants": [null, "success", "failure"],
            "nodes": [{"op": "element", "tag": 0, "parent": null}],
            "stateSlots": [{"initialExpression": 0, "frameSlot": 0}],
            "expressions": [
                {"instructions": [{"op": "constant", "constant": 0}, {"op": "return"}]},
                {"instructions": [{"op": "constant", "constant": 1}, {"op": "return"}]},
                {"instructions": [{"op": "constant", "constant": 2}, {"op": "return"}]}
            ],
            "actions": [
                {
                    "frameSlots": 4,
                    "instructions": [
                        {"op": "callFrame", "parameter": 1, "arguments": [], "successPc": 1, "failurePc": 4, "resultSlot": 2, "errorSlot": 3},
                        {"op": "evaluate", "expression": 1},
                        {"op": "storeState", "state": 0},
                        {"op": "return"},
                        {"op": "evaluate", "expression": 2},
                        {"op": "storeState", "state": 0},
                        {"op": "return"}
                    ]
                },
                {"instructions": [{"op": "return", "value": 1}]},
                {"instructions": [{"op": "return", "outcome": "failure", "value": 2}]}
            ]
        }))
        .unwrap()
    }

    #[test]
    fn call_frame_executes_action_handles_from_frame_slots_with_both_continuations() {
        let mut runtime = TypedRuntime::new(call_frame_application()).unwrap();
        let mut metrics = UpdateMetrics::default();

        runtime
            .execute_action_with_frame(
                0,
                &[(1, RuntimeValue::Number(1.0))],
                None,
                None,
                &mut metrics,
            )
            .unwrap();
        assert!(matches!(runtime.states[0], RuntimeValue::String(ref value) if value == "success"));

        runtime
            .execute_action_with_frame(
                0,
                &[(1, RuntimeValue::Number(2.0))],
                None,
                None,
                &mut metrics,
            )
            .unwrap();
        assert!(matches!(runtime.states[0], RuntimeValue::String(ref value) if value == "failure"));
    }

    #[test]
    #[cfg(feature = "fetch")]
    fn fetch_uses_json_stringify_results_as_raw_request_bodies() {
        let app: TypedApplication = serde_json::from_value(json!({
            "rootNode": 0,
            "strings": ["div", "title"],
            "constants": ["/todos", "Plec"],
            "nodes": [{"op": "element", "tag": 0, "parent": null}],
            "expressions": [
                {"instructions": [{"op": "constant", "constant": 0}, {"op": "return"}]},
                {"instructions": [
                    {"op": "constant", "constant": 1},
                    {"op": "makeRecord", "fields": [1]},
                    {"op": "string", "kind": "jsonStringify", "count": 1},
                    {"op": "return"}
                ]}
            ],
            "actions": [{
                "frameSlots": 2,
                "instructions": [
                    {"op": "capabilityRequest", "capability": "fetch", "request": {
                        "url": 0,
                        "method": "POST",
                        "body": 1,
                        "decode": "responseJson",
                        "requireOk": true
                    }, "successPc": 1, "failurePc": 2, "resultSlot": 0, "errorSlot": 1},
                    {"op": "return"},
                    {"op": "return", "outcome": "failure"}
                ]
            }]
        }))
        .unwrap();
        let mut runtime = TypedRuntime::new(app).unwrap();
        let mut metrics = UpdateMetrics::default();

        runtime
            .execute_action_with_frame(0, &[], None, None, &mut metrics)
            .unwrap();

        let first = runtime.take_pending_fetches();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].body.as_deref(), Some(r#"{"title":"Plec"}"#));
        assert_eq!(first[0].continuation.fetch_accounting.borrow().fetches, 1);
        assert_eq!(first[0].continuation.fetch_accounting.borrow().bytes, 0);
        first[0]
            .continuation
            .fetch_accounting
            .borrow_mut()
            .charge_response_bytes(64)
            .unwrap();

        runtime
            .execute_action_with_frame(0, &[], None, None, &mut metrics)
            .unwrap();
        let second = runtime.take_pending_fetches();
        assert_eq!(second.len(), 1);
        assert!(!Rc::ptr_eq(
            &first[0].continuation.fetch_accounting,
            &second[0].continuation.fetch_accounting,
        ));
        assert_eq!(second[0].continuation.fetch_accounting.borrow().bytes, 0);
    }

    // Error-path budget tests (action loop, self-requeuing reaction) live in
    // crates/plec-runtime/tests/untrusted_input_limits.rs: host binaries
    // cannot touch JsValue without tripping wasm-bindgen's non-wasm stubs.
}

impl TypedRuntime {
    pub fn mutate_collection(
        &mut self,
        input: usize,
        kind: &str,
        key: String,
        value: Option<RuntimeValue>,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let next = value.as_ref().and_then(|value| value.record().cloned());
        let collection = self.collections.entry(input).or_default();
        match kind {
            "append" => {
                let value = next
                    .ok_or_else(|| JsValue::from_str("collection append value must be a record"))?;
                if collection.rows.contains_key(&key) {
                    return Err(JsValue::from_str("collection append key already exists"));
                }
                collection.order.push(key.clone());
                collection.rows.insert(key, value);
            }
            "keyedReplace" => {
                let value = next.ok_or_else(|| {
                    JsValue::from_str("collection replace value must be a record")
                })?;
                if !collection.rows.contains_key(&key) {
                    return Err(JsValue::from_str("collection replace key missing"));
                }
                collection.rows.insert(key, value);
            }
            "keyedRemove" => {
                if value.is_some() {
                    return Err(JsValue::from_str("collection remove forbids a value"));
                }
                if collection.rows.remove(&key).is_none() {
                    return Err(JsValue::from_str("collection remove key missing"));
                }
                collection.order.retain(|entry| entry != &key);
            }
            _ => return Err(JsValue::from_str("unknown collection mutation kind")),
        }
        self.invalidate_collection(input, metrics)
    }
}

impl TypedRuntime {
    pub fn invalidate_collection(
        &mut self,
        input: usize,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let snapshot = self.collections.get(&input).cloned().unwrap_or_default();
        let targets = self
            .app
            .loops
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| (entry.input == Some(input)).then_some(index))
            .collect::<Vec<_>>();
        for loop_index in targets {
            if snapshot.order.len() > plec_ir::limits::MAX_LOOP_ROWS {
                return Err(JsValue::from_str("LOOP_ROW_LIMIT_EXCEEDED"));
            }
            let parent = self.parent_for_loop(loop_index)?;
            let projection = snapshot
                .order
                .iter()
                .filter_map(|key| {
                    snapshot
                        .rows
                        .get(key)
                        .cloned()
                        .map(|row| (key.clone(), row))
                })
                .collect();
            self.reconcile_loop(loop_index, &parent, projection, metrics)?;
        }
        Ok(())
    }
}

impl TypedRuntime {
    pub fn refresh_state(
        &mut self,
        state: usize,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let targets = self
            .app
            .dependency_edges
            .iter()
            .filter_map(|edge| {
                (edge.source.kind == "state" && edge.source.handle == state)
                    .then(|| (edge.target.kind.clone(), edge.target.handle))
            })
            .collect::<Vec<_>>();
        let mut row_updates = Vec::new();
        for (_, handle) in targets.iter().filter(|(kind, _)| kind == "conditional") {
            if self.conditionals.contains_key(handle) {
                self.reconcile_static_conditional(*handle, metrics)?;
            }
        }
        if targets.iter().any(|(kind, _)| kind == "conditional") {
            row_updates.extend(
                self.loops
                    .iter()
                    .flat_map(|(loop_index, rows)| {
                        rows.rows.keys().cloned().map(move |key| (*loop_index, key))
                    })
                    .collect::<Vec<_>>(),
            );
        }
        for target in targets
            .iter()
            .filter_map(|(kind, handle)| match kind.as_str() {
                "binding" => self.app.bindings.get(*handle).map(|binding| binding.target),
                "propProgram" => self
                    .app
                    .prop_programs
                    .get(*handle)
                    .map(|program| program.target),
                // Component-call targets have no direct DOM write here; the row
                // scan below queues their prop refreshes through the owner row.
                "component" => Some(*handle),
                _ => None,
            })
        {
            row_updates.extend(self.loops.iter().flat_map(|(loop_index, rows)| {
                rows.rows.iter().filter_map(move |(key, row)| {
                    (row.nodes.contains_key(&target)
                        || row
                            .conditionals
                            .values()
                            .any(|region| region.nodes.contains_key(&target)))
                    .then(|| (*loop_index, key.clone()))
                })
            }));
        }
        row_updates.sort();
        row_updates.dedup();
        for (loop_index, key) in row_updates {
            if let Some(values) = self
                .loops
                .get(&loop_index)
                .and_then(|rows| rows.rows.get(&key))
                .map(|row| row.values.clone())
            {
                self.update_typed_row(loop_index, &key, values, Some(state), metrics)?;
            }
        }
        for (kind, handle) in targets {
            if kind == "conditional" {
                continue;
            }
            if kind == "binding" {
                if let (Some(binding), Some(node)) = (
                    self.app.bindings.get(handle).cloned(),
                    self.nodes.get(&self.app.bindings[handle].target).cloned(),
                ) {
                    typed_apply_binding(&self.app, &binding, &node, &self.states, None, 0)?;
                    metrics.dom_operations += 1;
                    metrics.bindings_touched += 1;
                }
            } else if kind == "propProgram" {
                if let Some(program) = self.app.prop_programs.get(handle).cloned() {
                    if let Some(node) = self.nodes.get(&program.target).cloned() {
                        for write in program.writes {
                            let value = match write.expression {
                                Some(expression) => {
                                    typed_eval(&self.app, expression, &self.states, None, 0)?
                                }
                                None => write
                                    .constant
                                    .and_then(|index| self.app.constants.get(index))
                                    .cloned()
                                    .unwrap_or_default(),
                            };
                            if write.spread {
                                typed_apply_spread(&self.app, &write.kind, &node, value)?;
                            } else {
                                typed_apply_value(
                                    &self.app,
                                    &write.kind,
                                    write.name,
                                    &node,
                                    value,
                                )?;
                            }
                            metrics.dom_operations += 1;
                        }
                    }
                }
            } else if kind == "loop" {
                let parent = self.parent_for_loop(handle)?;
                self.render_loop(handle, &parent)?;
            }
        }
        // ponytail: scans mounted calls; index component dependency edges if profiles require it.
        self.queue_static_component_refreshes()?;
        self.queue_reactions_from("state", state);
        self.drain_reactions(metrics)?;
        Ok(())
    }

    fn queue_reactions_from(&mut self, kind: &str, handle: usize) {
        for edge in &self.app.dependency_edges {
            if edge.source.kind == kind
                && edge.source.handle == handle
                && edge.target.kind == "reaction"
                && !self.pending_reactions.contains(&edge.target.handle)
            {
                self.pending_reactions.push(edge.target.handle);
            }
        }
    }

    fn drain_reactions(&mut self, metrics: &mut UpdateMetrics) -> Result<(), JsValue> {
        // A reaction that (transitively) re-queues itself would otherwise
        // drain forever. The step budget bounds total work per drain and the
        // depth bound rejects the recursive drain chains a refresh_state
        // triggers from inside a reaction.
        if self.reaction_drain_depth >= plec_ir::limits::MAX_REACTION_DRAIN_DEPTH {
            return Err(JsValue::from_str("reaction drain depth exceeds limit"));
        }
        self.reaction_drain_depth += 1;
        let result = self.drain_reactions_bounded(metrics);
        self.reaction_drain_depth -= 1;
        result
    }

    fn drain_reactions_bounded(&mut self, metrics: &mut UpdateMetrics) -> Result<(), JsValue> {
        let mut budget = plec_ir::limits::MAX_REACTION_STEPS;
        while let Some(reaction) = self.pending_reactions.first().copied() {
            if budget == 0 {
                return Err(JsValue::from_str("reaction execution budget exceeded"));
            }
            budget -= 1;
            self.pending_reactions.remove(0);
            let reaction_def = self
                .app
                .reactions
                .get(reaction)
                .cloned()
                .ok_or_else(|| JsValue::from_str("reaction handle out of range"))?;
            if reaction_def
                .dependencies
                .iter()
                .any(|dependency| *dependency >= self.app.expressions.len())
            {
                return Err(JsValue::from_str("reaction dependency out of range"));
            }
            if let Some(cleanup) = self
                .reaction_cleanups
                .get_mut(reaction)
                .and_then(Option::take)
            {
                self.execute_action(cleanup, &[], None, None, metrics)?;
            }
            self.execute_action(reaction_def.action, &[], None, None, metrics)?;
            if let Some(slot) = self.reaction_cleanups.get_mut(reaction) {
                *slot = reaction_def.cleanup_action;
            }
        }
        Ok(())
    }

    pub fn refresh_prop(
        &mut self,
        prop: usize,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let targets = self
            .app
            .dependency_edges
            .iter()
            .filter_map(|edge| {
                (edge.source.kind == "prop" && edge.source.handle == prop)
                    .then(|| (edge.target.kind.clone(), edge.target.handle))
            })
            .collect::<Vec<_>>();
        for (_, handle) in targets.iter().filter(|(kind, _)| kind == "conditional") {
            if self.conditionals.contains_key(handle) {
                self.reconcile_static_conditional(*handle, metrics)?;
            }
        }
        for (kind, handle) in targets {
            match kind.as_str() {
                "binding" => {
                    if let (Some(binding), Some(node)) = (
                        self.app.bindings.get(handle).cloned(),
                        self.app
                            .bindings
                            .get(handle)
                            .and_then(|binding| self.nodes.get(&binding.target))
                            .cloned(),
                    ) {
                        typed_apply_binding(&self.app, &binding, &node, &self.states, None, 0)?;
                        metrics.dom_operations += 1;
                        metrics.bindings_touched += 1;
                    }
                }
                "propProgram" => {
                    if let Some(program) = self.app.prop_programs.get(handle).cloned() {
                        if let Some(node) = self.nodes.get(&program.target).cloned() {
                            for write in program.writes {
                                let value = write
                                    .expression
                                    .map(|expression| {
                                        typed_eval(&self.app, expression, &self.states, None, 0)
                                    })
                                    .transpose()?
                                    .or_else(|| {
                                        write
                                            .constant
                                            .and_then(|index| self.app.constants.get(index))
                                            .cloned()
                                    })
                                    .unwrap_or_default();
                                if write.spread {
                                    typed_apply_spread(&self.app, &write.kind, &node, value)?;
                                } else {
                                    typed_apply_value(
                                        &self.app,
                                        &write.kind,
                                        write.name,
                                        &node,
                                        value,
                                    )?;
                                }
                                metrics.dom_operations += 1;
                            }
                        }
                    }
                }
                "loop" => {
                    let parent = self.parent_for_loop(handle)?;
                    self.render_loop(handle, &parent)?;
                }
                _ => {}
            }
        }
        self.queue_static_component_refreshes()?;
        self.queue_reactions_from("prop", prop);
        self.drain_reactions(metrics)?;
        Ok(())
    }
}
