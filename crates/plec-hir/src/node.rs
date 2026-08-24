use crate::{BindingId, ComponentId, ExprId, HirCallable, NodeId, SourceSpan};

#[derive(Debug, Clone, PartialEq)]
pub enum HirProp {
    Static { name: String, value: String },
    Expression { name: String, value: ExprId },
    Callable { name: String, callable: HirCallable },
}

/// DOM event binding on an intrinsic element.
#[derive(Debug, Clone, PartialEq)]
pub struct HirEventBinding {
    /// Normalized event name (e.g., "click", "input", "submit").
    pub event: String,
    /// The callable value.
    pub callable: HirCallable,
    /// Source span for error reporting.
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HirNode {
    Element(HirElement),
    Text(HirText),
    Component(HirComponentCall),
    Fragment(HirFragment),
    Conditional(HirConditional),
    ForEach(HirForEach),
    /// The implicit React `children` insertion point of a component.
    Slot(HirSlot),
    Empty,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirElement {
    pub id: NodeId,
    pub tag: String,
    pub props: Vec<HirProp>,
    pub events: Vec<HirEventBinding>,
    /// This is intentionally not a prop: host ownership is structural.
    pub host_ref: Option<BindingId>,
    pub children: Vec<NodeId>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HirText {
    Static {
        id: NodeId,
        value: String,
        span: SourceSpan,
    },
    Expression {
        id: NodeId,
        expression: ExprId,
        span: SourceSpan,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirComponentCall {
    pub id: NodeId,
    /// Canonical defining component identity. The call-site span remains on
    /// this node, so source and target identity are intentionally separate.
    pub target: ComponentId,
    pub props: Vec<HirProp>,
    pub children: Vec<NodeId>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirSlot {
    pub id: NodeId,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirFragment {
    pub id: NodeId,
    pub children: Vec<NodeId>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirConditional {
    pub id: NodeId,
    pub test: ExprId,
    pub consequent: Vec<NodeId>,
    pub alternate: Vec<NodeId>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirForEach {
    pub id: NodeId,
    pub source: ExprId,
    /// Stable identity evaluated in the same lexical scope as `item_param` and `body`.
    pub identity: Option<ExprId>,
    pub item_binding: BindingId,
    pub body: Vec<NodeId>,
    pub span: SourceSpan,
}
