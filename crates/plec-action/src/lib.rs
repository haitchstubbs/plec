//! Host-neutral execution for typed action control flow.
//!
//! Hosts own expression evaluation, side effects, and capability transport.
//! This crate owns action frames, continuations, limits, and suspension.

use plec_schema::{
    delta::RuntimeValue,
    typed::{TypedAction, TypedActionInstruction, TypedCapabilityRequest, TypedReturnOutcome},
};
use std::sync::{Arc, Mutex};

const TAIL_CALL_NO_SLOT: usize = usize::MAX;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionError(pub String);

impl ActionError {
    pub fn unsupported(instruction: &str) -> Self {
        Self(format!("unsupported action instruction: {instruction}"))
    }
}

impl std::fmt::Display for ActionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::error::Error for ActionError {}

#[derive(Clone, Debug, PartialEq)]
pub enum ActionOutcome {
    Success(RuntimeValue),
    Failure(RuntimeValue),
}

#[derive(Clone, Debug)]
pub struct ActionFrame {
    pub action: usize,
    pub pc: usize,
    pub stack: Vec<RuntimeValue>,
    pub frame: Vec<RuntimeValue>,
}

#[derive(Clone, Debug)]
pub struct CallerContinuation {
    pub frame: ActionFrame,
    pub success_pc: usize,
    pub failure_pc: usize,
    pub result_slot: usize,
    pub error_slot: usize,
}

#[derive(Clone, Debug)]
pub struct ActionBudget {
    fuel: usize,
    fetches: usize,
    fetch_bytes: usize,
}

#[derive(Clone, Debug)]
pub struct Continuation {
    pub current: ActionFrame,
    pub callers: Vec<CallerContinuation>,
    budget: Arc<Mutex<ActionBudget>>,
}

#[derive(Clone, Debug)]
pub struct Suspension<Request> {
    pub continuation: Continuation,
    pub request: Request,
    pub success_pc: usize,
    pub failure_pc: usize,
    pub finally_pc: Option<usize>,
    pub result_slot: usize,
    pub error_slot: usize,
    finalizers: Vec<usize>,
    completed: Option<ActionOutcome>,
    finalizer_frame: Option<ActionFrame>,
}

#[derive(Clone, Debug)]
pub enum Run<Request> {
    Suspended(Suspension<Request>),
    Complete(ActionOutcome),
}

/// Adapter for values and effects that do not belong to action control flow.
/// A loader host normally supports only expression evaluation and fetch.
pub trait ActionHost {
    type Request: Clone;

    fn evaluate(
        &mut self,
        expression: usize,
        frame: &[RuntimeValue],
    ) -> Result<RuntimeValue, ActionError>;

    fn prepare_capability(
        &mut self,
        request: &TypedCapabilityRequest,
        frame: &[RuntimeValue],
    ) -> Result<Self::Request, ActionError>;

    /// State-slot write. Hosts own state storage and dependent refresh.
    fn store_state(&mut self, _state: usize, _value: RuntimeValue) -> Result<(), ActionError> {
        Err(ActionError::unsupported("storeState"))
    }

    /// Reference-slot write. Hosts own reference storage.
    fn store_ref(
        &mut self,
        _reference: usize,
        _value: RuntimeValue,
    ) -> Result<(), ActionError> {
        Err(ActionError::unsupported("storeRef"))
    }

    /// Records the host's current active element into a focus reference.
    fn capture_active_element(&mut self, _reference: usize) -> Result<(), ActionError> {
        Err(ActionError::unsupported("captureActiveElement"))
    }

    fn focus_host_ref(&mut self, _reference: usize) -> Result<(), ActionError> {
        Err(ActionError::unsupported("focusHostRef"))
    }

    fn focus_ref(&mut self, _reference: usize) -> Result<(), ActionError> {
        Err(ActionError::unsupported("focusRef"))
    }

    /// Suppresses the default behavior of the native event backing this run,
    /// when the host exposes one.
    fn prevent_default(&mut self) -> Result<(), ActionError> {
        Err(ActionError::unsupported("preventDefault"))
    }

    /// Stores a host-named reference; the host resolves the reference handle
    /// to its own string table.
    fn store_host_ref(&mut self, _reference: usize) -> Result<(), ActionError> {
        Err(ActionError::unsupported("storeHostRef"))
    }

    /// Delivers a callable component prop invocation. A missing required prop
    /// must fail; a missing optional prop completes without error and control
    /// simply falls through to the next instruction.
    fn call_prop(
        &mut self,
        _prop: usize,
        _arguments: Vec<RuntimeValue>,
        optional: bool,
    ) -> Result<(), ActionError> {
        Err(ActionError::unsupported(if optional {
            "callPropOptional"
        } else {
            "callProp"
        }))
    }

    /// Applies a keyed collection mutation. Hosts own collection storage,
    /// ordering, key stringification, and dependent invalidation.
    fn mutate_collection(
        &mut self,
        _input: usize,
        _kind: &str,
        _key: RuntimeValue,
        _value: Option<RuntimeValue>,
    ) -> Result<(), ActionError> {
        Err(ActionError::unsupported("collectionMutation"))
    }
}

pub fn start<H: ActionHost>(
    actions: &[TypedAction],
    action: usize,
    frame: Vec<RuntimeValue>,
    host: &mut H,
) -> Result<Run<H::Request>, ActionError> {
    let program = actions
        .get(action)
        .ok_or_else(|| ActionError("action handle out of range".into()))?;
    if frame.len() != program.frame_slots {
        return Err(ActionError("action frame size mismatch".into()));
    }
    drive(
        actions,
        Continuation {
            current: ActionFrame {
                action,
                pc: 0,
                stack: Vec::new(),
                frame,
            },
            callers: Vec::new(),
            budget: Arc::new(Mutex::new(ActionBudget {
                fuel: plec_ir::limits::MAX_ACTION_STEPS,
                fetches: 0,
                fetch_bytes: 0,
            })),
        },
        host,
        Vec::new(),
    )
}

/// Charges decoded response bytes before resume. A host reports actual
/// streamed bytes, while this shared machine enforces action-wide budget.
pub fn charge_response_bytes<Request>(
    suspension: &mut Suspension<Request>,
    bytes: usize,
) -> Result<(), ActionError> {
    let mut budget = suspension
        .continuation
        .budget
        .lock()
        .expect("action budget poisoned");
    let total = budget
        .fetch_bytes
        .checked_add(bytes)
        .ok_or_else(|| ActionError("fetch response bytes overflow per-action accounting".into()))?;
    if total > plec_ir::limits::MAX_FETCH_BYTES_PER_ACTION {
        return Err(ActionError(format!(
            "fetch response bytes exceed the {} per-action limit",
            plec_ir::limits::MAX_FETCH_BYTES_PER_ACTION
        )));
    }
    budget.fetch_bytes = total;
    Ok(())
}

pub fn resume<H: ActionHost>(
    actions: &[TypedAction],
    mut suspension: Suspension<H::Request>,
    result: Result<RuntimeValue, RuntimeValue>,
    host: &mut H,
) -> Result<Run<H::Request>, ActionError> {
    let terminal = suspension.completed.take();
    let finalizer_frame = suspension.finalizer_frame.take();
    let budget = suspension.continuation.budget.clone();
    let frame = &mut suspension.continuation.current.frame;
    let pc = match result {
        Ok(value) => {
            if suspension.result_slot >= frame.len() {
                return Err(ActionError("result frame slot out of range".into()));
            }
            frame[suspension.result_slot] = value;
            suspension.success_pc
        }
        Err(error) => {
            if suspension.error_slot >= frame.len() {
                return Err(ActionError("error frame slot out of range".into()));
            }
            frame[suspension.error_slot] = error;
            suspension.failure_pc
        }
    };
    suspension.continuation.current.pc = pc;
    let mut finalizers = suspension.finalizers;
    if let Some(finally_pc) = suspension.finally_pc {
        finalizers.push(finally_pc);
    }
    let next = drive(actions, suspension.continuation, host, finalizers)?;
    match (terminal, finalizer_frame, next) {
        (_, _, Run::Suspended(next)) => Ok(Run::Suspended(next)),
        (Some(outcome), Some(frame), Run::Complete(_)) => run_finalizers(
            actions,
            Continuation {
                current: frame,
                callers: Vec::new(),
                budget,
            },
            outcome,
            host,
            Vec::new(),
        ),
        (None, _, Run::Complete(outcome)) => {
            // `drive` runs deferred finalizers before returning terminal.
            Ok(Run::Complete(outcome))
        }
        (Some(outcome), None, Run::Complete(_)) => Ok(Run::Complete(outcome)),
    }
}

fn drive<H: ActionHost>(
    actions: &[TypedAction],
    mut continuation: Continuation,
    host: &mut H,
    finalizers: Vec<usize>,
) -> Result<Run<H::Request>, ActionError> {
    'run: loop {
        let action = continuation.current.action;
        let program = actions
            .get(action)
            .ok_or_else(|| ActionError("action handle out of range".into()))?;
        while let Some(instruction) = program.instructions.get(continuation.current.pc).cloned() {
            let mut budget = continuation.budget.lock().expect("action budget poisoned");
            if budget.fuel == 0 {
                return Err(ActionError("action execution budget exceeded".into()));
            }
            budget.fuel -= 1;
            drop(budget);
            match instruction {
                TypedActionInstruction::Evaluate { expression } => {
                    let value = host.evaluate(expression, &continuation.current.frame)?;
                    continuation.current.stack.push(value);
                }
                TypedActionInstruction::StoreFrame { slot } => {
                    let value =
                        continuation.current.stack.pop().ok_or_else(|| {
                            ActionError("action stack underflow: storeFrame".into())
                        })?;
                    let target = continuation
                        .current
                        .frame
                        .get_mut(slot)
                        .ok_or_else(|| ActionError("action frame slot out of range".into()))?;
                    *target = value;
                }
                TypedActionInstruction::Jump { target } => {
                    continuation.current.pc = target;
                    continue;
                }
                TypedActionInstruction::JumpIfFalse { target } => {
                    let value =
                        continuation.current.stack.pop().ok_or_else(|| {
                            ActionError("action stack underflow: jumpIfFalse".into())
                        })?;
                    if !value.truthy() {
                        continuation.current.pc = target;
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
                    enter_call(
                        actions,
                        &mut continuation,
                        target,
                        arguments,
                        success_pc,
                        failure_pc,
                        result_slot,
                        error_slot,
                        host,
                    )?;
                    continue 'run;
                }
                TypedActionInstruction::CallFrame {
                    parameter,
                    arguments,
                    success_pc,
                    failure_pc,
                    result_slot,
                    error_slot,
                } => {
                    let RuntimeValue::Number(target) = continuation
                        .current
                        .frame
                        .get(parameter)
                        .cloned()
                        .unwrap_or(RuntimeValue::Null)
                    else {
                        return Err(ActionError(
                            "callFrame parameter must be an action handle".into(),
                        ));
                    };
                    enter_call(
                        actions,
                        &mut continuation,
                        target as usize,
                        arguments,
                        success_pc,
                        failure_pc,
                        result_slot,
                        error_slot,
                        host,
                    )?;
                    continue 'run;
                }
                TypedActionInstruction::CapabilityRequest {
                    request,
                    success_pc,
                    failure_pc,
                    finally_pc,
                    result_slot,
                    error_slot,
                } => {
                    if matches!(request, TypedCapabilityRequest::Fetch(_)) {
                        let mut budget =
                            continuation.budget.lock().expect("action budget poisoned");
                        if budget.fetches >= plec_ir::limits::MAX_FETCHES_PER_ACTION {
                            return Err(ActionError(format!(
                                "fetch count exceeds the {} per-action limit",
                                plec_ir::limits::MAX_FETCHES_PER_ACTION
                            )));
                        }
                        budget.fetches += 1;
                    }
                    let request = host.prepare_capability(&request, &continuation.current.frame)?;
                    return Ok(Run::Suspended(Suspension {
                        continuation,
                        request,
                        success_pc,
                        failure_pc,
                        finally_pc,
                        result_slot,
                        error_slot,
                        finalizers,
                        completed: None,
                        finalizer_frame: None,
                    }));
                }
                TypedActionInstruction::Return { outcome, value } => {
                    let value = value
                        .map(|expression| host.evaluate(expression, &continuation.current.frame))
                        .transpose()?
                        .unwrap_or(RuntimeValue::Null);
                    if let Some(caller) = continuation.callers.pop() {
                        let mut caller_frame = caller.frame;
                        let (slot, other, next_pc) = match outcome {
                            TypedReturnOutcome::Success => {
                                (caller.result_slot, caller.error_slot, caller.success_pc)
                            }
                            TypedReturnOutcome::Failure => {
                                (caller.error_slot, caller.result_slot, caller.failure_pc)
                            }
                        };
                        if slot != TAIL_CALL_NO_SLOT {
                            let target = caller_frame.frame.get_mut(slot).ok_or_else(|| {
                                ActionError("caller continuation slot out of range".into())
                            })?;
                            *target = value;
                            let target = caller_frame.frame.get_mut(other).ok_or_else(|| {
                                ActionError("caller continuation slot out of range".into())
                            })?;
                            *target = RuntimeValue::Null;
                        }
                        caller_frame.pc = next_pc;
                        continuation.current = caller_frame;
                        continue 'run;
                    }
                    let outcome = match outcome {
                        TypedReturnOutcome::Success => ActionOutcome::Success(value),
                        TypedReturnOutcome::Failure => ActionOutcome::Failure(value),
                    };
                    return run_finalizers(actions, continuation, outcome, host, finalizers);
                }
                TypedActionInstruction::StoreState { state } => {
                    let value = continuation.current.stack.pop().ok_or_else(|| {
                        ActionError("action stack underflow: storeState".into())
                    })?;
                    host.store_state(state, value)?;
                }
                TypedActionInstruction::StoreRef { reference } => {
                    let value = continuation.current.stack.pop().ok_or_else(|| {
                        ActionError("action stack underflow: storeRef".into())
                    })?;
                    host.store_ref(reference, value)?;
                }
                TypedActionInstruction::CaptureActiveElement { reference } => {
                    host.capture_active_element(reference)?;
                }
                TypedActionInstruction::FocusHostRef { reference } => {
                    host.focus_host_ref(reference)?;
                }
                TypedActionInstruction::FocusRef { reference } => {
                    host.focus_ref(reference)?;
                }
                TypedActionInstruction::PreventDefault => {
                    host.prevent_default()?;
                }
                TypedActionInstruction::CallProp { prop, arguments } => {
                    let arguments =
                        evaluate_arguments(arguments, &continuation.current.frame, host)?;
                    host.call_prop(prop, arguments, false)?;
                }
                TypedActionInstruction::CallPropOptional { prop, arguments } => {
                    let arguments =
                        evaluate_arguments(arguments, &continuation.current.frame, host)?;
                    host.call_prop(prop, arguments, true)?;
                }
                TypedActionInstruction::CollectionMutation {
                    input,
                    kind,
                    key,
                    value,
                } => {
                    let key = host.evaluate(key, &continuation.current.frame)?;
                    let value = value
                        .map(|expression| host.evaluate(expression, &continuation.current.frame))
                        .transpose()?;
                    host.mutate_collection(input, &kind, key, value)?;
                }
                TypedActionInstruction::StoreHostRef { r#ref } => {
                    host.store_host_ref(r#ref)?;
                }
            }
            continuation.current.pc += 1;
        }
        if let Some(caller) = continuation.callers.pop() {
            let mut caller_frame = caller.frame;
            if caller.result_slot != TAIL_CALL_NO_SLOT {
                let target = caller_frame
                    .frame
                    .get_mut(caller.result_slot)
                    .ok_or_else(|| ActionError("caller continuation slot out of range".into()))?;
                *target = RuntimeValue::Null;
                let target = caller_frame
                    .frame
                    .get_mut(caller.error_slot)
                    .ok_or_else(|| ActionError("caller continuation slot out of range".into()))?;
                *target = RuntimeValue::Null;
            }
            caller_frame.pc = caller.success_pc;
            continuation.current = caller_frame;
            continue;
        }
        return run_finalizers(
            actions,
            continuation,
            ActionOutcome::Success(RuntimeValue::Null),
            host,
            finalizers,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn enter_call<H: ActionHost>(
    actions: &[TypedAction],
    continuation: &mut Continuation,
    target: usize,
    arguments: Vec<usize>,
    success_pc: Option<usize>,
    failure_pc: Option<usize>,
    result_slot: Option<usize>,
    error_slot: Option<usize>,
    host: &mut H,
) -> Result<(), ActionError> {
    let target_program = actions
        .get(target)
        .ok_or_else(|| ActionError("action call handle out of range".into()))?;
    if arguments.len() != target_program.parameter_slots.len() {
        return Err(ActionError("action call arity mismatch".into()));
    }
    if continuation.callers.len() >= plec_ir::limits::MAX_CALL_DEPTH {
        return Err(ActionError("action call depth exceeds limit".into()));
    }
    let mut child = vec![RuntimeValue::Null; target_program.frame_slots];
    for (expression, slot) in arguments.into_iter().zip(&target_program.parameter_slots) {
        child[*slot] = host.evaluate(expression, &continuation.current.frame)?;
    }
    let (success_pc, failure_pc, result_slot, error_slot) =
        match (success_pc, failure_pc, result_slot, error_slot) {
            (Some(success), Some(failure), Some(result), Some(error)) => {
                (success, failure, result, error)
            }
            (None, None, None, None) => (
                continuation.current.pc + 1,
                continuation.current.pc + 1,
                TAIL_CALL_NO_SLOT,
                TAIL_CALL_NO_SLOT,
            ),
            _ => return Err(ActionError("partial action call continuation".into())),
        };
    continuation.callers.push(CallerContinuation {
        frame: ActionFrame {
            pc: continuation.current.pc + 1,
            ..continuation.current.clone()
        },
        success_pc,
        failure_pc,
        result_slot,
        error_slot,
    });
    continuation.current = ActionFrame {
        action: target,
        pc: 0,
        stack: Vec::new(),
        frame: child,
    };
    Ok(())
}

fn run_finalizers<H: ActionHost>(
    actions: &[TypedAction],
    continuation: Continuation,
    outcome: ActionOutcome,
    host: &mut H,
    mut finalizers: Vec<usize>,
) -> Result<Run<H::Request>, ActionError> {
    while let Some(pc) = finalizers.pop() {
        let finalizer_frame = continuation.current.clone();
        let finalizer = Continuation {
            current: ActionFrame {
                pc,
                stack: Vec::new(),
                ..finalizer_frame.clone()
            },
            callers: Vec::new(),
            budget: continuation.budget.clone(),
        };
        match drive(actions, finalizer, host, Vec::new())? {
            Run::Complete(_) => {}
            Run::Suspended(mut suspended) => {
                suspended.finalizers = finalizers;
                suspended.completed = Some(outcome);
                suspended.finalizer_frame = Some(finalizer_frame);
                return Ok(Run::Suspended(suspended));
            }
        }
    }
    Ok(Run::Complete(outcome))
}

fn evaluate_arguments<H: ActionHost>(
    arguments: Vec<usize>,
    frame: &[RuntimeValue],
    host: &mut H,
) -> Result<Vec<RuntimeValue>, ActionError> {
    arguments
        .into_iter()
        .map(|expression| host.evaluate(expression, frame))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestHost;

    impl ActionHost for TestHost {
        type Request = String;

        fn evaluate(
            &mut self,
            expression: usize,
            frame: &[RuntimeValue],
        ) -> Result<RuntimeValue, ActionError> {
            Ok(match expression {
                0 => frame.first().cloned().unwrap_or(RuntimeValue::Null),
                1 => RuntimeValue::String("/api/data".into()),
                2 => RuntimeValue::Number(42.0),
                _ => RuntimeValue::Null,
            })
        }

        fn prepare_capability(
            &mut self,
            request: &TypedCapabilityRequest,
            _frame: &[RuntimeValue],
        ) -> Result<Self::Request, ActionError> {
            match request {
                TypedCapabilityRequest::Fetch(_) => Ok("fetch".into()),
                TypedCapabilityRequest::Cookie(_) => Err(ActionError(
                    "unsupported route loader capability: cookie".into(),
                )),
            }
        }
    }

    fn actions(value: serde_json::Value) -> Vec<TypedAction> {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn resumes_fetch_into_terminal_value() {
        let actions = actions(serde_json::json!([{
            "frameSlots": 2,
            "instructions": [
                {"op":"capabilityRequest","capability":"fetch","request":{"url":1,"method":"GET","decode":"responseJson"},"successPc":1,"failurePc":2,"resultSlot":0,"errorSlot":1},
                {"op":"return","value":0},
                {"op":"return","outcome":"failure","value":1}
            ]
        }]));
        let mut host = TestHost;
        let Run::Suspended(suspension) =
            start(&actions, 0, vec![RuntimeValue::Null; 2], &mut host).unwrap()
        else {
            panic!("expected suspension");
        };
        let Run::Complete(ActionOutcome::Success(value)) = resume(
            &actions,
            suspension,
            Ok(RuntimeValue::String("loaded".into())),
            &mut host,
        )
        .unwrap() else {
            panic!("expected completed success");
        };
        assert_eq!(value, RuntimeValue::String("loaded".into()));
    }

    #[test]
    fn calls_resume_caller_success_continuation() {
        let actions = actions(serde_json::json!([
            {"frameSlots":2,"instructions":[
                {"op":"call","action":1,"arguments":[2],"successPc":1,"failurePc":2,"resultSlot":0,"errorSlot":1},
                {"op":"return","value":0},
                {"op":"return","outcome":"failure","value":1}
            ]},
            {"frameSlots":1,"parameterSlots":[0],"instructions":[{"op":"return","value":0}]}
        ]));
        let mut host = TestHost;
        let Run::Complete(ActionOutcome::Success(value)) =
            start(&actions, 0, vec![RuntimeValue::Null; 2], &mut host).unwrap()
        else {
            panic!("expected completed success");
        };
        assert_eq!(value, RuntimeValue::Number(42.0));
    }

    #[test]
    fn deferred_finalizer_runs_after_fetch_completion() {
        let actions = actions(serde_json::json!([{
            "frameSlots":2,
            "instructions":[
                {"op":"capabilityRequest","capability":"fetch","request":{"url":1,"method":"GET","decode":"responseJson"},"successPc":1,"failurePc":1,"finallyPc":2,"resultSlot":0,"errorSlot":1},
                {"op":"return","value":0},
                {"op":"return"}
            ]
        }]));
        let mut host = TestHost;
        let Run::Suspended(suspension) =
            start(&actions, 0, vec![RuntimeValue::Null; 2], &mut host).unwrap()
        else {
            panic!("expected suspension");
        };
        let Run::Complete(ActionOutcome::Success(value)) = resume(
            &actions,
            suspension,
            Ok(RuntimeValue::String("loaded".into())),
            &mut host,
        )
        .unwrap() else {
            panic!("expected completed success");
        };
        assert_eq!(value, RuntimeValue::String("loaded".into()));
    }

    #[test]
    fn server_hosts_can_reject_cookie_capabilities_deterministically() {
        let actions = actions(serde_json::json!([{
            "frameSlots":2,
            "instructions":[
                {"op":"capabilityRequest","capability":"cookie","request":{"operation":"get","name":0},"successPc":1,"failurePc":1,"resultSlot":0,"errorSlot":1}
            ]
        }]));
        let error = start(&actions, 0, vec![RuntimeValue::Null; 2], &mut TestHost).unwrap_err();
        assert_eq!(error.0, "unsupported route loader capability: cookie");
    }

    struct RecordingHost {
        effects: Vec<String>,
    }

    impl ActionHost for RecordingHost {
        type Request = String;

        fn evaluate(
            &mut self,
            expression: usize,
            frame: &[RuntimeValue],
        ) -> Result<RuntimeValue, ActionError> {
            Ok(match expression {
                0 => frame.first().cloned().unwrap_or(RuntimeValue::Null),
                _ => RuntimeValue::Number(42.0),
            })
        }

        fn prepare_capability(
            &mut self,
            _request: &TypedCapabilityRequest,
            _frame: &[RuntimeValue],
        ) -> Result<Self::Request, ActionError> {
            Err(ActionError("unexpected capability request".into()))
        }

        fn store_state(
            &mut self,
            state: usize,
            value: RuntimeValue,
        ) -> Result<(), ActionError> {
            self.effects.push(format!("storeState:{state}:{value:?}"));
            Ok(())
        }

        fn store_ref(
            &mut self,
            reference: usize,
            value: RuntimeValue,
        ) -> Result<(), ActionError> {
            self.effects.push(format!("storeRef:{reference}:{value:?}"));
            Ok(())
        }

        fn capture_active_element(&mut self, reference: usize) -> Result<(), ActionError> {
            self.effects.push(format!("captureActiveElement:{reference}"));
            Ok(())
        }

        fn focus_host_ref(&mut self, reference: usize) -> Result<(), ActionError> {
            self.effects.push(format!("focusHostRef:{reference}"));
            Ok(())
        }

        fn focus_ref(&mut self, reference: usize) -> Result<(), ActionError> {
            self.effects.push(format!("focusRef:{reference}"));
            Ok(())
        }

        fn prevent_default(&mut self) -> Result<(), ActionError> {
            self.effects.push("preventDefault".into());
            Ok(())
        }

        fn store_host_ref(&mut self, reference: usize) -> Result<(), ActionError> {
            self.effects.push(format!("storeHostRef:{reference}"));
            Ok(())
        }

        fn call_prop(
            &mut self,
            prop: usize,
            arguments: Vec<RuntimeValue>,
            optional: bool,
        ) -> Result<(), ActionError> {
            self.effects
                .push(format!("callProp:{optional}:{prop}:{arguments:?}"));
            Ok(())
        }

        fn mutate_collection(
            &mut self,
            input: usize,
            kind: &str,
            key: RuntimeValue,
            value: Option<RuntimeValue>,
        ) -> Result<(), ActionError> {
            self.effects
                .push(format!("mutateCollection:{input}:{kind}:{key:?}:{value:?}"));
            Ok(())
        }
    }

    #[test]
    fn browser_effects_dispatch_to_host_in_order() {
        let actions = actions(serde_json::json!([{
            "frameSlots":1,
            "instructions":[
                {"op":"evaluate","expression":2},
                {"op":"storeState","state":7},
                {"op":"evaluate","expression":2},
                {"op":"storeRef","reference":8},
                {"op":"captureActiveElement","reference":9},
                {"op":"focusHostRef","reference":10},
                {"op":"focusRef","reference":11},
                {"op":"preventDefault"},
                {"op":"callProp","prop":12,"arguments":[0]},
                {"op":"callPropOptional","prop":13,"arguments":[]},
                {"op":"collectionMutation","input":14,"kind":"keyedReplace","key":0,"value":2},
                {"op":"storeHostRef","ref":15},
                {"op":"return"}
            ]
        }]));
        let mut host = RecordingHost { effects: Vec::new() };
        let Run::Complete(ActionOutcome::Success(_)) =
            start(&actions, 0, vec![RuntimeValue::Null; 1], &mut host).unwrap()
        else {
            panic!("expected completed success");
        };
        assert_eq!(
            host.effects,
            vec![
                "storeState:7:Number(42.0)",
                "storeRef:8:Number(42.0)",
                "captureActiveElement:9",
                "focusHostRef:10",
                "focusRef:11",
                "preventDefault",
                "callProp:false:12:[Null]",
                "callProp:true:13:[]",
                "mutateCollection:14:keyedReplace:Null:Some(Number(42.0))",
                "storeHostRef:15",
            ]
        );
    }

    #[test]
    fn loader_hosts_reject_browser_effects_deterministically() {
        for (instruction, name) in [
            (serde_json::json!({"op":"storeState","state":0}), "storeState"),
            (serde_json::json!({"op":"storeRef","reference":0}), "storeRef"),
            (
                serde_json::json!({"op":"captureActiveElement","reference":0}),
                "captureActiveElement",
            ),
            (
                serde_json::json!({"op":"focusHostRef","reference":0}),
                "focusHostRef",
            ),
            (serde_json::json!({"op":"focusRef","reference":0}), "focusRef"),
            (serde_json::json!({"op":"preventDefault"}), "preventDefault"),
            (
                serde_json::json!({"op":"callProp","prop":0,"arguments":[]}),
                "callProp",
            ),
            (
                serde_json::json!({"op":"callPropOptional","prop":0,"arguments":[]}),
                "callPropOptional",
            ),
            (
                serde_json::json!({"op":"collectionMutation","input":0,"kind":"append","key":0}),
                "collectionMutation",
            ),
            (serde_json::json!({"op":"storeHostRef","ref":0}), "storeHostRef"),
        ] {
            let actions = actions(serde_json::json!([{
                "frameSlots":1,
                "instructions":[
                    {"op":"evaluate","expression":0},
                    instruction,
                ]
            }]));
            let error = start(&actions, 0, vec![RuntimeValue::Null; 1], &mut TestHost)
                .unwrap_err();
            assert_eq!(error.0, format!("unsupported action instruction: {name}"));
        }
    }

    #[test]
    fn store_effects_require_stack_values() {
        for (instruction, message) in [
            (serde_json::json!({"op":"storeState","state":0}), "storeState"),
            (serde_json::json!({"op":"storeRef","reference":0}), "storeRef"),
        ] {
            let actions = actions(serde_json::json!([{
                "frameSlots":1,
                "instructions":[instruction]
            }]));
            let error = start(&actions, 0, vec![RuntimeValue::Null; 1], &mut TestHost)
                .unwrap_err();
            assert_eq!(error.0, format!("action stack underflow: {message}"));
        }
    }

    struct MissingPropHost;

    impl ActionHost for MissingPropHost {
        type Request = String;

        fn evaluate(
            &mut self,
            expression: usize,
            frame: &[RuntimeValue],
        ) -> Result<RuntimeValue, ActionError> {
            Ok(match expression {
                0 => frame.first().cloned().unwrap_or(RuntimeValue::Null),
                _ => RuntimeValue::Number(42.0),
            })
        }

        fn prepare_capability(
            &mut self,
            _request: &TypedCapabilityRequest,
            _frame: &[RuntimeValue],
        ) -> Result<Self::Request, ActionError> {
            Err(ActionError("unexpected capability request".into()))
        }

        fn call_prop(
            &mut self,
            _prop: usize,
            _arguments: Vec<RuntimeValue>,
            optional: bool,
        ) -> Result<(), ActionError> {
            if optional {
                return Ok(());
            }
            Err(ActionError("callable component prop missing".into()))
        }
    }

    #[test]
    fn required_call_prop_failure_propagates_from_host() {
        let actions = actions(serde_json::json!([{
            "frameSlots":1,
            "instructions":[
                {"op":"callProp","prop":1,"arguments":[]},
                {"op":"return"}
            ]
        }]));
        let error =
            start(&actions, 0, vec![RuntimeValue::Null; 1], &mut MissingPropHost).unwrap_err();
        assert_eq!(error.0, "callable component prop missing");
    }

    #[test]
    fn optional_call_prop_missing_falls_through_to_next_instruction() {
        let actions = actions(serde_json::json!([{
            "frameSlots":1,
            "instructions":[
                {"op":"callPropOptional","prop":1,"arguments":[]},
                {"op":"evaluate","expression":2},
                {"op":"storeFrame","slot":0},
                {"op":"return","value":0}
            ]
        }]));
        let Run::Complete(ActionOutcome::Success(value)) =
            start(&actions, 0, vec![RuntimeValue::Null; 1], &mut MissingPropHost).unwrap()
        else {
            panic!("expected completed success");
        };
        assert_eq!(value, RuntimeValue::Number(42.0));
    }

    struct CookieHost {
        effects: Vec<String>,
    }

    impl ActionHost for CookieHost {
        type Request = String;

        fn evaluate(
            &mut self,
            expression: usize,
            frame: &[RuntimeValue],
        ) -> Result<RuntimeValue, ActionError> {
            Ok(frame
                .get(expression)
                .cloned()
                .unwrap_or(RuntimeValue::Number(42.0)))
        }

        fn prepare_capability(
            &mut self,
            request: &TypedCapabilityRequest,
            frame: &[RuntimeValue],
        ) -> Result<Self::Request, ActionError> {
            match request {
                TypedCapabilityRequest::Cookie(request) => {
                    let value = request
                        .value
                        .map(|expression| self.evaluate(expression, frame))
                        .transpose()?;
                    self.effects.push(format!(
                        "prepareCookie:{}:{}",
                        request.operation,
                        value.is_some()
                    ));
                    Ok("cookie".into())
                }
                TypedCapabilityRequest::Fetch(_) => Ok("fetch".into()),
            }
        }

        fn prevent_default(&mut self) -> Result<(), ActionError> {
            self.effects.push("preventDefault".into());
            Ok(())
        }
    }

    #[test]
    fn cookie_capability_suspends_and_resumes_success() {
        let actions = actions(serde_json::json!([{
            "frameSlots":2,
            "instructions":[
                {"op":"capabilityRequest","capability":"cookie","request":{"operation":"set","name":0,"value":2},"successPc":1,"failurePc":2,"resultSlot":0,"errorSlot":1},
                {"op":"return","value":0},
                {"op":"return","outcome":"failure","value":1}
            ]
        }]));
        let mut host = CookieHost { effects: Vec::new() };
        let Run::Suspended(suspension) =
            start(&actions, 0, vec![RuntimeValue::Null; 2], &mut host).unwrap()
        else {
            panic!("expected suspension");
        };
        assert_eq!(suspension.request, "cookie");
        assert_eq!(host.effects, vec!["prepareCookie:set:true"]);
        let Run::Complete(ActionOutcome::Success(value)) = resume(
            &actions,
            suspension,
            Ok(RuntimeValue::String("cookie-value".into())),
            &mut host,
        )
        .unwrap() else {
            panic!("expected completed success");
        };
        assert_eq!(value, RuntimeValue::String("cookie-value".into()));
    }

    #[test]
    fn cookie_capability_resume_failure_writes_error_slot() {
        let actions = actions(serde_json::json!([{
            "frameSlots":2,
            "instructions":[
                {"op":"capabilityRequest","capability":"cookie","request":{"operation":"set","name":0,"value":2},"successPc":1,"failurePc":2,"resultSlot":0,"errorSlot":1},
                {"op":"return","value":0},
                {"op":"return","outcome":"failure","value":1}
            ]
        }]));
        let mut host = CookieHost { effects: Vec::new() };
        let Run::Suspended(suspension) =
            start(&actions, 0, vec![RuntimeValue::Null; 2], &mut host).unwrap()
        else {
            panic!("expected suspension");
        };
        let Run::Complete(ActionOutcome::Failure(value)) = resume(
            &actions,
            suspension,
            Err(RuntimeValue::String("denied".into())),
            &mut host,
        )
        .unwrap() else {
            panic!("expected completed failure");
        };
        assert_eq!(value, RuntimeValue::String("denied".into()));
    }

    #[test]
    fn cookie_finalizer_runs_after_terminal_resume() {
        let actions = actions(serde_json::json!([{
            "frameSlots":2,
            "instructions":[
                {"op":"capabilityRequest","capability":"cookie","request":{"operation":"get","name":0},"successPc":1,"failurePc":1,"finallyPc":3,"resultSlot":0,"errorSlot":1},
                {"op":"return","value":0},
                {"op":"return"},
                {"op":"preventDefault"},
                {"op":"return"}
            ]
        }]));
        let mut host = CookieHost { effects: Vec::new() };
        let Run::Suspended(suspension) =
            start(&actions, 0, vec![RuntimeValue::Null; 2], &mut host).unwrap()
        else {
            panic!("expected suspension");
        };
        let Run::Complete(ActionOutcome::Success(value)) = resume(
            &actions,
            suspension,
            Ok(RuntimeValue::String("loaded".into())),
            &mut host,
        )
        .unwrap() else {
            panic!("expected completed success");
        };
        assert_eq!(value, RuntimeValue::String("loaded".into()));
        assert_eq!(host.effects, vec!["prepareCookie:get:false", "preventDefault"]);
    }
}
