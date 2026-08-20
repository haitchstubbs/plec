use crate::{ComponentId, HirExprNode, HirNode, NodeId, SourceSpan};

#[derive(Debug, Clone, PartialEq)]
pub struct HirComponent {
    pub id: ComponentId,
    pub module_id: String,
    pub name: String,
    pub root_nodes: Vec<NodeId>,
    pub nodes: Vec<HirNode>,
    pub expressions: Vec<HirExprNode>,
    pub span: SourceSpan,
}

impl HirComponent {
    pub fn new(
        id: ComponentId,
        module_id: String,
        name: String,
        span: SourceSpan,
    ) -> Self {
        Self {
            id,
            module_id,
            name,
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
