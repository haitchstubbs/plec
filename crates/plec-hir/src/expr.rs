use crate::{ExprId, SourceSpan};

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
    Greater,
    GreaterEqual,
    Less,
    LessEqual,
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
    Identifier(String),
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
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirExprNode {
    pub id: ExprId,
    pub expression: HirExpr,
    pub span: SourceSpan,
}

impl HirExprNode {
    pub fn new(id: ExprId, expression: HirExpr, span: SourceSpan) -> Self {
        Self { id, expression, span }
    }
}
