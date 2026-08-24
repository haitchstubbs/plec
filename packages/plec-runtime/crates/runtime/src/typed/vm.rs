use crate::dom::bindings::*;
use crate::dom::platform::document;
use crate::eval::typed_vm::*;
use crate::runtime::lifecycle::*;
use crate::schema::typed::TypedCapabilityRequest;
use crate::typed::cookie::*;
#[cfg(feature = "fetch")]
use crate::typed::fetch::*;
use crate::typed::runtime::*;

#[derive(Clone)]
pub(crate) struct TypedActionFrame {
    pub(crate) action: usize,
    pub(crate) pc: usize,
    pub(crate) stack: Vec<RuntimeValue>,
    pub(crate) frame: Vec<RuntimeValue>,
    pub(crate) event: Vec<RuntimeValue>,
    pub(crate) row: Option<HashMap<String, RuntimeValue>>,
}

#[derive(Clone)]
pub(crate) struct TypedCallerContinuation {
    pub(crate) frame: TypedActionFrame,
    pub(crate) success_pc: usize,
    pub(crate) failure_pc: usize,
    pub(crate) result_slot: usize,
    pub(crate) error_slot: usize,
}

#[derive(Clone)]
pub(crate) struct TypedContinuationStack {
    pub(crate) current: TypedActionFrame,
    pub(crate) callers: Vec<TypedCallerContinuation>,
}

impl TypedRuntime {
    pub(crate) fn execute_action(
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
    pub(crate) fn execute_action_with_frame(
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
    pub(crate) fn execute_action_at(
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
            },
            native_event,
            metrics,
        )
    }

    pub(crate) fn execute_continuation(
        &mut self,
        mut continuation: TypedContinuationStack,
        native_event: Option<&Event>,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        'run: loop {
            let action = continuation.current.action;
            let mut pc = continuation.current.pc;
            let frame = continuation.current.frame.clone();
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
                    TypedActionInstruction::PreventDefault => {
                        if let Some(event) = native_event {
                            event.prevent_default();
                        }
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
                            (None, None, None, None) => self.execute_action_at(
                                target,
                                0,
                                child,
                                &event,
                                row.clone(),
                                native_event,
                                metrics,
                            )?,
                            _ => return Err(JsValue::from_str("partial action call continuation")),
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
                            }) {
                                return Err(JsValue::from_str("cookie operation is not declared"));
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
                                    .and_then(|value| value.json_body())
                                })
                                .transpose()?;
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
                                crate::schema::typed::TypedReturnOutcome::Success => {
                                    (caller.result_slot, caller.success_pc)
                                }
                                crate::schema::typed::TypedReturnOutcome::Failure => {
                                    (caller.error_slot, caller.failure_pc)
                                }
                            };
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

impl TypedRuntime {
    pub(crate) fn mutate_collection(
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
    pub(crate) fn invalidate_collection(
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
    pub(crate) fn refresh_state(
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
                            typed_apply_value(
                                &self.app,
                                &write.kind,
                                Some(write.name),
                                &node,
                                value,
                            )?;
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
        Ok(())
    }

    pub(crate) fn refresh_prop(
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
                                typed_apply_value(
                                    &self.app,
                                    &write.kind,
                                    Some(write.name),
                                    &node,
                                    value,
                                )?;
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
        Ok(())
    }
}
