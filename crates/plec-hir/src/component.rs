use crate::{BindingId, ComponentId, ExprId, HirExprNode, HirNode, NodeId, SourceSpan};

#[derive(Debug, Clone, PartialEq)]
pub struct HirBinding {
    pub id: BindingId,
    pub name: String,
    pub kind: HirBindingKind,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HirBindingKind {
    Parameter { callable: bool },
    Input { kind: String },
    Local,
    StateValue,
    StateSetter { state: BindingId },
    Callable,
    LoopItem,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirInput {
    pub binding: BindingId,
    pub name: String,
    pub kind: String,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirParameter {
    pub binding: BindingId,
    pub source: HirParameterSource,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HirParameterSource {
    Direct,
    Prop { name: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirLocal {
    pub binding: BindingId,
    pub initializer: ExprId,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirState {
    pub value: BindingId,
    pub setter: BindingId,
    pub initializer: ExprId,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirCallableDecl {
    pub binding: BindingId,
    pub parameters: Vec<BindingId>,
    pub body: HirCallableBody,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HirCallableBody {
    Expression(ExprId),
    Block(Vec<HirStmt>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum HirStmt {
    Expression {
        expression: ExprId,
        span: SourceSpan,
    },
    StateUpdate {
        state: BindingId,
        value: ExprId,
        span: SourceSpan,
    },
    If {
        test: ExprId,
        consequent: Vec<HirStmt>,
        alternate: Vec<HirStmt>,
        span: SourceSpan,
    },
    Return {
        value: Option<ExprId>,
        span: SourceSpan,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirComponent {
    pub id: ComponentId,
    pub parameters: Vec<HirParameter>,
    pub inputs: Vec<HirInput>,
    pub bindings: Vec<HirBinding>,
    pub locals: Vec<HirLocal>,
    pub states: Vec<HirState>,
    pub callables: Vec<HirCallableDecl>,
    pub root_nodes: Vec<NodeId>,
    pub nodes: Vec<HirNode>,
    pub expressions: Vec<HirExprNode>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirApplication {
    pub root: ComponentId,
    pub components: Vec<HirComponent>,
}

impl HirComponent {
    pub fn new(id: ComponentId, span: SourceSpan) -> Self {
        Self {
            id,
            parameters: Vec::new(),
            inputs: Vec::new(),
            bindings: Vec::new(),
            locals: Vec::new(),
            states: Vec::new(),
            callables: Vec::new(),
            root_nodes: Vec::new(),
            nodes: Vec::new(),
            expressions: Vec::new(),
            span,
        }
    }

    pub fn with_root_node(mut self, node_id: NodeId) -> Self {
        self.root_nodes.push(node_id);
        self
    }

    pub fn with_node(mut self, node: HirNode) -> Self {
        self.nodes.push(node);
        self
    }

    pub fn with_expression(mut self, expr: HirExprNode) -> Self {
        self.expressions.push(expr);
        self
    }
}
