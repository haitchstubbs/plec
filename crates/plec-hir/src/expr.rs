use crate::{BindingId, ExprId, HirCallableBody, SourceSpan};

/// Callable value representation.
///
/// Distinguishes between named references, inline bodies, and conditional callables.
#[derive(Debug, Clone, PartialEq)]
pub enum HirCallable {
    /// A resolved local callable or callable component parameter.
    Reference { binding: BindingId },
    /// Inline callable with parameters.
    Inline {
        parameters: Vec<BindingId>,
        body: HirCallableBody,
    },
    /// Conditional callable selection.
    Conditional {
        test: ExprId,
        consequent: Box<HirCallable>,
        alternate: Box<HirCallable>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum HirValue {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HirUnaryOp {
    Not,
    Plus,
    Minus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HirBinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,

    Equal,
    NotEqual,
    StrictEqual,
    StrictNotEqual,

    Greater,
    GreaterEqual,
    Less,
    LessEqual,

    InstanceOf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HirLogicalOp {
    And,
    Or,
    Coalesce,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HirTemplatePart {
    String(String),
    Expression(ExprId),
}

#[derive(Debug, Clone, PartialEq)]
pub enum HirExpr {
    Literal(HirValue),
    Binding(BindingId),
    Member {
        object: ExprId,
        property: String,
    },
    Unary {
        op: HirUnaryOp,
        argument: ExprId,
    },
    Binary {
        op: HirBinaryOp,
        left: ExprId,
        right: ExprId,
    },
    Logical {
        op: HirLogicalOp,
        left: ExprId,
        right: ExprId,
    },
    Conditional {
        test: ExprId,
        consequent: ExprId,
        alternate: ExprId,
    },
    Template {
        parts: Vec<HirTemplatePart>,
    },
    Object(Vec<(String, ExprId)>),
    Call {
        callee: ExprId,
        args: Vec<ExprId>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirExprNode {
    pub id: ExprId,
    pub expression: HirExpr,
    pub span: SourceSpan,
}

impl HirExprNode {
    pub fn new(id: ExprId, expression: HirExpr, span: SourceSpan) -> Self {
        Self {
            id,
            expression,
            span,
        }
    }
}
