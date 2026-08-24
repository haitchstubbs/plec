use std::collections::BTreeSet;

use plec_hir::{HirBindingKind, HirExpr, HirLogicalOp, HirUnaryOp, HirValue};
use plec_ir::{ExpressionInstruction, ExpressionProgram, Value};

use crate::{Ctx, LoweringError};

impl Ctx<'_> {
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
                Some(HirBindingKind::Input { kind }) if kind == "location" => {
                    let host = *self
                        .hosts
                        .get(&binding)
                        .ok_or_else(|| self.err("location used before lowering"))?;
                    code.push(ExpressionInstruction::LoadHost { host });
                }
                Some(HirBindingKind::Parameter { callable: false })
                    if self.action_parameters.contains_key(&binding) =>
                {
                    code.push(ExpressionInstruction::LoadFrame {
                        slot: self.action_parameters[&binding],
                    });
                }
                Some(HirBindingKind::AsyncValue) => {
                    let slot = *self
                        .async_slots
                        .get(&binding)
                        .ok_or_else(|| self.err("async value used outside its action"))?;
                    code.push(ExpressionInstruction::LoadFrame { slot });
                }
                Some(HirBindingKind::Parameter { callable: false }) => {
                    let prop = *self
                        .props
                        .get(&binding)
                        .ok_or_else(|| self.err("component prop used before lowering"))?;
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
                    return Err(self.err("row item must be accessed through a field"))
                }
                _ => return Err(self.err("binding is not executable in this expression")),
            },
            HirExpr::RefCurrent { reference } => {
                let reference = *self.refs.get(&reference).ok_or_else(|| self.err("ref used before lowering"))?;
                code.push(ExpressionInstruction::LoadRef { reference });
            }
            HirExpr::HostRefCurrent { .. } => return Err(self.err("hostRef.current requires a host capability primitive")),
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
                        plec_hir::HirBinaryOp::InstanceOf => {
                            return Err(self.err("instanceof is not executable"))
                        }
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
                for item in items {
                    self.emit(item, row, code, deps)?;
                }
                code.push(ExpressionInstruction::MakeArray { count });
            }
            HirExpr::Object(fields) => {
                let mut names = vec![];
                for (name, value) in fields {
                    names.push(self.string(&name));
                    self.emit(value, row, code, deps)?;
                }
                code.push(ExpressionInstruction::MakeRecord { fields: names });
            }
            _ => return Err(self.err("expression is not executable in the first IR slice")),
        };
        Ok(())
    }
}
