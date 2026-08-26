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

/// Array literals retain whether each source entry was spread so the
/// executable VM can preserve JavaScript's ordered flattening semantics.
#[derive(Debug, Clone, PartialEq)]
pub enum HirArrayItem {
    Value(ExprId),
    Spread(ExprId),
}

/// Object literals retain ordered spread entries so runtime construction keeps
/// JavaScript's source-order, last-write-wins semantics.
#[derive(Debug, Clone, PartialEq)]
pub enum HirObjectItem {
    Property { name: String, value: ExprId },
    Spread(ExprId),
}

#[derive(Debug, Clone, PartialEq)]
pub enum HirExpr {
    Literal(HirValue),
    /// A synchronous value supplied by the typed browser host. DOM nodes never
    /// cross this boundary; only serializable host values do.
    Host {
        kind: String,
        name: Option<String>,
    },
    Binding(BindingId),
    /// Reading non-reactive component storage.
    RefCurrent { reference: BindingId },
    /// Opaque host handle; ordinary expressions may not consume this.
    HostRefCurrent { reference: BindingId },
    Member {
        object: ExprId,
        property: String,
    },
    /// A serializable dynamic lookup. `optional` has the same nullish result
    /// contract as a missing field; it never exposes a browser object.
    ComputedMember {
        object: ExprId,
        property: ExprId,
        optional: bool,
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
    Array(Vec<HirArrayItem>),
    Object(Vec<HirObjectItem>),
    /// Object-rest destructuring over the serializable value graph. The
    /// compiler records the consumed property names so forwarding never leaks
    /// a destructured prop back to an intrinsic or component call.
    ObjectWithout {
        object: ExprId,
        excluded: Vec<String>,
    },
    /// Pure collection transforms. Their callback body runs with the current
    /// serializable item as the row record; no JavaScript function escapes
    /// into the executable graph.
    Map { source: ExprId, mapper: ExprId },
    Filter { source: ExprId, predicate: ExprId },
    /// A finite pure value operation implemented by the executable VM.
    Builtin {
        kind: String,
        args: Vec<ExprId>,
    },
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
