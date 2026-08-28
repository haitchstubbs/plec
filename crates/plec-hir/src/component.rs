use crate::{BindingId, ComponentId, ExprId, HirCallable, HirExprNode, HirNode, NodeId, SourceSpan};

#[derive(Debug, Clone, PartialEq)]
pub struct HirBinding {
    pub id: BindingId,
    pub name: String,
    pub kind: HirBindingKind,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HirBindingKind {
    Parameter {
        callable: bool,
    },
    Input {
        kind: String,
    },
    Local,
    StateValue,
    StateSetter {
        state: BindingId,
    },
    Callable,
    LoopItem,
    /// A value or error written by an async continuation frame.
    AsyncValue,
    /// Instance-owned, deliberately non-reactive mutable storage.
    RefSlot,
    /// A lifecycle-bound handle to one intrinsic host node.
    HostRef,
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
pub struct HirRefSlot {
    pub binding: BindingId,
    pub initializer: ExprId,
    pub span: SourceSpan,
}

/// A stable dependency-driven actor. Unlike React effects, this is never
/// mounted implicitly: the runtime schedules it only from declared edges.
#[derive(Debug, Clone, PartialEq)]
pub struct HirReaction {
    pub dependencies: Vec<ExprId>,
    pub body: HirCallableBody,
    pub cleanup: Option<HirCallableBody>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirListener {
    pub source: String,
    pub event: String,
    pub callable: HirCallable,
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
    CaptureActiveElement { reference: BindingId, span: SourceSpan },
    FocusHostRef { reference: BindingId, optional: bool, span: SourceSpan },
    FocusRef { reference: BindingId, optional: bool, span: SourceSpan },
    PreventDefault { span: SourceSpan },
    /// Browser-owned cookie access. Names and options are intentionally static
    /// so a graph declares its complete capability surface before execution.
    AwaitCookie {
        target: Option<BindingId>,
        operation: String,
        name: String,
        value: Option<ExprId>,
        path: String,
        same_site: Option<String>,
        secure: Option<bool>,
        max_age: Option<i64>,
        span: SourceSpan,
    },
    /// A deliberately narrow async boundary. Route loaders currently support
    /// only a terminal `return await fetch(url)` capability request.
    AwaitFetch {
        target: Option<BindingId>,
        url: ExprId,
        method: String,
        headers: Vec<(String, ExprId)>,
        body: Option<ExprId>,
        decode: String,
        span: SourceSpan,
    },
    AwaitCall {
        target: Option<BindingId>,
        callee: BindingId,
        arguments: Vec<ExprId>,
        span: SourceSpan,
    },
    Try {
        body: Vec<HirStmt>,
        catch: Option<(BindingId, Vec<HirStmt>)>,
        finally: Vec<HirStmt>,
        span: SourceSpan,
    },
    Expression {
        expression: ExprId,
        span: SourceSpan,
    },
    StateUpdate {
        state: BindingId,
        value: ExprId,
        span: SourceSpan,
    },
    RefUpdate {
        reference: BindingId,
        value: ExprId,
        span: SourceSpan,
    },
    /// An awaited response body is already buffered by the fetch capability;
    /// the source `await response.json()` only selects that serializable body.
    AsyncAssign {
        target: BindingId,
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
    Throw {
        value: ExprId,
        span: SourceSpan,
    },
    OptionalCall {
        callee: BindingId,
        arguments: Vec<ExprId>,
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
    pub ref_slots: Vec<HirRefSlot>,
    pub reactions: Vec<HirReaction>,
    pub listeners: Vec<HirListener>,
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

/// Route declarations are semantic input to the executable route manifest.
/// They intentionally reference canonical component IDs rather than runtime
/// graph IDs so lowering remains the only place that chooses artifact names.
#[derive(Debug, Clone, PartialEq)]
pub struct HirRouteApplication {
    pub root: ComponentId,
    pub routes: Vec<HirRoute>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirRoute {
    pub id: String,
    pub parent: Option<String>,
    pub path: String,
    pub component: ComponentId,
    pub pending_component: Option<ComponentId>,
    pub pending_mode: String,
    pub error_component: Option<ComponentId>,
    pub loader: Option<String>,
    pub outlet_id: String,
    pub metadata: HirRouteMetadata,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct HirRouteMetadata {
    pub title: Option<String>,
    pub description: Option<String>,
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
            ref_slots: Vec::new(),
            reactions: Vec::new(),
            listeners: Vec::new(),
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
