use plec_schema::delta::RuntimeValue;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub enum EvalInstruction {
    Constant(usize),
    State(usize),
    Prop(usize),
    Ref(usize),
    Frame(usize),
    Event(usize),
    RowRecord,
    RowField(usize),
    Host(usize),
    Field(usize),
    Index,
    Map(usize),
    Filter(usize),
    String(String, usize),
    MakeArray(usize, Vec<bool>),
    MakeRecord(Vec<usize>, Vec<bool>),
    OmitFields(Vec<usize>),
    Unary(String),
    Binary(String),
    Jump(usize),
    JumpIfFalse(usize),
    JumpIfTrue(usize),
    Return,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvalError {
    ProgramOutOfRange,
    StackUnderflow,
    StackValueLimit,
    StackByteLimit,
    FuelExhausted,
    NestingLimit,
}

pub trait ExpressionProgram {
    fn instruction(&self, program: usize, pc: usize) -> Option<EvalInstruction>;
    fn program_len(&self, program: usize) -> Option<usize>;
    fn constant(&self, handle: usize) -> RuntimeValue;
    fn string(&self, handle: usize) -> Option<&str>;
}

pub trait ExpressionHost {
    fn load_state(&mut self, slot: usize) -> RuntimeValue {
        let _ = slot;
        RuntimeValue::Null
    }
    fn load_prop(&mut self, slot: usize) -> RuntimeValue {
        let _ = slot;
        RuntimeValue::Null
    }
    fn load_ref(&mut self, slot: usize) -> RuntimeValue {
        let _ = slot;
        RuntimeValue::Null
    }
    fn load_frame(&mut self, slot: usize) -> RuntimeValue {
        let _ = slot;
        RuntimeValue::Null
    }
    fn load_event(&mut self, slot: usize) -> RuntimeValue {
        let _ = slot;
        RuntimeValue::Null
    }
    fn load_row_record(&mut self) -> RuntimeValue {
        RuntimeValue::Null
    }
    fn load_row_field(&mut self, field: &str) -> RuntimeValue {
        let _ = field;
        RuntimeValue::Null
    }
    fn load_host(&mut self, slot: usize) -> RuntimeValue {
        let _ = slot;
        RuntimeValue::Null
    }
}

pub fn evaluate<P: ExpressionProgram, H: ExpressionHost>(
    program: &P,
    host: &mut H,
    handle: usize,
) -> Result<RuntimeValue, EvalError> {
    let mut fuel = plec_ir::limits::MAX_EXPRESSION_STEPS;
    evaluate_bounded(program, host, handle, &mut fuel, 0)
}

fn push(
    stack: &mut Vec<RuntimeValue>,
    sizes: &mut Vec<usize>,
    bytes: &mut usize,
    value: RuntimeValue,
) -> Result<(), EvalError> {
    if stack.len() >= plec_ir::limits::MAX_EVAL_STACK_VALUES {
        return Err(EvalError::StackValueLimit);
    }
    let size = value.estimated_size_bytes();
    if bytes.saturating_add(size) > plec_ir::limits::MAX_EVAL_STACK_BYTES {
        return Err(EvalError::StackByteLimit);
    }
    stack.push(value);
    sizes.push(size);
    *bytes += size;
    Ok(())
}
fn pop(
    stack: &mut Vec<RuntimeValue>,
    sizes: &mut Vec<usize>,
    bytes: &mut usize,
) -> Result<RuntimeValue, EvalError> {
    let value = stack.pop().ok_or(EvalError::StackUnderflow)?;
    *bytes -= sizes.pop().unwrap_or_default();
    Ok(value)
}
fn take(
    stack: &mut Vec<RuntimeValue>,
    sizes: &mut Vec<usize>,
    bytes: &mut usize,
    count: usize,
) -> Result<Vec<RuntimeValue>, EvalError> {
    if stack.len() < count {
        return Err(EvalError::StackUnderflow);
    }
    let start = stack.len() - count;
    *bytes -= sizes.drain(start..).sum::<usize>();
    Ok(stack.drain(start..).collect())
}
fn truthy(v: &RuntimeValue) -> bool {
    v.truthy()
}
fn number(v: &RuntimeValue) -> f64 {
    v.number()
}
fn string(v: &RuntimeValue) -> String {
    v.dom_string()
}

fn evaluate_bounded<P: ExpressionProgram>(
    program: &P,
    host: &mut dyn ExpressionHost,
    handle: usize,
    fuel: &mut usize,
    depth: usize,
) -> Result<RuntimeValue, EvalError> {
    if depth > plec_ir::limits::MAX_EVAL_NESTING {
        return Err(EvalError::NestingLimit);
    }
    let len = program
        .program_len(handle)
        .ok_or(EvalError::ProgramOutOfRange)?;
    let (mut stack, mut sizes, mut bytes, mut pc) = (Vec::new(), Vec::new(), 0usize, 0usize);
    while pc < len {
        if *fuel == 0 {
            return Err(EvalError::FuelExhausted);
        }
        *fuel -= 1;
        let ins = program
            .instruction(handle, pc)
            .ok_or(EvalError::ProgramOutOfRange)?;
        match ins {
            EvalInstruction::Constant(i) => {
                push(&mut stack, &mut sizes, &mut bytes, program.constant(i))?
            }
            EvalInstruction::State(i) => {
                push(&mut stack, &mut sizes, &mut bytes, host.load_state(i))?
            }
            EvalInstruction::Prop(i) => {
                push(&mut stack, &mut sizes, &mut bytes, host.load_prop(i))?
            }
            EvalInstruction::Ref(i) => push(&mut stack, &mut sizes, &mut bytes, host.load_ref(i))?,
            EvalInstruction::Frame(i) => {
                push(&mut stack, &mut sizes, &mut bytes, host.load_frame(i))?
            }
            EvalInstruction::Event(i) => {
                push(&mut stack, &mut sizes, &mut bytes, host.load_event(i))?
            }
            EvalInstruction::RowRecord => {
                push(&mut stack, &mut sizes, &mut bytes, host.load_row_record())?
            }
            EvalInstruction::RowField(i) => push(
                &mut stack,
                &mut sizes,
                &mut bytes,
                program
                    .string(i)
                    .map(|s| host.load_row_field(s))
                    .unwrap_or(RuntimeValue::Null),
            )?,
            EvalInstruction::Host(i) => {
                push(&mut stack, &mut sizes, &mut bytes, host.load_host(i))?
            }
            EvalInstruction::Field(i) => {
                let object = pop(&mut stack, &mut sizes, &mut bytes)?;
                let key = program.string(i).unwrap_or("");
                let value = match object {
                    RuntimeValue::Record(m) => m.get(key).cloned().unwrap_or_default(),
                    RuntimeValue::Array(a) if key == "length" => {
                        RuntimeValue::Number(a.len() as f64)
                    }
                    RuntimeValue::String(s) if key == "length" => {
                        RuntimeValue::Number(s.chars().count() as f64)
                    }
                    _ => RuntimeValue::Null,
                };
                push(&mut stack, &mut sizes, &mut bytes, value)?;
            }
            EvalInstruction::Index => {
                let k = pop(&mut stack, &mut sizes, &mut bytes)?;
                let o = pop(&mut stack, &mut sizes, &mut bytes)?;
                let key = match k {
                    RuntimeValue::String(s) => s,
                    RuntimeValue::Number(n) if n.is_finite() && n.fract() == 0. => n.to_string(),
                    _ => String::new(),
                };
                let v = match o {
                    RuntimeValue::Record(m) => m.get(&key).cloned().unwrap_or_default(),
                    RuntimeValue::Array(a) => key
                        .parse::<usize>()
                        .ok()
                        .and_then(|i| a.get(i))
                        .cloned()
                        .unwrap_or_default(),
                    RuntimeValue::String(s) => key
                        .parse::<usize>()
                        .ok()
                        .and_then(|i| s.chars().nth(i))
                        .map(|c| RuntimeValue::String(c.to_string()))
                        .unwrap_or_default(),
                    _ => RuntimeValue::Null,
                };
                push(&mut stack, &mut sizes, &mut bytes, v)?;
            }
            EvalInstruction::Map(child) | EvalInstruction::Filter(child) => {
                let is_map = matches!(ins, EvalInstruction::Map(_));
                let source = pop(&mut stack, &mut sizes, &mut bytes)?;
                let mut out = Vec::new();
                for item in source.array().unwrap_or(&[]).iter().cloned() {
                    let row = item.record().cloned().unwrap_or_default();
                    let mut row_host = RowHost {
                        parent: host,
                        row: row.clone(),
                    };
                    let value = evaluate_bounded(program, &mut row_host, child, fuel, depth + 1)?;
                    if is_map {
                        out.push(value);
                    } else if truthy(&value) {
                        out.push(RuntimeValue::Record(row));
                    }
                }
                push(&mut stack, &mut sizes, &mut bytes, RuntimeValue::Array(out))?;
            }
            EvalInstruction::String(kind, count) => {
                let parts = take(&mut stack, &mut sizes, &mut bytes, count)?;
                let first = parts.first().cloned().unwrap_or_default();
                let value = match kind.as_str() {
                    "trim" => RuntimeValue::String(string(&first).trim().to_owned()),
                    "lower" => RuntimeValue::String(string(&first).to_lowercase()),
                    "upper" => RuntimeValue::String(string(&first).to_uppercase()),
                    "encodeUriComponent" => {
                        RuntimeValue::String(encode_uri_component(&string(&first)))
                    }
                    "jsonStringify" => {
                        RuntimeValue::String(serde_json::to_string(&first).unwrap_or_default())
                    }
                    "includes" => RuntimeValue::Bool(
                        string(&first)
                            .contains(&string(parts.get(1).unwrap_or(&RuntimeValue::Null))),
                    ),
                    _ => RuntimeValue::String(parts.iter().map(string).collect()),
                };
                push(&mut stack, &mut sizes, &mut bytes, value)?;
            }
            EvalInstruction::MakeArray(count, spreads) => {
                let values = take(&mut stack, &mut sizes, &mut bytes, count)?;
                let mut out = Vec::new();
                for (i, v) in values.into_iter().enumerate() {
                    if spreads.get(i).copied().unwrap_or(false) {
                        out.extend(v.array().unwrap_or(&[]).iter().cloned());
                    } else {
                        out.push(v);
                    }
                }
                push(&mut stack, &mut sizes, &mut bytes, RuntimeValue::Array(out))?;
            }
            EvalInstruction::MakeRecord(fields, spreads) => {
                let values = take(&mut stack, &mut sizes, &mut bytes, fields.len())?;
                let mut out = HashMap::new();
                for (i, (field, v)) in fields.into_iter().zip(values).enumerate() {
                    if spreads.get(i).copied().unwrap_or(false) {
                        if let RuntimeValue::Record(m) = v {
                            out.extend(m);
                        }
                    } else if let Some(name) = program.string(field) {
                        out.insert(name.to_owned(), v);
                    }
                }
                push(
                    &mut stack,
                    &mut sizes,
                    &mut bytes,
                    RuntimeValue::Record(out),
                )?;
            }
            EvalInstruction::OmitFields(fields) => {
                let mut v = pop(&mut stack, &mut sizes, &mut bytes)?
                    .record()
                    .cloned()
                    .unwrap_or_default();
                for f in fields {
                    if let Some(name) = program.string(f) {
                        v.remove(name);
                    }
                }
                push(&mut stack, &mut sizes, &mut bytes, RuntimeValue::Record(v))?;
            }
            EvalInstruction::Unary(kind) => {
                let v = pop(&mut stack, &mut sizes, &mut bytes)?;
                let out = match kind.as_str() {
                    "not" => RuntimeValue::Bool(!truthy(&v)),
                    "minus" => RuntimeValue::Number(-number(&v)),
                    _ => v,
                };
                push(&mut stack, &mut sizes, &mut bytes, out)?;
            }
            EvalInstruction::Binary(kind) => {
                let r = pop(&mut stack, &mut sizes, &mut bytes)?;
                let l = pop(&mut stack, &mut sizes, &mut bytes)?;
                let out = match kind.as_str() {
                    "equal" => RuntimeValue::Bool(l == r),
                    "notEqual" => RuntimeValue::Bool(l != r),
                    "instanceofError" => RuntimeValue::Bool(
                        matches!(&l,RuntimeValue::Record(m) if m.contains_key("kind")||m.contains_key("message")),
                    ),
                    "and" => {
                        if truthy(&l) {
                            r
                        } else {
                            l
                        }
                    }
                    "or" => {
                        if truthy(&l) {
                            l
                        } else {
                            r
                        }
                    }
                    "coalesce" => {
                        if l.is_null() {
                            r
                        } else {
                            l
                        }
                    }
                    "add" => match (l, r) {
                        (RuntimeValue::Number(a), RuntimeValue::Number(b)) => {
                            RuntimeValue::Number(a + b)
                        }
                        (a, b) => RuntimeValue::String(format!("{}{}", string(&a), string(&b))),
                    },
                    "subtract" => RuntimeValue::Number(number(&l) - number(&r)),
                    "multiply" => RuntimeValue::Number(number(&l) * number(&r)),
                    "divide" => RuntimeValue::Number(number(&l) / number(&r)),
                    "greater" => RuntimeValue::Bool(number(&l) > number(&r)),
                    "greaterEqual" => RuntimeValue::Bool(number(&l) >= number(&r)),
                    "less" => RuntimeValue::Bool(number(&l) < number(&r)),
                    "lessEqual" => RuntimeValue::Bool(number(&l) <= number(&r)),
                    _ => RuntimeValue::Null,
                };
                push(&mut stack, &mut sizes, &mut bytes, out)?;
            }
            EvalInstruction::Jump(t) => {
                pc = t;
                continue;
            }
            EvalInstruction::JumpIfFalse(t) => {
                if !truthy(&pop(&mut stack, &mut sizes, &mut bytes)?) {
                    pc = t;
                    continue;
                }
            }
            EvalInstruction::JumpIfTrue(t) => {
                if truthy(&pop(&mut stack, &mut sizes, &mut bytes)?) {
                    pc = t;
                    continue;
                }
            }
            EvalInstruction::Return => {
                return Ok(pop(&mut stack, &mut sizes, &mut bytes).unwrap_or_default())
            }
            EvalInstruction::Unknown => {}
        }
        pc += 1;
    }
    Ok(pop(&mut stack, &mut sizes, &mut bytes).unwrap_or_default())
}

fn encode_uri_component(value: &str) -> String {
    let mut out = String::new();
    for b in value.bytes() {
        if b.is_ascii_alphanumeric()
            || matches!(
                b,
                b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')'
            )
        {
            out.push(b as char)
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

struct RowHost<'a, H: ?Sized> {
    parent: &'a mut H,
    row: HashMap<String, RuntimeValue>,
}
impl<H: ExpressionHost + ?Sized> ExpressionHost for RowHost<'_, H> {
    fn load_state(&mut self, i: usize) -> RuntimeValue {
        self.parent.load_state(i)
    }
    fn load_prop(&mut self, i: usize) -> RuntimeValue {
        self.parent.load_prop(i)
    }
    fn load_ref(&mut self, i: usize) -> RuntimeValue {
        self.parent.load_ref(i)
    }
    fn load_frame(&mut self, i: usize) -> RuntimeValue {
        self.parent.load_frame(i)
    }
    fn load_event(&mut self, i: usize) -> RuntimeValue {
        self.parent.load_event(i)
    }
    fn load_host(&mut self, i: usize) -> RuntimeValue {
        self.parent.load_host(i)
    }
    fn load_row_record(&mut self) -> RuntimeValue {
        RuntimeValue::Record(self.row.clone())
    }
    fn load_row_field(&mut self, f: &str) -> RuntimeValue {
        self.row.get(f).cloned().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Program {
        code: Vec<Vec<EvalInstruction>>,
        constants: Vec<RuntimeValue>,
        strings: Vec<String>,
    }
    impl ExpressionProgram for Program {
        fn instruction(&self, p: usize, pc: usize) -> Option<EvalInstruction> {
            self.code.get(p)?.get(pc).cloned()
        }
        fn program_len(&self, p: usize) -> Option<usize> {
            self.code.get(p).map(Vec::len)
        }
        fn constant(&self, h: usize) -> RuntimeValue {
            self.constants.get(h).cloned().unwrap_or_default()
        }
        fn string(&self, h: usize) -> Option<&str> {
            self.strings.get(h).map(String::as_str)
        }
    }
    #[derive(Default)]
    struct Host {
        state: RuntimeValue,
    }
    impl ExpressionHost for Host {
        fn load_state(&mut self, _: usize) -> RuntimeValue {
            self.state.clone()
        }
    }
    fn run(
        code: Vec<EvalInstruction>,
        constants: Vec<RuntimeValue>,
        strings: Vec<String>,
    ) -> Result<RuntimeValue, EvalError> {
        evaluate(
            &Program {
                code: vec![code],
                constants,
                strings,
            },
            &mut Host::default(),
            0,
        )
    }

    #[test]
    fn evaluates_instruction_families_and_host_reads() {
        let p = Program {
            code: vec![vec![
                EvalInstruction::State(0),
                EvalInstruction::Field(0),
                EvalInstruction::Constant(0),
                EvalInstruction::Binary("add".into()),
                EvalInstruction::Return,
            ]],
            constants: vec![RuntimeValue::Number(2.)],
            strings: vec!["n".into()],
        };
        let mut host = Host {
            state: RuntimeValue::Record(HashMap::from([("n".into(), RuntimeValue::Number(3.))])),
        };
        assert_eq!(evaluate(&p, &mut host, 0), Ok(RuntimeValue::Number(5.)));
    }
    #[test]
    fn fuel_and_stack_limits_are_reported() {
        assert_eq!(
            run(vec![EvalInstruction::Jump(0)], vec![], vec![]),
            Err(EvalError::FuelExhausted)
        );
        let count = plec_ir::limits::MAX_EVAL_STACK_VALUES + 1;
        let code = (0..count).map(|_| EvalInstruction::Constant(0)).collect();
        assert_eq!(
            run(code, vec![RuntimeValue::Null], vec![]),
            Err(EvalError::StackValueLimit)
        );
        assert_eq!(
            run(
                vec![EvalInstruction::Constant(0)],
                vec![RuntimeValue::String(
                    "x".repeat(plec_ir::limits::MAX_EVAL_STACK_BYTES + 1)
                )],
                vec![]
            ),
            Err(EvalError::StackByteLimit)
        );
    }
    #[test]
    fn nested_map_filter_share_fuel_and_nesting_limits() {
        let p = Program {
            code: vec![
                vec![
                    EvalInstruction::Constant(0),
                    EvalInstruction::Filter(1),
                    EvalInstruction::Return,
                ],
                vec![EvalInstruction::RowRecord, EvalInstruction::Return],
            ],
            constants: vec![RuntimeValue::Array(vec![RuntimeValue::Null])],
            strings: vec![],
        };
        assert_eq!(
            evaluate(&p, &mut Host::default(), 0),
            Ok(RuntimeValue::Array(vec![RuntimeValue::Record(
                HashMap::new()
            )]))
        );
        let recursive = Program {
            code: vec![vec![
                EvalInstruction::Constant(0),
                EvalInstruction::Filter(0),
                EvalInstruction::Return,
            ]],
            constants: vec![RuntimeValue::Array(vec![RuntimeValue::Null])],
            strings: vec![],
        };
        assert_eq!(
            evaluate(&recursive, &mut Host::default(), 0),
            Err(EvalError::NestingLimit)
        );
    }

    #[test]
    fn map_items_consume_one_shared_fuel_budget() {
        let p = Program {
            code: vec![
                vec![
                    EvalInstruction::Constant(0),
                    EvalInstruction::Map(1),
                    EvalInstruction::Return,
                ],
                vec![EvalInstruction::RowRecord, EvalInstruction::Return],
            ],
            constants: vec![RuntimeValue::Array(vec![
                RuntimeValue::Null,
                RuntimeValue::Null,
            ])],
            strings: vec![],
        };
        let mut host = Host::default();
        let mut fuel = 4;
        assert_eq!(
            evaluate_bounded(&p, &mut host, 0, &mut fuel, 0),
            Err(EvalError::FuelExhausted)
        );
        assert_eq!(fuel, 0);
    }
}
