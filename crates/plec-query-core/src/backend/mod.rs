//! Per-dialect SQL rendering backends.
//!
//! This module defines the [`SqlBackend`] trait that encapsulates all
//! dialect-specific SQL rendering rules for a single database system. Rendering
//! functions accept a [`BackendRef`] and call [`BackendRef::capabilities`]
//! instead of branching on a dialect enum directly.
//!
//! # Migration status (issue #18)
//!
//! Only [`PostgresBackend`] and [`MySqlBackend`] and [`SqliteBackend`] are
//! dedicated structs so far. All other dialects fall back to
//! [`GenericBackend`], which delegates to [`capabilities_for_dialect`] so that
//! existing behavior is fully preserved during the incremental migration.
//!
//! # Selecting a backend
//!
//! ```text
//! let backend = backend_for_dialect_name("postgres");
//! assert!(backend.capabilities().returning);
//! ```

use crate::dialect::{capabilities_for_dialect, Dialect, DialectCapabilities, PlaceholderStyle};
use crate::types::SqlQuery;
use std::sync::Arc;

// ─── Trait ────────────────────────────────────────────────────────────────────

/// Contract for a dialect-specific SQL rendering backend.
///
/// Each implementor encapsulates the SQL rendering rules for one database
/// dialect (e.g. Postgres, MySQL, SQLite). Rendering functions receive a
/// `BackendRef` and call `capabilities()` instead of pattern-matching on a
/// dialect enum, so new dialects can be added without touching core rendering
/// paths.
///
/// # Implementing a new dialect
///
/// 1. Create a public zero-sized struct (e.g. `pub struct SnowflakeBackend;`).
/// 2. Implement `SqlBackend` for it.
/// 3. Add a match arm in [`backend_for_dialect`].
pub trait SqlBackend: Send + Sync {
    /// Canonical lowercase name of this dialect (e.g. `"postgres"`).
    fn name(&self) -> &str;

    /// Capability flags governing SQL rendering for this dialect.
    fn capabilities(&self) -> DialectCapabilities;

    /// Rewrite `?` placeholders in `query.text` to this dialect's native style.
    ///
    /// Each `?` in `query.text` is replaced according to the dialect's
    /// [`PlaceholderStyle`]. The number of `?` tokens must equal the number of
    /// bound values in `query.values`.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The dialect does not support parameterized placeholders.
    /// - The count of `?` tokens does not match `query.values.len()`.
    fn compile_placeholders(&self, query: SqlQuery) -> Result<SqlQuery, String> {
        let style = self.capabilities().placeholder_style;

        if style == PlaceholderStyle::Unsupported {
            return Err(format!(
                "Dialect \"{}\" does not support SQL placeholder compilation in this builder.",
                self.name()
            ));
        }

        let value_count = query.values.len();
        let mut placeholder_index = 0usize;
        let mut compiled_text = String::with_capacity(query.text.len() + value_count * 3);

        for ch in query.text.chars() {
            if ch == '?' {
                placeholder_index += 1;
                if placeholder_index <= value_count {
                    match style {
                        PlaceholderStyle::DollarNumbered => {
                            compiled_text.push('$');
                            compiled_text.push_str(&placeholder_index.to_string());
                        }
                        PlaceholderStyle::QuestionMark => compiled_text.push('?'),
                        PlaceholderStyle::AtPNamed => {
                            compiled_text.push_str("@p");
                            compiled_text.push_str(&placeholder_index.to_string());
                        }
                        PlaceholderStyle::Unsupported => unreachable!(),
                    }
                } else {
                    compiled_text.push('?');
                }
            } else {
                compiled_text.push(ch);
            }
        }

        if placeholder_index != value_count {
            return Err(format!(
                "Dialect \"{}\" compilation expected {} placeholders but found {}.",
                self.name(),
                value_count,
                placeholder_index
            ));
        }

        Ok(SqlQuery {
            text: compiled_text,
            raw: query.raw,
            values: query.values,
        })
    }
}

// ─── BackendRef ───────────────────────────────────────────────────────────────

/// Reference-counted, cheaply cloneable handle to a [`SqlBackend`] implementation.
///
/// Wraps `Arc<dyn SqlBackend>` so that types that derive `Clone` (e.g.
/// `QueryParts`) can hold a backend without requiring `SqlBackend: Clone`.
/// Each clone is a cheap pointer increment — there is no allocation.
///
/// # Examples
///
/// ```text
/// let backend = backend_for_dialect_name("postgres");
/// let clone = backend.clone(); // cheap Arc clone
/// assert_eq!(backend.name(), clone.name());
/// ```
#[derive(Clone)]
pub struct BackendRef(Arc<dyn SqlBackend>);

impl BackendRef {
    /// Wrap `backend` in a new [`BackendRef`].
    pub fn new<T: SqlBackend + 'static>(backend: T) -> Self {
        Self(Arc::new(backend))
    }

    /// Returns the canonical dialect name for this backend.
    #[must_use]
    pub fn name(&self) -> &str {
        self.0.name()
    }

    /// Returns the capability flags for this backend's dialect.
    #[must_use]
    pub fn capabilities(&self) -> DialectCapabilities {
        self.0.capabilities()
    }

    /// Rewrite `?` placeholders in `query.text` to this dialect's native style.
    ///
    /// Delegates to [`SqlBackend::compile_placeholders`] on the inner backend.
    ///
    /// # Errors
    ///
    /// Propagates errors from the inner backend's `compile_placeholders`.
    pub fn compile_placeholders(&self, query: SqlQuery) -> Result<SqlQuery, String> {
        self.0.compile_placeholders(query)
    }
}

impl std::fmt::Debug for BackendRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("BackendRef").field(&self.0.name()).finish()
    }
}

// ─── Factory functions ────────────────────────────────────────────────────────

/// Create a [`BackendRef`] for the given dialect name string.
///
/// Falls back to [`GenericBackend`] for dialects that have not yet been given a
/// dedicated implementation. `GenericBackend` delegates to
/// [`capabilities_for_dialect`], so existing behavior is fully preserved.
///
/// # Examples
///
/// ```text
/// let pg = backend_for_dialect_name("postgres");
/// assert_eq!(pg.name(), "postgres");
/// ```
#[must_use]
pub fn backend_for_dialect_name(name: &str) -> BackendRef {
    backend_for_dialect(&Dialect::parse(name))
}

/// Create a [`BackendRef`] for the given [`Dialect`] value.
///
/// Only dialects that have a dedicated backend struct dispatch to a concrete
/// implementation; all others fall through to [`GenericBackend`].
#[must_use]
pub fn backend_for_dialect(dialect: &Dialect) -> BackendRef {
    match dialect {
        Dialect::Postgres => BackendRef::new(PostgresBackend),
        Dialect::Mysql => BackendRef::new(MySqlBackend),
        Dialect::Sqlite => BackendRef::new(SqliteBackend),
        other => BackendRef::new(GenericBackend {
            dialect: other.clone(),
        }),
    }
}

// ─── PostgresBackend ──────────────────────────────────────────────────────────

/// SQL rendering backend for PostgreSQL.
#[derive(Debug)]
pub struct PostgresBackend;

impl SqlBackend for PostgresBackend {
    fn name(&self) -> &str {
        "postgres"
    }

    fn capabilities(&self) -> DialectCapabilities {
        capabilities_for_dialect(&Dialect::Postgres)
    }
}

// ─── MySqlBackend ─────────────────────────────────────────────────────────────

/// SQL rendering backend for MySQL.
#[derive(Debug)]
pub struct MySqlBackend;

impl SqlBackend for MySqlBackend {
    fn name(&self) -> &str {
        "mysql"
    }

    fn capabilities(&self) -> DialectCapabilities {
        capabilities_for_dialect(&Dialect::Mysql)
    }
}

// ─── SqliteBackend ────────────────────────────────────────────────────────────

/// SQL rendering backend for SQLite.
#[derive(Debug)]
pub struct SqliteBackend;

impl SqlBackend for SqliteBackend {
    fn name(&self) -> &str {
        "sqlite"
    }

    fn capabilities(&self) -> DialectCapabilities {
        capabilities_for_dialect(&Dialect::Sqlite)
    }
}

// ─── GenericBackend (migration scaffold) ─────────────────────────────────────

/// Fallback backend for dialects that do not yet have a dedicated struct.
///
/// Delegates all capability lookups to [`capabilities_for_dialect`] so that
/// existing behavior is fully preserved during the incremental migration to
/// dedicated per-dialect structs (see issue #18).
///
/// Once every dialect in [`backend_for_dialect`] dispatches to a named struct,
/// this type and [`capabilities_for_dialect`] can both be deleted.
#[derive(Debug)]
struct GenericBackend {
    dialect: Dialect,
}

impl SqlBackend for GenericBackend {
    fn name(&self) -> &str {
        self.dialect.as_str()
    }

    fn capabilities(&self) -> DialectCapabilities {
        capabilities_for_dialect(&self.dialect)
    }
}

// Rust guideline compliant 2026-02-21
