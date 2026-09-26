//! Route-loader body compilation.
//!
//! Compiles the deliberately static loader subset into a terminal loader
//! action on the route's component graph. The grammar is a statement-level
//! subset, not general JavaScript:
//!
//! - one destructured parameter object: `{ params }` (a `signal` key is
//!   accepted for source compatibility; the runtime owns cancellation)
//! - `const <id> = await fetch(url)` where `url` is a string literal or a
//!   template literal over params/binding members. A fetch init object is
//!   restricted to the recognized `{ signal }` form and normalized away
//! - `if (predicate) { ... } else { ... }` with the same statement grammar
//! - `throw redirect(location, { replace }?)` and `throw notFound()`
//! - `return <value>`, `return await fetch(url)`, or a bare `return`
//!
//! Loader-local `await fetch()` evaluates to the decoded response body, not
//! the Web `Response` and not the transport envelope. Transport failures are
//! `require_ok` failures before any body logic runs. Everything the grammar
//! does not cover must fail with a precise diagnostic; nothing is silently
//! reinterpreted.

use std::collections::HashMap;

use plec_hir::HirRoute;
use plec_ir::{
    ActionInstruction, CapabilityRequest, ExpressionInstruction, ExpressionProgram, ReturnOutcome,
    StateSlot, Value,
};
use plec_parser::ParsedModule;
use swc_ecma_ast::{
    ArrowFunctionBody, BinaryOp, Decl, Expr, KeyValueProp, MemberExpr, ModuleItem, ObjectPatProp,
    Pat, Prop, PropName, PropOrSpread, Stmt, UnaryOp, VarDecl,
};

use crate::routes::RouteError;

/// Frame-slot layout shared by every compiled loader program. Clients seed
/// slot 0 with matched params and slot 1 with the request location; the
/// server host mirrors that contract.
const PARAMS_SLOT: usize = 0;
const LOCATION_SLOT: usize = 1;
const ERROR_SLOT: usize = 2;
const FIRST_LOCAL_SLOT: usize = 3;
const NO_BINDING: usize = usize::MAX;

pub struct CompiledLoader {
    pub frame_slots: usize,
    pub parameter_slots: Vec<usize>,
    pub loader_result_state: Option<usize>,
    pub loader_decode_body: bool,
    /// The loader can resolve as not found; the route chain must declare a
    /// not-found boundary for the outcome to render deterministically.
    pub uses_not_found: bool,
    pub instructions: Vec<ActionInstruction>,
}

/// The route component pools the loader program is compiled into. The loader
/// action shares the component's constant/string/expression/state pools so
/// both hosts evaluate it with the standard executors.
pub struct LoaderPools<'a> {
    pub strings: &'a mut Vec<String>,
    pub constants: &'a mut Vec<Value>,
    pub expressions: &'a mut Vec<ExpressionProgram>,
    pub state_slots: &'a mut Vec<StateSlot>,
}

pub(crate) fn loader_declaration<'a>(
    modules: &'a [ParsedModule],
    route: &HirRoute,
) -> Result<&'a Expr, RouteError> {
    let (module_id, local) = route
        .id
        .rsplit_once('#')
        .ok_or_else(|| RouteError("route loader id is invalid".into()))?;
    let module = modules
        .iter()
        .find(|module| module.id == module_id)
        .ok_or_else(|| RouteError("route loader module is missing".into()))?;
    module
        .ast
        .body
        .iter()
        .filter_map(exported_var)
        .flat_map(|declaration| declaration.decls.iter())
        .find(|declaration| matches!(&declaration.name, Pat::Ident(name) if name.id.sym == *local))
        .and_then(|declaration| declaration.init.as_deref())
        .and_then(|expression| match expression {
            Expr::Call(call)
                if callee_name(&call.callee) == Some("createRoute")
                    || callee_name(&call.callee) == Some("createRootRoute") =>
            {
                call.args.first()
            }
            _ => None,
        })
        .and_then(|argument| {
            argument
                .expr
                .as_object()
                .and_then(|options| prop(options.props.as_slice(), "loader"))
        })
        .ok_or_else(|| RouteError("route loader declaration is missing".into()))
}

struct Emitter<'a> {
    pools: LoaderPools<'a>,
    instructions: Vec<ActionInstruction>,
    scratch: Vec<ExpressionInstruction>,
    bindings: HashMap<String, usize>,
    next_slot: usize,
}

impl Emitter<'_> {
    fn string(&mut self, value: &str) -> usize {
        let pools = &mut self.pools;
        if let Some(index) = pools.strings.iter().position(|existing| existing == value) {
            return index;
        }
        pools.strings.push(value.to_owned());
        pools.strings.len() - 1
    }

    fn constant(&mut self, value: Value) -> usize {
        self.pools.constants.push(value);
        self.pools.constants.len() - 1
    }

    /// Materializes the scratch program as a stored expression program.
    fn take_scratch(&mut self) -> usize {
        let instructions = std::mem::take(&mut self.scratch);
        self.pools.expressions.push(ExpressionProgram { instructions });
        self.pools.expressions.len() - 1
    }

    fn alloc_slot(&mut self) -> usize {
        let slot = self.next_slot;
        self.next_slot += 1;
        slot
    }

    fn emit(&mut self, instruction: ActionInstruction) {
        self.instructions.push(instruction);
    }
}

/// Compiles a loader body into the terminal loader action program.
pub(crate) fn compile_loader(loader: &Expr, pools: LoaderPools<'_>) -> Result<CompiledLoader, RouteError> {
    let mut emitter = Emitter {
        pools,
        instructions: Vec::new(),
        scratch: Vec::new(),
        bindings: HashMap::new(),
        next_slot: FIRST_LOCAL_SLOT,
    };

    match loader {
        Expr::Arrow(arrow) => match arrow.body.as_ref() {
            ArrowFunctionBody::Expr(expression) => {
                compile_await_fetch(&mut emitter, expression, true)?;
            }
            ArrowFunctionBody::FunctionBody(body) => {
                let pats: Vec<&Pat> = arrow.params.iter().collect();
                compile_parameters(&mut emitter, &pats)?;
                compile_statements(&mut emitter, &body.stmts)?;
            }
        },
        Expr::Fn(function) => {
            let function = &function.function;
            let pats: Vec<&Pat> = function.params.iter().map(|param| &param.pat).collect();
            compile_parameters(&mut emitter, &pats)?;
            let body = function
                .body
                .as_ref()
                .ok_or_else(|| RouteError("route loader body is required".into()))?;
            compile_statements(&mut emitter, &body.stmts)?;
        }
        _ => return Err(RouteError("route loader must be a function".into())),
    }

    let fetches = emitter
        .instructions
        .iter()
        .filter(|instruction| matches!(instruction, ActionInstruction::CapabilityRequest { .. }))
        .count();
    if fetches == 0 {
        return Err(RouteError(
            "route loader must contain at least one await fetch(url)".into(),
        ));
    }
    let uses_not_found = emitter
        .instructions
        .iter()
        .any(|instruction| matches!(instruction, ActionInstruction::Return { outcome: ReturnOutcome::NotFound, .. }));

    // Terminal failure return: re-export the capability error record. Every
    // capability request jumps here when its transport fails.
    let failure_pc = emitter.instructions.len();
    emitter.scratch = vec![
        ExpressionInstruction::LoadFrame { slot: ERROR_SLOT },
        ExpressionInstruction::Return,
    ];
    let failure_expression = emitter.take_scratch();
    emitter.emit(ActionInstruction::Return {
        outcome: ReturnOutcome::Failure,
        value: Some(failure_expression),
    });
    for instruction in &mut emitter.instructions {
        if let ActionInstruction::CapabilityRequest { failure_pc: target, .. } = instruction {
            if *target == usize::MAX {
                *target = failure_pc;
            }
        }
    }

    // Loader result state: a null-initialized slot the hosts overwrite with
    // the resolved loader value. The state index (its position in the
    // component's state pool) and its frame slot are independent numbers.
    let null_constant = emitter.constant(Value::Null);
    emitter.scratch = vec![
        ExpressionInstruction::Constant {
            constant: null_constant,
        },
        ExpressionInstruction::Return,
    ];
    let initial_expression = emitter.take_scratch();
    let state_index = emitter.pools.state_slots.len();
    let state_frame_slot = emitter.alloc_slot();
    emitter.pools.state_slots.push(StateSlot {
        initial_expression,
        frame_slot: state_frame_slot,
    });

    Ok(CompiledLoader {
        frame_slots: emitter.next_slot,
        parameter_slots: vec![PARAMS_SLOT, LOCATION_SLOT],
        loader_result_state: Some(state_index),
        loader_decode_body: true,
        uses_not_found,
        instructions: emitter.instructions,
    })
}

fn compile_parameters(emitter: &mut Emitter, params: &[&Pat]) -> Result<(), RouteError> {
    match params {
        [] => Ok(()),
        [one] => {
            let Pat::Object(object) = one else {
                return Err(RouteError(
                    "route loader parameters must be destructured ({ params })".into(),
                ));
            };
            for property in &object.props {
                let name = match property {
                    ObjectPatProp::Assign(assign) => assign.key.id.sym.to_string(),
                    ObjectPatProp::KeyValue(key_value) => match key_value.value.as_ident() {
                        Some(ident) => ident.id.sym.to_string(),
                        None => {
                            return Err(RouteError(
                                "route loader parameters must be destructured ({ params })"
                                    .into(),
                            ))
                        }
                    },
                    ObjectPatProp::Rest(_) => {
                        return Err(RouteError(
                            "route loader parameters must be destructured ({ params })".into(),
                        ))
                    }
                };
                match name.as_str() {
                    "params" => {
                        emitter.bindings.insert("params".into(), PARAMS_SLOT);
                    }
                    // Accepted for source compatibility; cancellation is
                    // host-owned and `signal` has no loader-side value.
                    "signal" => {}
                    other => {
                        return Err(RouteError(format!(
                            "route loader parameter {other} is not supported; loaders accept params"
                        )))
                    }
                }
            }
            Ok(())
        }
        _ => Err(RouteError(
            "route loader accepts a single ({ params }) parameter".into(),
        )),
    }
}

fn compile_statements(emitter: &mut Emitter, statements: &[Stmt]) -> Result<(), RouteError> {
    for statement in statements {
        compile_statement(emitter, statement)?;
    }
    Ok(())
}

fn compile_statement(emitter: &mut Emitter, statement: &Stmt) -> Result<(), RouteError> {
    match statement {
        Stmt::Decl(Decl::Var(decl)) => {
            let decls = &decl.decls;
            if decls.len() != 1 {
                return Err(RouteError(
                    "route loader declarations must bind one value".into(),
                ));
            }
            let declaration = &decls[0];
            let Pat::Ident(ident) = &declaration.name else {
                return Err(RouteError(
                    "route loader declarations must bind a simple name".into(),
                ));
            };
            let Some(initializer) = declaration.init.as_deref().map(unwrap_static) else {
                return Err(RouteError(
                    "route loader declarations must initialize their value".into(),
                ));
            };
            let binding = compile_await_fetch(emitter, initializer, false)?;
            emitter
                .bindings
                .insert(ident.id.sym.to_string(), binding);
            Ok(())
        }
        Stmt::Return(returned) => match returned.arg.as_deref().map(unwrap_static) {
            None => {
                emitter.emit(ActionInstruction::Return {
                    outcome: ReturnOutcome::Success,
                    value: None,
                });
                Ok(())
            }
            Some(Expr::Await(awaited)) => {
                compile_await_fetch(emitter, &awaited.arg, true)?;
                Ok(())
            }
            Some(expression) => {
                let value = compile_value_expression(emitter, expression)?;
                emitter.emit(ActionInstruction::Return {
                    outcome: ReturnOutcome::Success,
                    value: Some(value),
                });
                Ok(())
            }
        },
        Stmt::Throw(thrown) => compile_throw(emitter, &thrown.arg),
        Stmt::If(if_statement) => {
            let predicate = compile_value_expression(emitter, &if_statement.test)?;
            emitter.emit(ActionInstruction::Evaluate { expression: predicate });
            let jump = emitter.instructions.len();
            emitter.emit(ActionInstruction::JumpIfFalse { target: jump });
            compile_block_stmt(emitter, &if_statement.cons)?;
            let alternate = match &if_statement.alt {
                None => None,
                Some(alternate) => {
                    let end = emitter.instructions.len();
                    emitter.emit(ActionInstruction::Jump { target: end });
                    Some((end, alternate.as_ref()))
                }
            };
            let else_pc = emitter.instructions.len();
            emitter.instructions[jump] = ActionInstruction::JumpIfFalse { target: else_pc };
            if let Some((end, alternate)) = alternate {
                compile_block_stmt(emitter, alternate)?;
                let final_pc = emitter.instructions.len();
                emitter.instructions[end] = ActionInstruction::Jump { target: final_pc };
            }
            Ok(())
        }
        Stmt::Block(block) => compile_statements(emitter, &block.stmts),
        Stmt::Empty(_) => Ok(()),
        other => Err(RouteError(format!(
            "route loader statement is not supported: {}",
            statement_kind(other)
        ))),
    }
}

/// Strips parenthesized/assertion wrappers so `return (await fetch(url)) as
/// T` and `const x = (await fetch(url)) as T` keep their platform shape while
/// lowering to the same fetch contract.
fn unwrap_static(expression: &Expr) -> &Expr {
    match expression {
        Expr::Paren(parenthesized) => unwrap_static(&parenthesized.expr),
        Expr::TsAs(assertion) => unwrap_static(&assertion.expr),
        Expr::TsTypeAssertion(assertion) => unwrap_static(&assertion.expr),
        other => other,
    }
}

fn compile_block_stmt(emitter: &mut Emitter, statement: &Stmt) -> Result<(), RouteError> {
    // A brace-less if body is a single statement; both shapes compile the
    // same statement grammar.
    compile_statement(emitter, statement)
}

fn compile_throw(emitter: &mut Emitter, expression: &Expr) -> Result<(), RouteError> {
    let Some(call) = expression.as_call() else {
        return Err(RouteError(
            "route loaders can only throw redirect(...) or notFound()".into(),
        ));
    };
    match callee_name(&call.callee).unwrap_or_default() {
        "redirect" => {
            let [location, rest @ ..] = call.args.as_slice() else {
                return Err(RouteError(
                    "redirect requires a location string or template".into(),
                ));
            };
            if rest.len() > 1 {
                return Err(RouteError(
                    "redirect accepts a location and an optional options object".into(),
                ));
            }
            compile_value_instructions(emitter, &location.expr)?;
            match rest.first() {
                None => {
                    let truthy = emitter.constant(Value::Bool(true));
                    emitter.scratch.push(ExpressionInstruction::Constant {
                        constant: truthy,
                    });
                }
                Some(options) => {
                    let Expr::Object(object) = options.expr.as_ref() else {
                        return Err(RouteError(
                            "redirect options must be an object literal".into(),
                        ));
                    };
                    if object.props.len() != 1 {
                        return Err(RouteError(
                            "redirect options support only replace".into(),
                        ));
                    }
                    let replace =
                        prop(object.props.as_slice(), "replace").ok_or_else(|| {
                            RouteError("redirect options support only replace".into())
                        })?;
                    let Expr::Lit(swc_ecma_ast::Lit::Bool(replace)) = replace else {
                        return Err(RouteError(
                            "redirect replace must be a boolean literal".into(),
                        ));
                    };
                    let constant = emitter.constant(Value::Bool(replace.value));
                    emitter
                        .scratch
                        .push(ExpressionInstruction::Constant { constant });
                }
            }
            let location_name = emitter.string("location");
            let replace_name = emitter.string("replace");
            emitter.scratch.push(ExpressionInstruction::MakeRecord {
                fields: vec![location_name, replace_name],
                spreads: vec![],
            });
            emitter.scratch.push(ExpressionInstruction::Return);
            let value = emitter.take_scratch();
            emitter.emit(ActionInstruction::Return {
                outcome: ReturnOutcome::Redirect,
                value: Some(value),
            });
            Ok(())
        }
        "notFound" => {
            if !call.args.is_empty() {
                return Err(RouteError("notFound takes no arguments".into()));
            }
            emitter.emit(ActionInstruction::Return {
                outcome: ReturnOutcome::NotFound,
                value: None,
            });
            Ok(())
        }
        other => Err(RouteError(format!(
            "route loaders can only throw redirect(...) or notFound(), not {other}"
        ))),
    }
}

/// Compiles `await fetch(url, init?)`. With `terminal` the decoded body is
/// returned as the loader value; otherwise it is stored into a fresh binding
/// slot and that slot index is returned.
fn compile_await_fetch(
    emitter: &mut Emitter,
    expression: &Expr,
    terminal: bool,
) -> Result<usize, RouteError> {
    let Expr::Await(awaited) = expression else {
        return Err(RouteError(
            "route loaders can only await fetch(url)".into(),
        ));
    };
    let Some(call) = awaited.arg.as_call() else {
        return Err(RouteError(
            "route loaders can only await fetch(url)".into(),
        ));
    };
    if callee_name(&call.callee) != Some("fetch") {
        return Err(RouteError(
            "route loaders can only await fetch(url)".into(),
        ));
    }
    let [url, rest @ ..] = call.args.as_slice() else {
        return Err(RouteError("fetch requires a url".into()));
    };
    if rest.len() > 1 {
        return Err(RouteError("fetch accepts a url and one init object".into()));
    }
    for option in rest {
        // The recognized init form is `{ signal }`. It normalizes away:
        // cancellation is host-owned. Any other init member is rejected.
        let Expr::Object(object) = option.expr.as_ref() else {
            return Err(RouteError("fetch init must be an object literal".into()));
        };
        let recognized = object.props.len() == 1
            && object
                .props
                .first()
                .and_then(PropOrSpread::as_prop)
                .is_some_and(|prop| {
                    matches!(&**prop, Prop::Shorthand(ident) if ident.sym == "signal")
                });
        if !recognized {
            return Err(RouteError(
                "loader fetch supports only the { signal } init; the runtime owns cancellation"
                    .into(),
            ));
        }
    }
    let url_expression = compile_value_expression(emitter, &url.expr)?;
    let envelope_slot = emitter.alloc_slot();
    let capability_pc = emitter.instructions.len();
    emitter.emit(ActionInstruction::CapabilityRequest {
        request: CapabilityRequest::Fetch {
            url: url_expression,
            method: "GET",
            headers: vec![],
            body: None,
            decode: "responseJson",
            require_ok: true,
        },
        success_pc: capability_pc + 1,
        failure_pc: usize::MAX,
        finally_pc: None,
        result_slot: envelope_slot,
        error_slot: ERROR_SLOT,
    });
    // Decode the envelope to its body: loader-local `await fetch()` is the
    // decoded body by contract.
    let body_name = emitter.string("body");
    emitter.scratch = vec![
        ExpressionInstruction::LoadFrame {
            slot: envelope_slot,
        },
        ExpressionInstruction::Field { field: body_name },
    ];
    if terminal {
        emitter.scratch.push(ExpressionInstruction::Return);
        let value = emitter.take_scratch();
        emitter.emit(ActionInstruction::Return {
            outcome: ReturnOutcome::Success,
            value: Some(value),
        });
        Ok(NO_BINDING)
    } else {
        let expression = emitter.take_scratch();
        emitter.emit(ActionInstruction::Evaluate { expression });
        let binding = emitter.alloc_slot();
        emitter.emit(ActionInstruction::StoreFrame { slot: binding });
        Ok(binding)
    }
}

fn compile_value_expression(
    emitter: &mut Emitter,
    expression: &Expr,
) -> Result<usize, RouteError> {
    emitter.scratch.clear();
    compile_value_instructions(emitter, expression)?;
    emitter.scratch.push(ExpressionInstruction::Return);
    Ok(emitter.take_scratch())
}

fn compile_value_instructions(
    emitter: &mut Emitter,
    expression: &Expr,
) -> Result<(), RouteError> {
    match expression {
        Expr::Lit(swc_ecma_ast::Lit::Str(value)) => {
            let constant = emitter
                .constant(Value::String(value.value.to_string_lossy().into_owned()));
            emitter.scratch.push(ExpressionInstruction::Constant { constant });
        }
        Expr::Lit(swc_ecma_ast::Lit::Num(value)) => {
            let constant = emitter.constant(Value::Number(value.value));
            emitter.scratch.push(ExpressionInstruction::Constant { constant });
        }
        Expr::Lit(swc_ecma_ast::Lit::Bool(value)) => {
            let constant = emitter.constant(Value::Bool(value.value));
            emitter.scratch.push(ExpressionInstruction::Constant { constant });
        }
        Expr::Lit(swc_ecma_ast::Lit::Null(_)) => {
            let constant = emitter.constant(Value::Null);
            emitter.scratch.push(ExpressionInstruction::Constant { constant });
        }
        Expr::Tpl(template) => {
            // SWC stores n+1 quasis around n expressions; interleave them to
            // keep the stack push order equal to source order.
            let mut count = 0usize;
            for (index, element) in template.quasis.iter().enumerate() {
                let constant = emitter.constant(Value::String(element.raw.to_string()));
                emitter.scratch.push(ExpressionInstruction::Constant { constant });
                count += 1;
                if let Some(expression) = template.exprs.get(index) {
                    compile_value_instructions(emitter, expression)?;
                    count += 1;
                }
            }
            emitter.scratch.push(ExpressionInstruction::String {
                kind: "concat",
                count,
            });
        }
        Expr::Ident(ident) => {
            let name = ident.sym.as_ref();
            let slot = emitter.bindings.get(name).copied().ok_or_else(|| {
                RouteError(format!(
                    "route loader value {name} is not a param or a fetched value"
                ))
            })?;
            emitter
                .scratch
                .push(ExpressionInstruction::LoadFrame { slot });
        }
        Expr::Member(member) => compile_member(emitter, member)?,
        Expr::Bin(binary) => {
            let kind = match binary.op {
                BinaryOp::EqEq | BinaryOp::EqEqEq => "equal",
                BinaryOp::NotEq | BinaryOp::NotEqEq => "notEqual",
                BinaryOp::Lt => "less",
                BinaryOp::LtEq => "lessEqual",
                BinaryOp::Gt => "greater",
                BinaryOp::GtEq => "greaterEqual",
                BinaryOp::LogicalAnd => "and",
                BinaryOp::LogicalOr => "or",
                BinaryOp::Add => "add",
                BinaryOp::Sub => "subtract",
                BinaryOp::Mul => "multiply",
                BinaryOp::Div => "divide",
                BinaryOp::NullishCoalescing => "coalesce",
                other => {
                    return Err(RouteError(format!(
                        "route loader operator {other:?} is not supported"
                    )))
                }
            };
            compile_value_instructions(emitter, &binary.left)?;
            compile_value_instructions(emitter, &binary.right)?;
            emitter.scratch.push(ExpressionInstruction::Binary { kind });
        }
        Expr::Unary(unary) => {
            let kind = match unary.op {
                UnaryOp::Bang => "not",
                UnaryOp::Minus => "minus",
                other => {
                    return Err(RouteError(format!(
                        "route loader unary operator {other:?} is not supported"
                    )))
                }
            };
            compile_value_instructions(emitter, unary.arg.as_ref())?;
            emitter.scratch.push(ExpressionInstruction::Unary { kind });
        }
        Expr::Paren(parenthesized) => compile_value_instructions(emitter, &parenthesized.expr)?,
        Expr::TsAs(assertion) => compile_value_instructions(emitter, &assertion.expr)?,
        Expr::TsTypeAssertion(assertion) => compile_value_instructions(emitter, &assertion.expr)?,
        other => {
            return Err(RouteError(format!(
                "route loader expression is not supported: {}",
                expression_kind(other)
            )))
        }
    }
    Ok(())
}

fn compile_member(emitter: &mut Emitter, member: &MemberExpr) -> Result<(), RouteError> {
    compile_value_instructions(emitter, &member.obj)?;
    let name = member
        .prop
        .as_ident()
        .map(|ident| ident.sym.to_string())
        .ok_or_else(|| RouteError("route loader member access must use plain names".into()))?;
    let field = emitter.string(&name);
    emitter.scratch.push(ExpressionInstruction::Field { field });
    Ok(())
}

fn exported_var(item: &ModuleItem) -> Option<&VarDecl> {
    match item {
        ModuleItem::ModuleDecl(swc_ecma_ast::ModuleDecl::ExportDecl(value)) => match &value.decl {
            Decl::Var(value) => Some(value),
            _ => None,
        },
        ModuleItem::Stmt(Stmt::Decl(Decl::Var(value))) => Some(value),
        _ => None,
    }
}

fn callee_name(callee: &swc_ecma_ast::Callee) -> Option<&str> {
    match callee {
        swc_ecma_ast::Callee::Expr(expr) => match expr.as_ref() {
            Expr::Ident(value) => Some(value.sym.as_ref()),
            _ => None,
        },
        _ => None,
    }
}

fn prop<'a>(props: &'a [PropOrSpread], name: &str) -> Option<&'a Expr> {
    props.iter().find_map(|entry| match entry {
        PropOrSpread::Prop(value) => match value.as_ref() {
            Prop::KeyValue(KeyValueProp { key, value }) if prop_name(key) == Some(name) => {
                Some(value.as_ref())
            }
            _ => None,
        },
        _ => None,
    })
}

fn prop_name(name: &PropName) -> Option<&str> {
    match name {
        PropName::Ident(value) => Some(value.sym.as_ref()),
        PropName::Str(value) => value.value.as_str(),
        _ => None,
    }
}

fn statement_kind(statement: &Stmt) -> &'static str {
    match statement {
        Stmt::Decl(_) => "declaration",
        Stmt::Expr(_) => "expression statement",
        Stmt::For(_) | Stmt::ForIn(_) | Stmt::ForOf(_) | Stmt::While(_) | Stmt::DoWhile(_) => {
            "loop"
        }
        Stmt::Try(_) => "try/catch",
        Stmt::Switch(_) => "switch",
        _ => "statement",
    }
}

fn expression_kind(expression: &Expr) -> &'static str {
    match expression {
        Expr::Call(_) => "function call",
        Expr::New(_) => "constructor call",
        Expr::Await(_) => "await",
        Expr::Array(_) => "array literal",
        Expr::Object(_) => "object literal",
        Expr::Arrow(_) | Expr::Fn(_) => "function",
        Expr::Assign(_) => "assignment",
        _ => "expression",
    }
}
