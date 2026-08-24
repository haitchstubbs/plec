use std::collections::HashMap;

use plec_hir::{BindingId, HirBindingKind, HirCallable, HirCallableBody, HirExpr, HirStmt};
use plec_ir::{
    ActionInstruction, ActionProgram, CapabilityRequest, ExpressionInstruction, ExpressionProgram,
};

use crate::{Ctx, LoweringError};

impl Ctx<'_> {
    pub(crate) fn callable(&mut self, callable: &HirCallable) -> Result<usize, LoweringError> {
        match callable {
            HirCallable::Reference { binding } => {
                if let Some(prop) = self.callback_props.get(binding).copied() {
                    let action = self.app.actions.len();
                    self.app.actions.push(ActionProgram {
                        frame_slots: 0,
                        parameter_slots: vec![],
                        loader_result_state: None,
                        route_loader: false,
                        instructions: vec![
                            ActionInstruction::CallProp {
                                prop,
                                arguments: vec![],
                            },
                            ActionInstruction::Return {
                                outcome: plec_ir::ReturnOutcome::Success,
                                value: None,
                            },
                        ],
                    });
                    Ok(action)
                } else {
                    self.callables.get(binding).copied().ok_or_else(|| {
                        self.err("event callable is not a zero-parameter local action")
                    })
                }
            }
            HirCallable::Inline { parameters, body } => {
                self.action(body, parameters, self.active_loop.is_some())
            }
            _ => Err(self.err("callable is not executable in the first IR slice")),
        }
    }
    pub(crate) fn action(
        &mut self,
        body: &HirCallableBody,
        parameters: &[BindingId],
        row: bool,
    ) -> Result<usize, LoweringError> {
        let previous = std::mem::replace(
            &mut self.action_parameters,
            parameters
                .iter()
                .enumerate()
                .map(|(slot, binding)| (*binding, slot))
                .collect(),
        );
        let mut frame_slots = parameters.len();
        let previous_async_slots = std::mem::replace(
            &mut self.async_slots,
            reserve_async_slots(body, &mut frame_slots),
        );
        let mut code = vec![];
        match body {
            HirCallableBody::Block(stmts) => {
                self.statements(stmts, &mut code, row, &mut frame_slots)?
            }
            HirCallableBody::Expression(expr) => {
                if let Ok((state, value)) = self.setter_call(*expr) {
                    let (expression, _) = self.expression(value, row)?;
                    code.push(ActionInstruction::Evaluate { expression });
                    code.push(ActionInstruction::StoreState { state });
                } else if let Some(mutation) = self.collection_mutation_call(*expr, row)? {
                    code.push(mutation);
                } else if let Some(call) = self.local_action_call(*expr, row)? {
                    code.push(call);
                } else {
                    let (prop, arguments) = self.callback_call(*expr, row)?;
                    code.push(ActionInstruction::CallProp { prop, arguments });
                }
            }
        }
        code.push(ActionInstruction::Return {
            outcome: plec_ir::ReturnOutcome::Success,
            value: None,
        });
        let mut failures = vec![];
        for (pc, instruction) in code.iter_mut().enumerate() {
            if let ActionInstruction::CapabilityRequest {
                success_pc,
                failure_pc,
                error_slot,
                ..
            } = instruction
            {
                if *success_pc == usize::MAX {
                    *success_pc = pc + 1;
                }
                if *failure_pc == usize::MAX {
                    failures.push((pc, *error_slot, false));
                }
            }
            if let ActionInstruction::Call {
                action,
                success_pc,
                failure_pc,
                result_slot,
                error_slot,
                ..
            } = instruction
            {
                if self.action_may_suspend(*action) && result_slot.is_none() {
                    *success_pc = Some(pc + 1);
                    *failure_pc = Some(usize::MAX);
                    *result_slot = Some(frame_slots);
                    *error_slot = Some(frame_slots + 1);
                    frame_slots += 2;
                }
                if failure_pc == &Some(usize::MAX) {
                    failures.push((
                        pc,
                        error_slot.expect("suspending calls have error slots"),
                        true,
                    ));
                }
            }
        }
        for (pc, error_slot, is_call) in failures {
            let expression = self.app.expressions.len();
            self.app.expressions.push(ExpressionProgram {
                instructions: vec![
                    ExpressionInstruction::LoadFrame { slot: error_slot },
                    ExpressionInstruction::Return,
                ],
            });
            let failure_pc = code.len();
            code.push(ActionInstruction::Return {
                outcome: plec_ir::ReturnOutcome::Failure,
                value: Some(expression),
            });
            match &mut code[pc] {
                ActionInstruction::CapabilityRequest {
                    failure_pc: target, ..
                } => *target = failure_pc,
                ActionInstruction::Call {
                    failure_pc: target, ..
                } if is_call => *target = Some(failure_pc),
                _ => unreachable!("async failure source changed during lowering"),
            }
        }
        self.action_parameters = previous;
        self.async_slots = previous_async_slots;
        let index = self.app.actions.len();
        self.app.actions.push(ActionProgram {
            frame_slots,
            parameter_slots: (0..parameters.len()).collect(),
            loader_result_state: None,
            route_loader: false,
            instructions: code,
        });
        Ok(index)
    }
    fn statements(
        &mut self,
        stmts: &[HirStmt],
        code: &mut Vec<ActionInstruction>,
        row: bool,
        frame_slots: &mut usize,
    ) -> Result<(), LoweringError> {
        for stmt in stmts {
            match stmt {
                HirStmt::AwaitFetch {
                    target,
                    url,
                    method,
                    decode,
                    ..
                } => {
                    let (url, _) = self.expression(*url, row)?;
                    let result_slot = target
                        .and_then(|binding| self.async_slots.get(&binding).copied())
                        .unwrap_or_else(|| {
                            let slot = *frame_slots;
                            *frame_slots += 1;
                            slot
                        });
                    let error_slot = {
                        let slot = *frame_slots;
                        *frame_slots += 1;
                        slot
                    };
                    code.push(ActionInstruction::CapabilityRequest {
                        request: CapabilityRequest::Fetch {
                            url,
                            method: fetch_method(method)?,
                            decode: fetch_decode(decode)?,
                            require_ok: true,
                        },
                        success_pc: usize::MAX,
                        failure_pc: usize::MAX,
                        finally_pc: None,
                        result_slot,
                        error_slot,
                    });
                }
                HirStmt::AwaitCookie { target, operation, name, value, path, same_site, secure, max_age, .. } => {
                    let operation = match operation.as_str() {
                        "get" => "get", "set" => "set", "delete" => "delete",
                        _ => return Err(self.err("unsupported cookie operation")),
                    };
                    self.cookie_capability(operation, name, path, same_site.as_deref(), *secure, *max_age)?;
                    let value = value.map(|value| self.expression(value, row).map(|(expression, _)| expression)).transpose()?;
                    let result_slot = target.and_then(|binding| self.async_slots.get(&binding).copied()).unwrap_or_else(|| {
                        let slot = *frame_slots; *frame_slots += 1; slot
                    });
                    let error_slot = { let slot = *frame_slots; *frame_slots += 1; slot };
                    code.push(ActionInstruction::CapabilityRequest {
                        request: CapabilityRequest::Cookie {
                            operation, name: self.string(name), value, path: path.clone(),
                            same_site: same_site.clone(), secure: *secure,
                            expiry: if max_age.is_some() { "maxAge" } else { "session" }, max_age: *max_age,
                        },
                        success_pc: usize::MAX, failure_pc: usize::MAX, finally_pc: None, result_slot, error_slot,
                    });
                }
                HirStmt::RefUpdate { reference, value, .. } => {
                    let reference = *self.refs.get(reference).ok_or_else(|| self.err("ref used before lowering"))?;
                    let (expression, _) = self.expression(*value, row)?;
                    code.push(ActionInstruction::Evaluate { expression });
                    code.push(ActionInstruction::StoreRef { reference });
                }
                HirStmt::AwaitCall {
                    target,
                    callee,
                    arguments,
                    ..
                } => {
                    let action = *self
                        .callables
                        .get(callee)
                        .ok_or_else(|| self.err("awaited action is not lowered"))?;
                    let arguments = arguments
                        .iter()
                        .map(|argument| self.expression(*argument, row).map(|(value, _)| value))
                        .collect::<Result<Vec<_>, _>>()?;
                    let result_slot = target
                        .and_then(|binding| self.async_slots.get(&binding).copied())
                        .unwrap_or_else(|| {
                            let slot = *frame_slots;
                            *frame_slots += 1;
                            slot
                        });
                    let error_slot = {
                        let slot = *frame_slots;
                        *frame_slots += 1;
                        slot
                    };
                    code.push(ActionInstruction::Call {
                        action,
                        arguments,
                        success_pc: Some(code.len() + 1),
                        failure_pc: Some(usize::MAX),
                        result_slot: Some(result_slot),
                        error_slot: Some(error_slot),
                    });
                }
                HirStmt::StateUpdate { state, value, .. } => {
                    let state = *self
                        .states
                        .get(state)
                        .ok_or_else(|| self.err("state update target missing"))?;
                    let (expression, _) = self.expression(*value, row)?;
                    code.push(ActionInstruction::Evaluate { expression });
                    code.push(ActionInstruction::StoreState { state });
                }
                HirStmt::If {
                    test,
                    consequent,
                    alternate,
                    ..
                } => {
                    let (expression, _) = self.expression(*test, row)?;
                    code.push(ActionInstruction::Evaluate { expression });
                    let jump_false = code.len();
                    code.push(ActionInstruction::JumpIfFalse { target: 0 });
                    self.statements(consequent, code, row, frame_slots)?;
                    let jump_end = code.len();
                    code.push(ActionInstruction::Jump { target: 0 });
                    let alternate_start = code.len();
                    self.statements(alternate, code, row, frame_slots)?;
                    let end = code.len();
                    code[jump_false] = ActionInstruction::JumpIfFalse {
                        target: alternate_start,
                    };
                    code[jump_end] = ActionInstruction::Jump { target: end };
                }
                HirStmt::Return { value, .. } => {
                    let value = value
                        .map(|value| self.expression(value, row))
                        .transpose()?
                        .map(|(value, _)| value);
                    code.push(ActionInstruction::Return {
                        outcome: plec_ir::ReturnOutcome::Success,
                        value,
                    });
                }
                HirStmt::Expression { expression, .. } => {
                    if let Some(mutation) = self.collection_mutation_call(*expression, row)? {
                        code.push(mutation);
                    } else if let Some(call) = self.local_action_call(*expression, row)? {
                        code.push(call);
                    } else {
                        let (prop, arguments) = self.callback_call(*expression, row)?;
                        code.push(ActionInstruction::CallProp { prop, arguments });
                    }
                }
                HirStmt::Try {
                    body,
                    catch,
                    finally,
                    ..
                } => {
                    let start = code.len();
                    self.statements(body, code, row, frame_slots)?;
                    let jump_after_body = code.len();
                    code.push(ActionInstruction::Jump { target: 0 });
                    let catch_start = code.len();
                    if let Some((binding, statements)) = catch {
                        let error_slot = self
                            .async_slots
                            .get(binding)
                            .copied()
                            .ok_or_else(|| self.err("catch binding frame slot missing"))?;
                        for instruction in &mut code[start..jump_after_body] {
                            match instruction {
                                ActionInstruction::CapabilityRequest {
                                    failure_pc,
                                    error_slot: slot,
                                    ..
                                } => {
                                    *failure_pc = catch_start;
                                    *slot = error_slot;
                                }
                                ActionInstruction::Call {
                                    failure_pc,
                                    error_slot: slot,
                                    ..
                                } => {
                                    *failure_pc = Some(catch_start);
                                    *slot = Some(error_slot);
                                }
                                _ => {}
                            }
                        }
                        self.statements(statements, code, row, frame_slots)?;
                    }
                    let jump_after_catch = code.len();
                    code.push(ActionInstruction::Jump { target: 0 });
                    let finally_start = code.len();
                    if !finally.is_empty() {
                        self.statements(finally, code, row, frame_slots)?;
                    }
                    let end = code.len();
                    code[jump_after_body] = ActionInstruction::Jump { target: end };
                    code[jump_after_catch] = ActionInstruction::Jump { target: end };
                    if !finally.is_empty() {
                        for instruction in &mut code[start..jump_after_body] {
                            if let ActionInstruction::CapabilityRequest { finally_pc, .. } =
                                instruction
                            {
                                *finally_pc = Some(finally_start);
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
    fn setter_call(
        &self,
        id: plec_hir::ExprId,
    ) -> Result<(usize, plec_hir::ExprId), LoweringError> {
        let HirExpr::Call { callee, args } = self.expr(id)? else {
            return Err(self.err("inline action must be a direct state setter call"));
        };
        if args.len() != 1 {
            return Err(self.err("state setter requires one value"));
        }
        let HirExpr::Binding(binding) = self.expr(*callee)? else {
            return Err(self.err("state setter must be a direct binding"));
        };
        let HirBindingKind::StateSetter { state } =
            self.component.bindings[binding.0 as usize].kind
        else {
            return Err(self.err("inline action is not a state setter"));
        };
        Ok((
            *self
                .states
                .get(&state)
                .ok_or_else(|| self.err("state setter target missing"))?,
            args[0],
        ))
    }

    fn local_action_call(
        &mut self,
        id: plec_hir::ExprId,
        row: bool,
    ) -> Result<Option<ActionInstruction>, LoweringError> {
        let HirExpr::Call { callee, args } = self.expr(id)?.clone() else {
            return Ok(None);
        };
        let HirExpr::Binding(binding) = self.expr(callee)? else {
            return Ok(None);
        };
        let Some(action) = self.callables.get(&binding).copied() else {
            return Ok(None);
        };
        let arguments = args
            .into_iter()
            .map(|argument| {
                self.expression(argument, row)
                    .map(|(expression, _)| expression)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(ActionInstruction::Call {
            action,
            arguments,
            success_pc: None,
            failure_pc: None,
            result_slot: None,
            error_slot: None,
        }))
    }

    fn action_may_suspend(&self, action: usize) -> bool {
        let Some((binding, _)) = self
            .callables
            .iter()
            .find(|(_, candidate)| **candidate == action)
        else {
            return false;
        };
        self.component
            .callables
            .iter()
            .find(|callable| callable.binding == *binding)
            .is_some_and(|callable| callable_body_may_suspend(&callable.body))
    }

    fn collection_mutation_call(
        &mut self,
        id: plec_hir::ExprId,
        row: bool,
    ) -> Result<Option<ActionInstruction>, LoweringError> {
        let (callee, args) = match self.expr(id)?.clone() {
            HirExpr::Call { callee, args } => (callee, args),
            _ => return Ok(None),
        };
        let (object, kind) = match self.expr(callee)?.clone() {
            HirExpr::Member { object, property } => (object, property),
            _ => return Ok(None),
        };
        let HirExpr::Binding(binding) = self.expr(object)? else {
            return Ok(None);
        };
        let Some(input) = self.inputs.get(binding).copied() else {
            return Ok(None);
        };
        let (key, value) = match (kind.as_str(), args.as_slice()) {
            ("append" | "keyedReplace", [key, value]) => (*key, Some(*value)),
            ("keyedRemove", [key]) => (*key, None),
            _ => return Err(self.err("collection mutation arguments are invalid")),
        };
        let key = self.expression(key, row)?.0;
        let value = value
            .map(|value| {
                self.expression(value, row)
                    .map(|(expression, _)| expression)
            })
            .transpose()?;
        Ok(Some(ActionInstruction::CollectionMutation {
            input,
            kind: match kind.as_str() {
                "append" => "append",
                "keyedReplace" => "keyedReplace",
                "keyedRemove" => "keyedRemove",
                _ => unreachable!(),
            },
            key,
            value,
        }))
    }

    fn callback_call(
        &mut self,
        id: plec_hir::ExprId,
        row: bool,
    ) -> Result<(usize, Vec<usize>), LoweringError> {
        let (callee, args) = match self.expr(id)?.clone() {
            HirExpr::Call { callee, args } => (callee, args),
            _ => return Err(self.err("callable action must be a direct component prop call")),
        };
        let HirExpr::Binding(binding) = self.expr(callee)? else {
            return Err(self.err("callable component prop must be a direct binding"));
        };
        let prop = self
            .callback_props
            .get(binding)
            .copied()
            .ok_or_else(|| self.err("action is not a callable component prop"))?;
        let arguments = args
            .iter()
            .map(|argument| {
                self.expression(*argument, row)
                    .map(|(expression, _)| expression)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok((prop, arguments))
    }
}

fn callable_body_may_suspend(body: &HirCallableBody) -> bool {
    match body {
        HirCallableBody::Expression(_) => false,
        HirCallableBody::Block(statements) => statements.iter().any(statement_may_suspend),
    }
}

fn reserve_async_slots(body: &HirCallableBody, next: &mut usize) -> HashMap<BindingId, usize> {
    let mut slots = HashMap::new();
    let HirCallableBody::Block(statements) = body else {
        return slots;
    };
    fn visit(statements: &[HirStmt], slots: &mut HashMap<BindingId, usize>, next: &mut usize) {
        for statement in statements {
            match statement {
                HirStmt::AwaitFetch {
                    target: Some(binding),
                    ..
                }
                | HirStmt::AwaitCall {
                    target: Some(binding),
                    ..
                }
                | HirStmt::AwaitCookie {
                    target: Some(binding),
                    ..
                } => {
                    slots.entry(*binding).or_insert_with(|| {
                        let slot = *next;
                        *next += 1;
                        slot
                    });
                }
                HirStmt::If {
                    consequent,
                    alternate,
                    ..
                } => {
                    visit(consequent, slots, next);
                    visit(alternate, slots, next);
                }
                HirStmt::Try {
                    body,
                    catch,
                    finally,
                    ..
                } => {
                    visit(body, slots, next);
                    if let Some((binding, statements)) = catch {
                        slots.entry(*binding).or_insert_with(|| {
                            let slot = *next;
                            *next += 1;
                            slot
                        });
                        visit(statements, slots, next);
                    }
                    visit(finally, slots, next);
                }
                _ => {}
            }
        }
    }
    visit(statements, &mut slots, next);
    slots
}

fn fetch_method(value: &str) -> Result<&'static str, LoweringError> {
    match value {
        "GET" => Ok("GET"),
        "POST" => Ok("POST"),
        "PUT" => Ok("PUT"),
        "PATCH" => Ok("PATCH"),
        "DELETE" => Ok("DELETE"),
        _ => Err(LoweringError("unsupported fetch method".into())),
    }
}

fn fetch_decode(value: &str) -> Result<&'static str, LoweringError> {
    match value {
        "json" => Ok("json"),
        "text" => Ok("text"),
        "empty" => Ok("empty"),
        _ => Err(LoweringError("unsupported fetch decoder".into())),
    }
}

fn statement_may_suspend(statement: &HirStmt) -> bool {
    match statement {
        HirStmt::AwaitFetch { .. } => true,
        HirStmt::AwaitCookie { .. } => true,
        HirStmt::AwaitCall { .. } => true,
        HirStmt::If {
            consequent,
            alternate,
            ..
        } => {
            consequent.iter().any(statement_may_suspend)
                || alternate.iter().any(statement_may_suspend)
        }
        HirStmt::Try {
            body,
            catch,
            finally,
            ..
        } => {
            body.iter().any(statement_may_suspend)
                || catch
                    .as_ref()
                    .is_some_and(|(_, body)| body.iter().any(statement_may_suspend))
                || finally.iter().any(statement_may_suspend)
        }
        _ => false,
    }
}
