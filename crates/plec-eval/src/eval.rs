use crate::core::{self, EvalError, EvalInstruction, ExpressionHost, ExpressionProgram};
use plec_schema::{
    delta::RuntimeValue,
    typed::{TypedApplication, TypedExpressionInstruction},
};
use std::collections::HashMap;
use wasm_bindgen::JsValue;

struct TypedProgram<'a>(&'a TypedApplication);
impl ExpressionProgram for TypedProgram<'_> {
    fn program_len(&self, p: usize) -> Option<usize> {
        self.0.expressions.get(p).map(|p| p.instructions.len())
    }
    fn constant(&self, h: usize) -> RuntimeValue {
        self.0.constants.get(h).cloned().unwrap_or_default()
    }
    fn string(&self, h: usize) -> Option<&str> {
        self.0.strings.get(h).map(String::as_str)
    }
    fn instruction(&self, p: usize, pc: usize) -> Option<EvalInstruction> {
        Some(match self.0.expressions.get(p)?.instructions.get(pc)? {
            TypedExpressionInstruction::Constant { constant } => {
                EvalInstruction::Constant(*constant)
            }
            TypedExpressionInstruction::LoadState { state } => EvalInstruction::State(*state),
            TypedExpressionInstruction::LoadProp { prop } => EvalInstruction::Prop(*prop),
            TypedExpressionInstruction::LoadRef { reference } => EvalInstruction::Ref(*reference),
            TypedExpressionInstruction::LoadFrame { slot } => EvalInstruction::Frame(*slot),
            TypedExpressionInstruction::LoadEventField { field } => EvalInstruction::Event(*field),
            TypedExpressionInstruction::LoadRowRecord => EvalInstruction::RowRecord,
            TypedExpressionInstruction::LoadRowField { field } => EvalInstruction::RowField(*field),
            TypedExpressionInstruction::LoadHost { host } => EvalInstruction::Host(*host),
            TypedExpressionInstruction::Field { field } => EvalInstruction::Field(*field),
            TypedExpressionInstruction::Index => EvalInstruction::Index,
            TypedExpressionInstruction::Map { mapper, .. } => EvalInstruction::Map(*mapper),
            TypedExpressionInstruction::Filter { predicate, .. } => {
                EvalInstruction::Filter(*predicate)
            }
            TypedExpressionInstruction::String { kind, count } => {
                EvalInstruction::String(kind.clone(), *count)
            }
            TypedExpressionInstruction::MakeArray { count, spreads } => {
                EvalInstruction::MakeArray(*count, spreads.clone())
            }
            TypedExpressionInstruction::MakeRecord { fields, spreads } => {
                EvalInstruction::MakeRecord(fields.clone(), spreads.clone())
            }
            TypedExpressionInstruction::OmitFields { fields } => {
                EvalInstruction::OmitFields(fields.clone())
            }
            TypedExpressionInstruction::Unary { kind } => EvalInstruction::Unary(kind.clone()),
            TypedExpressionInstruction::Binary { kind } => EvalInstruction::Binary(kind.clone()),
            TypedExpressionInstruction::Jump { target } => EvalInstruction::Jump(*target),
            TypedExpressionInstruction::JumpIfFalse { target } => {
                EvalInstruction::JumpIfFalse(*target)
            }
            TypedExpressionInstruction::JumpIfTrue { target } => {
                EvalInstruction::JumpIfTrue(*target)
            }
            TypedExpressionInstruction::Return => EvalInstruction::Return,
        })
    }
}

struct BrowserHost<'a> {
    app: &'a TypedApplication,
    cookie_policy: Option<&'a plec_dom::cookie::CookiePolicyMap>,
    states: &'a [RuntimeValue],
    props: &'a [RuntimeValue],
    refs: &'a [RuntimeValue],
    frame: &'a [RuntimeValue],
    event: &'a [RuntimeValue],
    row: Option<&'a HashMap<String, RuntimeValue>>,
}
impl ExpressionHost for BrowserHost<'_> {
    fn load_state(&mut self, i: usize) -> RuntimeValue {
        self.states.get(i).cloned().unwrap_or_default()
    }
    fn load_prop(&mut self, i: usize) -> RuntimeValue {
        self.props
            .get(i)
            .cloned()
            .or_else(|| self.app.runtime_props.get(i).cloned())
            .unwrap_or_default()
    }
    fn load_ref(&mut self, i: usize) -> RuntimeValue {
        self.refs
            .get(i)
            .cloned()
            .or_else(|| self.app.ref_values.get(i).cloned())
            .unwrap_or_default()
    }
    fn load_frame(&mut self, i: usize) -> RuntimeValue {
        self.frame.get(i).cloned().unwrap_or_default()
    }
    fn load_event(&mut self, i: usize) -> RuntimeValue {
        self.event.get(i).cloned().unwrap_or_default()
    }
    fn load_row_record(&mut self) -> RuntimeValue {
        RuntimeValue::Record(self.row.cloned().unwrap_or_default())
    }
    fn load_row_field(&mut self, f: &str) -> RuntimeValue {
        self.row.and_then(|r| r.get(f)).cloned().unwrap_or_default()
    }
    fn load_host(&mut self, i: usize) -> RuntimeValue {
        let Some(slot) = self.app.host_slots.get(i) else {
            return RuntimeValue::Null;
        };
        match slot.kind.as_str() {
            "cookie" => slot
                .name
                .and_then(|n| self.app.strings.get(n))
                .and_then(|n| plec_dom::cookie::read_sync_cookie(self.cookie_policy, n).ok())
                .unwrap_or_default(),
            "location" => self
                .app
                .host_inputs
                .get("location.pathname")
                .cloned()
                .map(|pathname| {
                    RuntimeValue::Record(HashMap::from([
                        ("pathname".into(), pathname),
                        (
                            "search".into(),
                            self.app
                                .host_inputs
                                .get("location.search")
                                .cloned()
                                .unwrap_or_default(),
                        ),
                        (
                            "hash".into(),
                            self.app
                                .host_inputs
                                .get("location.hash")
                                .cloned()
                                .unwrap_or_default(),
                        ),
                    ]))
                })
                .or_else(|| {
                    plec_dom::platform::window().ok().and_then(|w| {
                        let l = w.location();
                        Some(RuntimeValue::Record(HashMap::from([
                            ("pathname".into(), RuntimeValue::String(l.pathname().ok()?)),
                            ("search".into(), RuntimeValue::String(l.search().ok()?)),
                            ("hash".into(), RuntimeValue::String(l.hash().ok()?)),
                        ])))
                    })
                })
                .unwrap_or_default(),
            "mediaQuery" => slot
                .query
                .and_then(|q| self.app.strings.get(q))
                .and_then(|q| {
                    plec_dom::platform::window()
                        .ok()?
                        .match_media(q)
                        .ok()?
                        .map(|m| {
                            RuntimeValue::Record(HashMap::from([(
                                "matches".into(),
                                RuntimeValue::Bool(m.matches()),
                            )]))
                        })
                })
                .unwrap_or_default(),
            "currentYear" => RuntimeValue::Number(js_sys::Date::new_0().get_full_year() as f64),
            "loaderData" => self
                .app
                .host_inputs
                .get("loaderData")
                .cloned()
                .unwrap_or_default(),
            "routeParams" => self
                .app
                .host_inputs
                .get("routeParams")
                .cloned()
                .unwrap_or_default(),
            "routeSearch" => self
                .app
                .host_inputs
                .get("routeSearch")
                .cloned()
                .unwrap_or_default(),
            _ => RuntimeValue::Null,
        }
    }
}

pub fn typed_eval(
    app: &TypedApplication,
    cookie_policy: Option<&plec_dom::cookie::CookiePolicyMap>,
    program: usize,
    states: &[RuntimeValue],
    row: Option<&HashMap<String, RuntimeValue>>,
    _row_index: usize,
) -> Result<RuntimeValue, JsValue> {
    typed_eval_frame(app, cookie_policy, program, states, row, 0, &[], &[])
}

#[allow(clippy::too_many_arguments)]
pub fn typed_eval_frame(
    app: &TypedApplication,
    cookie_policy: Option<&plec_dom::cookie::CookiePolicyMap>,
    program: usize,
    states: &[RuntimeValue],
    row: Option<&HashMap<String, RuntimeValue>>,
    _row_index: usize,
    frame: &[RuntimeValue],
    event: &[RuntimeValue],
) -> Result<RuntimeValue, JsValue> {
    let mut host = BrowserHost {
        app,
        cookie_policy,
        states,
        props: &app.runtime_props,
        refs: &app.ref_values,
        frame,
        event,
        row,
    };
    core::evaluate(&TypedProgram(app), &mut host, program).map_err(|e| {
        JsValue::from_str(match e {
            EvalError::ProgramOutOfRange => "expression handle out of range",
            EvalError::StackUnderflow => "expression stack underflow",
            EvalError::StackValueLimit => "expression evaluation stack exceeds value limit",
            EvalError::StackByteLimit => "expression evaluation stack exceeds byte limit",
            EvalError::FuelExhausted => "expression execution budget exceeded",
            EvalError::NestingLimit => "expression nesting exceeds limit",
        })
    })
}

pub fn typed_truthy(value: &RuntimeValue) -> bool {
    value.truthy()
}
pub fn typed_value_string(value: &RuntimeValue) -> String {
    value.dom_string()
}
