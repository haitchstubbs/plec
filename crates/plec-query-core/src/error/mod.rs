//! Query-builder error types.
//!
//! All public functions that can fail return `Result<T, QueryError>`.
//! Callers should match on the variant to decide how to surface the error;
//! the `Display` impl produces a human-readable message suitable for
//! end-user display or logging.

use serde::Serialize;
use std::fmt;

// Rust guideline compliant 2026-02-21

/// Internal error type for builder, registry, and SQL-rendering operations.
#[derive(Debug, thiserror::Error)]
pub enum BuilderError {
    #[error("Invalid builder handle: {0}")]
    InvalidHandle(String),

    #[error("Builder handle '{0}' not found or expired")]
    HandleNotFound(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Render error: {0}")]
    Render(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// An error produced during query building or SQL rendering.
///
/// Variants are non-exhaustive so that future error categories can be added
/// without breaking downstream `match` arms.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum QueryError {
    /// A SQL feature that is fundamentally incompatible with the active dialect.
    ///
    /// The renderer must not emit silent, incorrect SQL; callers must handle
    /// this error and never suppress it.
    ///
    /// # Examples
    ///
    /// ```
    /// // RETURNING on MySQL has no valid rewrite — it is a hard error.
    /// ```
    HardError {
        /// Short identifier for the unsupported feature (e.g. `"RETURNING"`).
        feature: String,
        /// The canonical dialect name (e.g. `"mysql"`).
        dialect: String,
        /// Human-readable explanation of why the feature is unavailable.
        message: String,
    },

    /// A structural or semantic error in the query state that prevents rendering.
    ///
    /// This maps to the existing `String` errors produced by validation
    /// and rendering helpers, allowing a gradual migration from
    /// `Result<T, String>` to `Result<T, QueryError>`.
    Validation(String),
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HardError {
                feature,
                dialect,
                message,
            } => write!(
                f,
                "Dialect \"{dialect}\" does not support {feature}: {message}"
            ),
            Self::Validation(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for QueryError {}

/// Converts a legacy `String` error into a [`QueryError::Validation`].
///
/// This `From` impl exists solely to ease the incremental migration from
/// `Result<T, String>` to `Result<T, QueryError>`.  It will be removed once
/// all call-sites have been updated to produce typed errors directly.
impl From<String> for QueryError {
    fn from(msg: String) -> Self {
        Self::Validation(msg)
    }
}

// ─── UnsupportedFeature ───────────────────────────────────────────────────────

/// A specific SQL feature that a dialect does not support.
///
/// Used as a typed discriminant in [`ValidationError`] so that callers can
/// programmatically react to specific unsupported features rather than
/// parsing message strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum UnsupportedFeature {
    /// `RIGHT JOIN` syntax.
    RightJoin,
    /// `FULL OUTER JOIN` syntax.
    FullJoin,
    /// `RETURNING` clause on an `UPDATE` statement.
    ReturningOnUpdate,
    /// Window function expressions (e.g. `COUNT(*) OVER (...)`).
    WindowFunction,
}

impl fmt::Display for UnsupportedFeature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::RightJoin => "RightJoin",
            Self::FullJoin => "FullJoin",
            Self::ReturningOnUpdate => "ReturningOnUpdate",
            Self::WindowFunction => "WindowFunction",
        };
        f.write_str(s)
    }
}

// ─── ValidationError ─────────────────────────────────────────────────────────

/// A structured error from the dialect-aware pre-render validation pipeline.
///
/// Returned before SQL rendering when a feature in the query state is
/// unsupported by the active dialect. Use the `feature` field to
/// programmatically react to specific violations without parsing message text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidationError {
    /// Stable snake_case code for machine matching (e.g. `"right_join"`).
    pub code: &'static str,
    /// Human-readable explanation of why the feature is unavailable.
    pub message: String,
    /// Typed discriminant identifying the unsupported feature.
    pub feature: UnsupportedFeature,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (feature: {})", self.message, self.feature)
    }
}

#[cfg(test)]
#[path = "error.test.rs"]
mod error_test;
