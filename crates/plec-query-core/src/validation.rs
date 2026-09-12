//! Dialect-aware pre-render validation pipeline.
//!
//! This module defines the [`DialectValidator`] trait and per-dialect
//! implementations. Validation runs after query state is finalised but
//! **before** SQL rendering, so that unsupported feature combinations
//! surface as structured [`ValidationError`]s rather than silent bad SQL or
//! a Rust panic.
//!
//! # Implementing a new dialect validator
//!
//! 1. Create a zero-sized struct (e.g. `pub struct MssqlDialectValidator;`).
//! 2. Implement [`DialectValidator`] for it.
//! 3. Add a match arm in [`validator_for_dialect`].

use crate::builder::{JoinType, QueryParts, StatementKind};
use crate::dialect::Dialect;
use crate::error::{UnsupportedFeature, ValidationError};

/// Contract for pre-render dialect validation.
///
/// Each implementor checks the query state for features that are incompatible
/// with its dialect. Validation runs before SQL rendering; the renderer never
/// receives a state that has failed validation.
///
/// # Errors
///
/// Returns `Err(Vec<ValidationError>)` when one or more unsupported features
/// are detected. The vec is always non-empty on the error path.
pub trait DialectValidator: Send + Sync {
    /// Validate `parts` against this dialect's constraints.
    ///
    /// # Errors
    ///
    /// Returns a non-empty list of [`ValidationError`]s when `parts` contains
    /// features that are unsupported by this dialect.
    fn validate(&self, parts: &QueryParts) -> Result<(), Vec<ValidationError>>;
}

/// Return the dedicated [`DialectValidator`] for `dialect`.
///
/// Dialects without a dedicated validator return a [`DefaultDialectValidator`]
/// that accepts all queries.
#[must_use]
pub fn validator_for_dialect(dialect: &Dialect) -> &'static dyn DialectValidator {
    match dialect {
        Dialect::Sqlite => &SqliteDialectValidator,
        _ => &DefaultDialectValidator,
    }
}

pub struct DefaultDialectValidator;

impl DialectValidator for DefaultDialectValidator {
    fn validate(&self, _parts: &QueryParts) -> Result<(), Vec<ValidationError>> {
        Ok(())
    }
}

pub struct SqliteDialectValidator;

impl DialectValidator for SqliteDialectValidator {
    fn validate(&self, parts: &QueryParts) -> Result<(), Vec<ValidationError>> {
        let mut errors: Vec<ValidationError> = Vec::new();
        let dialect_name = parts.dialect.as_str();

        // RIGHT JOIN: unsupported before SQLite 3.39.0 — reject conservatively.
        let has_right_join = parts.joins.iter().any(|j| j.raw.contains("RIGHT JOIN"))
            || parts
                .pending_join
                .as_ref()
                .is_some_and(|pj| pj.join_type == JoinType::Right);

        if has_right_join {
            errors.push(ValidationError {
                code: "right_join",
                message: format!(
                    "Dialect \"{dialect_name}\" does not support RIGHT JOIN \
                     (unsupported before SQLite 3.39.0)."
                ),
                feature: UnsupportedFeature::RightJoin,
            });
        }

        // FULL OUTER JOIN: unsupported before SQLite 3.39.0 — reject conservatively.
        let has_full_join = parts
            .joins
            .iter()
            .any(|j| j.raw.contains("FULL JOIN") || j.raw.contains("FULL OUTER JOIN"))
            || parts
                .pending_join
                .as_ref()
                .is_some_and(|pj| pj.join_type == JoinType::FullOuter);

        if has_full_join {
            errors.push(ValidationError {
                code: "full_join",
                message: format!(
                    "Dialect \"{dialect_name}\" does not support FULL OUTER JOIN \
                     (unsupported before SQLite 3.39.0)."
                ),
                feature: UnsupportedFeature::FullJoin,
            });
        }

        // RETURNING on UPDATE: unsupported in older SQLite — reject conservatively.
        let has_returning_on_update =
            parts.returning.is_some() && parts.statement == Some(StatementKind::Update);

        if has_returning_on_update {
            errors.push(ValidationError {
                code: "returning_on_update",
                message: format!(
                    "Dialect \"{dialect_name}\" does not support RETURNING on UPDATE \
                     statements (unsupported in older versions)."
                ),
                feature: UnsupportedFeature::ReturningOnUpdate,
            });
        }

        // Window functions: unsupported before SQLite 3.25.0 — reject conservatively.
        let has_window_function = parts
            .select
            .as_ref()
            .is_some_and(|s| s.raw.contains("OVER (") || s.raw.contains("OVER("));

        if has_window_function {
            errors.push(ValidationError {
                code: "window_function",
                message: format!(
                    "Dialect \"{dialect_name}\" does not support window functions \
                     (unsupported before SQLite 3.25.0)."
                ),
                feature: UnsupportedFeature::WindowFunction,
            });
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}
