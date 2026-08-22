// IDs
pub use ids::{ComponentId, ExprId, NodeId};

// Span
pub use span::SourceSpan;

// Expressions
pub use expr::{HirCallable, HirExpr, HirExprNode, HirLogicalOp, HirTemplatePart, HirUnaryOp, HirBinaryOp, HirValue};

// Nodes
pub use node::{
    HirComponentCall, HirConditional, HirElement, HirEventBinding, HirForEach, HirFragment, HirNode, HirProp, HirText,
};

// Component
pub use component::HirComponent;

mod ids;
mod span;
mod expr;
mod node;
mod component;

#[cfg(test)]
mod tests {
    use super::*;
    use HirExpr::*;
    use HirNode::*;

    #[test]
    fn constructs_div_with_static_prop_and_expression_text() {
        let span = SourceSpan::new(0, 100);

        // Expressions (indexed by ExprId)
        let expressions = vec![
            HirExprNode::new(
                ExprId(0),
                Identifier("user".to_string()),
                SourceSpan::new(10, 14),
            ),
            HirExprNode::new(
                ExprId(1),
                Member {
                    object: ExprId(0),
                    property: "name".to_string(),
                },
                SourceSpan::new(10, 19),
            ),
        ];

        // Nodes (indexed by NodeId)
        let nodes = vec![
            Text(HirText::Static {
                id: NodeId(0),
                value: "Hello ".to_string(),
                span: SourceSpan::new(20, 26),
            }),
            Text(HirText::Expression {
                id: NodeId(1),
                expression: ExprId(1),
                span: SourceSpan::new(26, 38),
            }),
            Element(HirElement {
                id: NodeId(2),
                tag: "div".to_string(),
                props: vec![HirProp::Static {
                    name: "className".to_string(),
                    value: "card".to_string(),
                }],
                events: vec![],
                children: vec![NodeId(0), NodeId(1)],
                span: SourceSpan::new(0, 50),
            }),
        ];

        let component = HirComponent {
            id: ComponentId(0),
            module_id: "App.tsx".to_string(),
            name: "App".to_string(),
            root_nodes: vec![NodeId(2)],
            nodes,
            expressions,
            span,
        };

        // Verify structure
        assert_eq!(component.root_nodes, vec![NodeId(2)]);
        assert_eq!(component.nodes.len(), 3);
        assert_eq!(component.expressions.len(), 2);

        // Verify element
        if let HirNode::Element(el) = &component.nodes[2] {
            assert_eq!(el.tag, "div");
            assert_eq!(el.props.len(), 1);
            assert_eq!(el.children, vec![NodeId(0), NodeId(1)]);
        } else {
            panic!("Expected Element node");
        }

        // Verify static text
        if let HirNode::Text(HirText::Static { value, .. }) = &component.nodes[0] {
            assert_eq!(value, "Hello ");
        } else {
            panic!("Expected Static text node");
        }

        // Verify expression text
        if let HirNode::Text(HirText::Expression { expression, .. }) = &component.nodes[1] {
            assert_eq!(*expression, ExprId(1));
        } else {
            panic!("Expected Expression text node");
        }

        // Verify member expression
        if let Member { object, property } = &component.expressions[1].expression {
            assert_eq!(*object, ExprId(0));
            assert_eq!(property, "name");
        } else {
            panic!("Expected Member expression");
        }
    }

    #[test]
    fn source_span_new_works() {
        let span = SourceSpan::new(10, 20);
        assert_eq!(span.start, 10);
        assert_eq!(span.end, 20);
    }

    #[test]
    fn expr_id_and_node_id_are_copy() {
        let expr_id = ExprId(5);
        let copied = expr_id;
        assert_eq!(expr_id.0, copied.0);

        let node_id = NodeId(10);
        let copied = node_id;
        assert_eq!(node_id.0, copied.0);
    }

    #[test]
    fn hir_component_builder_methods() {
        let span = SourceSpan::new(0, 100);
        let component = HirComponent::new(
            ComponentId(0),
            "Test.tsx".to_string(),
            "Test".to_string(),
            span,
        )
        .with_root_node(NodeId(0))
        .with_node(HirNode::Empty)
        .with_expression(HirExprNode::new(
            ExprId(0),
            HirExpr::Literal(HirValue::Null),
            span,
        ));

        assert_eq!(component.root_nodes.len(), 1);
        assert_eq!(component.nodes.len(), 1);
        assert_eq!(component.expressions.len(), 1);
    }

    #[test]
    fn all_unary_ops_exist() {
        let _ = HirUnaryOp::Not;
        let _ = HirUnaryOp::Plus;
        let _ = HirUnaryOp::Minus;
    }

    #[test]
    fn all_binary_ops_exist() {
        let _ = HirBinaryOp::Add;
        let _ = HirBinaryOp::Subtract;
        let _ = HirBinaryOp::Multiply;
        let _ = HirBinaryOp::Divide;
        let _ = HirBinaryOp::Equal;
        let _ = HirBinaryOp::NotEqual;
        let _ = HirBinaryOp::Greater;
        let _ = HirBinaryOp::GreaterEqual;
        let _ = HirBinaryOp::Less;
        let _ = HirBinaryOp::LessEqual;
    }

    #[test]
    fn all_logical_ops_exist() {
        let _ = HirLogicalOp::And;
        let _ = HirLogicalOp::Or;
        let _ = HirLogicalOp::Coalesce;
    }

    #[test]
    fn hir_value_variants() {
        let _ = HirValue::Null;
        let _ = HirValue::Bool(true);
        let _ = HirValue::Number(1.0);
        let _ = HirValue::String("test".to_string());
    }

    #[test]
    fn template_part_variants() {
        let _ = HirTemplatePart::String("text".to_string());
        let _ = HirTemplatePart::Expression(ExprId(0));
    }
}
