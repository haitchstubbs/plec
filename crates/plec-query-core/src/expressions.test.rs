use super::{arith_binary_for_dialect, cmp_for_dialect, fn_call_for_dialect};
use crate::dialect::Dialect;
use crate::types::{Primitive, SqlValue};

#[test]
fn cmp_for_dialect_rejects_unsupported_operator() {
    let err = cmp_for_dialect(
        SqlValue::Primitive(Primitive::String("a".to_string())),
        "~~".to_string(),
        SqlValue::Primitive(Primitive::String("b".to_string())),
        &Dialect::Postgres,
    )
    .unwrap_err();

    assert_eq!(
        err,
        r#"Dialect "postgres" does not support operator "~~" in this builder."#
    );
}

#[test]
fn fn_call_for_dialect_normalizes_supported_names() {
    let query = fn_call_for_dialect(
        "lower".to_string(),
        vec![SqlValue::Primitive(Primitive::String("ABC".to_string()))],
        &Dialect::Postgres,
    )
    .unwrap();

    assert_eq!(query.raw, "LOWER('ABC')");
}

#[test]
fn arith_binary_for_dialect_rejects_unknown_dialect() {
    let err = arith_binary_for_dialect(
        SqlValue::Primitive(Primitive::Number(1_u8.into())),
        "+".to_string(),
        SqlValue::Primitive(Primitive::Number(2_u8.into())),
        &Dialect::Unknown("mongodb".to_string()),
    )
    .unwrap_err();

    assert_eq!(
        err,
        r#"Dialect "mongodb" does not support operator "+" in this builder."#
    );
}
