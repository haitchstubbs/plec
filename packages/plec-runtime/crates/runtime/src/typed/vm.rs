use crate::dom::bindings::*;
use crate::eval::typed_vm::*;
use crate::runtime::lifecycle::*;
use crate::typed::{fetch::*, runtime::*};

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
        mut pc: usize,
        frame: Vec<RuntimeValue>,
        event: &[RuntimeValue],
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
        let mut stack = Vec::new();
        while let Some(instruction) = program.instructions.get(pc).cloned() {
            match instruction {
                TypedActionInstruction::Evaluate { expression } => stack.push(typed_eval_frame(
                    &self.app,
                    expression,
                    &self.states,
                    row.as_ref(),
                    0,
                    &frame,
                    event,
                )?),
                TypedActionInstruction::StoreState { state } => {
                    let value = stack
                        .pop()
                        .ok_or_else(|| JsValue::from_str("action stack underflow: storeState"))?;
                    if state >= self.states.len() {
                        return Err(JsValue::from_str("state handle out of range"));
                    }
                    self.states[state] = value;
                    self.refresh_state(state, metrics)?;
                }
                TypedActionInstruction::PreventDefault => {
                    if let Some(event) = native_event {
                        event.prevent_default();
                    }
                }
                TypedActionInstruction::Jump { target } => {
                    pc = target;
                    continue;
                }
                TypedActionInstruction::JumpIfFalse { target } => {
                    if !typed_truthy(
                        &stack.pop().ok_or_else(|| {
                            JsValue::from_str("action stack underflow: jumpIfFalse")
                        })?,
                    ) {
                        pc = target;
                        continue;
                    }
                }
                TypedActionInstruction::Call {
                    action: target,
                    arguments,
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
                            event,
                        )?;
                    }
                    self.execute_action_at(
                        target,
                        0,
                        child,
                        event,
                        row.clone(),
                        native_event,
                        metrics,
                    )?;
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
                        event,
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
                                event,
                            )
                        })
                        .transpose()?;
                    self.mutate_collection(input, &kind, key, value, metrics)?;
                }
                TypedActionInstruction::CapabilityRequest {
                    capability,
                    request,
                    success_pc,
                    failure_pc,
                    finally_pc,
                    result_slot,
                    error_slot,
                } => {
                    if capability != "fetch" {
                        return Err(JsValue::from_str("unsupported typed capability"));
                    }
                    #[cfg(not(feature = "fetch"))]
                    return Err(JsValue::from_str("fetch capability is disabled"));
                    #[cfg(feature = "fetch")]
                    {
                        let url = typed_value_string(&typed_eval_frame(
                            &self.app,
                            request.url,
                            &self.states,
                            row.as_ref(),
                            0,
                            &frame,
                            event,
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
                                        event,
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
                                    event,
                                )
                                .and_then(|value| value.json_body())
                            })
                            .transpose()?;
                        self.pending_fetches.push(TypedPendingFetch {
                            action,
                            success_pc,
                            failure_pc,
                            finally_pc,
                            finalizers: Vec::new(),
                            result_slot,
                            error_slot,
                            frame,
                            event: event.to_vec(),
                            row,
                            url,
                            method: request.method,
                            headers,
                            body,
                            decode: request.decode,
                            require_ok: request.require_ok,
                        });
                        return Ok(());
                    }
                }
                TypedActionInstruction::Return => return Ok(()),
            }
            pc += 1;
        }
        Ok(())
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
        for (kind, handle) in targets {
            if kind == "binding" {
                if let (Some(binding), Some(node)) = (
                    self.app.bindings.get(handle).cloned(),
                    self.nodes.get(&self.app.bindings[handle].target).cloned(),
                ) {
                    typed_apply_binding(&self.app, &binding, &node, &self.states, None, 0)?;
                    metrics.dom_operations += 1;
                    metrics.bindings_touched += 1;
                }
            } else if kind == "loop" {
                let parent = self.parent_for_loop(handle)?;
                self.render_loop(handle, &parent)?;
            }
        }
        Ok(())
    }
}
