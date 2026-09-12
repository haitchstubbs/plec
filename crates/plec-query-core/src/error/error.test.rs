use super::{BuilderError, QueryError};

#[test]
fn hard_error_display_mentions_feature_and_dialect() {
    let err = QueryError::HardError {
        feature: "RETURNING".to_string(),
        dialect: "mysql".to_string(),
        message: "MySQL does not support RETURNING.".to_string(),
    };
    let msg = err.to_string();
    assert!(msg.contains("RETURNING"), "got: {msg}");
    assert!(msg.contains("mysql"), "got: {msg}");
}

#[test]
fn validation_error_display_passes_through_message() {
    let err = QueryError::Validation("missing table name".to_string());
    assert_eq!(err.to_string(), "missing table name");
}

#[test]
fn from_string_produces_validation_variant() {
    let err: QueryError = "oops".to_string().into();
    assert_eq!(err, QueryError::Validation("oops".to_string()));
}

#[test]
fn builder_error_display_matches_boundary_messages() {
    assert_eq!(
        BuilderError::InvalidHandle("abc".to_string()).to_string(),
        "Invalid builder handle: abc"
    );
    assert_eq!(
        BuilderError::HandleNotFound("42".to_string()).to_string(),
        "Builder handle '42' not found or expired"
    );
}
