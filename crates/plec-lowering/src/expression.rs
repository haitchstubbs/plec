use std::collections::BTreeSet;

use plec_hir::{HirBindingKind, HirExpr, HirLogicalOp, HirObjectItem, HirUnaryOp, HirValue};
use plec_ir::{ExpressionInstruction, ExpressionProgram, Value};

use crate::{Ctx, LoweringError};

impl Ctx<'_> {
    /// Turns statically named call-site props into the single record consumed
    /// by a library component declared as `(props) => <svg {...props} />`.
    pub(crate) fn component_props_record(
        &mut self,
        props: &[plec_hir::HirProp],
        row: bool,
    ) -> Result<(usize, BTreeSet<usize>), LoweringError> {
        let mut code = Vec::new();
        let mut deps = BTreeSet::new();
        let mut fields = Vec::new();
        for prop in props {
            match prop {
                plec_hir::HirProp::Static { name, value } => {
                    fields.push(self.string(name));
                    code.push(ExpressionInstruction::Constant {
                        constant: self.constant(Value::String(value.clone())),
                    });
                }
                plec_hir::HirProp::Expression { name, value } => {
                    fields.push(self.string(name));
                    self.emit(*value, row, &mut code, &mut deps)?;
                }
                plec_hir::HirProp::Spread { value } => {
                    fields.push(self.string(""));
                    self.emit(*value, row, &mut code, &mut deps)?;
                }
                plec_hir::HirProp::Callable { .. } | plec_hir::HirProp::Component { .. } => {
                    return Err(self.err("direct component props must be static value props"));
                }
            }
        }
        let spreads = props
            .iter()
            .map(|prop| matches!(prop, plec_hir::HirProp::Spread { .. }))
            .collect::<Vec<_>>();
        code.push(ExpressionInstruction::MakeRecord {
            fields,
            spreads: spreads
                .iter()
                .any(|spread| *spread)
                .then_some(spreads)
                .unwrap_or_default(),
        });
        code.push(ExpressionInstruction::Return);
        let index = self.app.expressions.len();
        self.app
            .expressions
            .push(ExpressionProgram { instructions: code });
        Ok((index, deps))
    }

    pub(crate) fn expression(
        &mut self,
        id: plec_hir::ExprId,
        row: bool,
    ) -> Result<(usize, BTreeSet<usize>), LoweringError> {
        let mut code = vec![];
        let mut deps = BTreeSet::new();
        self.emit(id, row, &mut code, &mut deps)?;
        code.push(ExpressionInstruction::Return);
        let index = self.app.expressions.len();
        self.app
            .expressions
            .push(ExpressionProgram { instructions: code });
        Ok((index, deps))
    }
    fn emit(
        &mut self,
        id: plec_hir::ExprId,
        row: bool,
        code: &mut Vec<ExpressionInstruction>,
        deps: &mut BTreeSet<usize>,
    ) -> Result<(), LoweringError> {
        match self.expr(id)?.clone() {
            HirExpr::Literal(v) => {
                let v = match v {
                    HirValue::Null => Value::Null,
                    HirValue::Bool(v) => Value::Bool(v),
                    HirValue::Number(v) => Value::Number(v),
                    HirValue::String(v) => Value::String(v),
                };
                code.push(ExpressionInstruction::Constant {
                    constant: self.constant(v),
                });
            }
            HirExpr::Host { kind, name } => {
                let host = match kind.as_str() {
                    "cookie" => self.host("cookie", name.as_deref())?,
                    _ => return Err(self.err("host value is not executable")),
                };
                code.push(ExpressionInstruction::LoadHost { host });
            }
            HirExpr::Binding(binding) => match self
                .component
                .bindings
                .get(binding.0 as usize)
                .map(|b| &b.kind)
            {
                Some(HirBindingKind::StateValue) => {
                    let state = *self
                        .states
                        .get(&binding)
                        .ok_or_else(|| self.err("state used before lowering"))?;
                    deps.insert(state);
                    code.push(ExpressionInstruction::LoadState { state });
                }
                Some(HirBindingKind::MutationPending { mutation })
                | Some(HirBindingKind::MutationError { mutation })
                | Some(HirBindingKind::MutationData { mutation }) => {
                    let state = *self.mutation_states.get(&binding).ok_or_else(|| {
                        self.err(&format!(
                            "mutation field '{}' used before lowering",
                            self.component.bindings[binding.0 as usize].name
                        ))
                    })?;
                    deps.insert(state);
                    code.push(ExpressionInstruction::LoadState { state });
                    let _ = mutation;
                }
                Some(HirBindingKind::MutationRun { mutation }) => {
                    let action = *self
                        .callables
                        .get(&binding)
                        .ok_or_else(|| self.err("mutation action used before lowering"))?;
                    code.push(ExpressionInstruction::Constant {
                        constant: self.constant(plec_ir::Value::Number(action as f64)),
                    });
                    let _ = mutation;
                }
                Some(HirBindingKind::RouteReload) => {
                    let action = *self
                        .callables
                        .get(&binding)
                        .ok_or_else(|| self.err("route reload used before lowering"))?;
                    code.push(ExpressionInstruction::Constant {
                        constant: self.constant(plec_ir::Value::Number(action as f64)),
                    });
                }
                Some(HirBindingKind::Input { kind }) if kind == "location" => {
                    let host = *self
                        .hosts
                        .get(&binding)
                        .ok_or_else(|| self.err("location used before lowering"))?;
                    code.push(ExpressionInstruction::LoadHost { host });
                }
                Some(HirBindingKind::Input { kind }) if kind == "loaderData" => {
                    let host = *self
                        .hosts
                        .get(&binding)
                        .ok_or_else(|| self.err("loader data used before lowering"))?;
                    code.push(ExpressionInstruction::LoadHost { host });
                }
                Some(HirBindingKind::Parameter { callable: false })
                    if self.action_parameters.contains_key(&binding) =>
                {
                    code.push(ExpressionInstruction::LoadFrame {
                        slot: self.action_parameters[&binding],
                    });
                }
                Some(HirBindingKind::Parameter { callable: true })
                    if self.action_parameters.contains_key(&binding) =>
                {
                    code.push(ExpressionInstruction::LoadFrame {
                        slot: self.action_parameters[&binding],
                    });
                }
                Some(HirBindingKind::Callable) => {
                    let action = *self
                        .callables
                        .get(&binding)
                        .ok_or_else(|| self.err("callable used before lowering"))?;
                    code.push(ExpressionInstruction::Constant {
                        constant: self.constant(plec_ir::Value::Number(action as f64)),
                    });
                }
                Some(HirBindingKind::AsyncValue) => {
                    let slot = *self.async_slots.get(&binding).ok_or_else(|| {
                        self.err(&format!(
                            "async value '{}' used outside its action",
                            self.component.bindings[binding.0 as usize].name,
                        ))
                    })?;
                    code.push(ExpressionInstruction::LoadFrame { slot });
                }
                Some(HirBindingKind::Parameter { callable: false }) => {
                    let prop = *self.props.get(&binding).ok_or_else(|| {
                        self.err(&format!(
                            "component prop '{}' used before lowering",
                            self.component.bindings[binding.0 as usize].name,
                        ))
                    })?;
                    code.push(ExpressionInstruction::LoadProp { prop });
                }
                Some(HirBindingKind::Local) => {
                    if self.active_locals.contains(&binding) {
                        return Err(self.err("cyclic local initializer"));
                    }
                    let local = *self
                        .locals
                        .get(&binding)
                        .ok_or_else(|| self.err("local initializer missing"))?;
                    self.active_locals.push(binding);
                    self.emit(local, row, code, deps)?;
                    self.active_locals.pop();
                }
                Some(HirBindingKind::LoopItem) if row => {
                    code.push(ExpressionInstruction::LoadRowRecord);
                }
                _ => return Err(self.err("binding is not executable in this expression")),
            },
            HirExpr::RefCurrent { reference } => {
                let reference = *self
                    .refs
                    .get(&reference)
                    .ok_or_else(|| self.err("ref used before lowering"))?;
                code.push(ExpressionInstruction::LoadRef { reference });
            }
            HirExpr::HostRefCurrent { .. } => {
                return Err(self.err("hostRef.current requires a host capability primitive"))
            }
            HirExpr::Member { object, property } => {
                if matches!(self.expr(object)?, HirExpr::Binding(binding) if matches!(self.component.bindings[binding.0 as usize].kind, HirBindingKind::LoopItem))
                {
                    if !row {
                        return Err(self.err("row field used outside a loop"));
                    }
                    code.push(ExpressionInstruction::LoadRowField {
                        field: self.string(&property),
                    });
                } else {
                    self.emit(object, row, code, deps)?;
                    code.push(ExpressionInstruction::Field {
                        field: self.string(&property),
                    });
                }
            }
            HirExpr::ComputedMember {
                object, property, ..
            } => {
                if matches!(self.expr(object)?, HirExpr::Binding(binding) if matches!(self.component.bindings[binding.0 as usize].kind, HirBindingKind::LoopItem))
                {
                    if !row {
                        return Err(self.err("row item used outside a loop"));
                    }
                    code.push(ExpressionInstruction::LoadRowRecord);
                } else {
                    self.emit(object, row, code, deps)?;
                }
                self.emit(property, row, code, deps)?;
                code.push(ExpressionInstruction::Index);
            }
            HirExpr::Unary { op, argument } => {
                self.emit(argument, row, code, deps)?;
                code.push(ExpressionInstruction::Unary {
                    kind: match op {
                        HirUnaryOp::Not => "not",
                        HirUnaryOp::Plus => "plus",
                        HirUnaryOp::Minus => "minus",
                    },
                });
            }
            HirExpr::Binary { op, left, right } => {
                self.emit(left, row, code, deps)?;
                self.emit(right, row, code, deps)?;
                code.push(ExpressionInstruction::Binary {
                    kind: match op {
                        plec_hir::HirBinaryOp::Add => "add",
                        plec_hir::HirBinaryOp::Subtract => "subtract",
                        plec_hir::HirBinaryOp::Multiply => "multiply",
                        plec_hir::HirBinaryOp::Divide => "divide",
                        plec_hir::HirBinaryOp::Equal | plec_hir::HirBinaryOp::StrictEqual => {
                            "equal"
                        }
                        plec_hir::HirBinaryOp::NotEqual | plec_hir::HirBinaryOp::StrictNotEqual => {
                            "notEqual"
                        }
                        plec_hir::HirBinaryOp::Greater => "greater",
                        plec_hir::HirBinaryOp::GreaterEqual => "greaterEqual",
                        plec_hir::HirBinaryOp::Less => "less",
                        plec_hir::HirBinaryOp::LessEqual => "lessEqual",
                        plec_hir::HirBinaryOp::InstanceOf => "instanceofError",
                    },
                });
            }
            HirExpr::Logical { op, left, right } => {
                self.emit(left, row, code, deps)?;
                self.emit(right, row, code, deps)?;
                code.push(ExpressionInstruction::Binary {
                    kind: match op {
                        HirLogicalOp::And => "and",
                        HirLogicalOp::Or => "or",
                        HirLogicalOp::Coalesce => "coalesce",
                    },
                });
            }
            HirExpr::Conditional {
                test,
                consequent,
                alternate,
            } => {
                self.emit(test, row, code, deps)?;
                let jump_false = code.len();
                code.push(ExpressionInstruction::JumpIfFalse { target: usize::MAX });
                self.emit(consequent, row, code, deps)?;
                let jump_end = code.len();
                code.push(ExpressionInstruction::Jump { target: usize::MAX });
                let alternate_start = code.len();
                self.emit(alternate, row, code, deps)?;
                let end = code.len();
                code[jump_false] = ExpressionInstruction::JumpIfFalse {
                    target: alternate_start,
                };
                code[jump_end] = ExpressionInstruction::Jump { target: end };
            }
            HirExpr::Template { parts } => {
                let count = parts.len();
                for part in parts {
                    match part {
                        plec_hir::HirTemplatePart::String(v) => {
                            code.push(ExpressionInstruction::Constant {
                                constant: self.constant(Value::String(v)),
                            })
                        }
                        plec_hir::HirTemplatePart::Expression(v) => {
                            self.emit(v, row, code, deps)?
                        }
                    }
                }
                code.push(ExpressionInstruction::String {
                    kind: "concat",
                    count,
                });
            }
            HirExpr::Array(items) => {
                let count = items.len();
                let mut spreads = Vec::with_capacity(count);
                for item in items {
                    let (item, spread) = match item {
                        plec_hir::HirArrayItem::Value(item) => (item, false),
                        plec_hir::HirArrayItem::Spread(item) => (item, true),
                    };
                    self.emit(item, row, code, deps)?;
                    spreads.push(spread);
                }
                code.push(ExpressionInstruction::MakeArray { count, spreads });
            }
            HirExpr::Object(fields) => {
                let mut names = vec![];
                let mut spreads = vec![];
                for field in fields {
                    match field {
                        HirObjectItem::Property { name, value } => {
                            names.push(self.string(&name));
                            spreads.push(false);
                            self.emit(value, row, code, deps)?;
                        }
                        HirObjectItem::Spread(value) => {
                            // The name is ignored for spreads but keeps instruction and
                            // stack arity deterministic.
                            names.push(self.string(""));
                            spreads.push(true);
                            self.emit(value, row, code, deps)?;
                        }
                    }
                }
                let spreads = spreads
                    .iter()
                    .any(|spread| *spread)
                    .then_some(spreads)
                    .unwrap_or_default();
                code.push(ExpressionInstruction::MakeRecord {
                    fields: names,
                    spreads,
                });
            }
            HirExpr::ObjectWithout { object, excluded } => {
                self.emit(object, row, code, deps)?;
                code.push(ExpressionInstruction::OmitFields {
                    fields: excluded.iter().map(|field| self.string(field)).collect(),
                });
            }
            HirExpr::Map { source, mapper } => {
                self.emit(source, row, code, deps)?;
                let (mapper, mapper_deps) = self.expression(mapper, true)?;
                deps.extend(mapper_deps);
                code.push(ExpressionInstruction::Map {
                    mapper,
                    item_slot: 0,
                    index_slot: None,
                });
            }
            HirExpr::Filter { source, predicate } => {
                self.emit(source, row, code, deps)?;
                let (predicate, predicate_deps) = self.expression(predicate, true)?;
                deps.extend(predicate_deps);
                code.push(ExpressionInstruction::Filter {
                    predicate,
                    item_slot: 0,
                    index_slot: None,
                });
            }
            HirExpr::Builtin { kind, args } => {
                for arg in &args {
                    self.emit(*arg, row, code, deps)?;
                }
                let kind = match kind.as_str() {
                    "jsonStringify" => "jsonStringify",
                    "encodeUriComponent" => "encodeUriComponent",
                    "trim" => "trim",
                    "lower" => "lower",
                    "upper" => "upper",
                    "includes" => "includes",
                    _ => return Err(self.err("builtin is not executable")),
                };
                code.push(ExpressionInstruction::String {
                    kind,
                    count: args.len(),
                });
            }
            expression => {
                return Err(self.err(&format!("expression is not executable: {expression:?}")))
            }
        };
        Ok(())
    }
}
