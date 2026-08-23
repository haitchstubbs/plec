use std::collections::{BTreeSet, HashMap};

use plec_hir::{
    BindingId, HirBindingKind, HirCallable, HirCallableBody, HirComponent, HirExpr, HirLogicalOp,
    HirNode, HirProp, HirStmt, HirText, HirUnaryOp, HirValue, NodeId,
};
use plec_ir::{
    ActionInstruction, ActionProgram, Binding, DependencyEdge, DependencyEndpoint, Event, Input,
    ExecutableApplication, ExpressionInstruction, ExpressionProgram, Loop, Node, PropProgram,
    PropWrite, StateSlot, Text, Value,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoweringError(pub String);
impl std::fmt::Display for LoweringError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for LoweringError {}

pub fn lower_component_to_executable(
    component: &HirComponent,
) -> Result<ExecutableApplication, LoweringError> {
    let mut ctx = Ctx::new(component);
    for state in &component.states {
        let initial_expression = ctx.expression(state.initializer, false)?.0;
        let slot = ctx.app.state_slots.len();
        ctx.states.insert(state.value, slot);
        ctx.app.state_slots.push(StateSlot {
            initial_expression,
            frame_slot: slot,
        });
    }
    for input in &component.inputs {
        if input.kind != "collection" {
            return Err(Ctx::new(component).err("input kind is not executable"));
        }
        let name = ctx.string(&input.name);
        let slot = ctx.app.inputs.len();
        ctx.inputs.insert(input.binding, slot);
        ctx.app.inputs.push(Input { name, kind: "collection" });
    }
    for callable in &component.callables {
        if !callable.parameters.is_empty() {
            return Err(ctx.err("callable parameters are not executable yet"));
        }
        let action = ctx.action(&callable.body, false)?;
        ctx.callables.insert(callable.binding, action);
    }
    if component.root_nodes.len() != 1 {
        return Err(ctx.err("executable roots require exactly one node"));
    }
    ctx.app.root_node = ctx.node(component.root_nodes[0], None)?;
    Ok(ctx.app)
}

struct Ctx<'a> {
    component: &'a HirComponent,
    app: ExecutableApplication,
    strings: HashMap<String, usize>,
    constants: HashMap<String, usize>,
    states: HashMap<BindingId, usize>,
    inputs: HashMap<BindingId, usize>,
    callables: HashMap<BindingId, usize>,
    locals: HashMap<BindingId, plec_hir::ExprId>,
    active_locals: Vec<BindingId>,
    active_loop: Option<usize>,
}
impl<'a> Ctx<'a> {
    fn new(component: &'a HirComponent) -> Self {
        Self {
            component,
            app: ExecutableApplication::default(),
            strings: HashMap::new(),
            constants: HashMap::new(),
            states: HashMap::new(),
            inputs: HashMap::new(),
            callables: HashMap::new(),
            locals: component
                .locals
                .iter()
                .map(|v| (v.binding, v.initializer))
                .collect(),
            active_locals: vec![],
            active_loop: None,
        }
    }
    fn err(&self, message: &str) -> LoweringError {
        LoweringError(message.into())
    }
    fn string(&mut self, value: &str) -> usize {
        if let Some(id) = self.strings.get(value) {
            return *id;
        }
        let id = self.app.strings.len();
        self.app.strings.push(value.into());
        self.strings.insert(value.into(), id);
        id
    }
    fn constant(&mut self, value: Value) -> usize {
        let key = format!("{value:?}");
        if let Some(id) = self.constants.get(&key) {
            return *id;
        }
        let id = self.app.constants.len();
        self.app.constants.push(value);
        self.constants.insert(key, id);
        id
    }
    fn expr(&self, id: plec_hir::ExprId) -> Result<&HirExpr, LoweringError> {
        self.component
            .expressions
            .get(id.0 as usize)
            .map(|e| &e.expression)
            .ok_or_else(|| self.err("expression handle missing"))
    }
    fn expression(
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
            HirExpr::Member { object, property } => {
                if matches!(self.expr(object)?, HirExpr::Binding(binding) if matches!(self.component.bindings[binding.0 as usize].kind, HirBindingKind::LoopItem)) {
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
    fn node(&mut self, id: NodeId, parent: Option<usize>) -> Result<usize, LoweringError> {
        match self
            .component
            .nodes
            .get(id.0 as usize)
            .ok_or_else(|| self.err("node handle missing"))?
            .clone()
        {
            HirNode::Element(element) => {
                let index = self.app.nodes.len();
                let tag = self.string(&element.tag);
                self.app.nodes.push(Node::Element {
                    tag,
                    parent,
                    children: vec![],
                });
                let mut children = vec![];
                for child in element.children {
                    children.push(self.node(child, Some(index))?);
                }
                if let Node::Element {
                    children: target, ..
                } = &mut self.app.nodes[index]
                {
                    *target = children;
                }
                for prop in element.props {
                    self.prop(index, prop)?;
                }
                for event in element.events {
                    let action = self.callable(&event.callable)?;
                    let event_type = self.string(&event.event);
                    self.app.events.push(Event {
                        target: index,
                        event_type,
                        action,
                        r#loop: self.active_loop,
                        fields: vec![],
                    });
                }
                Ok(index)
            }
            HirNode::Text(HirText::Static { value, .. }) => {
                let text = self.app.texts.len();
                self.app.texts.push(Text {
                    value: Some(value),
                    binding: None,
                });
                let index = self.app.nodes.len();
                self.app.nodes.push(Node::Text { text, parent });
                Ok(index)
            }
            HirNode::Text(HirText::Expression { expression, .. }) => {
                let text = self.app.texts.len();
                self.app.texts.push(Text {
                    value: None,
                    binding: None,
                });
                let index = self.app.nodes.len();
                self.app.nodes.push(Node::Text { text, parent });
                let (expression, deps) = self.expression(expression, parent.is_some())?;
                let binding = self.app.bindings.len();
                self.app.bindings.push(Binding {
                    target: index,
                    sink: "text",
                    name: None,
                    expression,
                });
                self.app.texts[text].binding = Some(binding);
                self.edges(deps, "binding", binding);
                self.row_edges(expression, "binding", binding);
                Ok(index)
            }
            HirNode::ForEach(loop_node) => {
                let parent = parent.ok_or_else(|| self.err("loop requires an element parent"))?;
                let key = loop_node
                    .identity
                    .ok_or_else(|| self.err("ForEach requires a key"))?;
                if loop_node.body.len() != 1 {
                    return Err(self.err("ForEach requires one row root"));
                }
                let input = match self.expr(loop_node.source)? {
                    HirExpr::Binding(binding) => self.inputs.get(binding).copied(),
                    _ => None,
                };
                let (source, deps) = if input.is_some() {
                    let index = self.app.expressions.len();
                    self.app.expressions.push(ExpressionProgram {
                        instructions: vec![ExpressionInstruction::MakeArray { count: 0 }, ExpressionInstruction::Return],
                    });
                    (index, BTreeSet::new())
                } else {
                    self.expression(loop_node.source, false)?
                };
                let (key_expression, _) = self.expression(key, true)?;
                let loop_index = self.app.loops.len();
                let previous_loop = self.active_loop.replace(loop_index);
                let row_template = self.node(loop_node.body[0], None)?;
                self.active_loop = previous_loop;
                self.app.loops.push(Loop {
                    source_expression: source,
                    key_expression,
                    item_slot: loop_node.item_binding.0 as usize,
                    row_template,
                    dependency_slots: deps.iter().copied().collect(),
                    input,
                });
                self.edges(deps, "loop", loop_index);
                let index = self.app.nodes.len();
                self.app.nodes.push(Node::Loop {
                    r#loop: loop_index,
                    parent: Some(parent),
                });
                Ok(index)
            }
            HirNode::Conditional(conditional) => {
                if conditional.consequent.len() != 1 || conditional.alternate.len() > 1 {
                    return Err(self.err("conditional branches require at most one node"));
                }
                let (test, deps) = self.expression(conditional.test, self.active_loop.is_some())?;
                let index = self.app.nodes.len();
                self.app.nodes.push(Node::Conditional {
                    test,
                    parent,
                    consequent: 0,
                    alternate: None,
                });
                let consequent = self.node(conditional.consequent[0], None)?;
                let alternate = conditional.alternate.first().map(|node| self.node(*node, None)).transpose()?;
                self.app.nodes[index] = Node::Conditional { test, parent, consequent, alternate };
                self.edges(deps, "conditional", index);
                self.row_edges(test, "conditional", index);
                Ok(index)
            }
            _ => Err(self.err("node is not executable in the first IR slice")),
        }
    }
    fn prop(&mut self, target: usize, prop: HirProp) -> Result<(), LoweringError> {
        match prop {
            HirProp::Static { name, value } => {
                let program = self.prop_program(target);
                let name = self.string(&name);
                let constant = self.constant(Value::String(value));
                self.app.prop_programs[program].writes.push(PropWrite {
                    name,
                    kind: "attribute",
                    constant: Some(constant),
                    expression: None,
                });
            }
            HirProp::Expression { name, value } => {
                let (expression, deps) = self.expression(value, self.active_loop.is_some())?;
                let program = self.prop_program(target);
                let name = self.string(&name);
                self.app.prop_programs[program].writes.push(PropWrite {
                    name,
                    kind: "attribute",
                    constant: None,
                    expression: Some(expression),
                });
                self.edges(deps, "propProgram", program);
                self.row_edges(expression, "propProgram", program);
            }
            HirProp::Callable { .. } => {
                return Err(self.err("callable component props are not executable yet"))
            }
        };
        Ok(())
    }
    fn prop_program(&mut self, target: usize) -> usize {
        if let Some((i, _)) = self
            .app
            .prop_programs
            .iter()
            .enumerate()
            .find(|(_, p)| p.target == target)
        {
            i
        } else {
            let i = self.app.prop_programs.len();
            self.app.prop_programs.push(PropProgram {
                target,
                writes: vec![],
            });
            i
        }
    }
    fn edges(&mut self, deps: BTreeSet<usize>, kind: &'static str, handle: usize) {
        for state in deps {
            self.app.dependency_edges.push(DependencyEdge {
                source: DependencyEndpoint {
                    kind: "state",
                    handle: state,
                    r#loop: None,
                },
                target: DependencyEndpoint { kind, handle, r#loop: None },
            })
        }
    }
    fn row_edges(&mut self, expression: usize, kind: &'static str, target_handle: usize) {
        let Some(loop_index) = self.active_loop else { return };
        let fields = self.app.expressions[expression].instructions.iter().filter_map(|instruction| match instruction {
            ExpressionInstruction::LoadRowField { field } => Some(*field),
            _ => None,
        }).collect::<BTreeSet<_>>();
        for field in fields {
            self.app.dependency_edges.push(DependencyEdge {
                source: DependencyEndpoint { kind: "rowField", handle: field, r#loop: Some(loop_index) },
                target: DependencyEndpoint { kind, handle: target_handle, r#loop: None },
            });
        }
    }
    fn callable(&mut self, callable: &HirCallable) -> Result<usize, LoweringError> {
        match callable {
            HirCallable::Reference { binding } => self
                .callables
                .get(binding)
                .copied()
                .ok_or_else(|| self.err("event callable is not a zero-parameter local action")),
            HirCallable::Inline { parameters, body } if parameters.is_empty() => self.action(body, self.active_loop.is_some()),
            _ => Err(self.err("callable is not executable in the first IR slice")),
        }
    }
    fn action(&mut self, body: &HirCallableBody, row: bool) -> Result<usize, LoweringError> {
        let mut code = vec![];
        match body {
            HirCallableBody::Block(stmts) => self.statements(stmts, &mut code, row)?,
            HirCallableBody::Expression(expr) => {
                let (state, value) = self.setter_call(*expr)?;
                let (expression, _) = self.expression(value, row)?;
                code.push(ActionInstruction::Evaluate { expression });
                code.push(ActionInstruction::StoreState { state });
            }
        }
        code.push(ActionInstruction::Return);
        let index = self.app.actions.len();
        self.app.actions.push(ActionProgram { instructions: code });
        Ok(index)
    }
    fn statements(
        &mut self,
        stmts: &[HirStmt],
        code: &mut Vec<ActionInstruction>, row: bool,
    ) -> Result<(), LoweringError> {
        for stmt in stmts {
            match stmt {
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
                    self.statements(consequent, code, row)?;
                    let jump_end = code.len();
                    code.push(ActionInstruction::Jump { target: 0 });
                    let alternate_start = code.len();
                    self.statements(alternate, code, row)?;
                    let end = code.len();
                    code[jump_false] = ActionInstruction::JumpIfFalse {
                        target: alternate_start,
                    };
                    code[jump_end] = ActionInstruction::Jump { target: end };
                }
                HirStmt::Return { .. } => code.push(ActionInstruction::Return),
                HirStmt::Expression { .. } => {
                    return Err(self.err("generic action expressions are not executable yet"))
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{discover_root_component, lower_root_component};
    use plec_parser::parse_module;
    use plec_sema::build_semantic_graph;
    use std::collections::HashMap;

    fn lower(source: &str) -> ExecutableApplication {
        let module = parse_module("test.tsx", source).unwrap();
        let modules = vec![module];
        let graph = build_semantic_graph(&modules, &HashMap::new()).unwrap();
        let root = discover_root_component(&modules, &graph, "test.tsx", None).unwrap();
        let hir = lower_root_component(&root, &graph).unwrap();
        lower_component_to_executable(&hir).unwrap()
    }

    #[test]
    fn lowers_counter_to_v09_actions_and_dependencies() {
        let app = lower(
            r#"
            export function Counter() {
                const [count, setCount] = useState(0);
                function increment() { setCount(count + 1); }
                return <button className="counter" onClick={increment}>{count}</button>;
            }
        "#,
        );
        assert_eq!(app.version, plec_ir::VERSION);
        assert_eq!(app.state_slots.len(), 1);
        assert_eq!(app.events.len(), 1);
        assert_eq!(app.actions.len(), 1);
        assert!(matches!(
            app.actions[0].instructions[1],
            ActionInstruction::StoreState { state: 0 }
        ));
        assert!(app
            .dependency_edges
            .iter()
            .any(|edge| edge.source.kind == "state" && edge.target.kind == "binding"));
        assert!(app.prop_programs[0].writes[0].constant.is_some());
    }

    #[test]
    fn lowers_keyed_state_array_loop() {
        let app = lower(
            r#"
            export function Todos() {
                const [todos, setTodos] = useState([{ id: "one", title: "One" }]);
                return <ul>{todos.map(todo => <li key={todo.id}>{todo.title}</li>)}</ul>;
            }
        "#,
        );
        assert_eq!(app.loops.len(), 1);
        assert!(app.expressions[app.state_slots[0].initial_expression]
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, ExpressionInstruction::MakeArray { .. })));
        assert!(app
            .dependency_edges
            .iter()
            .any(|edge| edge.target.kind == "loop"));
    }

    #[test]
    fn lowers_collection_rows_with_conditional_and_row_action() {
        let app = lower(
            r#"
            export function Todos() {
                const todos = useCollection("items");
                const [selected, setSelected] = useState("");
                return <ul>{todos.map(todo => <li key={todo.id}>{todo.title}{todo.done && <button onClick={() => setSelected(todo.title)}>Done</button>}</li>)}</ul>;
            }
        "#,
        );
        assert_eq!(app.inputs.len(), 1);
        assert_eq!(app.inputs[0].kind, "collection");
        assert_eq!(app.loops[0].input, Some(0));
        assert!(matches!(app.nodes.iter().find(|node| matches!(node, Node::Conditional { .. })), Some(_)));
        assert!(app.events.iter().any(|event| event.r#loop == Some(0)));
        assert!(app.dependency_edges.iter().any(|edge| edge.source.kind == "rowField" && edge.target.kind == "binding"));
        assert!(app.dependency_edges.iter().any(|edge| edge.source.kind == "rowField" && edge.target.kind == "conditional"));
    }

    #[test]
    fn rejects_unkeyed_loops() {
        let source = r#"
            export function Todos() {
                const [todos, setTodos] = useState([{ id: "one" }]);
                return <ul>{todos.map(todo => <li>{todo.id}</li>)}</ul>;
            }
        "#;
        let module = parse_module("test.tsx", source).unwrap();
        let modules = vec![module];
        let graph = build_semantic_graph(&modules, &HashMap::new()).unwrap();
        let root = discover_root_component(&modules, &graph, "test.tsx", None).unwrap();
        let hir = lower_root_component(&root, &graph).unwrap();
        assert!(lower_component_to_executable(&hir)
            .unwrap_err()
            .to_string()
            .contains("ForEach requires a key"));
    }
}
