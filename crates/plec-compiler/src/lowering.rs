use std::collections::{BTreeSet, HashMap};

use plec_hir::{
    BindingId, ComponentId, HirApplication, HirBindingKind, HirCallable, HirCallableBody,
    HirComponent, HirExpr, HirLogicalOp, HirNode, HirProp, HirStmt, HirText, HirUnaryOp, HirValue,
    NodeId,
};
use plec_ir::{
    ActionInstruction, ActionProgram, Binding, CapabilityRequest, ComponentApplication, ComponentParameter,
    ComponentProp, DependencyEdge, DependencyEndpoint, Event, ExecutableApplication,
    ExecutableComponent, ExpressionInstruction, ExpressionProgram, Input, Loop, Node, PropProgram,
    PropWrite, RouteOutlet, StateSlot, Text, Value, COMPONENT_VERSION,
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
    lower_component(component, None)
}

/// Compile the narrow route-loader contract used by the typed router. The
/// loader must be a local callable ending in `return await fetch(url)` and
/// writes directly to the named state slot when the request resolves.
pub fn lower_route_loader_to_executable(
    component: &HirComponent,
    loader_name: &str,
    result_state_name: &str,
    outlet_id: &str,
) -> Result<ExecutableApplication, LoweringError> {
    let mut app = lower_component(component, None)?;
    let loader = component
        .callables
        .iter()
        .position(|callable| {
            component.bindings[callable.binding.0 as usize].name == loader_name
        })
        .ok_or_else(|| LoweringError("route loader callable missing".into()))?;
    let state = component
        .states
        .iter()
        .position(|state| {
            component.bindings[state.value.0 as usize].name == result_state_name
        })
        .ok_or_else(|| LoweringError("route loader result state missing".into()))?;
    let action = app
        .actions
        .get_mut(loader)
        .ok_or_else(|| LoweringError("route loader action missing".into()))?;
    if !action.instructions.iter().any(|instruction| {
        matches!(instruction, ActionInstruction::CapabilityRequest {
            request: CapabilityRequest::Fetch { .. }, ..
        })
    }) {
        return Err(LoweringError("route loader requires return await fetch(url)".into()));
    }
    action.route_loader = true;
    action.loader_result_state = Some(state);
    app.route_outlets.push(RouteOutlet {
        id: outlet_id.into(),
        node: app.root_node,
    });
    Ok(app)
}

pub fn lower_application_to_executable(
    application: &HirApplication,
) -> Result<ComponentApplication, LoweringError> {
    let mut targets = HashMap::new();
    for (index, component) in application.components.iter().enumerate() {
        let props = component
            .parameters
            .iter()
            .map(|parameter| match &parameter.source {
                plec_hir::HirParameterSource::Prop { name } => Ok((
                    name.clone(),
                    matches!(
                        component.bindings[parameter.binding.0 as usize].kind,
                        HirBindingKind::Parameter { callable: true }
                    ),
                )),
                plec_hir::HirParameterSource::Direct => Err(LoweringError(
                    "direct component parameters are not executable".into(),
                )),
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|(name, _)| name != "children")
            .collect::<Vec<_>>();
        let has_slot = component
            .nodes
            .iter()
            .filter(|node| matches!(node, HirNode::Slot(_)))
            .count();
        if has_slot > 1 {
            return Err(LoweringError("components support one children slot".into()));
        }
        targets.insert(component.id.clone(), (index, props, has_slot == 1));
    }
    let root_component = *targets
        .get(&application.root)
        .map(|(index, _, _)| index)
        .ok_or_else(|| LoweringError("application root component missing".into()))?;
    let components = application
        .components
        .iter()
        .map(|component| {
            let app = lower_component(component, Some(&targets))?;
            Ok(ExecutableComponent {
                id: format!("{}#{}", component.id.module_id, component.id.local_name),
                root_node: app.root_node,
                strings: app.strings,
                constants: app.constants,
                nodes: app.nodes,
                texts: app.texts,
                bindings: app.bindings,
                prop_programs: app.prop_programs,
                events: app.events,
                inputs: app.inputs,
                state_slots: app.state_slots,
                parameters: app.parameters,
                expressions: app.expressions,
                actions: app.actions,
                loops: app.loops,
                dependency_edges: app.dependency_edges,
            })
        })
        .collect::<Result<Vec<_>, LoweringError>>()?;
    Ok(ComponentApplication {
        version: COMPONENT_VERSION,
        root_component,
        components,
    })
}

fn lower_component(
    component: &HirComponent,
    targets: Option<&HashMap<ComponentId, (usize, Vec<(String, bool)>, bool)>>,
) -> Result<ExecutableApplication, LoweringError> {
    let mut ctx = Ctx::new(component, targets);
    for parameter in &component.parameters {
        let plec_hir::HirParameterSource::Prop { name } = &parameter.source else {
            return Err(ctx.err("direct component parameters are not executable"));
        };
        let callable = matches!(
            component.bindings[parameter.binding.0 as usize].kind,
            HirBindingKind::Parameter { callable: true }
        );
        let slot = ctx.app.parameters.len();
        ctx.props.insert(parameter.binding, slot);
        if callable {
            ctx.callback_props.insert(parameter.binding, slot);
        }
        let name = ctx.string(name);
        ctx.app
            .parameters
            .push(ComponentParameter { name, callable });
    }
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
            return Err(Ctx::new(component, targets).err("input kind is not executable"));
        }
        let name = ctx.string(&input.name);
        let slot = ctx.app.inputs.len();
        ctx.inputs.insert(input.binding, slot);
        ctx.app.inputs.push(Input {
            name,
            kind: "collection",
        });
    }
    for callable in &component.callables {
        let action = ctx.app.actions.len();
        ctx.app.actions.push(ActionProgram {
            frame_slots: 0,
            parameter_slots: vec![],
            loader_result_state: None,
            route_loader: false,
            instructions: vec![],
        });
        ctx.callables.insert(callable.binding, action);
    }
    for callable in &component.callables {
        let action = *ctx
            .callables
            .get(&callable.binding)
            .expect("action reserved");
        ctx.action(&callable.body, &callable.parameters, false)?;
        let lowered = ctx.app.actions.pop().expect("action lowered");
        ctx.app.actions[action] = lowered;
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
    props: HashMap<BindingId, usize>,
    callback_props: HashMap<BindingId, usize>,
    action_parameters: HashMap<BindingId, usize>,
    targets: Option<&'a HashMap<ComponentId, (usize, Vec<(String, bool)>, bool)>>,
    locals: HashMap<BindingId, plec_hir::ExprId>,
    active_locals: Vec<BindingId>,
    active_loop: Option<usize>,
}
impl<'a> Ctx<'a> {
    fn new(
        component: &'a HirComponent,
        targets: Option<&'a HashMap<ComponentId, (usize, Vec<(String, bool)>, bool)>>,
    ) -> Self {
        Self {
            component,
            app: ExecutableApplication::default(),
            strings: HashMap::new(),
            constants: HashMap::new(),
            states: HashMap::new(),
            inputs: HashMap::new(),
            callables: HashMap::new(),
            props: HashMap::new(),
            callback_props: HashMap::new(),
            action_parameters: HashMap::new(),
            targets,
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
                Some(HirBindingKind::Parameter { callable: false })
                    if self.action_parameters.contains_key(&binding) =>
                {
                    code.push(ExpressionInstruction::LoadFrame {
                        slot: self.action_parameters[&binding],
                    });
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
                self.prop_edges(expression, "binding", binding);
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
                        instructions: vec![
                            ExpressionInstruction::MakeArray { count: 0 },
                            ExpressionInstruction::Return,
                        ],
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
                self.prop_edges(source, "loop", loop_index);
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
                let alternate = conditional
                    .alternate
                    .first()
                    .map(|node| self.node(*node, None))
                    .transpose()?;
                self.app.nodes[index] = Node::Conditional {
                    test,
                    parent,
                    consequent,
                    alternate,
                };
                self.edges(deps, "conditional", index);
                self.prop_edges(test, "conditional", index);
                self.row_edges(test, "conditional", index);
                Ok(index)
            }
            HirNode::Component(call) => {
                let targets = self
                    .targets
                    .ok_or_else(|| self.err("component calls are not executable in IR 0.9"))?;
                let (component, parameters, has_slot) = targets
                    .get(&call.target)
                    .ok_or_else(|| self.err("component target missing from application"))?;
                let mut supplied = BTreeSet::new();
                let mut props = Vec::new();
                let mut dependencies = BTreeSet::new();
                for prop in call.props {
                    let (name, prop, deps) = match prop {
                        HirProp::Static { name, value } => {
                            let constant = self.constant(Value::String(value));
                            let expression = self.app.expressions.len();
                            self.app.expressions.push(ExpressionProgram {
                                instructions: vec![
                                    ExpressionInstruction::Constant { constant },
                                    ExpressionInstruction::Return,
                                ],
                            });
                            (
                                name.clone(),
                                ComponentProp::Value {
                                    name: self.string(&name),
                                    expression,
                                },
                                BTreeSet::new(),
                            )
                        }
                        HirProp::Expression { name, value } => {
                            let (expression, deps) =
                                self.expression(value, self.active_loop.is_some())?;
                            (
                                name.clone(),
                                ComponentProp::Value {
                                    name: self.string(&name),
                                    expression,
                                },
                                deps,
                            )
                        }
                        HirProp::Callable { name, callable } => {
                            let action = self.callable(&callable)?;
                            (
                                name.clone(),
                                ComponentProp::Callable {
                                    name: self.string(&name),
                                    action,
                                },
                                BTreeSet::new(),
                            )
                        }
                    };
                    if !supplied.insert(name.clone()) {
                        return Err(self.err("duplicate component prop"));
                    }
                    let Some((_, callable)) =
                        parameters.iter().find(|(parameter, _)| parameter == &name)
                    else {
                        return Err(self.err("unknown component prop"));
                    };
                    if *callable != matches!(prop, ComponentProp::Callable { .. }) {
                        return Err(self.err("component prop kind does not match parameter"));
                    }
                    dependencies.extend(deps);
                    props.push(prop);
                }
                if supplied.len() != parameters.len()
                    || parameters.iter().any(|(name, _)| !supplied.contains(name))
                {
                    return Err(self.err("missing component prop"));
                }
                if !has_slot && !call.children.is_empty() {
                    return Err(self.err("component children require a target Slot"));
                }
                let children = call
                    .children
                    .into_iter()
                    .map(|child| self.node(child, None))
                    .collect::<Result<Vec<_>, _>>()?;
                let index = self.app.nodes.len();
                self.app.nodes.push(Node::Component {
                    component: *component,
                    parent,
                    props,
                    children,
                });
                self.edges(dependencies, "component", index);
                let expressions = match &self.app.nodes[index] {
                    Node::Component { props, .. } => props
                        .iter()
                        .filter_map(|prop| match prop {
                            ComponentProp::Value { expression, .. } => Some(*expression),
                            ComponentProp::Callable { .. } => None,
                        })
                        .collect::<Vec<_>>(),
                    _ => unreachable!(),
                };
                for expression in expressions {
                    self.prop_edges(expression, "component", index);
                    self.row_edges(expression, "component", index);
                }
                Ok(index)
            }
            HirNode::Slot(_) => {
                let parent =
                    parent.ok_or_else(|| self.err("component slot requires an element parent"))?;
                let index = self.app.nodes.len();
                self.app.nodes.push(Node::Slot {
                    parent: Some(parent),
                });
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
                self.prop_edges(expression, "propProgram", program);
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
                target: DependencyEndpoint {
                    kind,
                    handle,
                    r#loop: None,
                },
            })
        }
    }
    fn row_edges(&mut self, expression: usize, kind: &'static str, target_handle: usize) {
        let Some(loop_index) = self.active_loop else {
            return;
        };
        let fields = self.app.expressions[expression]
            .instructions
            .iter()
            .filter_map(|instruction| match instruction {
                ExpressionInstruction::LoadRowField { field } => Some(*field),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        for field in fields {
            self.app.dependency_edges.push(DependencyEdge {
                source: DependencyEndpoint {
                    kind: "rowField",
                    handle: field,
                    r#loop: Some(loop_index),
                },
                target: DependencyEndpoint {
                    kind,
                    handle: target_handle,
                    r#loop: None,
                },
            });
        }
    }
    fn prop_edges(&mut self, expression: usize, kind: &'static str, target_handle: usize) {
        let props = self.app.expressions[expression]
            .instructions
            .iter()
            .filter_map(|instruction| match instruction {
                ExpressionInstruction::LoadProp { prop } => Some(*prop),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        for prop in props {
            self.app.dependency_edges.push(DependencyEdge {
                source: DependencyEndpoint {
                    kind: "prop",
                    handle: prop,
                    r#loop: None,
                },
                target: DependencyEndpoint {
                    kind,
                    handle: target_handle,
                    r#loop: None,
                },
            });
        }
    }
    fn callable(&mut self, callable: &HirCallable) -> Result<usize, LoweringError> {
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
                            ActionInstruction::Return,
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
    fn action(
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
        let mut code = vec![];
        match body {
            HirCallableBody::Block(stmts) => self.statements(stmts, &mut code, row)?,
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
        self.action_parameters = previous;
        code.push(ActionInstruction::Return);
        let return_pc = code.len() - 1;
        let mut frame_slots = parameters.len();
        for instruction in &mut code {
            if let ActionInstruction::CapabilityRequest {
                success_pc,
                failure_pc,
                result_slot,
                error_slot,
                ..
            } = instruction
            {
                *success_pc = return_pc;
                *failure_pc = return_pc;
                *result_slot = frame_slots;
                *error_slot = frame_slots + 1;
                frame_slots += 2;
            }
        }
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
    ) -> Result<(), LoweringError> {
        for stmt in stmts {
            match stmt {
                HirStmt::AwaitFetch { url, .. } => {
                    let (url, _) = self.expression(*url, row)?;
                    code.push(ActionInstruction::CapabilityRequest {
                        request: CapabilityRequest::Fetch {
                            url,
                            method: "GET",
                            decode: "text",
                            require_ok: true,
                        },
                        success_pc: 0,
                        failure_pc: 0,
                        result_slot: 0,
                        error_slot: 0,
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
        Ok(Some(ActionInstruction::Call { action, arguments }))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{discover_root_component, lower_application, lower_root_component};
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
    fn lowers_parameterized_local_action_calls() {
        let app = lower(
            r#"
            export function Counter() {
                const [count, setCount] = useState(0);
                const incrementBy = (step) => setCount(count + step);
                return <button onClick={() => incrementBy(1)}>{count}</button>;
            }
        "#,
        );
        assert_eq!(app.actions[0].frame_slots, 1);
        assert_eq!(app.actions[0].parameter_slots, [0]);
        assert!(app.expressions.iter().any(|expression| {
            expression.instructions.iter().any(|instruction| {
                matches!(instruction, ExpressionInstruction::LoadFrame { slot: 0 })
            })
        }));
        assert!(matches!(
            app.actions[1].instructions[0],
            ActionInstruction::Call { action: 0, .. }
        ));
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
        assert!(matches!(
            app.nodes
                .iter()
                .find(|node| matches!(node, Node::Conditional { .. })),
            Some(_)
        ));
        assert!(app.events.iter().any(|event| event.r#loop == Some(0)));
        assert!(app
            .dependency_edges
            .iter()
            .any(|edge| edge.source.kind == "rowField" && edge.target.kind == "binding"));
        assert!(app
            .dependency_edges
            .iter()
            .any(|edge| edge.source.kind == "rowField" && edge.target.kind == "conditional"));
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

    #[test]
    fn lowers_component_application_with_scalar_props() {
        let modules = vec![parse_module(
            "test.tsx",
            r#"
            export function App() {
                const [name, setName] = useState("Ada");
                return <main><Child name={name} /></main>;
            }
            function Child({ name }) {
                const [suffix, setSuffix] = useState("!");
                return <button onClick={() => setSuffix("?")}>{name}{suffix}</button>;
            }
        "#,
        )
        .unwrap()];
        let graph = build_semantic_graph(&modules, &HashMap::new()).unwrap();
        let root = discover_root_component(&modules, &graph, "test.tsx", Some("App")).unwrap();
        let hir = lower_application(&modules, &root, &graph).unwrap();
        let app = lower_application_to_executable(&hir).unwrap();
        assert_eq!(app.version, "0.10");
        assert_eq!(app.components.len(), 2);
        assert!(app.components[0]
            .nodes
            .iter()
            .any(|node| matches!(node, Node::Component { component: 1, .. })));
        assert!(app.components[1]
            .expressions
            .iter()
            .any(
                |expression| expression.instructions.iter().any(|instruction| matches!(
                    instruction,
                    ExpressionInstruction::LoadProp { prop: 0 }
                ))
            ));
        assert!(app.components[1]
            .dependency_edges
            .iter()
            .any(|edge| edge.source.kind == "prop" && edge.target.kind == "binding"));
    }

    #[test]
    fn lowers_implicit_children_as_a_parent_owned_slot_template() {
        let modules = vec![parse_module(
            "test.tsx",
            r#"
            export function App() { return <Frame><p>Inside</p></Frame>; }
            function Frame({ children }) { return <section>{children}</section>; }
        "#,
        )
        .unwrap()];
        let graph = build_semantic_graph(&modules, &HashMap::new()).unwrap();
        let root = discover_root_component(&modules, &graph, "test.tsx", Some("App")).unwrap();
        let hir = lower_application(&modules, &root, &graph).unwrap();
        let app = lower_application_to_executable(&hir).unwrap();
        assert!(
            matches!(app.components[0].nodes.iter().find(|node| matches!(node, Node::Component { .. })),
            Some(Node::Component { children, .. }) if children.len() == 1)
        );
        assert!(app.components[1]
            .nodes
            .iter()
            .any(|node| matches!(node, Node::Slot { .. })));
    }

    #[test]
    fn rejects_children_for_components_without_a_slot() {
        let modules = vec![parse_module(
            "test.tsx",
            r#"
            export function App() { return <Frame><p>Inside</p></Frame>; }
            function Frame() { return <section />; }
        "#,
        )
        .unwrap()];
        let graph = build_semantic_graph(&modules, &HashMap::new()).unwrap();
        let root = discover_root_component(&modules, &graph, "test.tsx", Some("App")).unwrap();
        let hir = lower_application(&modules, &root, &graph).unwrap();
        assert!(lower_application_to_executable(&hir)
            .unwrap_err()
            .to_string()
            .contains("component children require a target Slot"));
    }

    #[test]
    fn lowers_keyed_callable_component_prop() {
        let modules = vec![parse_module("test.tsx", r#"
            export function Todos() {
                const todos = useCollection("items");
                const [selected, setSelected] = useState("");
                return <main><p>{selected}</p><ul>{todos.map(todo => <Child key={todo.id} onPick={() => setSelected(todo.title)} />)}</ul></main>;
            }
            function Child({ onPick }: { onPick: () => void }) {
                return <button onClick={onPick}>Pick</button>;
            }
        "#).unwrap()];
        let graph = build_semantic_graph(&modules, &HashMap::new()).unwrap();
        let root = discover_root_component(&modules, &graph, "test.tsx", Some("Todos")).unwrap();
        let hir = lower_application(&modules, &root, &graph).unwrap();
        let app = lower_application_to_executable(&hir).unwrap();
        assert_eq!(app.version, "0.10");
        assert!(
            matches!(app.components[0].nodes.iter().find(|node| matches!(node, Node::Component { .. })), Some(Node::Component { props, .. }) if matches!(props[0], ComponentProp::Callable { .. }))
        );
        assert!(app.components[1].parameters[0].callable);
        assert!(app.components[1].actions.iter().any(|action| matches!(
            action.instructions.first(),
            Some(ActionInstruction::CallProp { prop: 0, arguments }) if arguments.is_empty()
        )));
    }

    #[test]
    fn component_application_rejects_missing_props_and_preserves_loop_call_edges() {
        let modules = vec![parse_module(
            "test.tsx",
            r#"
            export function App() { return <Child />; }
            function Child({ name }) { return <div>{name}</div>; }
        "#,
        )
        .unwrap()];
        let graph = build_semantic_graph(&modules, &HashMap::new()).unwrap();
        let root = discover_root_component(&modules, &graph, "test.tsx", Some("App")).unwrap();
        let hir = lower_application(&modules, &root, &graph).unwrap();
        assert!(lower_application_to_executable(&hir)
            .unwrap_err()
            .to_string()
            .contains("missing component prop"));

        let modules = vec![parse_module("test.tsx", r#"
            export function App() { const rows = [{ id: "1" }]; return <ul>{rows.map(row => <Child key={row.id} name={row.id} />)}</ul>; }
            function Child({ name }) { return <li>{name}</li>; }
        "#).unwrap()];
        let graph = build_semantic_graph(&modules, &HashMap::new()).unwrap();
        let root = discover_root_component(&modules, &graph, "test.tsx", Some("App")).unwrap();
        let hir = lower_application(&modules, &root, &graph).unwrap();
        let app = lower_application_to_executable(&hir).unwrap();
        assert!(app.components[0]
            .nodes
            .iter()
            .any(|node| matches!(node, Node::Component { .. })));
        assert!(app.components[0]
            .dependency_edges
            .iter()
            .any(|edge| edge.source.kind == "rowField" && edge.target.kind == "component"));
    }
}
