use crate::component_discovery::{
    ComponentDeclaration, ReturnedComponentExpression, RootComponent,
};
use plec_hir::{
    BindingId, ComponentId, ExprId, HirApplication, HirArrayItem, HirBinaryOp, HirBinding,
    HirBindingKind, HirCallable, HirCallableBody, HirCallableDecl, HirComponent, HirComponentCall,
    HirComponentTarget, HirConditional, HirElement, HirEventBinding, HirExpr, HirExprNode,
    HirForEach, HirFragment, HirInput, HirListener, HirLocal, HirLogicalOp, HirNode, HirObjectItem,
    HirParameter, HirParameterSource, HirProp, HirReaction, HirRefSlot, HirSlot, HirState, HirStmt,
    HirTemplatePart, HirText, HirUnaryOp, HirValue, NodeId, SourceSpan,
};
use plec_ir::sink::is_safe_attribute_value;
use plec_model::{resolve_component, ComponentPropKind, SemanticGraph};
use swc_common::{Span, Spanned};
use swc_ecma_ast::{
    ArrowFunctionBody, BinaryOp, Callee, Decl, Expr, Function, JSXAttr, JSXAttrName,
    JSXAttrOrSpread, JSXAttrValue, JSXElement, JSXElementName, JSXExpr, JSXFragment, JSXObject,
    Lit, Pat, Prop, PropName, PropOrSpread, Stmt, TsFnOrConstructorType, TsType, VarDeclKind,
};

/// Normalize JSX event name to DOM event name.
fn normalize_event_name(jsx_name: &str) -> Option<String> {
    if jsx_name.starts_with("on") {
        let event = &jsx_name[2..];
        match event {
            "Click" => Some("click".to_string()),
            "Input" => Some("input".to_string()),
            "Submit" => Some("submit".to_string()),
            "Change" => Some("change".to_string()),
            "Blur" => Some("blur".to_string()),
            "Focus" => Some("focus".to_string()),
            "KeyDown" => Some("keydown".to_string()),
            "KeyUp" => Some("keyup".to_string()),
            "KeyPress" => Some("keypress".to_string()),
            "MouseDown" => Some("mousedown".to_string()),
            "MouseUp" => Some("mouseup".to_string()),
            "MouseMove" => Some("mousemove".to_string()),
            "MouseEnter" => Some("mouseenter".to_string()),
            "MouseLeave" => Some("mouseleave".to_string()),
            "DoubleClick" => Some("dblclick".to_string()),
            "ContextMenu" => Some("contextmenu".to_string()),
            "Wheel" => Some("wheel".to_string()),
            "DragStart" => Some("dragstart".to_string()),
            "Drag" => Some("drag".to_string()),
            "DragEnd" => Some("dragend".to_string()),
            "DragEnter" => Some("dragenter".to_string()),
            "DragLeave" => Some("dragleave".to_string()),
            "DragOver" => Some("dragover".to_string()),
            "Drop" => Some("drop".to_string()),
            "Scroll" => Some("scroll".to_string()),
            _ if event
                .chars()
                .next()
                .map(char::is_uppercase)
                .unwrap_or(false) =>
            {
                // Generic onXxx -> lowercase-xxx conversion (CamelCase to lowercase-with-dashes)
                let mut result = String::new();
                for (i, c) in event.chars().enumerate() {
                    if i == 0 {
                        result.extend(c.to_lowercase());
                    } else if c.is_uppercase() {
                        result.push('-');
                        result.extend(c.to_lowercase());
                    } else {
                        result.push(c);
                    }
                }
                Some(result)
            }
            _ => None,
        }
    } else {
        None
    }
}

#[derive(Clone, Copy)]
enum CallablePolicy {
    InlineOnly,
    CallableValue,
}

/// Lower a callable-shaped JSX value. `Reference` remains an expression-backed
/// placeholder until executable callable identity is introduced.
fn lower_callable(
    expr: &Expr,
    policy: CallablePolicy,
    ctx: &mut HirLoweringCtx<'_>,
) -> Result<Option<HirCallable>, String> {
    match expr {
        Expr::Ident(ident) if matches!(policy, CallablePolicy::CallableValue) => {
            let binding = ctx.resolve_binding(&ident.sym)?;
            if ctx.is_callable_binding(binding) {
                Ok(Some(HirCallable::Reference { binding }))
            } else if let Some(state) = ctx.setter_state(binding) {
                let span = source_span_from_swc(ident.span, ctx.module_id);
                ctx.push_scope();
                let parameter = ctx.declare_binding(
                    format!("__plec_setter_value_{}", ctx.bindings.len()),
                    HirBindingKind::Parameter { callable: false },
                    span.clone(),
                )?;
                let value = ctx.alloc_expr_id();
                ctx.add_expression(HirExprNode::new(
                    value,
                    HirExpr::Binding(parameter),
                    span.clone(),
                ));
                ctx.pop_scope();
                Ok(Some(HirCallable::Inline {
                    parameters: vec![parameter],
                    body: HirCallableBody::Block(vec![HirStmt::StateUpdate { state, value, span }]),
                }))
            } else {
                Err(format!("'{}' is not a callable binding", ident.sym))
            }
        }
        Expr::Arrow(arrow) => {
            let (parameters, body) = lower_arrow_callable(arrow, ctx)?;
            Ok(Some(HirCallable::Inline { parameters, body }))
        }
        Expr::Cond(cond) => {
            let test_id = lower_expression(&cond.test, ctx)?;
            let consequent = lower_callable(&cond.cons, policy, ctx)?;
            let alternate = lower_callable(&cond.alt, policy, ctx)?;
            match (consequent, alternate) {
                (Some(consequent), Some(alternate)) => Ok(Some(HirCallable::Conditional {
                    test: test_id,
                    consequent: Box::new(consequent),
                    alternate: Box::new(alternate),
                })),
                (None, None) if matches!(policy, CallablePolicy::InlineOnly) => Ok(None),
                (None, _) => Err("Conditional consequent must be callable".into()),
                (_, None) => Err("Conditional alternate must be callable".into()),
            }
        }
        _ if matches!(policy, CallablePolicy::CallableValue) => Err(
            "Callable values must be resolved identifiers, inline arrows, or conditionals"
                .to_string(),
        ),
        _ => Ok(None),
    }
}

/// HIR lowering context.
///
/// Maintains ID generation and node/expression storage while lowering.
#[derive(Debug, Clone)]
struct HirLoweringCtx<'a> {
    next_expr_id: u32,
    next_node_id: u32,
    next_binding_id: u32,
    expressions: Vec<HirExprNode>,
    nodes: Vec<HirNode>,
    bindings: Vec<HirBinding>,
    parameters: Vec<HirParameter>,
    inputs: Vec<HirInput>,
    locals: Vec<HirLocal>,
    states: Vec<HirState>,
    ref_slots: Vec<HirRefSlot>,
    reactions: Vec<HirReaction>,
    listeners: Vec<HirListener>,
    callables: Vec<HirCallableDecl>,
    component_aliases: std::collections::HashMap<String, ComponentId>,
    component_selectors: std::collections::HashMap<String, (ExprId, ComponentId, ComponentId)>,
    loader_data_binding: Option<BindingId>,
    scopes: Vec<std::collections::HashMap<String, BindingId>>,
    semantic_graph: &'a SemanticGraph,
    module_id: &'a str,
    /// Trusted custom element tags from the compiler configuration. The
    /// compile-time element diagnostics and the runtime element policy
    /// enforce the same identity rule (crates/plec-ir/src/sink.rs).
    custom_elements: std::collections::BTreeSet<String>,
}

impl<'a> HirLoweringCtx<'a> {
    fn new(
        semantic_graph: &'a SemanticGraph,
        module_id: &'a str,
        custom_elements: &std::collections::BTreeSet<String>,
    ) -> Self {
        Self {
            next_expr_id: 0,
            next_node_id: 0,
            next_binding_id: 0,
            expressions: Vec::new(),
            nodes: Vec::new(),
            bindings: Vec::new(),
            parameters: Vec::new(),
            inputs: Vec::new(),
            locals: Vec::new(),
            states: Vec::new(),
            ref_slots: Vec::new(),
            reactions: Vec::new(),
            listeners: Vec::new(),
            callables: Vec::new(),
            component_aliases: std::collections::HashMap::new(),
            component_selectors: std::collections::HashMap::new(),
            loader_data_binding: None,
            scopes: vec![std::collections::HashMap::new()],
            semantic_graph,
            module_id,
            custom_elements: custom_elements.clone(),
        }
    }

    fn alloc_expr_id(&mut self) -> ExprId {
        let id = ExprId(self.next_expr_id);
        self.next_expr_id += 1;
        id
    }

    fn alloc_node_id(&mut self) -> NodeId {
        let id = NodeId(self.next_node_id);
        self.next_node_id += 1;
        id
    }

    fn is_children_parameter(&self, binding: BindingId) -> bool {
        self.bindings
            .get(binding.0 as usize)
            .is_some_and(|binding| {
                binding.name == "children"
                    && matches!(binding.kind, HirBindingKind::Parameter { callable: false })
            })
    }

    fn declare_binding(
        &mut self,
        name: String,
        kind: HirBindingKind,
        span: SourceSpan,
    ) -> Result<BindingId, String> {
        let scope = self.scopes.last_mut().expect("scope stack is never empty");
        if scope.contains_key(&name) {
            return Err(format!("Duplicate binding '{name}'"));
        }
        let id = BindingId(self.next_binding_id);
        self.next_binding_id += 1;
        scope.insert(name.clone(), id);
        self.bindings.push(HirBinding {
            id,
            name,
            kind,
            span,
        });
        Ok(id)
    }

    fn resolve_binding(&self, name: &str) -> Result<BindingId, String> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
            .ok_or_else(|| format!("Unresolved lexical binding '{name}'"))
    }

    fn binding_kind(&self, id: BindingId) -> &HirBindingKind {
        &self.bindings[id.0 as usize].kind
    }

    fn is_callable_binding(&self, id: BindingId) -> bool {
        matches!(
            self.binding_kind(id),
            HirBindingKind::Callable | HirBindingKind::Parameter { callable: true }
        )
    }

    fn setter_state(&self, id: BindingId) -> Option<BindingId> {
        match self.binding_kind(id) {
            HirBindingKind::StateSetter { state } => Some(*state),
            _ => None,
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(std::collections::HashMap::new());
    }
    fn pop_scope(&mut self) {
        self.scopes.pop().expect("scope stack is never empty");
    }

    fn add_expression(&mut self, expr: HirExprNode) {
        self.expressions.push(expr);
    }

    fn add_node(&mut self, node: HirNode) -> NodeId {
        let id = self.alloc_node_id();
        self.nodes.push(node);
        id
    }
}

/// Lower a discovered root component to HIR.
pub fn lower_root_component(
    root: &RootComponent<'_>,
    semantic_graph: &SemanticGraph,
) -> Result<HirComponent, String> {
    lower_root_component_with_options(root, semantic_graph, &std::collections::BTreeSet::new())
}

/// Lower a discovered root component to HIR under explicit compiler options.
pub fn lower_root_component_with_options(
    root: &RootComponent<'_>,
    semantic_graph: &SemanticGraph,
    custom_elements: &std::collections::BTreeSet<String>,
) -> Result<HirComponent, String> {
    let mut ctx = HirLoweringCtx::new(semantic_graph, &root.symbol.module_id, custom_elements);
    let span = source_span_from_swc(root.symbol.span, &root.symbol.module_id);

    lower_component_program(&root.declaration, &root.symbol.local_name, &mut ctx)?;

    let root_node_id = match lower_returned_expression(&root.returned, &mut ctx)? {
        Some(node_id) => node_id,
        None => ctx.add_node(HirNode::Empty),
    };

    Ok(HirComponent {
        id: ComponentId::new(&root.symbol.module_id, &root.symbol.local_name),
        parameters: ctx.parameters,
        inputs: ctx.inputs,
        bindings: ctx.bindings,
        locals: ctx.locals,
        states: ctx.states,
        ref_slots: ctx.ref_slots,
        reactions: ctx.reactions,
        listeners: ctx.listeners,
        callables: ctx.callables,
        root_nodes: vec![root_node_id],
        nodes: ctx.nodes,
        expressions: ctx.expressions,
        span,
    })
}

pub fn lower_application(
    parsed_modules: &[plec_parser::ParsedModule],
    root: &RootComponent<'_>,
    semantic_graph: &SemanticGraph,
) -> Result<HirApplication, String> {
    lower_application_with_options(
        parsed_modules,
        root,
        semantic_graph,
        &std::collections::BTreeSet::new(),
    )
}

pub fn lower_application_with_options(
    parsed_modules: &[plec_parser::ParsedModule],
    root: &RootComponent<'_>,
    semantic_graph: &SemanticGraph,
    custom_elements: &std::collections::BTreeSet<String>,
) -> Result<HirApplication, String> {
    fn visit(
        parsed_modules: &[plec_parser::ParsedModule],
        root: &RootComponent<'_>,
        semantic_graph: &SemanticGraph,
        custom_elements: &std::collections::BTreeSet<String>,
        visiting: &mut Vec<ComponentId>,
        components: &mut Vec<HirComponent>,
    ) -> Result<(), String> {
        let id = ComponentId::new(&root.symbol.module_id, &root.symbol.local_name);
        if visiting.contains(&id) {
            return Err(format!(
                "Recursive component '{}' is unsupported",
                root.symbol.local_name
            ));
        }
        if components.iter().any(|component| component.id == id) {
            return Ok(());
        }
        visiting.push(id.clone());
        let component = lower_root_component_with_options(root, semantic_graph, custom_elements)?;
        let targets = component
            .nodes
            .iter()
            .flat_map(|node| match node {
                HirNode::Component(call) => {
                    let mut targets = Vec::new();
                    if let HirComponentTarget::Static(target) = &call.target {
                        targets.push(target.clone());
                    }
                    targets.extend(call.props.iter().filter_map(|prop| match prop {
                        HirProp::Component { target, .. }
                            if !target.module_id.starts_with("host:") =>
                        {
                            Some(target.clone())
                        }
                        _ => None,
                    }));
                    targets
                }
                _ => Vec::new(),
            })
            .collect::<Vec<_>>();
        components.push(component);
        for target in targets {
            let child = crate::discover_root_component(
                parsed_modules,
                semantic_graph,
                &target.module_id,
                Some(&target.local_name),
            )
            .map_err(|error| error.to_string())?;
            visit(
                parsed_modules,
                &child,
                semantic_graph,
                custom_elements,
                visiting,
                components,
            )?;
        }
        visiting.pop();
        Ok(())
    }

    let root_id = ComponentId::new(&root.symbol.module_id, &root.symbol.local_name);
    let mut components = Vec::new();
    visit(
        parsed_modules,
        root,
        semantic_graph,
        custom_elements,
        &mut Vec::new(),
        &mut components,
    )?;
    Ok(HirApplication {
        root: root_id,
        components,
    })
}

fn lower_component_program(
    declaration: &ComponentDeclaration<'_>,
    component_name: &str,
    ctx: &mut HirLoweringCtx<'_>,
) -> Result<(), String> {
    let body = match declaration {
        ComponentDeclaration::Function(decl) => {
            lower_component_function_parameters(&decl.function.params, component_name, ctx)?;
            decl.function.body.as_ref()
        }
        ComponentDeclaration::Arrow { arrow, .. } => {
            lower_component_pattern_parameters(&arrow.params, component_name, ctx)?;
            match arrow.body.as_ref() {
                ArrowFunctionBody::FunctionBody(body) => Some(body),
                ArrowFunctionBody::Expr(_) => None,
            }
        }
        ComponentDeclaration::FunctionExpression { function, .. } => {
            lower_component_function_parameters(&function.params, component_name, ctx)?;
            function.body.as_ref()
        }
    };
    let Some(body) = body else {
        return Ok(());
    };

    for stmt in &body.stmts {
        if let Stmt::Decl(Decl::Fn(function)) = stmt {
            let span = source_span_from_swc(function.function.span, ctx.module_id);
            ctx.declare_binding(
                function.ident.sym.to_string(),
                HirBindingKind::Callable,
                span,
            )?;
        }
    }

    for stmt in &body.stmts {
        match stmt {
            Stmt::Decl(Decl::Fn(function)) => {
                lower_function_callable(function.ident.sym.to_string(), &function.function, ctx)?
            }
            Stmt::Decl(Decl::Var(var)) => lower_component_var(var, ctx)?,
            Stmt::Expr(expression) => lower_component_primitive(expression.expr.as_ref(), ctx)?,
            Stmt::Return(_) => {}
            unsupported => return Err(format!("Unsupported component statement: {unsupported:?}")),
        }
    }
    Ok(())
}

fn lower_component_function_parameters(
    params: &[swc_ecma_ast::Param],
    component_name: &str,
    ctx: &mut HirLoweringCtx<'_>,
) -> Result<(), String> {
    lower_component_patterns(
        &params.iter().map(|param| &param.pat).collect::<Vec<_>>(),
        component_name,
        ctx,
    )
}

fn lower_component_pattern_parameters(
    params: &[Pat],
    component_name: &str,
    ctx: &mut HirLoweringCtx<'_>,
) -> Result<(), String> {
    lower_component_patterns(&params.iter().collect::<Vec<_>>(), component_name, ctx)
}

fn lower_component_patterns(
    params: &[&Pat],
    component_name: &str,
    ctx: &mut HirLoweringCtx<'_>,
) -> Result<(), String> {
    if params.len() > 1 {
        return Err("Components support at most one parameter".to_string());
    }
    let Some(param) = params.first() else {
        return Ok(());
    };
    let callable_props = ctx
        .semantic_graph
        .get_module(ctx.module_id)
        .and_then(|m| m.component_props.get(component_name));
    match *param {
        Pat::Ident(ident) => {
            let span = source_span_from_swc(ident.id.span, ctx.module_id);
            let binding = ctx.declare_binding(
                ident.id.sym.to_string(),
                HirBindingKind::Parameter { callable: false },
                span.clone(),
            )?;
            ctx.parameters.push(HirParameter {
                binding,
                source: HirParameterSource::Direct,
                span,
            });
        }
        Pat::Object(object) => {
            let rest = object.props.iter().find_map(|prop| match prop {
                swc_ecma_ast::ObjectPatProp::Rest(rest) => Some(rest),
                _ => None,
            });
            if let Some(rest) = rest {
                // A rest pattern is represented as one direct prop bag plus
                // ordinary local reads. This preserves a complete source-order
                // record for `{ ...props }` forwarding without inventing a JS
                // object runtime.
                let Pat::Ident(rest_ident) = rest.arg.as_ref() else {
                    return Err("Nested component rest patterns are unsupported".into());
                };
                let span = source_span_from_swc(rest_ident.id.span, ctx.module_id);
                let bag = ctx.declare_binding(
                    "__plec_props".into(),
                    HirBindingKind::Parameter { callable: false },
                    span.clone(),
                )?;
                ctx.parameters.push(HirParameter {
                    binding: bag,
                    source: HirParameterSource::Direct,
                    span: span.clone(),
                });
                let bag_expr = ctx.alloc_expr_id();
                ctx.add_expression(HirExprNode::new(
                    bag_expr,
                    HirExpr::Binding(bag),
                    span.clone(),
                ));
                let mut excluded = Vec::new();
                for prop in &object.props {
                    match prop {
                        swc_ecma_ast::ObjectPatProp::KeyValue(key_value) => {
                            let name = match &key_value.key {
                                swc_ecma_ast::PropName::Ident(key) => key.sym.to_string(),
                                _ => {
                                    return Err(
                                        "Component props must use identifier keys".to_string()
                                    )
                                }
                            };
                            let Pat::Ident(local) = key_value.value.as_ref() else {
                                return Err(
                                    "Nested component prop patterns are unsupported".to_string()
                                );
                            };
                            excluded.push(name.clone());
                            let local_span = source_span_from_swc(local.id.span, ctx.module_id);
                            let value = ctx.alloc_expr_id();
                            ctx.add_expression(HirExprNode::new(
                                value,
                                HirExpr::Member {
                                    object: bag_expr,
                                    property: name,
                                },
                                local_span.clone(),
                            ));
                            let binding = ctx.declare_binding(
                                local.id.sym.to_string(),
                                HirBindingKind::Local,
                                local_span.clone(),
                            )?;
                            ctx.locals.push(HirLocal {
                                binding,
                                initializer: value,
                                span: local_span,
                            });
                        }
                        swc_ecma_ast::ObjectPatProp::Assign(assign) => {
                            if assign.value.is_some() {
                                return Err(format!(
                                    "Default component parameter '{}' is unsupported",
                                    assign.key.sym
                                ));
                            }
                            let name = assign.key.sym.to_string();
                            excluded.push(name.clone());
                            let local_span = source_span_from_swc(assign.key.span, ctx.module_id);
                            let value = ctx.alloc_expr_id();
                            ctx.add_expression(HirExprNode::new(
                                value,
                                HirExpr::Member {
                                    object: bag_expr,
                                    property: name.clone(),
                                },
                                local_span.clone(),
                            ));
                            let binding = ctx.declare_binding(
                                name,
                                HirBindingKind::Local,
                                local_span.clone(),
                            )?;
                            ctx.locals.push(HirLocal {
                                binding,
                                initializer: value,
                                span: local_span,
                            });
                        }
                        swc_ecma_ast::ObjectPatProp::Rest(_) => {}
                    }
                }
                let rest_expr = ctx.alloc_expr_id();
                ctx.add_expression(HirExprNode::new(
                    rest_expr,
                    HirExpr::ObjectWithout {
                        object: bag_expr,
                        excluded,
                    },
                    span.clone(),
                ));
                let binding = ctx.declare_binding(
                    rest_ident.id.sym.to_string(),
                    HirBindingKind::Local,
                    span.clone(),
                )?;
                ctx.locals.push(HirLocal {
                    binding,
                    initializer: rest_expr,
                    span,
                });
                return Ok(());
            }
            for prop in &object.props {
                let swc_ecma_ast::ObjectPatProp::KeyValue(key_value) = prop else {
                    if let swc_ecma_ast::ObjectPatProp::Assign(assign) = prop {
                        if assign.value.is_some() {
                            return Err(format!(
                                "Default component parameter '{}' is unsupported",
                                assign.key.sym
                            ));
                        }
                        let name = assign.key.sym.to_string();
                        let callable = callable_props.and_then(|props| props.get(&name))
                            == Some(&plec_model::ComponentPropKind::Callable);
                        let span = source_span_from_swc(assign.key.span, ctx.module_id);
                        let binding = ctx.declare_binding(
                            name.clone(),
                            HirBindingKind::Parameter { callable },
                            span.clone(),
                        )?;
                        ctx.parameters.push(HirParameter {
                            binding,
                            source: HirParameterSource::Prop { name },
                            span,
                        });
                        continue;
                    }
                    return Err(format!(
                        "Unsupported component parameter pattern in {component_name}"
                    ));
                };
                let name = match &key_value.key {
                    swc_ecma_ast::PropName::Ident(key) => key.sym.to_string(),
                    _ => return Err("Component props must use identifier keys".to_string()),
                };
                let Pat::Ident(local) = key_value.value.as_ref() else {
                    return Err("Nested component prop patterns are unsupported".to_string());
                };
                let callable = callable_props.and_then(|props| props.get(&name))
                    == Some(&plec_model::ComponentPropKind::Callable);
                let span = source_span_from_swc(local.id.span, ctx.module_id);
                let binding = ctx.declare_binding(
                    local.id.sym.to_string(),
                    HirBindingKind::Parameter { callable },
                    span.clone(),
                )?;
                ctx.parameters.push(HirParameter {
                    binding,
                    source: HirParameterSource::Prop { name },
                    span,
                });
            }
        }
        _ => {
            return Err(format!(
                "Unsupported component parameter pattern in {component_name}"
            ))
        }
    }
    Ok(())
}

fn lower_component_var(
    var: &swc_ecma_ast::VarDecl,
    ctx: &mut HirLoweringCtx<'_>,
) -> Result<(), String> {
    if var.kind != VarDeclKind::Const || var.decls.len() != 1 {
        return Err("Component declarations must be single const declarations".to_string());
    }
    let declarator = &var.decls[0];
    let init = declarator
        .init
        .as_deref()
        .ok_or("Component declaration requires an initializer")?;
    if let (Pat::Ident(alias), Expr::Ident(target)) = (&declarator.name, init) {
        if let Some(symbol) = resolve_component(ctx.semantic_graph, ctx.module_id, &target.sym) {
            ctx.component_aliases.insert(
                alias.id.sym.to_string(),
                ComponentId::new(symbol.module_id, symbol.local_name),
            );
            return Ok(());
        }
    }
    if let (Pat::Ident(alias), Expr::Cond(selector)) = (&declarator.name, init) {
        if let (Expr::Ident(consequent), Expr::Ident(alternate)) =
            (selector.cons.as_ref(), selector.alt.as_ref())
        {
            let test = lower_expression(&selector.test, ctx)?;
            ctx.component_selectors.insert(
                alias.id.sym.to_string(),
                (
                    test,
                    ComponentId::new(ctx.module_id, consequent.sym.to_string()),
                    ComponentId::new(ctx.module_id, alternate.sym.to_string()),
                ),
            );
            return Ok(());
        }
        let resolve = |expression: &Expr, ctx: &HirLoweringCtx<'_>| match expression {
            Expr::Ident(ident) => resolve_component(ctx.semantic_graph, ctx.module_id, &ident.sym)
                .map(|symbol| ComponentId::new(symbol.module_id, symbol.local_name))
                .or_else(|| ctx.component_aliases.get(&ident.sym.to_string()).cloned())
                .or_else(|| {
                    ident
                        .sym
                        .chars()
                        .next()
                        .is_some_and(char::is_uppercase)
                        .then(|| ComponentId::new(ctx.module_id, ident.sym.to_string()))
                }),
            _ => None,
        };
        if let (Some(consequent), Some(alternate)) =
            (resolve(&selector.cons, ctx), resolve(&selector.alt, ctx))
        {
            let test = lower_expression(&selector.test, ctx)?;
            ctx.component_selectors
                .insert(alias.id.sym.to_string(), (test, consequent, alternate));
            return Ok(());
        }
    }
    match &declarator.name {
        Pat::Array(array) => lower_state_declaration(
            array,
            init,
            source_span_from_swc(var.span, ctx.module_id),
            ctx,
        ),
        Pat::Ident(ident) => match init {
            Expr::Call(call) if matches!(&call.callee, Callee::Expr(callee) if matches!(callee.as_ref(), Expr::Ident(callee) if callee.sym == "useRef")) =>
            {
                if call.args.len() != 1 || call.args[0].spread.is_some() {
                    return Err("useRef requires one non-spread initializer".into());
                }
                let span = source_span_from_swc(ident.id.span, ctx.module_id);
                let initializer = lower_expression(&call.args[0].expr, ctx)?;
                let binding = ctx.declare_binding(
                    ident.id.sym.to_string(),
                    HirBindingKind::RefSlot,
                    span.clone(),
                )?;
                ctx.ref_slots.push(HirRefSlot {
                    binding,
                    initializer,
                    span,
                });
                Ok(())
            }
            Expr::Call(call) if matches!(&call.callee, Callee::Expr(callee) if matches!(callee.as_ref(), Expr::Ident(callee) if callee.sym == "useHostRef")) =>
            {
                if !call.args.is_empty() {
                    return Err("useHostRef requires no arguments".into());
                }
                let span = source_span_from_swc(ident.id.span, ctx.module_id);
                ctx.declare_binding(ident.id.sym.to_string(), HirBindingKind::HostRef, span)?;
                Ok(())
            }
            Expr::Call(call) if matches!(&call.callee, Callee::Expr(callee) if matches!(callee.as_ref(), Expr::Ident(callee) if callee.sym == "useLocation")) =>
            {
                if !call.args.is_empty() {
                    return Err("useLocation requires no arguments".to_string());
                }
                let span = source_span_from_swc(ident.id.span, ctx.module_id);
                let binding = ctx.declare_binding(
                    ident.id.sym.to_string(),
                    HirBindingKind::Input {
                        kind: "location".to_string(),
                    },
                    span.clone(),
                )?;
                ctx.inputs.push(HirInput {
                    binding,
                    name: "location".to_string(),
                    kind: "location".to_string(),
                    span,
                });
                Ok(())
            }
            Expr::Call(call) if matches!(&call.callee, Callee::Expr(callee) if matches!(callee.as_ref(), Expr::Ident(callee) if callee.sym == "useCollection")) =>
            {
                if call.args.len() != 1 || call.args[0].spread.is_some() {
                    return Err("useCollection requires one static name".to_string());
                }
                let Expr::Lit(Lit::Str(name)) = call.args[0].expr.as_ref() else {
                    return Err("useCollection requires one static name".to_string());
                };
                let span = source_span_from_swc(ident.id.span, ctx.module_id);
                let binding = ctx.declare_binding(
                    ident.id.sym.to_string(),
                    HirBindingKind::Input {
                        kind: "collection".to_string(),
                    },
                    span.clone(),
                )?;
                ctx.inputs.push(HirInput {
                    binding,
                    name: name.value.to_string_lossy().into_owned(),
                    kind: "collection".to_string(),
                    span,
                });
                Ok(())
            }
            Expr::Arrow(arrow) => {
                let span = source_span_from_swc(ident.id.span, ctx.module_id);
                let binding = ctx.declare_binding(
                    ident.id.sym.to_string(),
                    HirBindingKind::Callable,
                    span.clone(),
                )?;
                let (parameters, body) = lower_arrow_callable(arrow, ctx)?;
                ctx.callables.push(HirCallableDecl {
                    binding,
                    parameters,
                    body,
                    span,
                });
                Ok(())
            }
            Expr::Fn(function) => {
                let span = source_span_from_swc(ident.id.span, ctx.module_id);
                let binding = ctx.declare_binding(
                    ident.id.sym.to_string(),
                    HirBindingKind::Callable,
                    span.clone(),
                )?;
                let (parameters, body) = lower_function_callable_body(&function.function, ctx)?;
                ctx.callables.push(HirCallableDecl {
                    binding,
                    parameters,
                    body,
                    span,
                });
                Ok(())
            }
            _ => {
                let initializer = lower_expression(init, ctx)?;
                let span = source_span_from_swc(ident.id.span, ctx.module_id);
                let binding = ctx.declare_binding(
                    ident.id.sym.to_string(),
                    HirBindingKind::Local,
                    span.clone(),
                )?;
                ctx.locals.push(HirLocal {
                    binding,
                    initializer,
                    span,
                });
                Ok(())
            }
        },
        _ => Err("Unsupported component declaration pattern".to_string()),
    }
}

fn component_target_from_symbol(symbol: &plec_model::ResolvedSymbol) -> HirComponentTarget {
    if let Some(provider) = symbol.module_id.strip_prefix("host:") {
        HirComponentTarget::Host {
            provider: provider.to_string(),
            component: symbol.local_name.clone(),
        }
    } else {
        HirComponentTarget::Static(ComponentId::new(
            symbol.module_id.clone(),
            symbol.local_name.clone(),
        ))
    }
}

fn component_target_from_id(id: &ComponentId) -> HirComponentTarget {
    if let Some(provider) = id.module_id.strip_prefix("host:") {
        HirComponentTarget::Host {
            provider: provider.to_string(),
            component: id.local_name.clone(),
        }
    } else {
        HirComponentTarget::Static(id.clone())
    }
}

fn lower_state_declaration(
    array: &swc_ecma_ast::ArrayPat,
    init: &Expr,
    span: SourceSpan,
    ctx: &mut HirLoweringCtx<'_>,
) -> Result<(), String> {
    let Expr::Call(call) = init else {
        return Err("Array declarations are only supported for useState".to_string());
    };
    let Callee::Expr(callee) = &call.callee else {
        return Err("State initializer must call useState".to_string());
    };
    if !matches!(callee.as_ref(), Expr::Ident(ident) if ident.sym == "useState") {
        return Err("Only direct useState(initializer) is supported".to_string());
    }
    if call.args.len() != 1 || call.args[0].spread.is_some() {
        return Err("useState requires one non-spread initializer".to_string());
    }
    if array.elems.len() != 2 {
        return Err("useState destructuring requires [value, setter]".to_string());
    }
    let value = array.elems[0]
        .as_ref()
        .and_then(|p| match p {
            Pat::Ident(i) => Some(i),
            _ => None,
        })
        .ok_or("useState value must be an identifier")?;
    let setter = array.elems[1]
        .as_ref()
        .and_then(|p| match p {
            Pat::Ident(i) => Some(i),
            _ => None,
        })
        .ok_or("useState setter must be an identifier")?;
    let initializer = lower_expression(&call.args[0].expr, ctx)?;
    let value_binding = ctx.declare_binding(
        value.id.sym.to_string(),
        HirBindingKind::StateValue,
        source_span_from_swc(value.id.span, ctx.module_id),
    )?;
    let setter_binding = ctx.declare_binding(
        setter.id.sym.to_string(),
        HirBindingKind::StateSetter {
            state: value_binding,
        },
        source_span_from_swc(setter.id.span, ctx.module_id),
    )?;
    ctx.states.push(HirState {
        value: value_binding,
        setter: setter_binding,
        initializer,
        span,
    });
    Ok(())
}

fn lower_function_callable(
    name: String,
    function: &Function,
    ctx: &mut HirLoweringCtx<'_>,
) -> Result<(), String> {
    let binding = ctx.resolve_binding(&name)?;
    let span = source_span_from_swc(function.span, ctx.module_id);
    let (parameters, body) = lower_function_callable_body(function, ctx)?;
    ctx.callables.push(HirCallableDecl {
        binding,
        parameters,
        body,
        span,
    });
    Ok(())
}

fn lower_function_callable_body(
    function: &Function,
    ctx: &mut HirLoweringCtx<'_>,
) -> Result<(Vec<BindingId>, HirCallableBody), String> {
    let body = function.body.as_ref().ok_or("Callable requires a body")?;
    ctx.push_scope();
    let parameters = function
        .params
        .iter()
        .map(|param| lower_callable_parameter(&param.pat, ctx))
        .collect::<Result<Vec<_>, _>>()?;
    let statements = lower_callable_statements(&body.stmts, ctx)?;
    ctx.pop_scope();
    Ok((parameters, HirCallableBody::Block(statements)))
}

fn lower_arrow_callable(
    arrow: &swc_ecma_ast::ArrowExpr,
    ctx: &mut HirLoweringCtx<'_>,
) -> Result<(Vec<BindingId>, HirCallableBody), String> {
    ctx.push_scope();
    let parameters = arrow
        .params
        .iter()
        .map(|param| lower_callable_parameter(param, ctx))
        .collect::<Result<Vec<_>, _>>()?;
    let body = match arrow.body.as_ref() {
        ArrowFunctionBody::Expr(expr) => {
            if let Expr::Assign(assign) = expr.as_ref() {
                if let swc_ecma_ast::AssignTarget::Simple(
                    swc_ecma_ast::SimpleAssignTarget::Member(member),
                ) = &assign.left
                {
                    if let (Expr::Ident(reference), swc_ecma_ast::MemberProp::Ident(property)) =
                        (member.obj.as_ref(), &member.prop)
                    {
                        if property.sym == *"current" {
                            let binding = ctx.resolve_binding(&reference.sym)?;
                            if matches!(
                                ctx.bindings[binding.0 as usize].kind,
                                HirBindingKind::RefSlot
                            ) {
                                HirCallableBody::Block(vec![HirStmt::RefUpdate {
                                    reference: binding,
                                    value: lower_expression(&assign.right, ctx)?,
                                    span: source_span_from_swc(assign.span, ctx.module_id),
                                }])
                            } else {
                                return Err("only useRef.current can be assigned".into());
                            }
                        } else {
                            return Err("only ref.current assignment is supported".into());
                        }
                    } else {
                        return Err("only ref.current assignment is supported".into());
                    }
                } else {
                    return Err("only ref.current assignment is supported".into());
                }
            } else if let Expr::Call(call) = expr.as_ref() {
                if let Callee::Expr(callee) = &call.callee {
                    if let Expr::Ident(setter) = callee.as_ref() {
                        let binding = ctx.resolve_binding(&setter.sym)?;
                        if let Some(state) = ctx.setter_state(binding) {
                            if call.args.len() != 1 || call.args[0].spread.is_some() {
                                return Err("State setters require one non-spread value".into());
                            }
                            HirCallableBody::Block(vec![HirStmt::StateUpdate {
                                state,
                                value: lower_state_update_value(&call.args[0].expr, state, ctx)?,
                                span: source_span_from_swc(call.span, ctx.module_id),
                            }])
                        } else {
                            HirCallableBody::Expression(lower_expression(expr, ctx)?)
                        }
                    } else {
                        HirCallableBody::Expression(lower_expression(expr, ctx)?)
                    }
                } else {
                    HirCallableBody::Expression(lower_expression(expr, ctx)?)
                }
            } else {
                HirCallableBody::Expression(lower_expression(expr, ctx)?)
            }
        }
        ArrowFunctionBody::FunctionBody(body) => {
            HirCallableBody::Block(lower_callable_statements(&body.stmts, ctx)?)
        }
    };
    ctx.pop_scope();
    Ok((parameters, body))
}

fn lower_callable_parameter(
    pattern: &Pat,
    ctx: &mut HirLoweringCtx<'_>,
) -> Result<BindingId, String> {
    let Pat::Ident(ident) = pattern else {
        return Err("Callable parameters must be identifiers".to_string());
    };
    let span = source_span_from_swc(ident.id.span, ctx.module_id);
    let callable = matches!(
        ident
            .type_ann
            .as_deref()
            .map(|annotation| annotation.type_ann.as_ref()),
        Some(TsType::TsFnOrConstructorType(
            TsFnOrConstructorType::TsFnType(_)
        ))
    );
    ctx.declare_binding(
        ident.id.sym.to_string(),
        HirBindingKind::Parameter { callable },
        span,
    )
}

fn lower_callable_statements(
    stmts: &[Stmt],
    ctx: &mut HirLoweringCtx<'_>,
) -> Result<Vec<HirStmt>, String> {
    stmts
        .iter()
        .map(|stmt| lower_callable_statement(stmt, ctx))
        .collect()
}

fn lower_awaited_call(
    awaited: &swc_ecma_ast::AwaitExpr,
    target: Option<BindingId>,
    span: SourceSpan,
    ctx: &mut HirLoweringCtx<'_>,
) -> Result<HirStmt, String> {
    let Expr::Call(call) = awaited.arg.as_ref() else {
        return Err("await must call fetch or a local action".into());
    };
    let Callee::Expr(callee) = &call.callee else {
        return Err("await must call fetch or a local action".into());
    };
    if let Expr::Member(member) = callee.as_ref() {
        if matches!(&member.prop, swc_ecma_ast::MemberProp::Ident(name) if name.sym == "json" || name.sym == "text")
            && call.args.is_empty()
        {
            let target = target.ok_or("awaited response body must be assigned")?;
            let object = lower_expression(&member.obj, ctx)?;
            let value = ctx.alloc_expr_id();
            ctx.add_expression(HirExprNode::new(
                value,
                HirExpr::Member {
                    object,
                    property: "body".into(),
                },
                span.clone(),
            ));
            return Ok(HirStmt::AsyncAssign {
                target,
                value,
                span,
            });
        }
    }
    if let Some(cookie) = lower_cookie_call(call, target, span.clone(), ctx)? {
        return Ok(cookie);
    }
    if matches!(callee.as_ref(), Expr::Ident(ident) if ident.sym == *"fetch") {
        return lower_awaited_fetch(call, target, span, ctx);
    }
    // A local async helper may receive a finite inline callback such as
    // `request("create", () => fetch(...))`. The callback never escapes the
    // graph, so lower its known fetch continuation directly instead of treating
    // it as an arbitrary JavaScript function value.
    if let Some(fetch) = call.args.iter().find_map(|argument| match argument.expr.as_ref() {
        Expr::Arrow(arrow) if arrow.params.is_empty() => match arrow.body.as_ref() {
            ArrowFunctionBody::Expr(body) => match body.as_ref() {
                Expr::Call(fetch)
                    if matches!(&fetch.callee, Callee::Expr(callee) if matches!(callee.as_ref(), Expr::Ident(ident) if ident.sym == *"fetch")) =>
                {
                    Some(fetch)
                }
                _ => None,
            },
            ArrowFunctionBody::FunctionBody(_) => None,
        },
        _ => None,
    }) {
        return lower_awaited_fetch(fetch, target, span, ctx);
    }
    let Expr::Ident(callee) = callee.as_ref() else {
        return Err("await must call fetch or a local action".into());
    };
    let callee = ctx.resolve_binding(&callee.sym)?;
    if !ctx.is_callable_binding(callee) || call.args.iter().any(|arg| arg.spread.is_some()) {
        return Err("awaited calls require a local action and non-spread arguments".into());
    }
    Ok(HirStmt::AwaitCall {
        target,
        callee,
        arguments: call
            .args
            .iter()
            .map(|arg| lower_expression(&arg.expr, ctx))
            .collect::<Result<_, _>>()?,
        span,
    })
}

fn lower_awaited_fetch(
    call: &swc_ecma_ast::CallExpr,
    target: Option<BindingId>,
    span: SourceSpan,
    ctx: &mut HirLoweringCtx<'_>,
) -> Result<HirStmt, String> {
    if call.args.is_empty()
        || call.args.len() > 2
        || call.args.iter().any(|arg| arg.spread.is_some())
    {
        return Err("fetch requires a URL and optional static method".into());
    }
    let mut method = "GET".to_string();
    let mut headers = Vec::new();
    let mut body = None;
    if let Some(options) = call.args.get(1) {
        let Expr::Object(options) = options.expr.as_ref() else {
            return Err("fetch options must be a static object".into());
        };
        for option in &options.props {
            let swc_ecma_ast::PropOrSpread::Prop(option) = option else {
                return Err("fetch options cannot use object spread".into());
            };
            let swc_ecma_ast::Prop::KeyValue(option) = option.as_ref() else {
                return Err("fetch options must use named properties".into());
            };
            let name = match &option.key {
                swc_ecma_ast::PropName::Ident(name) => name.sym.to_string(),
                swc_ecma_ast::PropName::Str(name) => name.value.to_string_lossy().into_owned(),
                _ => return Err("fetch option names must be static".into()),
            };
            match name.as_str() {
                "method" => match option.value.as_ref() {
                    Expr::Lit(Lit::Str(value)) => {
                        method = value.value.to_string_lossy().into_owned()
                    }
                    _ => return Err("fetch method must be a static string".into()),
                },
                "headers" => {
                    let Expr::Object(object) = option.value.as_ref() else {
                        return Err("fetch headers must be a static object".into());
                    };
                    for header in &object.props {
                        let swc_ecma_ast::PropOrSpread::Prop(header) = header else {
                            return Err("fetch headers cannot use object spread".into());
                        };
                        let swc_ecma_ast::Prop::KeyValue(header) = header.as_ref() else {
                            return Err("fetch headers must use named properties".into());
                        };
                        let header_name = match &header.key {
                            swc_ecma_ast::PropName::Ident(name) => name.sym.to_string(),
                            swc_ecma_ast::PropName::Str(name) => {
                                name.value.to_string_lossy().into_owned()
                            }
                            _ => return Err("fetch header names must be static".into()),
                        };
                        headers.push((header_name, lower_expression(&header.value, ctx)?));
                    }
                }
                "body" => body = Some(lower_expression(&option.value, ctx)?),
                // Signals are attached by the browser runtime's capability lifecycle.
                "signal" => {}
                _ => return Err(format!("unsupported fetch option '{name}'")),
            }
        }
    }
    Ok(HirStmt::AwaitFetch {
        target,
        url: lower_expression(&call.args[0].expr, ctx)?,
        method,
        headers,
        body,
        decode: if target.is_some() {
            "responseJson"
        } else {
            "text"
        }
        .to_string(),
        span,
    })
}

fn lower_component_primitive(expr: &Expr, ctx: &mut HirLoweringCtx<'_>) -> Result<(), String> {
    let Expr::Call(call) = expr else {
        return Err(
            "Only useReaction and useListener calls are valid component expressions".into(),
        );
    };
    let Callee::Expr(callee) = &call.callee else {
        return Err("Unsupported component call".into());
    };
    let Expr::Ident(name) = callee.as_ref() else {
        return Err("Unsupported component call".into());
    };
    let span = source_span_from_swc(call.span, ctx.module_id);
    if name.sym == *"useEffect" {
        return Err("useEffect is unsupported; use useReaction or useListener".into());
    }
    if name.sym == *"useReaction" {
        if call.args.len() != 2 || call.args.iter().any(|arg| arg.spread.is_some()) {
            return Err(
                "useReaction requires a callback and a non-empty static dependency array".into(),
            );
        }
        let Expr::Arrow(callback) = call.args[0].expr.as_ref() else {
            return Err("useReaction callback must be an inline arrow".into());
        };
        if !callback.params.is_empty() {
            return Err("useReaction callback cannot accept parameters".into());
        }
        let Expr::Array(deps) = call.args[1].expr.as_ref() else {
            return Err("useReaction dependencies must be a static non-empty array".into());
        };
        if deps.elems.is_empty() {
            return Err("useReaction requires at least one dependency".into());
        }
        let dependencies = deps
            .elems
            .iter()
            .map(|dep| {
                let dep = dep
                    .as_ref()
                    .ok_or("useReaction dependencies cannot contain holes")?;
                if dep.spread.is_some() {
                    return Err("useReaction dependencies cannot use spread".into());
                }
                lower_expression(&dep.expr, ctx)
            })
            .collect::<Result<Vec<_>, String>>()?;
        let (body, cleanup) = lower_reaction_callback(callback, ctx)?;
        ctx.reactions.push(HirReaction {
            dependencies,
            body,
            cleanup,
            span,
        });
        return Ok(());
    }
    if name.sym == *"useListener" {
        if call.args.len() != 3 || call.args.iter().any(|arg| arg.spread.is_some()) {
            return Err("useListener requires target, static event name, and handler".into());
        }
        let Expr::Ident(source) = call.args[0].expr.as_ref() else {
            return Err("useListener target must be window or document".into());
        };
        if !matches!(source.sym.as_ref(), "window" | "document") {
            return Err("useListener target must be window or document".into());
        }
        let Expr::Lit(Lit::Str(event)) = call.args[1].expr.as_ref() else {
            return Err("useListener event name must be static".into());
        };
        let callable = lower_callable(&call.args[2].expr, CallablePolicy::CallableValue, ctx)?
            .ok_or("useListener handler must be callable")?;
        ctx.listeners.push(HirListener {
            source: source.sym.to_string(),
            event: event.value.to_string_lossy().into_owned(),
            callable,
            span,
        });
        return Ok(());
    }
    Err(format!("Unsupported component primitive '{}'", name.sym))
}

fn lower_reaction_callback(
    callback: &swc_ecma_ast::ArrowExpr,
    ctx: &mut HirLoweringCtx<'_>,
) -> Result<(HirCallableBody, Option<HirCallableBody>), String> {
    if !callback.params.is_empty() {
        return Err("useReaction callback cannot accept parameters".into());
    }
    let ArrowFunctionBody::FunctionBody(block) = callback.body.as_ref() else {
        return Err("useReaction callback requires a block body".into());
    };
    ctx.push_scope();
    let mut statements = Vec::new();
    let mut cleanup = None;
    for (index, statement) in block.stmts.iter().enumerate() {
        if let Stmt::Return(returned) = statement {
            if index + 1 != block.stmts.len() {
                ctx.pop_scope();
                return Err("useReaction cleanup must be the final return".into());
            }
            let Some(Expr::Arrow(cleanup_arrow)) = returned.arg.as_deref() else {
                ctx.pop_scope();
                return Err("useReaction may return only a cleanup arrow".into());
            };
            let (parameters, body) = lower_arrow_callable(cleanup_arrow, ctx)?;
            if !parameters.is_empty() {
                ctx.pop_scope();
                return Err("useReaction cleanup cannot accept parameters".into());
            }
            cleanup = Some(body);
        } else {
            statements.push(lower_callable_statement(statement, ctx)?);
        }
    }
    ctx.pop_scope();
    Ok((HirCallableBody::Block(statements), cleanup))
}

fn lower_cookie_call(
    call: &swc_ecma_ast::CallExpr,
    target: Option<BindingId>,
    span: SourceSpan,
    ctx: &mut HirLoweringCtx<'_>,
) -> Result<Option<HirStmt>, String> {
    let Callee::Expr(callee) = &call.callee else {
        return Ok(None);
    };
    let Expr::Member(member) = callee.as_ref() else {
        return Ok(None);
    };
    let Expr::Ident(object) = member.obj.as_ref() else {
        return Ok(None);
    };
    if object.sym != *"cookie" {
        return Ok(None);
    }
    let swc_ecma_ast::MemberProp::Ident(property) = &member.prop else {
        return Err("cookie operation must be a named method".into());
    };
    let operation = property.sym.to_string();
    if !matches!(operation.as_str(), "get" | "set" | "delete") {
        return Err("cookie supports get, set, and delete actions".into());
    }
    if call.args.iter().any(|argument| argument.spread.is_some()) {
        return Err("cookie arguments cannot use spread".into());
    }
    let expected = if operation == "set" { 2..=3 } else { 1..=2 };
    if !expected.contains(&call.args.len()) {
        return Err(format!("cookie.{operation} has invalid arguments"));
    }
    let Expr::Lit(Lit::Str(name)) = call.args[0].expr.as_ref() else {
        return Err(format!("cookie.{operation} requires a static cookie name"));
    };
    let value = if operation == "set" {
        Some(lower_expression(&call.args[1].expr, ctx)?)
    } else {
        None
    };
    let options = if operation == "set" {
        call.args.get(2)
    } else {
        call.args.get(1)
    };
    let (path, same_site, secure, max_age) =
        lower_cookie_options(options.map(|argument| argument.expr.as_ref()))?;
    Ok(Some(HirStmt::AwaitCookie {
        target,
        operation,
        name: name.value.to_string_lossy().into_owned(),
        value,
        path,
        same_site,
        secure,
        max_age,
        span,
    }))
}

fn lower_cookie_options(
    value: Option<&Expr>,
) -> Result<(String, Option<String>, Option<bool>, Option<i64>), String> {
    let Some(value) = value else {
        return Ok(("/".into(), None, None, None));
    };
    let Expr::Object(object) = value else {
        return Err("cookie options must be a static object".into());
    };
    let mut path = "/".to_string();
    let mut same_site = None;
    let mut secure = None;
    let mut max_age = None;
    for property in &object.props {
        let PropOrSpread::Prop(property) = property else {
            return Err("cookie options cannot use spread".into());
        };
        let Prop::KeyValue(property) = property.as_ref() else {
            return Err("cookie options must use named properties".into());
        };
        let key = match &property.key {
            PropName::Ident(value) => value.sym.as_ref(),
            PropName::Str(value) => value
                .value
                .as_str()
                .ok_or("cookie option key must be UTF-8")?,
            _ => return Err("cookie options must use static names".into()),
        };
        match (key, property.value.as_ref()) {
            ("path", Expr::Lit(Lit::Str(value))) => {
                path = value.value.to_string_lossy().into_owned()
            }
            ("sameSite", Expr::Lit(Lit::Str(value))) => {
                let value = value.value.to_string_lossy().into_owned();
                if !matches!(value.as_str(), "lax" | "strict" | "none") {
                    return Err("cookie.sameSite must be lax, strict, or none".into());
                }
                same_site = Some(value);
            }
            ("secure", Expr::Lit(Lit::Bool(value))) => secure = Some(value.value),
            ("maxAge", Expr::Lit(Lit::Num(value)))
                if value.value.is_finite()
                    && value.value.fract() == 0.0
                    && value.value >= i64::MIN as f64
                    && value.value <= i64::MAX as f64 =>
            {
                max_age = Some(value.value as i64)
            }
            ("path" | "sameSite" | "secure" | "maxAge", _) => {
                return Err(
                    "cookie options must be static path, sameSite, secure, and maxAge values"
                        .into(),
                )
            }
            _ => return Err("unsupported cookie option".into()),
        }
    }
    Ok((path, same_site, secure, max_age))
}

fn lower_async_variable(
    var: &swc_ecma_ast::VarDecl,
    ctx: &mut HirLoweringCtx<'_>,
) -> Result<HirStmt, String> {
    if var.decls.len() != 1 || var.kind == VarDeclKind::Var {
        return Err("async result declarations require one let or const identifier".into());
    }
    let declaration = &var.decls[0];
    let Pat::Ident(name) = &declaration.name else {
        return Err("async result declarations require an identifier".into());
    };
    let span = source_span_from_swc(var.span, ctx.module_id);
    let target = ctx.declare_binding(
        name.id.sym.to_string(),
        HirBindingKind::AsyncValue,
        source_span_from_swc(name.id.span, ctx.module_id),
    )?;
    match declaration.init.as_deref().map(erase_type_assertions) {
        Some(Expr::Await(awaited)) => lower_awaited_call(awaited, Some(target), span, ctx),
        Some(initializer) => Ok(HirStmt::AsyncAssign {
            target,
            value: lower_expression(initializer, ctx)?,
            span,
        }),
        None => Err("callable declarations require an initializer".into()),
    }
}

fn erase_type_assertions(expression: &Expr) -> &Expr {
    match expression {
        Expr::TsAs(assertion) => erase_type_assertions(&assertion.expr),
        Expr::TsTypeAssertion(assertion) => erase_type_assertions(&assertion.expr),
        Expr::Paren(parenthesized) => erase_type_assertions(&parenthesized.expr),
        expression => expression,
    }
}

fn lower_callable_statement(stmt: &Stmt, ctx: &mut HirLoweringCtx<'_>) -> Result<HirStmt, String> {
    let span = source_span_from_swc(stmt.span(), ctx.module_id);
    match stmt {
        Stmt::Decl(Decl::Var(var)) => lower_async_variable(var, ctx),
        Stmt::Expr(expr_stmt) => {
            if let Expr::Assign(assign) = expr_stmt.expr.as_ref() {
                if let swc_ecma_ast::AssignTarget::Simple(
                    swc_ecma_ast::SimpleAssignTarget::Member(member),
                ) = &assign.left
                {
                    if let (Expr::Ident(reference), swc_ecma_ast::MemberProp::Ident(property)) =
                        (member.obj.as_ref(), &member.prop)
                    {
                        if property.sym == *"current" {
                            let binding = ctx.resolve_binding(&reference.sym)?;
                            if matches!(
                                ctx.bindings[binding.0 as usize].kind,
                                HirBindingKind::RefSlot
                            ) {
                                if matches!(assign.right.as_ref(), Expr::Member(right) if matches!(right.obj.as_ref(), Expr::Ident(document) if document.sym == *"document") && matches!(right.prop, swc_ecma_ast::MemberProp::Ident(ref property) if property.sym == *"activeElement"))
                                {
                                    return Ok(HirStmt::CaptureActiveElement {
                                        reference: binding,
                                        span,
                                    });
                                }
                                return Ok(HirStmt::RefUpdate {
                                    reference: binding,
                                    value: lower_expression(&assign.right, ctx)?,
                                    span,
                                });
                            }
                            if matches!(
                                ctx.bindings[binding.0 as usize].kind,
                                HirBindingKind::HostRef
                            ) {
                                return Err(
                                    "hostRef.current is lifecycle-owned and cannot be assigned"
                                        .into(),
                                );
                            }
                        }
                    }
                }
                return Err("only ref.current assignment is supported".into());
            }
            if let Expr::Await(awaited) = expr_stmt.expr.as_ref() {
                return lower_awaited_call(awaited, None, span, ctx);
            }
            let expression = match expr_stmt.expr.as_ref() {
                Expr::Unary(unary) if matches!(unary.op, swc_ecma_ast::UnaryOp::Void) => {
                    unary.arg.as_ref()
                }
                expression => expression,
            };
            // SWC wraps `ref.current?.focus()` in nested OptChain nodes rather
            // than a normal CallExpr.  It is still the one typed focus primitive
            // we permit; keep it out of ordinary expression lowering.
            if let Some(reference) = optional_focus_reference(expression, ctx)? {
                return match ctx.bindings[reference.0 as usize].kind {
                    HirBindingKind::HostRef => Ok(HirStmt::FocusHostRef {
                        reference,
                        optional: true,
                        span,
                    }),
                    HirBindingKind::RefSlot => Ok(HirStmt::FocusRef {
                        reference,
                        optional: true,
                        span,
                    }),
                    _ => Err("focus() requires a useHostRef or captured active-element ref".into()),
                };
            }
            if let Expr::OptChain(chain) = expression {
                if let swc_ecma_ast::OptChainBase::Call(call) = chain.base.as_ref() {
                    if let Expr::Ident(callee) = call.callee.as_ref() {
                        let binding = ctx.resolve_binding(&callee.sym)?;
                        if ctx.is_callable_binding(binding)
                            && call.args.iter().all(|argument| argument.spread.is_none())
                        {
                            return Ok(HirStmt::OptionalCall {
                                callee: binding,
                                arguments: call
                                    .args
                                    .iter()
                                    .map(|argument| lower_expression(&argument.expr, ctx))
                                    .collect::<Result<Vec<_>, _>>()?,
                                span,
                            });
                        }
                    }
                }
            }
            if let Expr::Call(call) = expression {
                if let Callee::Expr(callee) = &call.callee {
                    if let Expr::Member(method) = callee.as_ref() {
                        if matches!(&method.prop, swc_ecma_ast::MemberProp::Ident(name) if name.sym == *"preventDefault")
                            && call.args.is_empty()
                        {
                            return Ok(HirStmt::PreventDefault { span });
                        }
                        if matches!(&method.prop, swc_ecma_ast::MemberProp::Ident(name) if name.sym == *"focus")
                            && call.args.is_empty()
                        {
                            if let Expr::Member(current) = method.obj.as_ref() {
                                if let (
                                    Expr::Ident(reference),
                                    swc_ecma_ast::MemberProp::Ident(property),
                                ) = (current.obj.as_ref(), &current.prop)
                                {
                                    if property.sym == *"current" {
                                        let binding = ctx.resolve_binding(&reference.sym)?;
                                        return match ctx.bindings[binding.0 as usize].kind {
                                            HirBindingKind::HostRef => Ok(HirStmt::FocusHostRef { reference: binding, optional: true, span }),
                                            HirBindingKind::RefSlot => Ok(HirStmt::FocusRef { reference: binding, optional: true, span }),
                                            _ => Err("focus() requires a useHostRef or captured active-element ref".into()),
                                        };
                                    }
                                }
                            }
                        }
                    }
                }
                if let Some(cookie) = lower_cookie_call(call, None, span.clone(), ctx)? {
                    return Ok(cookie);
                }
            }
            if let Expr::Call(call) = expression {
                if let Callee::Expr(callee) = &call.callee {
                    if let Expr::Ident(ident) = callee.as_ref() {
                        let binding = ctx.resolve_binding(&ident.sym)?;
                        if let Some(state) = ctx.setter_state(binding) {
                            if call.args.len() != 1 || call.args[0].spread.is_some() {
                                return Err(
                                    "State setters require one non-spread value".to_string()
                                );
                            }
                            let value =
                                lower_state_update_value(call.args[0].expr.as_ref(), state, ctx)?;
                            return Ok(HirStmt::StateUpdate { state, value, span });
                        }
                    }
                }
            }
            Ok(HirStmt::Expression {
                expression: lower_expression(expression, ctx)?,
                span,
            })
        }
        Stmt::If(if_stmt) => {
            let test = lower_expression(&if_stmt.test, ctx)?;
            let consequent = lower_statement_branch(&if_stmt.cons, ctx)?;
            let alternate = if_stmt
                .alt
                .as_deref()
                .map(|branch| lower_statement_branch(branch, ctx))
                .transpose()?
                .unwrap_or_default();
            Ok(HirStmt::If {
                test,
                consequent,
                alternate,
                span,
            })
        }
        Stmt::Return(return_stmt) => {
            if let Some(Expr::Await(awaited)) = return_stmt.arg.as_deref() {
                return lower_awaited_call(awaited, None, span, ctx);
            }
            Ok(HirStmt::Return {
                value: return_stmt
                    .arg
                    .as_deref()
                    .map(|expr| lower_expression(expr, ctx))
                    .transpose()?,
                span,
            })
        }
        Stmt::Throw(throw_stmt) => Ok(HirStmt::Throw {
            value: lower_expression(throw_stmt.arg.as_ref(), ctx)?,
            span,
        }),
        Stmt::Try(try_stmt) => {
            let body = lower_callable_statements(&try_stmt.block.stmts, ctx)?;
            let catch = try_stmt
                .handler
                .as_ref()
                .map(|handler| {
                    ctx.push_scope();
                    let binding = match handler.param.as_ref() {
                        None => None,
                        Some(Pat::Ident(ident)) => Some(ctx.declare_binding(
                            ident.id.sym.to_string(),
                            HirBindingKind::AsyncValue,
                            source_span_from_swc(ident.id.span, ctx.module_id),
                        )?),
                        _ => return Err("catch parameters must be identifiers".to_string()),
                    };
                    let statements = lower_callable_statements(&handler.body.stmts, ctx)?;
                    ctx.pop_scope();
                    Ok(binding.map(|binding| (binding, statements)))
                })
                .transpose()?
                .flatten();
            let finally = try_stmt
                .finalizer
                .as_ref()
                .map(|block| lower_callable_statements(&block.stmts, ctx))
                .transpose()?
                .unwrap_or_default();
            Ok(HirStmt::Try {
                body,
                catch,
                finally,
                span,
            })
        }
        unsupported => Err(format!("Unsupported callable statement: {unsupported:?}")),
    }
}

/// React-style functional setters are lowered as a pure expression whose
/// updater parameter is an alias of the state slot.  The action VM evaluates
/// that expression immediately before StoreState, preserving the atomic
/// read-modify-write behavior without serializing a closure.
fn lower_state_update_value(
    value: &Expr,
    state: BindingId,
    ctx: &mut HirLoweringCtx<'_>,
) -> Result<ExprId, String> {
    let Expr::Arrow(updater) = value else {
        return lower_expression(value, ctx);
    };
    if updater.params.len() != 1 {
        return Err("State updater callbacks require one identifier parameter".into());
    }
    let Pat::Ident(parameter) = &updater.params[0] else {
        return Err("State updater callbacks require one identifier parameter".into());
    };
    let expression = match updater.body.as_ref() {
        ArrowFunctionBody::Expr(expression) => expression.as_ref(),
        ArrowFunctionBody::FunctionBody(body) if body.stmts.len() == 1 => match &body.stmts[0] {
            Stmt::Return(returned) => returned
                .arg
                .as_deref()
                .ok_or("State updater return requires a value")?,
            _ => return Err("State updater blocks must contain one return expression".into()),
        },
        ArrowFunctionBody::FunctionBody(_) => {
            return Err("State updater blocks must contain one return expression".into())
        }
    };
    ctx.push_scope();
    ctx.scopes
        .last_mut()
        .expect("scope stack is never empty")
        .insert(parameter.id.sym.to_string(), state);
    let lowered = lower_expression(expression, ctx);
    ctx.pop_scope();
    lowered
}

/// Returns the reference targeted by the only supported optional call form:
/// `ref.current?.focus()`.  Optional browser values never enter the normal
/// expression/value VM.
fn optional_focus_reference(
    expr: &Expr,
    ctx: &HirLoweringCtx<'_>,
) -> Result<Option<BindingId>, String> {
    let Expr::OptChain(outer) = expr else {
        return Ok(None);
    };
    let swc_ecma_ast::OptChainBase::Call(call) = outer.base.as_ref() else {
        return Ok(None);
    };
    if !call.args.is_empty() {
        return Ok(None);
    }
    let Expr::OptChain(inner) = call.callee.as_ref() else {
        return Ok(None);
    };
    if !inner.optional {
        return Ok(None);
    }
    let swc_ecma_ast::OptChainBase::Member(method) = inner.base.as_ref() else {
        return Ok(None);
    };
    if !matches!(&method.prop, swc_ecma_ast::MemberProp::Ident(name) if name.sym == *"focus") {
        return Ok(None);
    }
    let Expr::Member(current) = method.obj.as_ref() else {
        return Ok(None);
    };
    let (Expr::Ident(reference), swc_ecma_ast::MemberProp::Ident(property)) =
        (current.obj.as_ref(), &current.prop)
    else {
        return Ok(None);
    };
    if property.sym != *"current" {
        return Ok(None);
    }
    Ok(Some(ctx.resolve_binding(&reference.sym)?))
}

fn lower_statement_branch(
    stmt: &Stmt,
    ctx: &mut HirLoweringCtx<'_>,
) -> Result<Vec<HirStmt>, String> {
    match stmt {
        Stmt::Block(block) => lower_callable_statements(&block.stmts, ctx),
        _ => Ok(vec![lower_callable_statement(stmt, ctx)?]),
    }
}

/// Lower the returned expression to HIR nodes.
///
/// Returns `None` for statically absent content (null, false, undefined).
fn lower_returned_expression(
    returned: &ReturnedComponentExpression<'_>,
    ctx: &mut HirLoweringCtx<'_>,
) -> Result<Option<NodeId>, String> {
    match returned {
        ReturnedComponentExpression::JsxElement(jsx) => Ok(Some(lower_jsx_element(jsx, ctx)?)),
        ReturnedComponentExpression::JsxFragment(fragment) => {
            Ok(Some(lower_jsx_fragment(fragment, ctx)?))
        }
        ReturnedComponentExpression::StructuralExpression(expr) => {
            Ok(Some(lower_structural_expression(expr, ctx)?))
        }
        ReturnedComponentExpression::StaticallyAbsent => Ok(None),
        ReturnedComponentExpression::NonJsxReturn(_) => {
            Err("Non-JSX component returns are not supported by structural HIR".to_string())
        }
    }
}

/// Lower a JSX element to HIR.
fn lower_jsx_element(jsx: &JSXElement, ctx: &mut HirLoweringCtx<'_>) -> Result<NodeId, String> {
    lower_jsx_element_with_consumed_key(jsx, ctx, false)
}

fn lower_jsx_element_with_consumed_key(
    jsx: &JSXElement,
    ctx: &mut HirLoweringCtx<'_>,
    key_consumed: bool,
) -> Result<NodeId, String> {
    let span = source_span_from_swc(jsx.span, ctx.module_id);

    let (tag_name, is_custom) = match &jsx.opening.name {
        JSXElementName::Ident(ident) => {
            let name = ident.sym.to_string();
            let is_custom = name.chars().next().is_some_and(char::is_uppercase);
            (name, is_custom)
        }
        JSXElementName::JSXMemberExpr(member) => {
            // member.object (should be ident) . member.prop
            match &member.obj {
                JSXObject::Ident(obj_ident) => {
                    let name = format!("{}.{}", obj_ident.sym, member.prop.sym);
                    (name, true)
                }
                _ => return Err("Unsupported JSX member expression".to_string()),
            }
        }
        JSXElementName::JSXNamespacedName(ns) => {
            let name = format!("{}:{}", ns.ns.sym, ns.name.sym);
            (name, false)
        }
    };

    // Router primitives are compiler-recognised syntax. They deliberately
    // lower as intrinsic DOM anchors/outlet hosts rather than pulling the
    // TypeScript compatibility component into the application graph.
    let resolved_component = is_custom
        .then(|| resolve_component(ctx.semantic_graph, ctx.module_id, &tag_name))
        .flatten();
    let router_link = resolved_component
        .as_ref()
        .is_some_and(|symbol| symbol.local_name == "Link");
    let router_outlet = resolved_component
        .as_ref()
        .is_some_and(|symbol| symbol.local_name == "Outlet");
    let target = if is_custom && !router_link && !router_outlet {
        if ctx.component_selectors.contains_key(&tag_name) {
            None
        } else if let Some(symbol) = resolved_component {
            Some(component_target_from_symbol(&symbol))
        } else if let Some(target) = ctx.component_aliases.get(&tag_name) {
            Some(component_target_from_id(target))
        } else if let Ok(binding) = ctx.resolve_binding(&tag_name) {
            if matches!(
                ctx.bindings[binding.0 as usize].kind,
                HirBindingKind::Parameter { callable: false }
            ) {
                Some(HirComponentTarget::Prop(binding))
            } else {
                return Err(format!("'{tag_name}' is not a component-valued prop"));
            }
        } else {
            return Err(format!(
                "Unresolved component '{tag_name}' in module '{}'",
                ctx.module_id
            ));
        }
    } else {
        None
    };
    let static_target = match target.as_ref() {
        Some(HirComponentTarget::Static(target)) => Some(target),
        _ => None,
    };
    let selector_target = ctx.component_selectors.get(&tag_name).cloned();

    let mut props = Vec::new();
    let mut events = Vec::new();
    let mut host_ref = None;
    for attr_or_spread in &jsx.opening.attrs {
        if let JSXAttrOrSpread::JSXAttr(attr) = attr_or_spread {
            if key_consumed && jsx_attr_name(attr).as_deref() == Some("key") {
                continue;
            }
            if target.is_none() && jsx_attr_name(attr).as_deref() == Some("ref") {
                let Some(JSXAttrValue::JSXExprContainer(container)) = &attr.value else {
                    return Err("intrinsic ref requires useHostRef()".into());
                };
                let JSXExpr::Expr(expr) = &container.expr else {
                    return Err("intrinsic ref requires a useHostRef binding".into());
                };
                let Expr::Ident(ident) = expr.as_ref() else {
                    return Err("intrinsic ref requires a useHostRef binding".into());
                };
                let binding = ctx.resolve_binding(&ident.sym)?;
                if !matches!(
                    ctx.bindings[binding.0 as usize].kind,
                    HirBindingKind::HostRef
                ) {
                    return Err("intrinsic ref requires useHostRef()".into());
                }
                if host_ref.replace(binding).is_some() {
                    return Err("intrinsic elements support one ref".into());
                }
            } else {
                lower_jsx_attr(attr, ctx, &mut props, static_target, &mut events)?;
                if router_link && jsx_attr_name(attr).as_deref() == Some("to") {
                    if let Some(prop) = props.last_mut() {
                        match prop {
                            HirProp::Static { name, .. }
                            | HirProp::Expression { name, .. }
                            | HirProp::Callable { name, .. }
                            | HirProp::Component { name, .. } => *name = "href".into(),
                            HirProp::Spread { .. } => {
                                unreachable!("attributes do not lower to spreads")
                            }
                        }
                    }
                }
            }
        } else if let JSXAttrOrSpread::SpreadElement(spread) = attr_or_spread {
            // Spread is a serializable value-graph operation. Component calls
            // retain it until target lowering verifies that the target accepts
            // a direct props bag; intrinsic writes use the prop program.
            props.push(HirProp::Spread {
                value: lower_expression(&spread.expr, ctx)?,
            });
        }
    }

    let route_outlet = if router_outlet {
        match props.iter().find_map(|prop| match prop {
            HirProp::Static { name, value } if name == "id" => Some(value.clone()),
            _ => None,
        }) {
            Some(id) => Some(id),
            None => Some("main".into()),
        }
    } else {
        None
    };

    let mut children = Vec::new();
    for child in &jsx.children {
        if let Some(node_id) = lower_jsx_child(child, ctx)? {
            children.push(node_id);
        }
    }

    let node_id = ctx.alloc_node_id();

    let node = if let Some((test, consequent, alternate)) = selector_target {
        let consequent_id = ctx.alloc_node_id();
        ctx.nodes.push(HirNode::Component(HirComponentCall {
            id: consequent_id,
            target: HirComponentTarget::Static(consequent),
            props: props.clone(),
            children: children.clone(),
            span: span.clone(),
        }));
        let alternate_id = ctx.alloc_node_id();
        ctx.nodes.push(HirNode::Component(HirComponentCall {
            id: alternate_id,
            target: HirComponentTarget::Static(alternate),
            props,
            children,
            span: span.clone(),
        }));
        HirNode::Conditional(HirConditional {
            id: node_id,
            test,
            consequent: vec![consequent_id],
            alternate: vec![alternate_id],
            span,
        })
    } else if let Some(target) = target {
        HirNode::Component(HirComponentCall {
            id: node_id,
            target,
            props,
            children,
            span,
        })
    } else {
        let element_tag = if router_link {
            "a".to_owned()
        } else if router_outlet {
            "div".to_owned()
        } else {
            tag_name.clone()
        };
        // Compile-time element-tag policy (crates/plec-ir/src/sink.rs): the
        // earliest layer that can reject a tag the runtime element policy
        // would fail closed on, with a precise diagnostic instead of a
        // load-time rejection of the finished artifact. JSX carries no
        // explicit namespace, so the tag is admitted when either namespace's
        // allowlist takes it (mirroring the SSR serializer); the runtime
        // resolves the concrete namespace from the lowered graph.
        let policy = plec_ir::sink::TagPolicy {
            custom_elements: ctx.custom_elements.clone(),
        };
        if !plec_ir::sink::is_allowed_element_tag_with_policy(&element_tag, "html", &policy)
            && !plec_ir::sink::is_allowed_element_tag_with_policy(&element_tag, "svg", &policy)
        {
            if plec_ir::sink::is_forbidden_element_tag(&element_tag) {
                return Err(format!(
                    "Forbidden element tag '{element_tag}': active, embedding, and document-metadata elements are not part of the Plec element policy"
                ));
            }
            if element_tag.contains('-') {
                return Err(format!(
                    "Custom element '{element_tag}' is not configured: add it to the [compiler] custom-elements list in plec.toml"
                ));
            }
            return Err(format!("Unsupported element tag '{element_tag}'"));
        }
        HirNode::Element(HirElement {
            id: node_id,
            tag: element_tag,
            props,
            events,
            host_ref,
            route_outlet,
            children,
            span,
        })
    };

    ctx.nodes.push(node);
    Ok(node_id)
}

/// Attribute names reserved by the structural DOM address protocol
/// (docs/dom-address-protocol.md). `data-plec-*` and `plec:*` are permanently
/// runtime-owned; `data-runtime-*` stays reserved while the legacy string-id
/// scheme exists (wasm-runtime-ixk.7). Authored JSX must never collide with
/// them: a user-written `data-plec-node` reaches adoption as a duplicate
/// marker and fails the whole page closed at runtime, so the rejection has to
/// happen here instead.
fn is_reserved_dom_attribute(name: &str) -> bool {
    name.starts_with("data-plec-") || name.starts_with("data-runtime-") || name.starts_with("plec:")
}

/// DOM-sink policy for authored intrinsic-element props
/// (crates/plec-ir/src/sink.rs). HTML attribute lookup is case-insensitive,
/// so any non-canonical `on*` spelling (`onclick`, `ONCLICK`) would reach the
/// runtime as a live event handler; canonical `onClick` routes through the
/// declared event contract instead. `srcdoc` is an inline document sink and
/// is never a supported prop.
fn reject_hostile_intrinsic_attr(name: &str) -> Result<(), String> {
    let lower = name.to_ascii_lowercase();
    if lower == "srcdoc" {
        return Err(format!(
            "JSX attribute '{name}' is a document sink and is not supported by Plec (docs/dom-address-protocol.md)"
        ));
    }
    if lower.starts_with("on") {
        let canonical = name.len() > 2
            && name[2..]
                .chars()
                .next()
                .map(|c| c.is_ascii_uppercase())
                .unwrap_or(false);
        if !canonical {
            return Err(format!(
                "JSX event attribute '{name}' must use the canonical camelCase form (onClick) and a function expression; string handlers are not supported"
            ));
        }
    }
    Ok(())
}

/// Lower JSX attributes to HIR props.
fn lower_jsx_attr(
    attr: &JSXAttr,
    ctx: &mut HirLoweringCtx<'_>,
    props: &mut Vec<HirProp>,
    component_target: Option<&ComponentId>,
    events: &mut Vec<HirEventBinding>,
) -> Result<(), String> {
    let name = jsx_attr_name(attr).expect("JSX attribute has a name");

    if is_reserved_dom_attribute(&name) {
        return Err(format!(
            "Reserved Plec DOM attribute '{name}' is owned by the runtime and cannot be authored in JSX (docs/dom-address-protocol.md)"
        ));
    }
    if component_target.is_none() {
        reject_hostile_intrinsic_attr(&name)?;
    }
    if name == "key" {
        return Err(
            "JSX key is only supported on the direct root of a .map() callback".to_string(),
        );
    }
    if component_target.is_some() && name == "children" {
        return Err("component children must use JSX child syntax".to_string());
    }

    let events_before = events.len();
    match &attr.value {
        Some(JSXAttrValue::Str(str_lit)) => {
            let value = str_lit
                .value
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_default();
            if component_target.is_none() && !is_safe_attribute_value(&name, &value) {
                return Err(format!(
                    "JSX attribute '{name}' value uses an unsafe URL scheme and cannot be authored in Plec (crates/plec-ir/src/sink.rs)"
                ));
            }
            props.push(HirProp::Static {
                name: name.clone(),
                value,
            });
        }
        Some(JSXAttrValue::JSXExprContainer(container)) => match &container.expr {
            JSXExpr::Expr(expr) => {
                let span = source_span_from_swc(container.span, ctx.module_id);

                // Check if this is a DOM event binding (only for intrinsic elements)
                if component_target.is_none() {
                    if let Some(event_name) = normalize_event_name(&name) {
                        // For DOM events, all valid expressions are treated as callables
                        if let Some(callable) =
                            lower_callable(expr, CallablePolicy::CallableValue, ctx)?
                        {
                            events.push(HirEventBinding {
                                event: event_name,
                                callable,
                                span,
                            });
                            return Ok(());
                        }
                    }
                }

                let callable = match component_target.and_then(|target| {
                    ctx.semantic_graph
                        .get_module(&target.module_id)
                        .and_then(|module| module.component_props.get(&target.local_name))
                        .and_then(|props| props.get(&name))
                }) {
                    Some(ComponentPropKind::Callable) => {
                        lower_callable(expr, CallablePolicy::CallableValue, ctx)?
                    }
                    _ => lower_callable(expr, CallablePolicy::InlineOnly, ctx)?,
                };
                if let Some(callable) = callable {
                    props.push(HirProp::Callable {
                        name: name.clone(),
                        callable,
                    });
                } else {
                    // Components travel in a dedicated graph-handle channel,
                    // never through the serializable expression VM.
                    if component_target.is_some() {
                        if let Expr::Ident(ident) = expr.as_ref() {
                            if let Some(symbol) =
                                resolve_component(ctx.semantic_graph, ctx.module_id, &ident.sym)
                            {
                                props.push(HirProp::Component {
                                    name: name.clone(),
                                    target: ComponentId::new(symbol.module_id, symbol.local_name),
                                });
                                return Ok(());
                            }
                        }
                    }
                    // Not a callable - regular expression prop
                    let expr_id = lower_expression(expr, ctx)?;
                    props.push(HirProp::Expression {
                        name: name.clone(),
                        value: expr_id,
                    });
                }
            }
            JSXExpr::JSXEmptyExpr(_) => {
                props.push(HirProp::Static {
                    name: name.clone(),
                    value: String::new(),
                });
            }
        },
        None => {
            return Err(format!(
                "Boolean JSX attribute '{name}' is not supported by structural HIR"
            ))
        }
        Some(_) => return Err(format!("Unsupported JSX attribute value for '{name}'")),
    }
    // An intrinsic `on*` prop that survived the value match never became a
    // declared event (string handlers, non-callable expressions), so it would
    // reach the runtime as a script-sink attribute. Fail the compile closed.
    if component_target.is_none()
        && name.to_ascii_lowercase().starts_with("on")
        && events.len() == events_before
    {
        return Err(format!(
            "JSX event attribute '{name}' must be a canonical camelCase handler (onClick={{fn}}); string handlers are not supported"
        ));
    }
    Ok(())
}

fn jsx_attr_name(attr: &JSXAttr) -> Option<String> {
    Some(match &attr.name {
        JSXAttrName::Ident(ident) => ident.sym.to_string(),
        JSXAttrName::JSXNamespacedName(ns) => format!("{}:{}", ns.ns.sym, ns.name.sym),
    })
}

/// Check if an expression is structural (contains JSX, conditionals, etc.)
fn is_structural_expression(expr: &Expr) -> bool {
    match expr {
        Expr::JSXElement(_) | Expr::JSXFragment(_) => true,
        Expr::Cond(_) | Expr::Bin(_) => true,
        Expr::Paren(paren) => is_structural_expression(&paren.expr),
        Expr::Call(call) => {
            // Check if this is a .map() call
            match &call.callee {
                Callee::Expr(callee_expr) => {
                    if let Expr::Member(member) = &**callee_expr {
                        if let swc_ecma_ast::MemberProp::Ident(prop_ident) = &member.prop {
                            return prop_ident.sym == "map";
                        }
                    }
                }
                _ => {}
            }
            false
        }
        _ => false,
    }
}

/// Lower a JSX child to HIR node.
///
/// Returns `None` for statically absent children.
fn lower_jsx_child(
    child: &swc_ecma_ast::JSXElementChild,
    ctx: &mut HirLoweringCtx<'_>,
) -> Result<Option<NodeId>, String> {
    match child {
        swc_ecma_ast::JSXElementChild::JSXText(text) => {
            let content = text.value.as_str().map(|s| s.trim()).unwrap_or("");
            if content.is_empty() {
                return Ok(None);
            }
            let span = source_span_from_swc(text.span, ctx.module_id);
            let node_id = ctx.alloc_node_id();
            ctx.nodes.push(HirNode::Text(HirText::Static {
                id: node_id,
                value: content.to_string(),
                span,
            }));
            Ok(Some(node_id))
        }
        swc_ecma_ast::JSXElementChild::JSXElement(jsx) => Ok(Some(lower_jsx_element(jsx, ctx)?)),
        swc_ecma_ast::JSXElementChild::JSXFragment(fragment) => {
            Ok(Some(lower_jsx_fragment(fragment, ctx)?))
        }
        swc_ecma_ast::JSXElementChild::JSXExprContainer(container) => match &container.expr {
            JSXExpr::Expr(expr) => {
                // Check if this is a structural expression (contains JSX, conditionals, etc.)
                if is_structural_expression(expr) {
                    // Structural expressions become direct nodes
                    Ok(Some(lower_structural_expression(expr, ctx)?))
                } else {
                    // Value expressions become text nodes
                    let span = source_span_from_swc(container.span, ctx.module_id);
                    let expr_id = lower_expression(expr, ctx)?;
                    let node_id = ctx.alloc_node_id();
                    if matches!(ctx.expressions[expr_id.0 as usize].expression, HirExpr::Binding(binding) if ctx.is_children_parameter(binding))
                    {
                        ctx.nodes.push(HirNode::Slot(HirSlot { id: node_id, span }));
                        return Ok(Some(node_id));
                    }
                    ctx.nodes.push(HirNode::Text(HirText::Expression {
                        id: node_id,
                        expression: expr_id,
                        span: span,
                    }));
                    Ok(Some(node_id))
                }
            }
            JSXExpr::JSXEmptyExpr(_) => Ok(None),
        },
        swc_ecma_ast::JSXElementChild::JSXSpreadChild(_) => {
            Err("JSX spread children are not supported by structural HIR".to_string())
        }
    }
}

/// Lower a .map() call to a ForEach node.
///
/// Handles the case of:
///   items.map((item) => <JSX...>)
///
/// Extracts the collection, item parameter, and body.
fn lower_map_call(
    call: &swc_ecma_ast::CallExpr,
    member: &swc_ecma_ast::MemberExpr,
    ctx: &mut HirLoweringCtx<'_>,
) -> Result<NodeId, String> {
    let span = source_span_from_swc(call.span, ctx.module_id);

    // Get the source collection (the object being mapped)
    let source_expr_id = lower_expression(&member.obj, ctx)?;

    // Extract the callback function
    let callback = call
        .args
        .first()
        .and_then(|arg| match &*arg.expr {
            Expr::Arrow(arrow) => Some(arrow),
            _ => None,
        })
        .ok_or("Expected .map() callback to be an arrow function")?;

    if callback.params.len() != 1 {
        return Err(".map() callbacks require exactly one item parameter".to_string());
    }
    let item_name = callback
        .params
        .first()
        .and_then(|param| match param {
            swc_ecma_ast::Pat::Ident(ident) => Some(ident.id.sym.to_string()),
            _ => None,
        })
        .ok_or("Expected .map() callback to have one parameter")?;

    ctx.push_scope();
    let item_binding = ctx.declare_binding(
        item_name,
        HirBindingKind::LoopItem,
        source_span_from_swc(callback.span, ctx.module_id),
    )?;
    // Lower the callback body and consume its structural identity, if present.
    let (body_node_id, identity) = match &*callback.body {
        swc_ecma_ast::ArrowFunctionBody::Expr(body_expr) => lower_for_each_body(&**body_expr, ctx)?,
        swc_ecma_ast::ArrowFunctionBody::FunctionBody(_) => {
            return Err("Block .map() callbacks not supported".to_string());
        }
    };

    ctx.pop_scope();
    let node_id = ctx.alloc_node_id();
    ctx.nodes.push(HirNode::ForEach(HirForEach {
        id: node_id,
        source: source_expr_id,
        identity,
        item_binding,
        body: vec![body_node_id],
        span,
    }));
    Ok(node_id)
}

fn lower_for_each_body(
    expr: &Expr,
    ctx: &mut HirLoweringCtx<'_>,
) -> Result<(NodeId, Option<ExprId>), String> {
    match expr {
        Expr::Paren(paren) => lower_for_each_body(&paren.expr, ctx),
        Expr::JSXElement(jsx) => {
            let identity = extract_for_each_identity(jsx, ctx)?;
            let body = lower_jsx_element_with_consumed_key(jsx, ctx, identity.is_some())?;
            Ok((body, identity))
        }
        _ => Ok((lower_structural_expression(expr, ctx)?, None)),
    }
}

fn extract_for_each_identity(
    jsx: &JSXElement,
    ctx: &mut HirLoweringCtx<'_>,
) -> Result<Option<ExprId>, String> {
    let mut key = None;
    for attr_or_spread in &jsx.opening.attrs {
        let JSXAttrOrSpread::JSXAttr(attr) = attr_or_spread else {
            continue;
        };
        if jsx_attr_name(attr).as_deref() != Some("key") {
            continue;
        }
        if key.is_some() {
            return Err("A .map() callback root may only declare one JSX key".to_string());
        }
        let Some(JSXAttrValue::JSXExprContainer(container)) = &attr.value else {
            return Err("A .map() callback key must be an expression".to_string());
        };
        let JSXExpr::Expr(expression) = &container.expr else {
            return Err("A .map() callback key must not be empty".to_string());
        };
        key = Some(lower_expression(expression, ctx)?);
    }
    Ok(key)
}

/// Lower a JSX fragment to HIR.
fn lower_jsx_fragment(
    fragment: &JSXFragment,
    ctx: &mut HirLoweringCtx<'_>,
) -> Result<NodeId, String> {
    let span = source_span_from_swc(fragment.span, ctx.module_id);

    let mut children = Vec::new();
    for child in &fragment.children {
        if let Some(node_id) = lower_jsx_child(child, ctx)? {
            children.push(node_id);
        }
    }

    let node_id = ctx.alloc_node_id();
    ctx.nodes.push(HirNode::Fragment(HirFragment {
        id: node_id,
        children,
        span,
    }));
    Ok(node_id)
}

/// Lower a structural expression to HIR.
///
/// Handles conditionals, logical expressions, etc.
fn lower_structural_expression(
    expr: &Expr,
    ctx: &mut HirLoweringCtx<'_>,
) -> Result<NodeId, String> {
    let span = span_from_expr(expr, ctx.module_id);

    match expr {
        Expr::Cond(cond) => {
            let test_expr_id = lower_expression(&cond.test, ctx)?;
            let consequent_id = lower_structural_expression(&cond.cons, ctx)?;
            let alternate_id = vec![lower_structural_expression(&cond.alt, ctx)?];

            let node_id = ctx.alloc_node_id();
            ctx.nodes.push(HirNode::Conditional(HirConditional {
                id: node_id,
                test: test_expr_id,
                consequent: vec![consequent_id],
                alternate: alternate_id,
                span,
            }));
            Ok(node_id)
        }
        Expr::Bin(bin) => {
            // Logical && and || become conditional structures
            match bin.op {
                BinaryOp::LogicalAnd => {
                    let test_expr_id = lower_expression(&bin.left, ctx)?;
                    let consequent_id = lower_structural_expression(&bin.right, ctx)?;
                    let node_id = ctx.alloc_node_id();
                    ctx.nodes.push(HirNode::Conditional(HirConditional {
                        id: node_id,
                        test: test_expr_id,
                        consequent: vec![consequent_id],
                        alternate: vec![],
                        span: span.clone(),
                    }));
                    Ok(node_id)
                }
                BinaryOp::LogicalOr => {
                    let test_expr_id = lower_expression(&bin.left, ctx)?;
                    // For ||, we need to negate the test
                    let not_id = ctx.alloc_expr_id();
                    ctx.add_expression(HirExprNode::new(
                        not_id,
                        HirExpr::Unary {
                            op: HirUnaryOp::Not,
                            argument: test_expr_id,
                        },
                        span.clone(),
                    ));
                    let consequent_id = lower_structural_expression(&bin.right, ctx)?;
                    let node_id = ctx.alloc_node_id();
                    ctx.nodes.push(HirNode::Conditional(HirConditional {
                        id: node_id,
                        test: not_id,
                        consequent: vec![consequent_id],
                        alternate: vec![],
                        span: span.clone(),
                    }));
                    Ok(node_id)
                }
                _ => Err(
                    "Non-logical binary expressions cannot render structural content".to_string(),
                ),
            }
        }
        Expr::Unary(_) => Err("Unary structural expressions are not supported".to_string()),
        Expr::JSXElement(jsx) => lower_jsx_element(jsx, ctx),
        Expr::JSXFragment(fragment) => lower_jsx_fragment(fragment, ctx),
        // Primitive value expressions that render as text
        Expr::Lit(_) | Expr::Tpl(_) | Expr::Ident(_) => {
            // These are values that should render as text
            let span = source_span_from_swc(expr.span(), ctx.module_id);
            let node_id = ctx.alloc_node_id();
            let expr_id = lower_expression(expr, ctx)?;
            ctx.nodes.push(HirNode::Text(HirText::Expression {
                id: node_id,
                expression: expr_id,
                span,
            }));
            Ok(node_id)
        }
        Expr::Paren(paren) => lower_structural_expression(&paren.expr, ctx),
        // Handle .map() calls as ForEach
        Expr::Call(call) => {
            // Check if this is a .map() call: <expr>.map(callback)
            match &call.callee {
                Callee::Expr(callee_expr) => {
                    if let Expr::Member(member) = &**callee_expr {
                        if let swc_ecma_ast::MemberProp::Ident(prop_ident) = &member.prop {
                            if prop_ident.sym == "map" {
                                return lower_map_call(call, member, ctx);
                            }
                        }
                    }
                }
                _ => {}
            }
            Err(format!(
                "Unsupported structural expression: Call at {}..{} (only .map() is supported as structural)",
                span.start,
                span.end
            ))
        }
        _ => Err(format!(
            "Unsupported structural expression: {} at {}..{}",
            expr_name(expr),
            span.start,
            span.end
        )),
    }
}

/// Lower an expression to HIR.
fn lower_expression(expr: &Expr, ctx: &mut HirLoweringCtx<'_>) -> Result<ExprId, String> {
    let span = span_from_expr(expr, ctx.module_id);

    let hir_expr = match expr {
        Expr::Call(call) if matches!(&call.callee, Callee::Expr(callee) if matches!(callee.as_ref(), Expr::Member(member) if matches!(member.prop, swc_ecma_ast::MemberProp::Ident(ref name) if name.sym == "useLoaderData"))) =>
        {
            if !call.args.is_empty() {
                return Err("Route.useLoaderData() accepts no arguments".into());
            }
            let binding = if let Some(binding) = ctx.loader_data_binding {
                binding
            } else {
                let span = source_span_from_swc(call.span, ctx.module_id);
                let binding = ctx.declare_binding(
                    "__plec_loader_data".into(),
                    HirBindingKind::Input {
                        kind: "loaderData".into(),
                    },
                    span.clone(),
                )?;
                ctx.inputs.push(HirInput {
                    binding,
                    name: "loaderData".into(),
                    kind: "loaderData".into(),
                    span,
                });
                ctx.loader_data_binding = Some(binding);
                binding
            };
            HirExpr::Binding(binding)
        }
        Expr::Call(call) if matches!(&call.callee, Callee::Expr(callee) if matches!(callee.as_ref(), Expr::Member(member) if matches!(member.obj.as_ref(), Expr::Ident(object) if object.sym == "JSON") && matches!(member.prop, swc_ecma_ast::MemberProp::Ident(ref property) if property.sym == "stringify"))) =>
        {
            if call.args.len() != 1 || call.args[0].spread.is_some() {
                return Err("JSON.stringify requires one non-spread argument".into());
            }
            HirExpr::Builtin {
                kind: "jsonStringify".into(),
                args: vec![lower_expression(&call.args[0].expr, ctx)?],
            }
        }
        Expr::Call(call) if matches!(&call.callee, Callee::Expr(callee) if matches!(callee.as_ref(), Expr::Ident(ident) if ident.sym == "encodeURIComponent")) =>
        {
            if call.args.len() != 1 || call.args[0].spread.is_some() {
                return Err("encodeURIComponent requires one non-spread argument".into());
            }
            HirExpr::Builtin {
                kind: "encodeUriComponent".into(),
                args: vec![lower_expression(&call.args[0].expr, ctx)?],
            }
        }
        Expr::Call(call) if matches!(&call.callee, Callee::Expr(callee) if matches!(callee.as_ref(), Expr::Member(member) if matches!(member.obj.as_ref(), Expr::Ident(object) if object.sym == "cookie") && matches!(member.prop, swc_ecma_ast::MemberProp::Ident(ref property) if property.sym == "getSync"))) =>
        {
            if call.args.len() != 1 || call.args[0].spread.is_some() {
                return Err("cookie.getSync requires one static name".to_string());
            }
            let Expr::Lit(Lit::Str(name)) = call.args[0].expr.as_ref() else {
                return Err("cookie.getSync requires one static name".to_string());
            };
            HirExpr::Host {
                kind: "cookie".to_string(),
                name: Some(name.value.to_string_lossy().into_owned()),
            }
        }
        // The executable graph has one nullish representation.  `undefined`
        // is only accepted as the unshadowed global spelling of that value.
        Expr::Ident(ident) if ident.sym == *"undefined" => HirExpr::Literal(HirValue::Null),
        Expr::Ident(ident) => HirExpr::Binding(ctx.resolve_binding(&ident.sym)?),

        Expr::Lit(lit) => match lit {
            Lit::Str(s) => {
                let value = s.value.as_str().map(|s| s.to_string()).unwrap_or_default();
                HirExpr::Literal(HirValue::String(value))
            }
            Lit::Num(n) => HirExpr::Literal(HirValue::Number(n.value)),
            Lit::Bool(b) => HirExpr::Literal(HirValue::Bool(b.value)),
            Lit::Null(_) => HirExpr::Literal(HirValue::Null),
            _ => return Err("Unsupported literal".to_string()),
        },

        Expr::Member(member) => {
            let object_id = lower_expression(&member.obj, ctx)?;
            match &member.prop {
                swc_ecma_ast::MemberProp::Computed(computed) => HirExpr::ComputedMember {
                    object: object_id,
                    property: lower_expression(&computed.expr, ctx)?,
                    optional: false,
                },
                swc_ecma_ast::MemberProp::PrivateName(_) => {
                    return Err("Private member properties not supported".to_string())
                }
                swc_ecma_ast::MemberProp::Ident(ident) => {
                    let property = ident.sym.to_string();
                    if property == "current" {
                        if let HirExpr::Binding(binding) =
                            ctx.expressions[object_id.0 as usize].expression
                        {
                            match ctx.bindings[binding.0 as usize].kind {
                                HirBindingKind::RefSlot => {
                                    HirExpr::RefCurrent { reference: binding }
                                }
                                HirBindingKind::HostRef => {
                                    HirExpr::HostRefCurrent { reference: binding }
                                }
                                _ => HirExpr::Member {
                                    object: object_id,
                                    property,
                                },
                            }
                        } else {
                            HirExpr::Member {
                                object: object_id,
                                property,
                            }
                        }
                    } else {
                        HirExpr::Member {
                            object: object_id,
                            property,
                        }
                    }
                }
            }
        }

        // Optional reads are safe only for the existing non-computed member
        // subset.  `Field` already maps a nullish/missing record to Null in the
        // typed VM, so no DOM or host value escapes the expression boundary.
        Expr::OptChain(chain) => match chain.base.as_ref() {
            swc_ecma_ast::OptChainBase::Member(member) => {
                let object_id = lower_expression(&member.obj, ctx)?;
                match &member.prop {
                    swc_ecma_ast::MemberProp::Ident(ident) => HirExpr::Member {
                        object: object_id,
                        property: ident.sym.to_string(),
                    },
                    swc_ecma_ast::MemberProp::Computed(computed) => HirExpr::ComputedMember {
                        object: object_id,
                        property: lower_expression(&computed.expr, ctx)?,
                        optional: true,
                    },
                    _ => {
                        return Err(
                            "Private optional member properties are not supported".to_string()
                        )
                    }
                }
            }
            swc_ecma_ast::OptChainBase::Call(_) => {
                return Err("Optional calls are unsupported except ref.current?.focus()".to_string())
            }
        },

        Expr::New(new) if matches!(new.callee.as_ref(), Expr::Ident(ident) if ident.sym == "Error") =>
        {
            let args = new.args.as_ref().ok_or("Error requires a message")?;
            if args.len() > 1 || args.iter().any(|arg| arg.spread.is_some()) {
                return Err("Error requires zero or one non-spread message".into());
            }
            let kind = ctx.alloc_expr_id();
            ctx.add_expression(HirExprNode::new(
                kind,
                HirExpr::Literal(HirValue::String("Error".into())),
                span.clone(),
            ));
            let message = match args.first() {
                Some(argument) => lower_expression(&argument.expr, ctx)?,
                None => {
                    let id = ctx.alloc_expr_id();
                    ctx.add_expression(HirExprNode::new(
                        id,
                        HirExpr::Literal(HirValue::String(String::new())),
                        span.clone(),
                    ));
                    id
                }
            };
            HirExpr::Object(vec![
                HirObjectItem::Property {
                    name: "name".into(),
                    value: kind,
                },
                HirObjectItem::Property {
                    name: "message".into(),
                    value: message,
                },
            ])
        }

        Expr::Unary(unary) => {
            let argument_id = lower_expression(&unary.arg, ctx)?;
            let op = match unary.op {
                swc_ecma_ast::UnaryOp::Minus => HirUnaryOp::Minus,
                swc_ecma_ast::UnaryOp::Plus => HirUnaryOp::Plus,
                swc_ecma_ast::UnaryOp::Bang => HirUnaryOp::Not,
                swc_ecma_ast::UnaryOp::Void
                | swc_ecma_ast::UnaryOp::Delete
                | swc_ecma_ast::UnaryOp::TypeOf => {
                    return Err("Unsupported unary operator".to_string())
                }
                _ => return Err("Unsupported unary operator".to_string()),
            };
            HirExpr::Unary {
                op,
                argument: argument_id,
            }
        }

        Expr::Bin(bin) => {
            let left_id = lower_expression(&bin.left, ctx)?;
            let right_id = if bin.op == BinaryOp::InstanceOf
                && matches!(bin.right.as_ref(), Expr::Ident(ident) if ident.sym == "Error")
            {
                let id = ctx.alloc_expr_id();
                ctx.add_expression(HirExprNode::new(
                    id,
                    HirExpr::Literal(HirValue::String("Error".into())),
                    span.clone(),
                ));
                id
            } else {
                lower_expression(&bin.right, ctx)?
            };

            if is_logical_op(bin.op) {
                let op = logical_op_from_swc(bin.op)?;
                HirExpr::Logical {
                    op,
                    left: left_id,
                    right: right_id,
                }
            } else {
                let op = binary_op_from_swc(bin.op)?;
                HirExpr::Binary {
                    op,
                    left: left_id,
                    right: right_id,
                }
            }
        }

        Expr::Cond(cond) => {
            let test_id = lower_expression(&cond.test, ctx)?;
            let consequent_id = lower_expression(&cond.cons, ctx)?;
            let alternate_id = lower_expression(&cond.alt, ctx)?;
            HirExpr::Conditional {
                test: test_id,
                consequent: consequent_id,
                alternate: alternate_id,
            }
        }

        Expr::Tpl(tpl) => {
            let mut parts = Vec::new();
            for (i, part) in tpl.quasis.iter().enumerate() {
                if !part.raw.is_empty() {
                    parts.push(HirTemplatePart::String(part.raw.to_string()));
                }
                if let Some(expr) = tpl.exprs.get(i) {
                    let expr_id = lower_expression(expr, ctx)?;
                    parts.push(HirTemplatePart::Expression(expr_id));
                }
            }
            HirExpr::Template { parts }
        }

        Expr::Array(array) => {
            let mut items = Vec::new();
            for item in &array.elems {
                let item = item.as_ref().ok_or("Array holes are not supported")?;
                let expression = lower_expression(&item.expr, ctx)?;
                items.push(if item.spread.is_some() {
                    HirArrayItem::Spread(expression)
                } else {
                    HirArrayItem::Value(expression)
                });
            }
            HirExpr::Array(items)
        }

        Expr::Object(obj) => {
            let mut props = Vec::new();
            for prop_or_spread in &obj.props {
                match prop_or_spread {
                    swc_ecma_ast::PropOrSpread::Prop(prop) => {
                        if let swc_ecma_ast::Prop::KeyValue(kv) = &**prop {
                            let key = match &kv.key {
                                swc_ecma_ast::PropName::Ident(ident) => ident.sym.to_string(),
                                swc_ecma_ast::PropName::Str(s) => {
                                    s.value.as_str().map(|s| s.to_string()).unwrap_or_default()
                                }
                                swc_ecma_ast::PropName::Num(n) => n.value.to_string(),
                                swc_ecma_ast::PropName::BigInt(n) => n.value.to_string(),
                                swc_ecma_ast::PropName::Computed(_) => {
                                    return Err("Computed property names not supported".to_string())
                                }
                            };
                            let value_id = lower_expression(&kv.value, ctx)?;
                            props.push(HirObjectItem::Property {
                                name: key,
                                value: value_id,
                            });
                        }
                    }
                    swc_ecma_ast::PropOrSpread::Spread(spread) => {
                        props.push(HirObjectItem::Spread(lower_expression(&spread.expr, ctx)?));
                    }
                }
            }
            HirExpr::Object(props)
        }

        Expr::Paren(paren) => return lower_expression(&paren.expr, ctx),

        Expr::TsAs(ts_as) => {
            // TypeScript type assertions are erased during lowering
            return lower_expression(&ts_as.expr, ctx);
        }

        Expr::Seq(seq) => {
            // Sequence expressions: use the last expression
            if let Some(last) = seq.exprs.last() {
                return lower_expression(last, ctx);
            }
            return Err("Empty sequence expression".to_string());
        }

        Expr::Call(call) => {
            if let Some(transform) = lower_collection_transform(call, ctx)? {
                transform
            } else if let Some(builtin) = lower_value_builtin_call(call, ctx)? {
                builtin
            } else {
                let callee_expr = match &call.callee {
                    Callee::Expr(expr) => expr.as_ref(),
                    Callee::Super(_) => {
                        return Err("Unsupported call expression: super()".to_string());
                    }
                    Callee::Import(_) => {
                        return Err("Unsupported call expression: import()".to_string());
                    }
                };

                let callee_id = lower_expression(callee_expr, ctx)?;

                let mut args = Vec::new();
                for arg in &call.args {
                    if arg.spread.is_some() {
                        return Err("Spread arguments not supported".to_string());
                    }
                    args.push(lower_expression(&arg.expr, ctx)?);
                }

                HirExpr::Call {
                    callee: callee_id,
                    args,
                }
            }
        }

        unsupported => return Err(format!("Unsupported expression: {unsupported:?}")),
    };

    let expr_id = ctx.alloc_expr_id();
    ctx.add_expression(HirExprNode::new(expr_id, hir_expr, span));
    Ok(expr_id)
}

fn lower_value_builtin_call(
    call: &swc_ecma_ast::CallExpr,
    ctx: &mut HirLoweringCtx<'_>,
) -> Result<Option<HirExpr>, String> {
    let Callee::Expr(callee) = &call.callee else {
        return Ok(None);
    };
    let Expr::Member(member) = callee.as_ref() else {
        return Ok(None);
    };
    let swc_ecma_ast::MemberProp::Ident(method) = &member.prop else {
        return Ok(None);
    };
    let kind = match method.sym.as_ref() {
        "trim" => "trim",
        "toLowerCase" => "lower",
        "toUpperCase" => "upper",
        "includes" => "includes",
        _ => return Ok(None),
    };
    let expected = if kind == "includes" { 1 } else { 0 };
    if call.args.len() != expected || call.args.iter().any(|arg| arg.spread.is_some()) {
        return Err(format!(
            "{}.{} has unsupported arguments",
            "value", method.sym
        ));
    }
    let mut args = vec![lower_expression(&member.obj, ctx)?];
    args.extend(
        call.args
            .iter()
            .map(|arg| lower_expression(&arg.expr, ctx))
            .collect::<Result<Vec<_>, _>>()?,
    );
    Ok(Some(HirExpr::Builtin {
        kind: kind.into(),
        args,
    }))
}

fn lower_collection_transform(
    call: &swc_ecma_ast::CallExpr,
    ctx: &mut HirLoweringCtx<'_>,
) -> Result<Option<HirExpr>, String> {
    let Callee::Expr(callee) = &call.callee else {
        return Ok(None);
    };
    let Expr::Member(member) = callee.as_ref() else {
        return Ok(None);
    };
    let swc_ecma_ast::MemberProp::Ident(method) = &member.prop else {
        return Ok(None);
    };
    if method.sym != *"map" && method.sym != *"filter" {
        return Ok(None);
    }
    if call.args.len() != 1 || call.args[0].spread.is_some() {
        return Err(format!(
            "{}.{} requires one inline callback",
            "collection", method.sym
        ));
    }
    let Expr::Arrow(callback) = call.args[0].expr.as_ref() else {
        return Err(format!(
            "collection.{} requires an inline arrow callback",
            method.sym
        ));
    };
    if callback.params.len() != 1 {
        return Err(format!(
            "collection.{} callback requires one item parameter",
            method.sym
        ));
    }
    let Pat::Ident(item) = &callback.params[0] else {
        return Err("collection callbacks require an identifier item parameter".into());
    };
    let source = lower_expression(&member.obj, ctx)?;
    ctx.push_scope();
    ctx.declare_binding(
        item.id.sym.to_string(),
        HirBindingKind::LoopItem,
        source_span_from_swc(item.id.span, ctx.module_id),
    )?;
    let body = match callback.body.as_ref() {
        ArrowFunctionBody::Expr(expression) => lower_expression(expression, ctx)?,
        ArrowFunctionBody::FunctionBody(body) if body.stmts.len() == 1 => match &body.stmts[0] {
            Stmt::Return(returned) => lower_expression(
                returned
                    .arg
                    .as_deref()
                    .ok_or("collection callback return requires a value")?,
                ctx,
            )?,
            _ => return Err("collection callback blocks require one return expression".into()),
        },
        _ => return Err("collection callback blocks require one return expression".into()),
    };
    ctx.pop_scope();
    Ok(Some(if method.sym == *"map" {
        HirExpr::Map {
            source,
            mapper: body,
        }
    } else {
        HirExpr::Filter {
            source,
            predicate: body,
        }
    }))
}

/// Convert SWC span to HIR SourceSpan.
fn source_span_from_swc(span: Span, module_id: &str) -> SourceSpan {
    SourceSpan {
        module_id: module_id.to_string(),
        start: span.lo.0 as u32,
        end: span.hi.0 as u32,
    }
}

/// Get span from expression using Spanned trait.
fn span_from_expr(expr: &Expr, module_id: &str) -> SourceSpan {
    source_span_from_swc(expr.span(), module_id)
}

/// Check if binary operator is logical (&&, ||, ??).
fn is_logical_op(op: BinaryOp) -> bool {
    matches!(
        op,
        BinaryOp::LogicalAnd | BinaryOp::LogicalOr | BinaryOp::NullishCoalescing
    )
}

/// Get a human-readable name for an expression type.
fn expr_name(expr: &Expr) -> &'static str {
    match expr {
        Expr::Array(_) => "Array",
        Expr::Arrow(_) => "Arrow",
        Expr::Assign(_) => "Assign",
        Expr::Await(_) => "Await",
        Expr::Bin(_) => "Bin",
        Expr::Call(_) => "Call",
        Expr::Class(_) => "Class",
        Expr::Cond(_) => "Cond",
        Expr::Fn(_) => "Fn",
        Expr::Ident(_) => "Ident",
        Expr::Lit(_) => "Lit",
        Expr::Member(_) => "Member",
        Expr::New(_) => "New",
        Expr::Object(_) => "Object",
        Expr::OptChain(_) => "OptChain",
        Expr::Paren(_) => "Paren",
        Expr::PrivateName(_) => "PrivateName",
        Expr::Seq(_) => "Seq",
        Expr::TaggedTpl(_) => "TaggedTpl",
        Expr::This(_) => "This",
        Expr::Tpl(_) => "Tpl",
        Expr::Unary(_) => "Unary",
        Expr::Update(_) => "Update",
        Expr::Yield(_) => "Yield",
        Expr::JSXElement(_) => "JSXElement",
        Expr::JSXFragment(_) => "JSXFragment",
        Expr::TsAs(_) => "TsAs",
        Expr::TsNonNull(_) => "TsNonNull",
        Expr::TsTypeAssertion(_) => "TsTypeAssertion",
        Expr::TsConstAssertion(_) => "TsConstAssertion",
        Expr::TsInstantiation(_) => "TsInstantiation",
        Expr::TsSatisfies(_) => "TsSatisfies",
        Expr::MetaProp(_) => "MetaProp",
        Expr::SuperProp(_) => "SuperProp",
        Expr::JSXMember(_) => "JSXMember",
        Expr::JSXNamespacedName(_) => "JSXNamespacedName",
        Expr::JSXEmpty(_) => "JSXEmpty",
        Expr::Invalid(_) => "Invalid",
    }
}

/// Convert SWC logical operator to HIR.
fn logical_op_from_swc(op: BinaryOp) -> Result<HirLogicalOp, String> {
    match op {
        BinaryOp::LogicalAnd => Ok(HirLogicalOp::And),
        BinaryOp::LogicalOr => Ok(HirLogicalOp::Or),
        BinaryOp::NullishCoalescing => Ok(HirLogicalOp::Coalesce),
        _ => Err("Not a logical operator".to_string()),
    }
}

/// Convert SWC binary operator to HIR.
fn binary_op_from_swc(op: BinaryOp) -> Result<HirBinaryOp, String> {
    match op {
        BinaryOp::Add => Ok(HirBinaryOp::Add),
        BinaryOp::Sub => Ok(HirBinaryOp::Subtract),
        BinaryOp::Mul => Ok(HirBinaryOp::Multiply),
        BinaryOp::Div => Ok(HirBinaryOp::Divide),

        BinaryOp::EqEq => Ok(HirBinaryOp::Equal),
        BinaryOp::NotEq => Ok(HirBinaryOp::NotEqual),
        BinaryOp::EqEqEq => Ok(HirBinaryOp::StrictEqual),
        BinaryOp::NotEqEq => Ok(HirBinaryOp::StrictNotEqual),

        BinaryOp::Gt => Ok(HirBinaryOp::Greater),
        BinaryOp::GtEq => Ok(HirBinaryOp::GreaterEqual),
        BinaryOp::Lt => Ok(HirBinaryOp::Less),
        BinaryOp::LtEq => Ok(HirBinaryOp::LessEqual),

        BinaryOp::InstanceOf => Ok(HirBinaryOp::InstanceOf),

        BinaryOp::LogicalAnd | BinaryOp::LogicalOr | BinaryOp::NullishCoalescing => {
            Err("Logical operators should be handled separately".to_string())
        }

        unsupported => Err(format!("Unsupported binary operator: {unsupported:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component_discovery::discover_root_component;
    use plec_model::build_semantic_graph;
    use plec_parser::parse_module;
    use std::collections::HashMap;

    fn build_and_lower(source: &str) -> Result<HirComponent, String> {
        let module = parse_module("test.tsx", source).expect("parse should succeed");
        let resolved_imports = HashMap::new();

        let modules = vec![module];
        let semantic_graph =
            build_semantic_graph(&modules, &resolved_imports).expect("graph should build");

        let root = discover_root_component(&modules, &semantic_graph, "test.tsx", None)
            .map_err(|e| e.to_string())?;

        lower_root_component(&root, &semantic_graph)
    }

    fn build_and_lower_with_name(source: &str, name: &str) -> Result<HirComponent, String> {
        let module = parse_module("test.tsx", source).expect("parse should succeed");
        let resolved_imports = HashMap::new();

        let modules = vec![module];
        let semantic_graph =
            build_semantic_graph(&modules, &resolved_imports).expect("graph should build");

        let root = discover_root_component(&modules, &semantic_graph, "test.tsx", Some(name))
            .map_err(|e| e.to_string())?;

        lower_root_component(&root, &semantic_graph)
    }

    #[test]
    fn lowers_simple_div() {
        let source = r#"
            export function App() {
                return <div>Hello</div>;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");

        assert_eq!(hir.id, ComponentId::new("test.tsx", "App"));
        assert_eq!(hir.root_nodes.len(), 1);
        assert_eq!(hir.nodes.len(), 2); // Element + Text

        if let HirNode::Element(el) = &hir.nodes[1] {
            assert_eq!(el.tag, "div");
            assert_eq!(el.children.len(), 1);
        } else {
            panic!("Expected element");
        }
    }

    #[test]
    fn lowers_static_attribute() {
        let source = r#"
            export function App() {
                return <div className="card">Hello</div>;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");

        if let HirNode::Element(el) = &hir.nodes[1] {
            assert_eq!(el.props.len(), 1);
            if let HirProp::Static { name, value } = &el.props[0] {
                assert_eq!(name, "className");
                assert_eq!(value, "card");
            } else {
                panic!("Expected static prop");
            }
        } else {
            panic!("Expected element");
        }
    }

    #[test]
    fn lowers_expression_attribute() {
        let source = r#"
            export function App({ name }) {
                return <div className={name}>Hello</div>;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");

        // Find the element node
        let element = hir.nodes.iter().find(|n| matches!(n, HirNode::Element(_)));
        assert!(element.is_some());

        if let HirNode::Element(el) = element.unwrap() {
            assert_eq!(el.props.len(), 1);
            if let HirProp::Expression { name, .. } = &el.props[0] {
                assert_eq!(name, "className");
            } else {
                panic!("Expected expression prop");
            }
        } else {
            panic!("Expected element");
        }
    }

    #[test]
    fn lowers_member_expression() {
        let source = r#"
            export function App() {
                const user = { name: "test" };
                return <div>Hello {user.name}</div>;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");

        // Should have a member expression
        let member_expr = hir
            .expressions
            .iter()
            .find(|e| matches!(e.expression, HirExpr::Member { .. }));
        assert!(member_expr.is_some());

        if let HirExpr::Member { property, .. } = &member_expr.unwrap().expression {
            assert_eq!(property, "name");
        } else {
            panic!("Expected member expression");
        }
    }

    #[test]
    fn lowers_location_and_sync_cookie_host_values() {
        let hir = build_and_lower(
            r#"
            export function App() {
                const location = useLocation();
                const [open, setOpen] = useState(cookie.getSync('sidebar') === 'open');
                return <div>{location.pathname}{open}</div>;
            }
        "#,
        )
        .expect("host values should lower");

        assert!(hir.inputs.iter().any(|input| input.kind == "location"));
        assert!(hir.expressions.iter().any(|expression| matches!(
            expression.expression,
            HirExpr::Host { ref kind, ref name } if kind == "cookie" && name.as_deref() == Some("sidebar")
        )));
        let executable = crate::lower_component_to_executable(&hir)
            .expect("host values should produce executable IR");
        assert_eq!(executable.host_slots.len(), 2);
        assert!(executable
            .host_slots
            .iter()
            .any(|slot| slot.kind == "location"));
        assert!(executable
            .host_slots
            .iter()
            .any(|slot| slot.kind == "cookie"));
        assert_eq!(executable.capabilities[0].name, "sidebar");
    }

    #[test]
    fn lowers_conditional() {
        let source = r#"
            export function App() {
                const ready = true;
                return ready ? <Ready /> : <Loading />;
            }

            function Ready() { return <div />; }
            function Loading() { return <div />; }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");

        assert_eq!(hir.root_nodes.len(), 1);

        if let HirNode::Conditional(cond) = &hir.nodes[hir.root_nodes[0].0 as usize] {
            assert_eq!(cond.consequent.len(), 1);
            assert_eq!(cond.alternate.len(), 1);
        } else {
            panic!("Expected conditional node");
        }
    }

    #[test]
    fn lowers_jsx_fragment() {
        let source = r#"
            export function App() {
                return <><div>A</div><div>B</div></>;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");

        assert_eq!(hir.root_nodes.len(), 1);

        if let HirNode::Fragment(frag) = &hir.nodes[hir.root_nodes[0].0 as usize] {
            assert_eq!(frag.children.len(), 2);
        } else {
            panic!("Expected fragment");
        }
    }

    #[test]
    fn lowers_custom_component() {
        let source = r#"
            export function App() {
                return <Header />;
            }

            function Header() { return <header />; }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");

        // Custom components start with uppercase, so they should be HirNode::Component
        if let HirNode::Component(comp) = &hir.nodes[hir.root_nodes[0].0 as usize] {
            assert_eq!(
                comp.target,
                HirComponentTarget::Static(ComponentId::new("test.tsx", "Header"))
            );
        } else {
            panic!("Expected component");
        }
    }

    #[test]
    fn lowers_null_to_empty() {
        let source = r#"
            export function NullComponent() {
                return null;
            }
        "#;

        let hir =
            build_and_lower_with_name(source, "NullComponent").expect("lowering should succeed");

        assert_eq!(hir.nodes.len(), 1);
        assert!(matches!(hir.nodes[0], HirNode::Empty));
    }

    #[test]
    fn lowers_false_to_empty() {
        let source = r#"
            export function FalseComponent() {
                return false;
            }
        "#;

        let hir =
            build_and_lower_with_name(source, "FalseComponent").expect("lowering should succeed");

        assert!(matches!(hir.nodes[0], HirNode::Empty));
    }

    #[test]
    fn lowers_event_handler_with_arrow() {
        let source = r#"
            export function Input() {
                const [value, setValue] = useState("");
                return <input onInput={(e) => setValue(e.target.value)} />;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");

        // Find the input element
        let element = hir
            .nodes
            .iter()
            .find(|n| matches!(n, HirNode::Element(el) if el.tag == "input"));
        assert!(element.is_some());

        if let HirNode::Element(el) = element.unwrap() {
            assert_eq!(el.props.len(), 0);
            assert_eq!(el.events.len(), 1);
            let event = &el.events[0];
            assert_eq!(event.event, "input");
            if let HirCallable::Inline { parameters, .. } = &event.callable {
                assert_eq!(parameters.len(), 1);
            } else {
                panic!("Expected Inline callable");
            }
        } else {
            panic!("Expected element");
        }
    }

    #[test]
    fn lowers_ts_as_erased() {
        let source = r#"
            export function App() {
                const input = { value: "test" };
                return <div>{(input as HTMLInputElement).value}</div>;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");

        // Should have a member expression for .value (the TypeScript cast should be erased)
        let member_expr = hir
            .expressions
            .iter()
            .find(|e| matches!(e.expression, HirExpr::Member { .. }));
        assert!(member_expr.is_some());

        if let HirExpr::Member { property, .. } = &member_expr.unwrap().expression {
            assert_eq!(property, "value");
        }
    }

    #[test]
    fn lowers_call_expression() {
        let source = r#"
            export function App() {
                const [value, setValue] = useState("");
                return <div>{setValue("test")}</div>;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");

        // Should have a call expression
        let call_expr = hir
            .expressions
            .iter()
            .find(|e| matches!(e.expression, HirExpr::Call { .. }));
        assert!(call_expr.is_some());

        if let HirExpr::Call { args, .. } = &call_expr.unwrap().expression {
            // Verify structure exists
            assert_eq!(args.len(), 1);
        }
    }

    #[test]
    fn lowers_logical_and_with_jsx() {
        let source = r#"
            export function App() {
                const show = true;
                return <div>{show && <p>Hello</p>}</div>;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");

        // Should have a conditional node (from && with JSX)
        let cond_node = hir
            .nodes
            .iter()
            .find(|n| matches!(n, HirNode::Conditional(_)));
        assert!(cond_node.is_some());

        if let HirNode::Conditional(cond) = cond_node.unwrap() {
            // Should have a consequent (the <p> element)
            assert_eq!(cond.consequent.len(), 1);
        }
    }

    #[test]
    fn lowers_conditional_with_string_branches() {
        let source = r#"
            export function App() {
                const count = 1;
                return <div>{count === 1 ? '1 item' : `${count} items`}</div>;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");

        // Should have a conditional node
        let cond_node = hir
            .nodes
            .iter()
            .find(|n| matches!(n, HirNode::Conditional(_)));
        assert!(cond_node.is_some());

        if let HirNode::Conditional(cond) = cond_node.unwrap() {
            // Both branches should be text nodes with expressions
            assert_eq!(cond.consequent.len(), 1);
            assert_eq!(cond.alternate.len(), 1);

            // Verify consequent is a text node with an expression
            if let HirNode::Text(HirText::Expression { .. }) =
                &hir.nodes[cond.consequent[0].0 as usize]
            {
                // Good
            } else {
                panic!("Expected consequent to be Expression text node");
            }

            // Verify alternate is a text node with an expression
            if let HirNode::Text(HirText::Expression { .. }) =
                &hir.nodes[cond.alternate[0].0 as usize]
            {
                // Good
            } else {
                panic!("Expected alternate to be Expression text node");
            }
        }
    }

    #[test]
    fn lowers_map_to_for_each() {
        let source = r#"
            export function App({ items }) {
                return <ul>{items.map((item) => <li key={item.id}>{item.id}</li>)}</ul>;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");

        // Should have a ForEach node
        let foreach_node = hir.nodes.iter().find(|n| matches!(n, HirNode::ForEach(_)));
        assert!(foreach_node.is_some());

        if let HirNode::ForEach(foreach) = foreach_node.unwrap() {
            assert!(matches!(
                hir.bindings[foreach.item_binding.0 as usize].kind,
                HirBindingKind::LoopItem
            ));
            assert_eq!(foreach.body.len(), 1);
            assert!(
                foreach.identity.is_some(),
                "key should become ForEach identity"
            );
            // Verify the body contains the <li> element
            if let HirNode::Element(el) = &hir.nodes[foreach.body[0].0 as usize] {
                assert_eq!(el.tag, "li");
                assert!(el.props.is_empty(), "key must not become an element prop");
            } else {
                panic!("Expected ForEach body to contain an element");
            }
        }
    }

    #[test]
    fn lowers_object_expression() {
        let source = r#"
            export function App() {
                function update() {}
                return <div onClick={() => update({ id: 123 })}>Click</div>;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");

        // Should have an object expression
        let obj_expr = hir
            .expressions
            .iter()
            .find(|e| matches!(e.expression, HirExpr::Object(_)));
        assert!(obj_expr.is_some());

        if let HirExpr::Object(props) = &obj_expr.unwrap().expression {
            assert_eq!(props.len(), 1);
            assert!(matches!(&props[0], HirObjectItem::Property { name, .. } if name == "id"));
        }
    }

    #[test]
    fn lowers_block_arrow_in_event_handler() {
        let source = r#"
            export function App() {
                const [value, setValue] = useState("");
                return <button onClick={() => {
                    setValue('test');
                }}>Click</button>;
            }
        "#;

        assert!(build_and_lower(source).is_ok());
    }

    #[test]
    fn component_callable_reference() {
        let source = r#"
            export function App() {
                function save() {}
                return <Child onSave={save} />;
            }

            function Child({ onSave }: { onSave(): void }) {
                return <button onClick={onSave} />;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");

        let component = hir
            .nodes
            .iter()
            .find(|n| matches!(n, HirNode::Component(comp) if matches!(&comp.target, HirComponentTarget::Static(target) if target.local_name == "Child")));
        assert!(component.is_some());

        if let HirNode::Component(comp) = component.unwrap() {
            assert_eq!(comp.props.len(), 1);
            if let HirProp::Callable {
                name,
                callable: HirCallable::Reference { .. },
            } = &comp.props[0]
            {
                assert_eq!(name, "onSave");
            } else {
                panic!("Expected callable reference prop");
            }
        }
    }

    #[test]
    fn component_inline_callable() {
        let source = r#"
            export function App() {
                function save() {}
                return <Child onSave={() => save()} />;
            }

            function Child({ onSave }: { onSave(): void }) {
                return <button onClick={onSave} />;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");

        let component = hir
            .nodes
            .iter()
            .find(|n| matches!(n, HirNode::Component(comp) if matches!(&comp.target, HirComponentTarget::Static(target) if target.local_name == "Child")));
        assert!(component.is_some());

        if let HirNode::Component(comp) = component.unwrap() {
            assert_eq!(comp.props.len(), 1);
            if let HirProp::Callable { name, callable } = &comp.props[0] {
                assert_eq!(name, "onSave");
                if let HirCallable::Inline { parameters, .. } = callable {
                    assert_eq!(parameters.len(), 0);
                } else {
                    panic!("Expected Inline callable");
                }
            } else {
                panic!("Expected Callable prop");
            }
        }
    }

    #[test]
    fn rejects_component_callable_member_reference() {
        let source = r#"
            export function App() {
                return <Child onSave={actions.save} />;
            }

            function Child({ onSave }: { onSave(): void }) {
                return <button onClick={onSave} />;
            }
        "#;

        assert!(build_and_lower(source)
            .unwrap_err()
            .contains("Callable values"));
    }

    #[test]
    fn rejects_reserved_runtime_attribute() {
        let source = r#"
            export function App() {
                return <div data-plec-node="x" />;
            }
        "#;

        let error = build_and_lower(source).unwrap_err();
        assert!(
            error.contains("Reserved Plec DOM attribute 'data-plec-node'"),
            "unexpected diagnostic: {error}"
        );
    }

    #[test]
    fn rejects_reserved_row_key_attribute() {
        let source = r#"
            export function App() {
                return <li data-runtime-row-key="y" />;
            }
        "#;

        let error = build_and_lower(source).unwrap_err();
        assert!(
            error.contains("Reserved Plec DOM attribute 'data-runtime-row-key'"),
            "unexpected diagnostic: {error}"
        );
    }

    #[test]
    fn rejects_reserved_marker_grammar_attribute() {
        let source = r#"
            export function App() {
                return <div plec:text="z" />;
            }
        "#;

        let error = build_and_lower(source).unwrap_err();
        assert!(
            error.contains("Reserved Plec DOM attribute 'plec:text'"),
            "unexpected diagnostic: {error}"
        );
    }

    #[test]
    fn rejects_reserved_attribute_on_component_call() {
        let source = r#"
            export function App() {
                return <Child data-runtime-node="w" />;
            }

            function Child({ value }: { value: string }) {
                return <div>{value}</div>;
            }
        "#;

        let error = build_and_lower(source).unwrap_err();
        assert!(
            error.contains("Reserved Plec DOM attribute 'data-runtime-node'"),
            "unexpected diagnostic: {error}"
        );
    }

    #[test]
    fn allows_unreserved_data_attribute_and_spread() {
        let source = r#"
            export function App({ bag }: { bag: Record<string, string> }) {
                return <div data-variant="ok" {...bag} />;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");
        let node_id = hir.root_nodes[0];
        let HirNode::Element(el) = &hir.nodes[node_id.0 as usize] else {
            panic!("expected element root");
        };
        assert!(matches!(&el.props[0], HirProp::Static { name, .. } if name == "data-variant"));
        assert!(matches!(&el.props[1], HirProp::Spread { .. }));
    }

    #[test]
    fn rejects_lowercase_string_event_handler_attribute() {
        let source = r#"
            export function App() {
                return <div onclick="alert(1)" />;
            }
        "#;

        let error = build_and_lower(source).unwrap_err();
        assert!(
            error.contains("canonical camelCase form (onClick)"),
            "unexpected diagnostic: {error}"
        );
    }

    #[test]
    fn rejects_uppercase_string_event_handler_attribute() {
        let source = r#"
            export function App() {
                return <div ONCLICK="alert(1)" />;
            }
        "#;

        let error = build_and_lower(source).unwrap_err();
        assert!(
            error.contains("'ONCLICK' must be a canonical camelCase handler"),
            "unexpected diagnostic: {error}"
        );
    }

    #[test]
    fn rejects_srcdoc_attribute() {
        let source = r#"
            export function App() {
                return <iframe srcdoc="<script>alert(1)</script>" />;
            }
        "#;

        let error = build_and_lower(source).unwrap_err();
        assert!(
            error.contains("'srcdoc' is a document sink"),
            "unexpected diagnostic: {error}"
        );
    }

    #[test]
    fn rejects_javascript_url_literal() {
        for scheme in [
            "javascript:alert(1)",
            "JAVASCRIPT:alert(1)",
            "vbscript:msgbox(1)",
        ] {
            let source = format!(
                r#"
                    export function App() {{
                        return <a href="{scheme}">x</a>;
                    }}
                "#
            );
            let error = build_and_lower(&source).unwrap_err();
            assert!(
                error.contains("unsafe URL scheme"),
                "unexpected diagnostic for {scheme}: {error}"
            );
        }
    }

    #[test]
    fn allows_canonical_event_handler_and_safe_url() {
        let source = r#"
            export function App() {
                return <a href="https://plec.dev" onClick={() => {}}>x</a>;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");
        let node_id = hir.root_nodes[0];
        let HirNode::Element(el) = &hir.nodes[node_id.0 as usize] else {
            panic!("expected element root");
        };
        assert!(matches!(&el.props[0], HirProp::Static { name, .. } if name == "href"));
        assert_eq!(el.events.len(), 1);
        assert_eq!(el.events[0].event, "click");
    }

    #[test]
    fn rejects_unresolved_value_member_reference() {
        let source = r#"
            export function App() {
                return <Child onStatus={actions.status} />;
            }

            function Child({ onStatus }: { onStatus: string }) {
                return <div>{onStatus}</div>;
            }
        "#;

        assert!(build_and_lower(source).is_err());
    }

    #[test]
    fn component_callable_conditional_uses_declared_prop() {
        let source = r#"
            export function App() {
                const editing = true;
                function onCancel() {}
                function onStartEdit() {}
                return <Child onSave={editing ? onCancel : onStartEdit} />;
            }

            function Child({ onSave }: { onSave(): void }) {
                return <button onClick={onSave} />;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");
        let component = hir.nodes.iter().find(|node| matches!(node, HirNode::Component(comp) if matches!(&comp.target, HirComponentTarget::Static(target) if target.local_name == "Child"))).unwrap();
        assert!(matches!(
            component,
            HirNode::Component(comp) if matches!(comp.props.first(), Some(HirProp::Callable { name, callable: HirCallable::Conditional { .. } }) if name == "onSave")
        ));
    }

    #[test]
    fn dom_named_event() {
        let source = r#"
            export function App() {
                function handleClick() {}
                return <button onClick={handleClick}>Click</button>;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");

        let element = hir
            .nodes
            .iter()
            .find(|n| matches!(n, HirNode::Element(el) if el.tag == "button"));
        assert!(element.is_some());

        if let HirNode::Element(el) = element.unwrap() {
            assert_eq!(el.props.len(), 0);
            assert_eq!(el.events.len(), 1);
            let event = &el.events[0];
            assert_eq!(event.event, "click");
            if let HirCallable::Reference { .. } = &event.callable {
                // Good - identifier in DOM event context IS treated as callable
            } else {
                panic!("Expected Reference callable for DOM event");
            }
        }
    }

    #[test]
    fn dom_inline_event() {
        let source = r#"
            export function App() {
                const [value, setValue] = useState("");
                return <input onInput={(event) => setValue(event.currentTarget.value)} />;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");

        let element = hir
            .nodes
            .iter()
            .find(|n| matches!(n, HirNode::Element(el) if el.tag == "input"));
        assert!(element.is_some());

        if let HirNode::Element(el) = element.unwrap() {
            assert_eq!(el.props.len(), 0);
            assert_eq!(el.events.len(), 1);
            let event = &el.events[0];
            assert_eq!(event.event, "input");
            if let HirCallable::Inline { parameters, .. } = &event.callable {
                assert_eq!(parameters.len(), 1);
            } else {
                panic!("Expected Inline callable");
            }
        }
    }

    #[test]
    fn dom_conditional_event() {
        let source = r#"
            export function App() {
                const editing = true;
                function onCancel() {}
                function onStartEdit() {}
                return <button onClick={editing ? onCancel : onStartEdit}>Toggle</button>;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");

        let element = hir
            .nodes
            .iter()
            .find(|n| matches!(n, HirNode::Element(el) if el.tag == "button"));
        assert!(element.is_some());

        if let HirNode::Element(el) = element.unwrap() {
            assert_eq!(el.props.len(), 0);
            assert_eq!(el.events.len(), 1);
            let event = &el.events[0];
            assert_eq!(event.event, "click");
            if let HirCallable::Conditional { test, .. } = &event.callable {
                // Verify test expression exists
                let test_expr = hir.expressions.iter().find(|e| e.id == *test);
                assert!(test_expr.is_some());
            } else {
                panic!("Expected Conditional callable");
            }
        }
    }

    #[test]
    fn event_name_normalization() {
        let source = r#"
            export function App() {
                function handleSubmit() {}
                function handleChange() {}
                return <form onSubmit={handleSubmit}><input onChange={handleChange} /></form>;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");

        let form = hir
            .nodes
            .iter()
            .find(|n| matches!(n, HirNode::Element(el) if el.tag == "form"));
        assert!(form.is_some());

        if let HirNode::Element(el) = form.unwrap() {
            assert_eq!(el.events.len(), 1);
            assert_eq!(el.events[0].event, "submit");
        }

        let input = hir
            .nodes
            .iter()
            .find(|n| matches!(n, HirNode::Element(el) if el.tag == "input"));
        assert!(input.is_some());

        if let HirNode::Element(el) = input.unwrap() {
            assert_eq!(el.events.len(), 1);
            assert_eq!(el.events[0].event, "change");
        }
    }

    #[test]
    fn component_on_prop_not_event() {
        let source = r#"
            export function App() {
                function save() {}
                function cancel() {}
                return <TodoRow onSave={() => save()} onCancel={cancel} />;
            }

            function TodoRow({ onSave, onCancel }: { onSave(): void; onCancel(): void }) {
                return <button onClick={onSave} />;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");

        let component = hir
            .nodes
            .iter()
            .find(|n| matches!(n, HirNode::Component(comp) if matches!(&comp.target, HirComponentTarget::Static(target) if target.local_name == "TodoRow")));
        assert!(component.is_some());

        if let HirNode::Component(comp) = component.unwrap() {
            // Component props should NOT have events - events only for intrinsic elements
            assert!(comp.props.len() >= 2);

            // Find onSave and onCancel
            let on_save = comp
                .props
                .iter()
                .find(|p| matches!(p, HirProp::Callable { name, .. } if name == "onSave"));
            assert!(on_save.is_some(), "onSave should be a Callable prop");

            let on_cancel = comp.props.iter().find(|p| {
                matches!(p, HirProp::Callable { name, .. } | HirProp::Expression { name, .. } if name == "onCancel")
            });
            assert!(on_cancel.is_some(), "onCancel should exist");
        }
    }

    #[test]
    fn preserves_state_callable_closure_and_hoisting() {
        let source = r#"
            export function Counter() {
                const [title, setTitle] = useState("");
                const prefix = "x";
                return <button onClick={save}>{title}</button>;
                function save(value) {
                    setTitle(prefix + value);
                }
            }
        "#;
        let hir = build_and_lower(source).unwrap();
        assert_eq!(hir.states.len(), 1);
        let state = &hir.states[0];
        assert!(matches!(
            hir.bindings[state.value.0 as usize].kind,
            HirBindingKind::StateValue
        ));
        assert!(
            matches!(hir.bindings[state.setter.0 as usize].kind, HirBindingKind::StateSetter { state: id } if id == state.value)
        );
        let save = hir
            .callables
            .iter()
            .find(|callable| hir.bindings[callable.binding.0 as usize].name == "save")
            .unwrap();
        let HirCallableBody::Block(statements) = &save.body else {
            panic!("save should have a block body")
        };
        assert!(
            matches!(statements.first(), Some(HirStmt::StateUpdate { state: id, .. }) if *id == state.value)
        );
        let button = hir
            .nodes
            .iter()
            .find_map(|node| match node {
                HirNode::Element(element) if element.tag == "button" => Some(element),
                _ => None,
            })
            .unwrap();
        assert!(
            matches!(button.events[0].callable, HirCallable::Reference { binding } if binding == save.binding)
        );
    }

    #[test]
    fn callable_component_parameter_uses_sema_classification() {
        let source = r#"
            export function Child({ onSave }: { onSave(): void }) {
                return <button onClick={onSave}>Save</button>;
            }
        "#;
        let hir = build_and_lower(source).unwrap();
        let parameter = hir.parameters.first().unwrap();
        assert!(matches!(
            hir.bindings[parameter.binding.0 as usize].kind,
            HirBindingKind::Parameter { callable: true }
        ));
        let event = hir
            .nodes
            .iter()
            .find_map(|node| match node {
                HirNode::Element(element) => element.events.first(),
                _ => None,
            })
            .unwrap();
        assert!(
            matches!(event.callable, HirCallable::Reference { binding } if binding == parameter.binding)
        );
    }

    #[test]
    fn lowers_functional_state_updaters_and_rejects_unknown_bindings() {
        let hir = build_and_lower(r#"export function App() { const [count, setCount] = useState(0); function increment() { setCount(c => c + 1); } return <button onClick={increment}>{count}</button>; }"#)
            .expect("functional updater should lower");
        let action = hir
            .callables
            .iter()
            .find(|callable| hir.bindings[callable.binding.0 as usize].name == "increment")
            .unwrap();
        assert!(
            matches!(action.body, HirCallableBody::Block(ref statements) if matches!(statements.first(), Some(HirStmt::StateUpdate { .. })))
        );
        assert!(
            build_and_lower(r#"export function App() { return <div>{missing}</div>; }"#)
                .unwrap_err()
                .contains("Unresolved lexical binding 'missing'")
        );
    }

    #[test]
    fn preserves_action_if_and_shadowed_callable_parameter() {
        let source = r#"
            export function App() {
                const value = "outer";
                const valid = true;
                const [status, setStatus] = useState("");
                function save(value) {
                    if (valid) {
                        setStatus("saved");
                    } else {
                        setStatus(value);
                    }
                }
                return <button onClick={save}>{status}</button>;
            }
        "#;
        let hir = build_and_lower(source).unwrap();
        let outer_value = hir
            .bindings
            .iter()
            .find(|binding| {
                binding.name == "value" && matches!(binding.kind, HirBindingKind::Local)
            })
            .unwrap()
            .id;
        let save = hir
            .callables
            .iter()
            .find(|callable| hir.bindings[callable.binding.0 as usize].name == "save")
            .unwrap();
        assert_ne!(outer_value, save.parameters[0]);
        let HirCallableBody::Block(statements) = &save.body else {
            panic!("save should have a block body")
        };
        assert!(
            matches!(statements.first(), Some(HirStmt::If { consequent, alternate, .. })
            if matches!(consequent.first(), Some(HirStmt::StateUpdate { .. }))
            && matches!(alternate.first(), Some(HirStmt::StateUpdate { .. })))
        );
    }

    #[test]
    fn resolves_cross_module_component_calls_to_defining_symbols() {
        let modules = vec![
            parse_module(
                "src/App.tsx",
                r#"import { Card as PrimaryCard } from "./primary"; import { Card as SecondaryCard } from "./secondary"; export function App() { return <PrimaryCard><SecondaryCard /></PrimaryCard>; }"#,
            )
            .unwrap(),
            parse_module(
                "src/primary.tsx",
                r#"export function Card() { return <div />; }"#,
            )
            .unwrap(),
            parse_module(
                "src/secondary.tsx",
                r#"export function Card() { return <section />; }"#,
            )
            .unwrap(),
        ];
        let resolved_imports = HashMap::from([
            (
                ("src/App.tsx".to_string(), "./primary".to_string()),
                "src/primary.tsx".to_string(),
            ),
            (
                ("src/App.tsx".to_string(), "./secondary".to_string()),
                "src/secondary.tsx".to_string(),
            ),
        ]);
        let graph = build_semantic_graph(&modules, &resolved_imports).unwrap();
        let root = discover_root_component(&modules, &graph, "src/App.tsx", Some("App")).unwrap();
        let hir = lower_root_component(&root, &graph).unwrap();
        let targets = hir
            .nodes
            .iter()
            .filter_map(|node| match node {
                HirNode::Component(call) => Some(call.target.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert!(
            targets.contains(&HirComponentTarget::Static(ComponentId::new(
                "src/primary.tsx",
                "Card"
            )))
        );
        assert!(
            targets.contains(&HirComponentTarget::Static(ComponentId::new(
                "src/secondary.tsx",
                "Card"
            )))
        );
    }

    #[test]
    fn resolves_re_exported_component_calls_to_defining_symbols() {
        let modules = vec![
            parse_module(
                "src/App.tsx",
                r#"import { Bar } from "./index"; export function App() { return <Bar />; }"#,
            )
            .unwrap(),
            parse_module("src/index.ts", r#"export { Foo as Bar } from "./foo";"#).unwrap(),
            parse_module(
                "src/foo.tsx",
                r#"export function Foo() { return <div />; }"#,
            )
            .unwrap(),
        ];
        let resolved_imports = HashMap::from([
            (
                ("src/App.tsx".to_string(), "./index".to_string()),
                "src/index.ts".to_string(),
            ),
            (
                ("src/index.ts".to_string(), "./foo".to_string()),
                "src/foo.tsx".to_string(),
            ),
        ]);
        let graph = build_semantic_graph(&modules, &resolved_imports).unwrap();
        let root = discover_root_component(&modules, &graph, "src/App.tsx", Some("App")).unwrap();
        let hir = lower_root_component(&root, &graph).unwrap();
        let call = hir
            .nodes
            .iter()
            .find_map(|node| match node {
                HirNode::Component(call) => Some(call),
                _ => None,
            })
            .unwrap();

        assert_eq!(
            call.target,
            HirComponentTarget::Static(ComponentId::new("src/foo.tsx", "Foo"))
        );
    }

    #[test]
    fn rejects_unresolved_component_calls() {
        let source = "export function App() { return <Missing />; }";
        assert!(build_and_lower(source)
            .unwrap_err()
            .contains("Unresolved component 'Missing'"));
    }

    #[test]
    fn rejects_unsupported_structural_paths() {
        assert!(
            build_and_lower("export function App() { return <div>{left + right}</div>; }")
                .unwrap_err()
                .contains("Non-logical binary expressions")
        );
        assert!(
            build_and_lower("export function App() { return <div>{...children}</div>; }")
                .unwrap_err()
                .contains("spread children")
        );
    }

    #[test]
    fn lowers_direct_state_setters_as_component_callback_props() {
        let hir = build_and_lower_with_name(
            "function Child({ onChange }: { onChange: (value: string) => void }) { return <button />; } export function App() { const [title, setTitle] = useState(''); return <Child onChange={setTitle} />; }",
            "App",
        )
        .unwrap();
        assert!(hir.nodes.iter().any(|node| matches!(
            node,
            HirNode::Component(component)
                if matches!(component.props.first(), Some(HirProp::Callable { callable: HirCallable::Inline { body, .. }, .. }) if matches!(body, HirCallableBody::Block(statements) if matches!(statements.first(), Some(HirStmt::StateUpdate { .. }))))
        )));
    }

    #[test]
    fn erases_void_for_known_local_calls_inside_action_branches() {
        let hir = build_and_lower(
            "export function App() { const [enabled, setEnabled] = useState(true); function save() {} return <button onClick={() => { if (enabled) void save(); }}>Save</button>; }",
        )
        .unwrap();
        assert!(hir
            .nodes
            .iter()
            .any(|node| matches!(node, HirNode::Element(_))));
    }

    #[test]
    fn lowers_object_rest_props_for_intrinsic_forwarding() {
        let hir = build_and_lower(
            "export function Icon({ title, ...props }) { return <svg {...props}>{title}</svg>; }",
        )
        .unwrap();
        assert!(hir
            .expressions
            .iter()
            .any(|expression| matches!(expression.expression, HirExpr::ObjectWithout { .. })));
        assert!(hir.nodes.iter().any(|node| matches!(node,
            HirNode::Element(HirElement { props, .. }) if props.iter().any(|prop| matches!(prop, HirProp::Spread { .. }))
        )));
    }

    #[test]
    fn lowers_dynamic_fetch_headers_and_json_body() {
        let hir = build_and_lower(
            r#"export function App() {
                const [title, setTitle] = useState('');
                async function save() {
                    await fetch('/todos', {
                        method: 'POST',
                        headers: { 'content-type': 'application/json' },
                        body: JSON.stringify({ title }),
                    });
                }
                return <button onClick={save}>Save</button>;
            }"#,
        )
        .unwrap();
        let action = hir
            .callables
            .iter()
            .find(|callable| hir.bindings[callable.binding.0 as usize].name == "save")
            .unwrap();
        assert!(matches!(action.body,
            HirCallableBody::Block(ref statements) if matches!(statements.first(), Some(HirStmt::AwaitFetch { headers, body: Some(_), .. }) if headers.len() == 1)
        ));
    }

    #[test]
    fn lowers_pure_map_and_filter_value_transforms() {
        let hir = build_and_lower(
            "export function App() { const [items, setItems] = useState([]); const visible = items.filter(item => item.title.toLowerCase().includes('a')); return <button onClick={() => setItems(values => values.map(item => ({ ...item, title: item.title.trim() })))}>{visible.length}</button>; }",
        ).unwrap();
        assert!(hir
            .expressions
            .iter()
            .any(|expression| matches!(expression.expression, HirExpr::Filter { .. })));
        assert!(hir
            .expressions
            .iter()
            .any(|expression| matches!(expression.expression, HirExpr::Map { .. })));
    }

    #[test]
    fn lowers_route_loader_data_as_a_typed_input() {
        let hir = build_and_lower(
            "export function Page() { const todos = Route.useLoaderData(); return <p>{todos.length}</p>; }",
        ).unwrap();
        assert!(hir.inputs.iter().any(|input| input.kind == "loaderData"));
    }

    #[test]
    fn lowers_typed_error_guards_and_throw_values() {
        let hir = build_and_lower(
            "export function App() { async function save() { try { throw new Error('no'); } catch (error) { return error instanceof Error ? error.message : 'unknown'; } } return <button onClick={save}>Save</button>; }",
        ).unwrap();
        let action = hir
            .callables
            .iter()
            .find(|callable| hir.bindings[callable.binding.0 as usize].name == "save")
            .unwrap();
        assert!(
            matches!(action.body, HirCallableBody::Block(ref statements) if matches!(statements.first(), Some(HirStmt::Try { body, .. }) if matches!(body.first(), Some(HirStmt::Throw { .. })) ))
        );
    }

    #[test]
    fn lowers_response_status_and_buffered_json_body() {
        let hir = build_and_lower(
            "type Todo = { id: string }; export function App() { async function load() { const response = await fetch('/todos'); if (!response.ok) throw new Error('no'); const todos = (await response.json()) as Todo[]; return todos; } return <button onClick={load}>Load</button>; }",
        ).unwrap();
        let action = hir
            .callables
            .iter()
            .find(|callable| hir.bindings[callable.binding.0 as usize].name == "load")
            .unwrap();
        assert!(
            matches!(action.body, HirCallableBody::Block(ref statements) if statements.iter().any(|statement| matches!(statement, HirStmt::AsyncAssign { .. })))
        );
    }

    #[test]
    fn lowers_function_typed_async_callback_parameters_and_local_values() {
        let hir = build_and_lower(
            "export function App() { async function request(label: string, action: () => Promise<Response>) { const normalized = label.trim(); const response = await action(); return normalized; } return <button onClick={request}>Run</button>; }",
        )
        .unwrap();
        let action = hir
            .callables
            .iter()
            .find(|callable| hir.bindings[callable.binding.0 as usize].name == "request")
            .unwrap();
        assert!(matches!(
            hir.bindings[action.parameters[1].0 as usize].kind,
            HirBindingKind::Parameter { callable: true }
        ));
        assert!(matches!(
            action.body,
            HirCallableBody::Block(ref statements)
                if matches!(statements.first(), Some(HirStmt::AsyncAssign { .. }))
                    && matches!(statements.get(1), Some(HirStmt::AwaitCall { .. }))
        ));
    }

    #[test]
    fn lowers_inline_fetch_callbacks_passed_to_local_async_helpers() {
        let hir = build_and_lower(
            "export function App() { async function request(action: () => Promise<Response>) { const result = await action(); return result; } async function save() { const response = await request(() => fetch('/todos', { method: 'POST', body: JSON.stringify({ title: 'Plec' }) })); return response; } return <button onClick={save}>Save</button>; }",
        )
        .unwrap();
        let save = hir
            .callables
            .iter()
            .find(|callable| hir.bindings[callable.binding.0 as usize].name == "save")
            .unwrap();
        assert!(matches!(
            save.body,
            HirCallableBody::Block(ref statements)
                if matches!(statements.first(), Some(HirStmt::AwaitFetch { method, body: Some(_), .. }) if method == "POST")
        ));
    }

    #[test]
    fn lowers_a_finite_component_ternary_as_a_reactive_conditional() {
        let hir = build_and_lower_with_name(
            "function A() { return <p>A</p>; } function B() { return <p>B</p>; } export function App() { const [on, setOn] = useState(true); const Choice = on ? A : B; return <Choice />; }",
            "App",
        ).unwrap();
        assert!(hir
            .nodes
            .iter()
            .any(|node| matches!(node, HirNode::Conditional(_))));
    }

    #[test]
    fn lowers_reachable_components_and_rejects_cycles() {
        let modules = vec![parse_module(
            "App.tsx",
            r#"
            export function App() { return <Child />; }
            function Child() { return <div>child</div>; }
        "#,
        )
        .unwrap()];
        let graph = build_semantic_graph(&modules, &HashMap::new()).unwrap();
        let root = discover_root_component(&modules, &graph, "App.tsx", Some("App")).unwrap();
        let app = lower_application(&modules, &root, &graph).unwrap();
        assert_eq!(app.components.len(), 2);
        assert_eq!(app.root, ComponentId::new("App.tsx", "App"));

        let modules = vec![parse_module(
            "Cycle.tsx",
            r#"
            export function App() { return <Child />; }
            function Child() { return <App />; }
        "#,
        )
        .unwrap()];
        let graph = build_semantic_graph(&modules, &HashMap::new()).unwrap();
        let root = discover_root_component(&modules, &graph, "Cycle.tsx", Some("App")).unwrap();
        assert!(lower_application(&modules, &root, &graph)
            .unwrap_err()
            .contains("Recursive component 'App'"));
    }

    fn lower_with_custom_elements(
        source: &str,
        custom_elements: &[&str],
    ) -> Result<HirComponent, String> {
        let module = parse_module("test.tsx", source).expect("parse should succeed");
        let modules = vec![module];
        let semantic_graph =
            build_semantic_graph(&modules, &HashMap::new()).expect("graph should build");
        let root = discover_root_component(&modules, &semantic_graph, "test.tsx", Some("App"))
            .map_err(|e| e.to_string())?;
        lower_root_component_with_options(
            &root,
            &semantic_graph,
            &custom_elements
                .iter()
                .map(|tag| tag.to_string())
                .collect::<std::collections::BTreeSet<_>>(),
        )
    }

    #[test]
    fn rejects_forbidden_intrinsic_elements_at_compile_time() {
        for tag in [
            "script", "base", "object", "embed", "iframe", "link", "meta", "style",
        ] {
            let error = lower_with_custom_elements(
                &format!(
                    "export function App() {{ return <{tag} src=\"https://example.test/x\"></{tag}>; }}"
                ),
                &[],
            )
            .unwrap_err();
            assert!(
                error.contains(&format!("Forbidden element tag '{tag}'")),
                "{tag} must produce a forbidden-tag diagnostic, got: {error}"
            );
        }
    }

    #[test]
    fn rejects_unsupported_intrinsic_elements_at_compile_time() {
        // `foo` is neither a standard HTML/SVG tag nor a configured custom
        // element; the runtime would reject the compiled artifact, so the
        // compiler rejects it first.
        let error =
            lower_with_custom_elements("export function App() { return <foo></foo>; }", &[])
                .unwrap_err();
        assert!(
            error.contains("Unsupported element tag 'foo'"),
            "must produce an unsupported-tag diagnostic, got: {error}"
        );
    }

    #[test]
    fn custom_elements_require_compiler_configuration() {
        let error = lower_with_custom_elements(
            "export function App() { return <my-widget></my-widget>; }",
            &[],
        )
        .unwrap_err();
        assert!(error.contains("Custom element 'my-widget' is not configured"));

        let hir = lower_with_custom_elements(
            "export function App() { return <my-widget></my-widget>; }",
            &["my-widget"],
        )
        .expect("configured custom element must compile");
        assert!(hir
            .nodes
            .iter()
            .any(|node| matches!(node, HirNode::Element(element) if element.tag == "my-widget")));
    }

    #[test]
    fn standard_intrinsic_elements_still_compile() {
        let hir = lower_with_custom_elements(
            "export function App() { return <section><svg viewBox=\"0 0 1 1\"><circle cx=\"0\" cy=\"0\" r=\"1\" /></svg></section>; }",
            &[],
        )
        .expect("standard HTML/SVG elements must compile");
        assert!(hir
            .nodes
            .iter()
            .any(|node| matches!(node, HirNode::Element(element) if element.tag == "circle")));
    }
}
