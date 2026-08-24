use std::collections::BTreeSet;

use plec_hir::*;
use plec_ir::{
    Binding, ComponentProp, DependencyEdge, DependencyEndpoint, Event, ExpressionInstruction,
    ExpressionProgram, Loop, Node, PropProgram, PropWrite, Text, Value,
};

use crate::{Ctx, LoweringError};

impl Ctx<'_> {
    pub(crate) fn node(
        &mut self,
        id: NodeId,
        parent: Option<usize>,
    ) -> Result<usize, LoweringError> {
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
                let host_ref = element.host_ref.map(|binding| self.host_refs.get(&binding).copied().ok_or_else(|| self.err("host ref used before lowering"))).transpose()?;
                self.app.nodes.push(Node::Element {
                    tag,
                    parent,
                    children: vec![],
                    host_ref,
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
}
