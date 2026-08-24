pub mod action;
pub mod application;
pub mod component;
mod context;
pub mod expression;
pub mod node;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoweringError(pub String);

impl std::fmt::Display for LoweringError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for LoweringError {}

pub use application::lower_application_to_executable;
pub use component::{lower_component_to_executable, lower_route_loader_to_executable};

pub(crate) use context::{ComponentTargets, Ctx};

#[cfg(test)]
mod tests {
    use plec_hir::{ComponentId, HirComponent, HirElement, HirNode, HirText, NodeId, SourceSpan};
    use plec_ir::Node;

    use super::lower_component_to_executable;

    #[test]
    fn lowers_a_static_element_tree() {
        let span = SourceSpan::new("test.tsx", 0, 0);
        let component = HirComponent::new(ComponentId::new("test.tsx", "Hello"), span.clone())
            .with_node(HirNode::Element(HirElement {
                id: NodeId(0),
                tag: "div".into(),
                props: vec![],
                events: vec![],
                children: vec![NodeId(1)],
                span: span.clone(),
            }))
            .with_node(HirNode::Text(HirText::Static {
                id: NodeId(1),
                value: "Hello".into(),
                span,
            }))
            .with_root_node(NodeId(0));

        let executable = lower_component_to_executable(&component).unwrap();

        assert!(matches!(executable.nodes[0], Node::Element { .. }));
        assert_eq!(executable.texts[0].value.as_deref(), Some("Hello"));
    }
}
