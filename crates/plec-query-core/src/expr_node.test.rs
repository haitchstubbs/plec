use super::{eval_expr_for_dialect, Dialect, ExprNode};

fn raw_expr(text: &str) -> ExprNode {
    ExprNode::Raw {
        text: text.to_string(),
    }
}

fn ref_expr(part: &str) -> ExprNode {
    ExprNode::Ref {
        parts: vec![part.to_string()],
    }
}

#[test]
fn supported_function_renders_for_sql_dialect() {
    let result = eval_expr_for_dialect(
        ExprNode::FnCall {
            name: "lower".to_string(),
            args: vec![ref_expr("email")],
        },
        &Dialect::Postgres,
    )
    .unwrap();

    assert_eq!(result.raw, "LOWER(email)");
}

#[test]
fn unknown_function_is_rejected_for_builder_expressions() {
    let error = eval_expr_for_dialect(
        ExprNode::FnCall {
            name: "unknown_func".to_string(),
            args: vec![ref_expr("email")],
        },
        &Dialect::Postgres,
    )
    .unwrap_err();

    assert_eq!(
        error,
        r#"Dialect "postgres" does not support function "UNKNOWN_FUNC" in this builder."#
    );
}

#[test]
fn supported_operators_render_for_sql_dialect() {
    let comparison = eval_expr_for_dialect(
        ExprNode::Cmp {
            left: Box::new(ref_expr("age")),
            op: ">=".to_string(),
            right: Box::new(raw_expr("18")),
        },
        &Dialect::Postgres,
    )
    .unwrap();
    let arithmetic = eval_expr_for_dialect(
        ExprNode::Arith {
            left: Box::new(ref_expr("age")),
            op: "+".to_string(),
            right: Box::new(raw_expr("1")),
        },
        &Dialect::Postgres,
    )
    .unwrap();

    assert_eq!(comparison.raw, "age >= 18");
    assert_eq!(arithmetic.raw, "(age + 1)");
}

#[test]
fn unsupported_operator_is_rejected() {
    let error = eval_expr_for_dialect(
        ExprNode::Cmp {
            left: Box::new(ref_expr("name")),
            op: "~~".to_string(),
            right: Box::new(raw_expr("'A%'")),
        },
        &Dialect::Postgres,
    )
    .unwrap_err();

    assert_eq!(
        error,
        r#"Dialect "postgres" does not support operator "~~" in this builder."#
    );
}

#[test]
fn window_function_renders_for_supported_dialect() {
    let result = eval_expr_for_dialect(
        ExprNode::Over {
            expr: Box::new(ExprNode::FnCall {
                name: "row_number".to_string(),
                args: vec![],
            }),
            partition_by: vec![],
            order_by: vec![],
            dialect: "postgres".to_string(),
        },
        &Dialect::Postgres,
    )
    .unwrap();

    assert_eq!(result.raw, "ROW_NUMBER() OVER ()");
}

#[test]
fn window_function_is_rejected_for_unsupported_dialect() {
    let error = eval_expr_for_dialect(
        ExprNode::Over {
            expr: Box::new(ExprNode::FnCall {
                name: "row_number".to_string(),
                args: vec![],
            }),
            partition_by: vec![],
            order_by: vec![],
            dialect: "mongodb".to_string(),
        },
        &Dialect::Unknown("mongodb".to_string()),
    )
    .unwrap_err();

    assert_eq!(
        error,
        r#"Dialect "mongodb" does not support window functions in this builder."#
    );
}
