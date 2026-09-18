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
                _ => return Err(ActionError::unsupported(instruction_name(&instruction))),
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

fn instruction_name(instruction: &TypedActionInstruction) -> &'static str {
    match instruction {
        TypedActionInstruction::StoreState { .. } => "storeState",
        TypedActionInstruction::StoreRef { .. } => "storeRef",
        TypedActionInstruction::CaptureActiveElement { .. } => "captureActiveElement",
        TypedActionInstruction::FocusHostRef { .. } => "focusHostRef",
        TypedActionInstruction::FocusRef { .. } => "focusRef",
        TypedActionInstruction::PreventDefault => "preventDefault",
        TypedActionInstruction::CallProp { .. } => "callProp",
        TypedActionInstruction::CallPropOptional { .. } => "callPropOptional",
        TypedActionInstruction::CollectionMutation { .. } => "collectionMutation",
        TypedActionInstruction::StoreHostRef { .. } => "storeHostRef",
        _ => "unsupported",
    }
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
}
