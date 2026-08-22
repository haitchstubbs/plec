use crate::component_discovery::{ReturnedComponentExpression, RootComponent};
use plec_hir::{
    ComponentId, ExprId, HirBinaryOp, HirCallable, HirComponent, HirComponentCall, HirConditional, HirElement,
    HirEventBinding, HirExpr, HirExprNode, HirForEach, HirFragment, HirLogicalOp, HirNode, HirProp,
    HirTemplatePart, HirText, HirUnaryOp, HirValue, NodeId, SourceSpan,
};
use plec_sema::{ComponentPropKind, SemanticGraph};
use std::collections::HashMap;
use swc_common::{Span, Spanned};
use swc_ecma_ast::{
    BinaryOp, Callee, Expr, JSXAttr, JSXAttrName, JSXAttrOrSpread, JSXAttrValue, JSXElement,
    JSXElementName, JSXExpr, JSXFragment, JSXObject, Lit,
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
            _ if event.chars().next().map(char::is_uppercase).unwrap_or(false) => {
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

/// Detect an explicitly inline callable when no component prop signature exists.
fn detect_inline_callable(expr: &Expr, ctx: &mut HirLoweringCtx) -> Result<Option<HirCallable>, String> {
    match expr {
        Expr::Arrow(arrow) => {
            let params: Vec<String> = arrow.params.iter().filter_map(|p| {
                match p {
                    swc_ecma_ast::Pat::Ident(ident) => Some(ident.id.sym.to_string()),
                    _ => None,
                }
            }).collect();

            let body_id = match &*arrow.body {
                swc_ecma_ast::ArrowFunctionBody::Expr(body_expr) => {
                    lower_expression(body_expr, ctx)?
                }
                swc_ecma_ast::ArrowFunctionBody::FunctionBody(block) => {
                    let mut last_expr_id = None;
                    for stmt in &block.stmts {
                        if let swc_ecma_ast::Stmt::Expr(expr_stmt) = stmt {
                            last_expr_id = Some(lower_expression(&*expr_stmt.expr, ctx)?);
                        }
                    }
                    last_expr_id.ok_or("Block function must have at least one expression")?
                }
            };

            Ok(Some(HirCallable::Inline { params, body: body_id }))
        }
        Expr::Cond(cond) => {
            let test_id = lower_expression(&cond.test, ctx)?;
            let consequent = detect_inline_callable(&cond.cons, ctx)?
                .ok_or("Conditional consequent must be callable")?;
            let alternate = detect_inline_callable(&cond.alt, ctx)?
                .ok_or("Conditional alternate must be callable")?;
            Ok(Some(HirCallable::Conditional {
                test: test_id,
                consequent: Box::new(consequent),
                alternate: Box::new(alternate),
            }))
        }
        _ => {
            // Without a declared callable prop, only inline function syntax is callable.
            Ok(None)
        }
    }
}

/// Detect a callable value for a DOM event or a declared callable component prop.
fn detect_callable_value(expr: &Expr, ctx: &mut HirLoweringCtx) -> Result<Option<HirCallable>, String> {
    match expr {
        Expr::Ident(ident) => {
            let expr_id = ctx.alloc_expr_id();
            ctx.add_expression(HirExprNode::new(
                expr_id,
                HirExpr::Identifier(ident.sym.to_string()),
                source_span_from_swc(ident.span),
            ));
            Ok(Some(HirCallable::Reference { expression: expr_id }))
        }
        Expr::Arrow(arrow) => {
            let params: Vec<String> = arrow.params.iter().filter_map(|p| {
                match p {
                    swc_ecma_ast::Pat::Ident(ident) => Some(ident.id.sym.to_string()),
                    _ => None,
                }
            }).collect();

            let body_id = match &*arrow.body {
                swc_ecma_ast::ArrowFunctionBody::Expr(body_expr) => {
                    lower_expression(body_expr, ctx)?
                }
                swc_ecma_ast::ArrowFunctionBody::FunctionBody(block) => {
                    let mut last_expr_id = None;
                    for stmt in &block.stmts {
                        if let swc_ecma_ast::Stmt::Expr(expr_stmt) = stmt {
                            last_expr_id = Some(lower_expression(&*expr_stmt.expr, ctx)?);
                        }
                    }
                    last_expr_id.ok_or("Block function must have at least one expression")?
                }
            };

            Ok(Some(HirCallable::Inline { params, body: body_id }))
        }
        Expr::Cond(cond) => {
            let test_id = lower_expression(&cond.test, ctx)?;
            let consequent = detect_callable_value(&cond.cons, ctx)?
                .ok_or("Conditional consequent must be callable")?;
            let alternate = detect_callable_value(&cond.alt, ctx)?
                .ok_or("Conditional alternate must be callable")?;
            Ok(Some(HirCallable::Conditional {
                test: test_id,
                consequent: Box::new(consequent),
                alternate: Box::new(alternate),
            }))
        }
        Expr::Member(_) | Expr::Call(_) | Expr::Paren(_) => {
            // These could be callables - lower as expression reference
            let expr_id = lower_expression(expr, ctx)?;
            Ok(Some(HirCallable::Reference { expression: expr_id }))
        }
        _ => {
            // Not a callable
            Ok(None)
        }
    }
}

/// HIR lowering context.
///
/// Maintains ID generation and node/expression storage while lowering.
#[derive(Debug, Clone)]
struct HirLoweringCtx {
    next_expr_id: u32,
    next_node_id: u32,
    expressions: Vec<HirExprNode>,
    nodes: Vec<HirNode>,
    component_props: HirComponentPropLookup,
}

impl HirLoweringCtx {
    fn new(component_props: &HirComponentPropLookup) -> Self {
        Self {
            next_expr_id: 0,
            next_node_id: 0,
            expressions: Vec::new(),
            nodes: Vec::new(),
            component_props: component_props.clone(),
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

    fn add_expression(&mut self, expr: HirExprNode) {
        self.expressions.push(expr);
    }

    fn add_node(&mut self, node: HirNode) -> NodeId {
        let id = self.alloc_node_id();
        self.nodes.push(node);
        id
    }
}

/// Compiler-owned view of the component prop facts HIR lowering needs.
#[derive(Debug, Clone, Default)]
pub struct HirComponentPropLookup {
    components: HashMap<String, HashMap<String, ComponentPropKind>>,
}

impl HirComponentPropLookup {
    fn prop_kind(&self, component: &str, prop: &str) -> Option<ComponentPropKind> {
        self.components.get(component)?.get(prop).copied()
    }
}

pub fn build_component_prop_lookup(
    semantic_graph: &SemanticGraph,
    module_id: &str,
) -> HirComponentPropLookup {
    let components = semantic_graph
        .get_module(module_id)
        .map(|module| module.component_props.clone())
        .unwrap_or_default();
    HirComponentPropLookup { components }
}

/// Lower a discovered root component to HIR.
pub fn lower_root_component(
    root: &RootComponent<'_>,
    component_props: &HirComponentPropLookup,
) -> Result<HirComponent, String> {
    let mut ctx = HirLoweringCtx::new(component_props);

    let span = source_span_from_swc(root.symbol.span);

    let root_node_id = match lower_returned_expression(&root.returned, &mut ctx)? {
        Some(node_id) => node_id,
        None => ctx.add_node(HirNode::Empty),
    };

    Ok(HirComponent {
        id: ComponentId(0),
        module_id: root.symbol.module_id.clone(),
        name: root.symbol.local_name.clone(),
        root_nodes: vec![root_node_id],
        nodes: ctx.nodes,
        expressions: ctx.expressions,
        span,
    })
}

/// Lower the returned expression to HIR nodes.
///
/// Returns `None` for statically absent content (null, false, undefined).
fn lower_returned_expression(
    returned: &ReturnedComponentExpression<'_>,
    ctx: &mut HirLoweringCtx,
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
        ReturnedComponentExpression::NonJsxReturn(_expr) => {
            // Non-JSX returns become empty nodes for now
            Ok(None)
        }
    }
}

/// Lower a JSX element to HIR.
fn lower_jsx_element(jsx: &JSXElement, ctx: &mut HirLoweringCtx) -> Result<NodeId, String> {
    lower_jsx_element_with_consumed_key(jsx, ctx, false)
}

fn lower_jsx_element_with_consumed_key(
    jsx: &JSXElement,
    ctx: &mut HirLoweringCtx,
    key_consumed: bool,
) -> Result<NodeId, String> {
    let span = source_span_from_swc(jsx.span);

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

    let mut props = Vec::new();
    let mut events = Vec::new();
    for attr_or_spread in &jsx.opening.attrs {
        if let JSXAttrOrSpread::JSXAttr(attr) = attr_or_spread {
            if key_consumed && jsx_attr_name(attr).as_deref() == Some("key") {
                continue;
            }
            lower_jsx_attr(attr, ctx, &mut props, is_custom, &tag_name, &mut events)?;
        }
    }

    let mut children = Vec::new();
    for child in &jsx.children {
        if let Some(node_id) = lower_jsx_child(child, ctx)? {
            children.push(node_id);
        }
    }

    let node_id = ctx.alloc_node_id();

    let node = if is_custom {
        HirNode::Component(HirComponentCall {
            id: node_id,
            name: tag_name.clone(),
            props,
            children,
            span,
        })
    } else {
        HirNode::Element(HirElement {
            id: node_id,
            tag: tag_name,
            props,
            events,
            children,
            span,
        })
    };

    ctx.nodes.push(node);
    Ok(node_id)
}

/// Lower JSX attributes to HIR props.
fn lower_jsx_attr(
    attr: &JSXAttr,
    ctx: &mut HirLoweringCtx,
    props: &mut Vec<HirProp>,
    is_custom: bool,
    component_name: &str,
    events: &mut Vec<HirEventBinding>,
) -> Result<(), String> {
    let name = jsx_attr_name(attr).expect("JSX attribute has a name");

    if name == "key" {
        return Err("JSX key is only supported on the direct root of a .map() callback".to_string());
    }

    match &attr.value {
        Some(JSXAttrValue::Str(str_lit)) => {
            let value = str_lit
                .value
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_default();
            props.push(HirProp::Static { name, value });
        }
        Some(JSXAttrValue::JSXExprContainer(container)) => match &container.expr {
            JSXExpr::Expr(expr) => {
                let span = source_span_from_swc(container.span);

                // Check if this is a DOM event binding (only for intrinsic elements)
                if !is_custom {
                    if let Some(event_name) = normalize_event_name(&name) {
                        // For DOM events, all valid expressions are treated as callables
                        if let Some(callable) = detect_callable_value(expr, ctx)? {
                            events.push(HirEventBinding {
                                event: event_name,
                                callable,
                                span,
                            });
                            return Ok(());
                        }
                    }
                }

                let callable = if is_custom
                    && matches!(ctx.component_props.prop_kind(component_name, &name), Some(ComponentPropKind::Callable))
                {
                    detect_callable_value(expr, ctx)?
                } else {
                    detect_inline_callable(expr, ctx)?
                };
                if let Some(callable) = callable {
                    props.push(HirProp::Callable { name, callable });
                } else {
                    // Not a callable - regular expression prop
                    let expr_id = lower_expression(expr, ctx)?;
                    props.push(HirProp::Expression {
                        name,
                        value: expr_id,
                    });
                }
            }
            JSXExpr::JSXEmptyExpr(_) => {
                props.push(HirProp::Static {
                    name,
                    value: String::new(),
                });
            }
        },
        None => {
            // Boolean attributes without value are treated as static "true"
            // For now, treat as static empty string
            props.push(HirProp::Static {
                name,
                value: String::new(),
            });
        }
        Some(_) => {
            // Other value types not supported
            props.push(HirProp::Static {
                name,
                value: String::new(),
            });
        }
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
    ctx: &mut HirLoweringCtx,
) -> Result<Option<NodeId>, String> {
    match child {
        swc_ecma_ast::JSXElementChild::JSXText(text) => {
            let content = text.value.as_str().map(|s| s.trim()).unwrap_or("");
            if content.is_empty() {
                return Ok(None);
            }
            let span = source_span_from_swc(text.span);
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
                    let span = source_span_from_swc(container.span);
                    let node_id = ctx.alloc_node_id();
                    let expr_id = lower_expression(expr, ctx)?;
                    ctx.nodes.push(HirNode::Text(HirText::Expression {
                        id: node_id,
                        expression: expr_id,
                        span,
                    }));
                    Ok(Some(node_id))
                }
            }
            JSXExpr::JSXEmptyExpr(_) => Ok(None),
        },
        swc_ecma_ast::JSXElementChild::JSXSpreadChild(_) => {
            // Spread children not supported yet
            Ok(None)
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
    ctx: &mut HirLoweringCtx,
) -> Result<NodeId, String> {
    let span = source_span_from_swc(call.span);

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

    // Extract the item parameter name
    let item_param = callback
        .params
        .first()
        .and_then(|param| match param {
            swc_ecma_ast::Pat::Ident(ident) => Some(ident.id.sym.to_string()),
            _ => None,
        })
        .ok_or("Expected .map() callback to have one parameter")?;

    // Lower the callback body and consume its structural identity, if present.
    let (body_node_id, identity) = match &*callback.body {
        swc_ecma_ast::ArrowFunctionBody::Expr(body_expr) => {
            lower_for_each_body(&**body_expr, ctx)?
        }
        swc_ecma_ast::ArrowFunctionBody::FunctionBody(_) => {
            return Err("Block .map() callbacks not supported".to_string());
        }
    };

    let node_id = ctx.alloc_node_id();
    ctx.nodes.push(HirNode::ForEach(HirForEach {
        id: node_id,
        source: source_expr_id,
        identity,
        item_param,
        body: vec![body_node_id],
        span,
    }));
    Ok(node_id)
}

fn lower_for_each_body(expr: &Expr, ctx: &mut HirLoweringCtx) -> Result<(NodeId, Option<ExprId>), String> {
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
    ctx: &mut HirLoweringCtx,
) -> Result<Option<ExprId>, String> {
    let mut key = None;
    for attr_or_spread in &jsx.opening.attrs {
        let JSXAttrOrSpread::JSXAttr(attr) = attr_or_spread else { continue };
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
fn lower_jsx_fragment(fragment: &JSXFragment, ctx: &mut HirLoweringCtx) -> Result<NodeId, String> {
    let span = source_span_from_swc(fragment.span);

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
fn lower_structural_expression(expr: &Expr, ctx: &mut HirLoweringCtx) -> Result<NodeId, String> {
    let span = span_from_expr(expr);

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
                        span,
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
                        span,
                    ));
                    let consequent_id = lower_structural_expression(&bin.right, ctx)?;
                    let node_id = ctx.alloc_node_id();
                    ctx.nodes.push(HirNode::Conditional(HirConditional {
                        id: node_id,
                        test: not_id,
                        consequent: vec![consequent_id],
                        alternate: vec![],
                        span,
                    }));
                    Ok(node_id)
                }
                _ => {
                    // Other binary expressions become empty nodes
                    let node_id = ctx.alloc_node_id();
                    ctx.nodes.push(HirNode::Empty);
                    Ok(node_id)
                }
            }
        }
        Expr::Unary(unary) => {
            // null, false, undefined become empty
            match unary.op {
                swc_ecma_ast::UnaryOp::Void => {
                    let node_id = ctx.alloc_node_id();
                    ctx.nodes.push(HirNode::Empty);
                    Ok(node_id)
                }
                _ => {
                    let node_id = ctx.alloc_node_id();
                    ctx.nodes.push(HirNode::Empty);
                    Ok(node_id)
                }
            }
        }
        Expr::JSXElement(jsx) => lower_jsx_element(jsx, ctx),
        Expr::JSXFragment(fragment) => lower_jsx_fragment(fragment, ctx),
        // Primitive value expressions that render as text
        Expr::Lit(_) | Expr::Tpl(_) | Expr::Ident(_) => {
            // These are values that should render as text
            let span = source_span_from_swc(expr.span());
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
        _ => {
            Err(format!(
                "Unsupported structural expression: {} at {}..{}",
                expr_name(expr),
                span.start,
                span.end
            ))
        }
    }
}

/// Lower an expression to HIR.
fn lower_expression(expr: &Expr, ctx: &mut HirLoweringCtx) -> Result<ExprId, String> {
    let span = span_from_expr(expr);

    let hir_expr = match expr {
        Expr::Ident(ident) => HirExpr::Identifier(ident.sym.to_string()),

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
            let property = match &member.prop {
                swc_ecma_ast::MemberProp::Ident(ident) => ident.sym.to_string(),
                swc_ecma_ast::MemberProp::Computed(_) => {
                    return Err("Computed member properties not supported".to_string())
                }
                swc_ecma_ast::MemberProp::PrivateName(_) => {
                    return Err("Private member properties not supported".to_string())
                }
            };
            HirExpr::Member {
                object: object_id,
                property,
            }
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
            let right_id = lower_expression(&bin.right, ctx)?;

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
                            props.push((key, value_id));
                        }
                    }
                    swc_ecma_ast::PropOrSpread::Spread(_) => {
                        return Err("Spread properties not supported".to_string())
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

        unsupported => return Err(format!("Unsupported expression: {unsupported:?}")),
    };

    let expr_id = ctx.alloc_expr_id();
    ctx.add_expression(HirExprNode::new(expr_id, hir_expr, span));
    Ok(expr_id)
}

/// Convert SWC span to HIR SourceSpan.
fn source_span_from_swc(span: Span) -> SourceSpan {
    SourceSpan {
        start: span.lo.0 as u32,
        end: span.hi.0 as u32,
    }
}

/// Get span from expression using Spanned trait.
fn span_from_expr(expr: &Expr) -> SourceSpan {
    source_span_from_swc(expr.span())
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
    use plec_parser::parse_module;
    use plec_sema::build_semantic_graph;
    use std::collections::HashMap;

    fn build_and_lower(source: &str) -> Result<HirComponent, String> {
        let module = parse_module("test.tsx", source).expect("parse should succeed");
        let resolved_imports = HashMap::new();

        let modules = vec![module];
        let semantic_graph =
            build_semantic_graph(&modules, &resolved_imports).expect("graph should build");

        let root = discover_root_component(&modules, &semantic_graph, "test.tsx", None)
            .map_err(|e| e.to_string())?;

        let component_props = build_component_prop_lookup(&semantic_graph, &root.symbol.module_id);
        lower_root_component(&root, &component_props)
    }

    fn build_and_lower_with_name(source: &str, name: &str) -> Result<HirComponent, String> {
        let module = parse_module("test.tsx", source).expect("parse should succeed");
        let resolved_imports = HashMap::new();

        let modules = vec![module];
        let semantic_graph =
            build_semantic_graph(&modules, &resolved_imports).expect("graph should build");

        let root = discover_root_component(&modules, &semantic_graph, "test.tsx", Some(name))
            .map_err(|e| e.to_string())?;

        let component_props = build_component_prop_lookup(&semantic_graph, &root.symbol.module_id);
        lower_root_component(&root, &component_props)
    }

    #[test]
    fn lowers_simple_div() {
        let source = r#"
            export function App() {
                return <div>Hello</div>;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");

        assert_eq!(hir.name, "App");
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
            export function App() {
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
    fn lowers_conditional() {
        let source = r#"
            export function App() {
                return ready ? <Ready /> : <Loading />;
            }
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
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");

        // Custom components start with uppercase, so they should be HirNode::Component
        if let HirNode::Component(comp) = &hir.nodes[hir.root_nodes[0].0 as usize] {
            assert_eq!(comp.name, "Header");
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
                return <input onInput={(e) => setValue(e.target.value)} />;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");

        // Find the input element
        let element = hir.nodes.iter().find(|n| matches!(n, HirNode::Element(el) if el.tag == "input"));
        assert!(element.is_some());

        if let HirNode::Element(el) = element.unwrap() {
            assert_eq!(el.props.len(), 0);
            assert_eq!(el.events.len(), 1);
            let event = &el.events[0];
            assert_eq!(event.event, "input");
            if let HirCallable::Inline { params, body } = &event.callable {
                assert_eq!(params, &["e"]);
                // Verify body is a call expression
                let body_expr = hir.expressions.iter().find(|e| e.id == *body);
                assert!(body_expr.is_some());
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
        let member_expr = hir.expressions.iter().find(|e| matches!(e.expression, HirExpr::Member { .. }));
        assert!(member_expr.is_some());

        if let HirExpr::Member { property, .. } = &member_expr.unwrap().expression {
            assert_eq!(property, "value");
        }
    }

    #[test]
    fn lowers_call_expression() {
        let source = r#"
            export function App() {
                return <div>{setValue("test")}</div>;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");

        // Should have a call expression
        let call_expr = hir.expressions.iter().find(|e| matches!(e.expression, HirExpr::Call { .. }));
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
                return <div>{show && <p>Hello</p>}</div>;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");

        // Should have a conditional node (from && with JSX)
        let cond_node = hir.nodes.iter().find(|n| matches!(n, HirNode::Conditional(_)));
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
        let cond_node = hir.nodes.iter().find(|n| matches!(n, HirNode::Conditional(_)));
        assert!(cond_node.is_some());

        if let HirNode::Conditional(cond) = cond_node.unwrap() {
            // Both branches should be text nodes with expressions
            assert_eq!(cond.consequent.len(), 1);
            assert_eq!(cond.alternate.len(), 1);

            // Verify consequent is a text node with an expression
            if let HirNode::Text(HirText::Expression { .. }) = &hir.nodes[cond.consequent[0].0 as usize] {
                // Good
            } else {
                panic!("Expected consequent to be Expression text node");
            }

            // Verify alternate is a text node with an expression
            if let HirNode::Text(HirText::Expression { .. }) = &hir.nodes[cond.alternate[0].0 as usize] {
                // Good
            } else {
                panic!("Expected alternate to be Expression text node");
            }
        }
    }

    #[test]
    fn lowers_map_to_for_each() {
        let source = r#"
            export function App() {
                const items = [{ id: 1 }, { id: 2 }];
                return <ul>{items.map((item) => <li key={item.id}>{item.id}</li>)}</ul>;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");

        // Should have a ForEach node
        let foreach_node = hir.nodes.iter().find(|n| matches!(n, HirNode::ForEach(_)));
        assert!(foreach_node.is_some());

        if let HirNode::ForEach(foreach) = foreach_node.unwrap() {
            assert_eq!(foreach.item_param, "item");
            assert_eq!(foreach.body.len(), 1);
            assert!(foreach.identity.is_some(), "key should become ForEach identity");
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
                return <div onClick={() => update({ id: 123 })}>Click</div>;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");

        // Should have an object expression
        let obj_expr = hir.expressions.iter().find(|e| matches!(e.expression, HirExpr::Object(_)));
        assert!(obj_expr.is_some());

        if let HirExpr::Object(props) = &obj_expr.unwrap().expression {
            assert_eq!(props.len(), 1);
            assert_eq!(props[0].0, "id");
        }
    }

    #[test]
    fn lowers_block_arrow_in_event_handler() {
        let source = r#"
            export function App() {
                return <button onClick={() => {
                    setValue('test');
                }}>Click</button>;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");

        // Should have an event handler
        let element = hir.nodes.iter().find(|n| matches!(n, HirNode::Element(el) if el.tag == "button"));
        assert!(element.is_some());

        if let HirNode::Element(el) = element.unwrap() {
            assert_eq!(el.events.len(), 1);
            let event = &el.events[0];
            assert_eq!(event.event, "click");
            if let HirCallable::Inline { params, .. } = &event.callable {
                assert_eq!(params.len(), 0);
            } else {
                panic!("Expected Inline callable");
            }
        }
    }

    #[test]
    fn component_callable_reference() {
        let source = r#"
            export function App() {
                return <Child onSave={save} />;
            }

            function Child({ onSave }: { onSave(): void }) {
                return <button onClick={onSave} />;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");

        let component = hir.nodes.iter().find(|n| matches!(n, HirNode::Component(comp) if comp.name == "Child"));
        assert!(component.is_some());

        if let HirNode::Component(comp) = component.unwrap() {
            assert_eq!(comp.props.len(), 1);
            if let HirProp::Callable { name, callable: HirCallable::Reference { .. } } = &comp.props[0] {
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
                return <Child onSave={() => save()} />;
            }

            function Child({ onSave }: { onSave(): void }) {
                return <button onClick={onSave} />;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");

        let component = hir.nodes.iter().find(|n| matches!(n, HirNode::Component(comp) if comp.name == "Child"));
        assert!(component.is_some());

        if let HirNode::Component(comp) = component.unwrap() {
            assert_eq!(comp.props.len(), 1);
            if let HirProp::Callable { name, callable } = &comp.props[0] {
                assert_eq!(name, "onSave");
                if let HirCallable::Inline { params, .. } = callable {
                    assert_eq!(params.len(), 0);
                } else {
                    panic!("Expected Inline callable");
                }
            } else {
                panic!("Expected Callable prop");
            }
        }
    }

    #[test]
    fn component_callable_member_reference_uses_prop_semantics() {
        let source = r#"
            export function App() {
                return <Child onSave={actions.save} />;
            }

            function Child({ onSave }: { onSave(): void }) {
                return <button onClick={onSave} />;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");
        let component = hir.nodes.iter().find(|node| matches!(node, HirNode::Component(comp) if comp.name == "Child")).unwrap();
        assert!(matches!(
            component,
            HirNode::Component(comp) if matches!(comp.props.first(), Some(HirProp::Callable { name, callable: HirCallable::Reference { .. } }) if name == "onSave")
        ));
    }

    #[test]
    fn component_value_member_reference_stays_expression() {
        let source = r#"
            export function App() {
                return <Child onStatus={actions.status} />;
            }

            function Child({ onStatus }: { onStatus: string }) {
                return <div>{onStatus}</div>;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");
        let component = hir.nodes.iter().find(|node| matches!(node, HirNode::Component(comp) if comp.name == "Child")).unwrap();
        assert!(matches!(
            component,
            HirNode::Component(comp) if matches!(comp.props.first(), Some(HirProp::Expression { name, .. }) if name == "onStatus")
        ));
    }

    #[test]
    fn component_callable_conditional_uses_declared_prop() {
        let source = r#"
            export function App() {
                return <Child onSave={editing ? onCancel : onStartEdit} />;
            }

            function Child({ onSave }: { onSave(): void }) {
                return <button onClick={onSave} />;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");
        let component = hir.nodes.iter().find(|node| matches!(node, HirNode::Component(comp) if comp.name == "Child")).unwrap();
        assert!(matches!(
            component,
            HirNode::Component(comp) if matches!(comp.props.first(), Some(HirProp::Callable { name, callable: HirCallable::Conditional { .. } }) if name == "onSave")
        ));
    }

    #[test]
    fn dom_named_event() {
        let source = r#"
            export function App() {
                return <button onClick={handleClick}>Click</button>;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");

        let element = hir.nodes.iter().find(|n| matches!(n, HirNode::Element(el) if el.tag == "button"));
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
                return <input onInput={(event) => setValue(event.currentTarget.value)} />;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");

        let element = hir.nodes.iter().find(|n| matches!(n, HirNode::Element(el) if el.tag == "input"));
        assert!(element.is_some());

        if let HirNode::Element(el) = element.unwrap() {
            assert_eq!(el.props.len(), 0);
            assert_eq!(el.events.len(), 1);
            let event = &el.events[0];
            assert_eq!(event.event, "input");
            if let HirCallable::Inline { params, body } = &event.callable {
                assert_eq!(params, &["event"]);
                // Verify body expression exists
                let body_expr = hir.expressions.iter().find(|e| e.id == *body);
                assert!(body_expr.is_some());
            } else {
                panic!("Expected Inline callable");
            }
        }
    }

    #[test]
    fn dom_conditional_event() {
        let source = r#"
            export function App() {
                return <button onClick={editing ? onCancel : onStartEdit}>Toggle</button>;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");

        let element = hir.nodes.iter().find(|n| matches!(n, HirNode::Element(el) if el.tag == "button"));
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
                return <form onSubmit={handleSubmit}><input onChange={handleChange} /></form>;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");

        let form = hir.nodes.iter().find(|n| matches!(n, HirNode::Element(el) if el.tag == "form"));
        assert!(form.is_some());

        if let HirNode::Element(el) = form.unwrap() {
            assert_eq!(el.events.len(), 1);
            assert_eq!(el.events[0].event, "submit");
        }

        let input = hir.nodes.iter().find(|n| matches!(n, HirNode::Element(el) if el.tag == "input"));
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
                return <TodoRow onSave={() => save()} onCancel={cancel} />;
            }
        "#;

        let hir = build_and_lower(source).expect("lowering should succeed");

        let component = hir.nodes.iter().find(|n| matches!(n, HirNode::Component(comp) if comp.name == "TodoRow"));
        assert!(component.is_some());

        if let HirNode::Component(comp) = component.unwrap() {
            // Component props should NOT have events - events only for intrinsic elements
            assert!(comp.props.len() >= 2);

            // Find onSave and onCancel
            let on_save = comp.props.iter().find(|p| matches!(p, HirProp::Callable { name, .. } if name == "onSave"));
            assert!(on_save.is_some(), "onSave should be a Callable prop");

            let on_cancel = comp.props.iter().find(|p| {
                matches!(p, HirProp::Callable { name, .. } | HirProp::Expression { name, .. } if name == "onCancel")
            });
            assert!(on_cancel.is_some(), "onCancel should exist");
        }
    }
}
