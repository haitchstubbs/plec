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

pub(crate) use context::{ComponentTarget, ComponentTargets, Ctx};

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
                host_ref: None,
                route_outlet: None,
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

    #[test]
    fn preserves_the_authored_route_outlet_node() {
        let span = SourceSpan::new("test.tsx", 0, 0);
        let component = HirComponent::new(ComponentId::new("test.tsx", "Layout"), span.clone())
            .with_node(HirNode::Element(HirElement {
                id: NodeId(0),
                tag: "div".into(),
                props: vec![],
                events: vec![],
                host_ref: None,
                route_outlet: Some("main".into()),
                children: vec![],
                span,
            }))
            .with_root_node(NodeId(0));
        let executable = lower_component_to_executable(&component).unwrap();
        assert_eq!(executable.route_outlets[0].id, "main");
        assert_eq!(executable.route_outlets[0].node, 0);
    }

    fn fanout_component(name: &str, children: usize) -> HirComponent {
        let span = SourceSpan::new("test.tsx", 0, 0);
        let child_ids: Vec<NodeId> = (1..=children).map(|id| NodeId(id as u32)).collect();
        let component = (0..=children).fold(
            HirComponent::new(ComponentId::new("test.tsx", name), span.clone()),
            |component, id| {
                let node = if id == 0 {
                    HirNode::Element(HirElement {
                        id: NodeId(0),
                        tag: "div".into(),
                        props: vec![],
                        events: vec![],
                        host_ref: None,
                        route_outlet: None,
                        children: child_ids.clone(),
                        span: span.clone(),
                    })
                } else {
                    HirNode::Element(HirElement {
                        id: NodeId(id as u32),
                        tag: "span".into(),
                        props: vec![],
                        events: vec![],
                        host_ref: None,
                        route_outlet: None,
                        children: vec![],
                        span: span.clone(),
                    })
                };
                component.with_node(node)
            },
        );
        component.with_root_node(NodeId(0))
    }

    #[test]
    fn rejects_lowered_components_beyond_per_collection_budget() {
        let component = fanout_component("Big", plec_ir::limits::MAX_COMPONENT_COLLECTION_LEN + 1);

        let error = lower_component_to_executable(&component)
            .expect_err("over-wide component should be rejected");

        assert!(
            error.0.contains("exceeds the maximum length"),
            "{}",
            error.0
        );
    }

    #[test]
    fn rejects_lowered_applications_beyond_aggregate_entry_budget() {
        let components: Vec<HirComponent> = (0..12)
            .map(|index| fanout_component(&format!("C{index}"), 90_000))
            .collect();
        let application = plec_hir::HirApplication {
            root: ComponentId::new("test.tsx", "C0"),
            components,
        };

        let error = super::lower_application_to_executable(&application)
            .expect_err("aggregate IR exhaustion should be rejected");

        assert!(error.0.contains("maximum total IR entries"), "{}", error.0);
    }

    #[test]
    fn rejects_lowered_applications_beyond_component_count_budget() {
        let components: Vec<HirComponent> = (0..=plec_ir::limits::MAX_COMPONENT_COUNT)
            .map(|index| fanout_component(&format!("C{index}"), 0))
            .collect();
        let application = plec_hir::HirApplication {
            root: ComponentId::new("test.tsx", "C0"),
            components,
        };

        let error = super::lower_application_to_executable(&application)
            .expect_err("component count exhaustion should be rejected");

        assert!(error.0.contains("maximum component count"), "{}", error.0);
    }
}
