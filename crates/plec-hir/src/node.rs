use crate::{ExprId, NodeId, SourceSpan};

#[derive(Debug, Clone, PartialEq)]
pub enum HirProp {
    Static {
        name: String,
        value: String,
    },
    Expression {
        name: String,
        value: ExprId,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum HirNode {
    Element(HirElement),
    Text(HirText),
    Component(HirComponentCall),
    Fragment(HirFragment),
    Conditional(HirConditional),
    Empty,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirElement {
    pub id: NodeId,
    pub tag: String,
    pub props: Vec<HirProp>,
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
    pub name: String,
    pub props: Vec<HirProp>,
    pub children: Vec<NodeId>,
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
