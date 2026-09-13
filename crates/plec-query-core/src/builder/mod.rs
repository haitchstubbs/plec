use crate::dialect::Dialect;
use crate::expr_node::ExprNode;
use crate::types::{SqlIdentifier, SqlQuery, SqlValue};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, OnceLock};

// ─── Sub-modules ─────────────────────────────────────────────────────────────

pub(crate) mod canonical;
pub mod methods;
pub mod registry;
pub mod render;

pub use methods::*;
pub use render::{
    builder_as, builder_insert_columns, builder_query, builder_raw, builder_selected_columns,
    builder_text, builder_values,
};

const VALIDATION_STATUS_UNKNOWN: u8 = 0;
const VALIDATION_STATUS_VALID: u8 = 1;
const VALIDATION_STATUS_INVALID: u8 = 2;

#[derive(Debug, Default)]
pub struct ValidationCache {
    status: AtomicU8,
    error: OnceLock<String>,
}

impl ValidationCache {
    pub fn new() -> Self {
        Self {
            status: AtomicU8::new(VALIDATION_STATUS_UNKNOWN),
            error: OnceLock::new(),
        }
    }

    pub fn get_or_try_init<F>(&self, init: F) -> Result<(), String>
    where
        F: FnOnce() -> Result<(), String>,
    {
        match self.status.load(Ordering::Acquire) {
            VALIDATION_STATUS_VALID => return Ok(()),
            VALIDATION_STATUS_INVALID => {
                return Err(self
                    .error
                    .get()
                    .cloned()
                    .unwrap_or_else(|| "Validation cache missing stored error".to_string()))
            }
            _ => {}
        }

        match init() {
            Ok(()) => {
                self.status
                    .store(VALIDATION_STATUS_VALID, Ordering::Release);
                Ok(())
            }
            Err(err) => {
                let stored_error = self.error.get_or_init(|| err).clone();
                self.status
                    .store(VALIDATION_STATUS_INVALID, Ordering::Release);
                Err(stored_error)
            }
        }
    }
}

// ─── Statement kind ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum StatementKind {
    Select,
    Insert,
    Update,
    Delete,
}

// ─── CTE ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CteDefinition {
    pub name: String,
    pub recursive: bool,
    pub columns: Vec<String>,
    pub query: SqlQuery,
}

// ─── JOIN ─────────────────────────────────────────────────────────────────────

/// Canonical SQL join type understood by the builder.
///
/// All public builder entry-points accept a `&str` and parse it through
/// [`JoinType::parse`] before storing state, so callers are insulated from
/// casing and minor alias differences (e.g. `"LEFT OUTER JOIN"` is accepted).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinType {
    /// `INNER JOIN`
    Inner,
    /// `LEFT JOIN` (alias: `LEFT OUTER JOIN`)
    Left,
    /// `RIGHT JOIN` (alias: `RIGHT OUTER JOIN`)
    Right,
    /// `FULL OUTER JOIN` (alias: `FULL JOIN`)
    FullOuter,
    /// `CROSS JOIN` — commits immediately without a predicate.
    Cross,
}

impl JoinType {
    /// Parse a case-insensitive join type string into a [`JoinType`].
    ///
    /// # Errors
    ///
    /// Returns an error string when `s` is not a recognized join keyword.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_uppercase().as_str() {
            "INNER JOIN" | "INNER" => Ok(JoinType::Inner),
            "LEFT JOIN" | "LEFT OUTER JOIN" | "LEFT" => Ok(JoinType::Left),
            "RIGHT JOIN" | "RIGHT OUTER JOIN" | "RIGHT" => Ok(JoinType::Right),
            "FULL OUTER JOIN" | "FULL JOIN" | "FULL OUTER" | "FULL" => Ok(JoinType::FullOuter),
            "CROSS JOIN" | "CROSS" => Ok(JoinType::Cross),
            other => Err(format!("Unknown join type: '{other}'")),
        }
    }

    /// Return the canonical SQL keyword string for this join type.
    #[must_use]
    pub fn as_sql(self) -> &'static str {
        match self {
            JoinType::Inner => "INNER JOIN",
            JoinType::Left => "LEFT JOIN",
            JoinType::Right => "RIGHT JOIN",
            JoinType::FullOuter => "FULL OUTER JOIN",
            JoinType::Cross => "CROSS JOIN",
        }
    }
}

impl std::fmt::Display for JoinType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_sql())
    }
}

/// Join state opened by a `join*()` call, awaiting `on()` / `using()` before
/// it is committed into the finalized join list at render time.
#[derive(Debug, Clone)]
pub struct PendingJoinState {
    pub join_type: JoinType,
    pub source: SqlQuery,
    pub predicate: Option<SqlQuery>,
    pub using: Vec<SqlIdentifier>,
}

// ─── Compound operators ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompoundOperator {
    Union,
    UnionAll,
    Intersect,
    Except,
}

impl std::fmt::Display for CompoundOperator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_sql())
    }
}

impl CompoundOperator {
    #[must_use]
    pub fn as_sql(self) -> &'static str {
        match self {
            CompoundOperator::Union => "UNION",
            CompoundOperator::UnionAll => "UNION ALL",
            CompoundOperator::Intersect => "INTERSECT",
            CompoundOperator::Except => "EXCEPT",
        }
    }

    pub fn from_keyword(s: &str) -> Result<Self, String> {
        match s {
            "UNION" => Ok(CompoundOperator::Union),
            "UNION ALL" => Ok(CompoundOperator::UnionAll),
            "INTERSECT" => Ok(CompoundOperator::Intersect),
            "EXCEPT" => Ok(CompoundOperator::Except),
            other => Err(format!("Unknown compound operator: {}", other)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompoundPart {
    pub operator: CompoundOperator,
    pub query: Arc<SqlQuery>,
}

// ─── INSERT ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InsertConflictTarget {
    Columns(Vec<String>),
    Constraint(String),
}

#[derive(Debug, Clone)]
pub enum InsertConflictAction {
    DoNothing,
    DoUpdate {
        assignments: Vec<UpdateAssignment>,
        predicate: Option<SqlQuery>,
    },
}

#[derive(Debug, Clone, Default)]
pub struct InsertConflict {
    pub target: Option<InsertConflictTarget>,
    pub action: Option<InsertConflictAction>,
}

#[derive(Debug, Clone, Default)]
pub struct InsertState {
    pub into: Option<String>,
    pub column_names: Vec<String>,
    pub rows: Vec<Vec<SqlValue>>,
    pub select: Option<SqlQuery>,
    pub conflict: InsertConflict,
}

// ─── UPDATE ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum AssignmentValue {
    Sql(SqlValue),
    Expr(ExprNode),
}

#[derive(Debug, Clone)]
pub struct UpdateAssignment {
    pub column: String,
    pub value: AssignmentValue,
}

#[derive(Debug, Clone, Default)]
pub struct UpdateState {
    pub table: Option<String>,
    pub assignments: Vec<UpdateAssignment>,
}

// ─── WHERE / HAVING clause storage ───────────────────────────────────────────

/// Logical operator joining a predicate onto the preceding one.
/// The first entry in a clause list ignores its op (no preceding clause).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicalOp {
    And,
    Or,
}

impl LogicalOp {
    pub fn as_str(self) -> &'static str {
        match self {
            LogicalOp::And => "AND",
            LogicalOp::Or => "OR",
        }
    }
}

/// One predicate entry in a flat WHERE / HAVING list.
#[derive(Debug, Clone)]
pub struct WhereEntry {
    pub pred: SqlQuery,
    /// Logical operator used to combine this predicate with the one before it.
    pub op: LogicalOp,
}

// ─── DELETE ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct DeleteState {
    pub from_table: Option<String>,
}

// ─── DISTINCT ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum DistinctMode {
    Distinct,
    On(Vec<SqlQuery>),
}

// ─── SELECT lock clauses ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockStrength {
    Update,
    Share,
}

impl LockStrength {
    pub fn as_sql(self) -> &'static str {
        match self {
            LockStrength::Update => "FOR UPDATE",
            LockStrength::Share => "FOR SHARE",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockModifier {
    NoWait,
    SkipLocked,
}

impl LockModifier {
    pub fn as_sql(self) -> &'static str {
        match self {
            LockModifier::NoWait => "NOWAIT",
            LockModifier::SkipLocked => "SKIP LOCKED",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectLockClause {
    pub strength: LockStrength,
    pub modifier: Option<LockModifier>,
}

// ─── SELECT pagination ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct PaginationState {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

// ─── QueryParts ──────────────────────────────────────────────────────────────

/// All accumulated state for a query-builder instance.
///
/// `select` stores only the column/expression content (without the SELECT keyword).
/// The SELECT keyword (and DISTINCT) are prepended at render time by `build_query`.
///
/// WHERE and HAVING predicates are stored as flat lists (`where_clauses`,
/// `having_clauses`) to keep the clone cost O(n) in the number of clauses rather
/// than O(n * text_length) as the deeply-nested binary-tree approach would produce.
/// Rendering composes them left-to-right at query-build time (which is cached).
#[derive(Debug, Clone)]
pub struct QueryParts {
    pub dialect: Dialect,
    pub statement: Option<StatementKind>,
    pub ctes: Arc<Vec<CteDefinition>>,
    pub distinct: Option<DistinctMode>,
    /// Content of the SELECT clause (no leading keyword). Built by builder_select_*.
    pub select: Option<Arc<SqlQuery>>,
    pub selected_columns: Vec<String>,
    pub from: Option<Arc<SqlQuery>>,
    pub joins: Arc<Vec<SqlQuery>>,
    pub pending_join: Option<PendingJoinState>,
    /// Flat list of WHERE predicates (replacing the old single nested SqlQuery).
    pub where_clauses: Arc<Vec<WhereEntry>>,
    pub group_by: Arc<Vec<SqlIdentifier>>,
    /// Flat list of HAVING predicates.
    pub having_clauses: Arc<Vec<WhereEntry>>,
    pub order_by: Arc<Vec<SqlQuery>>,
    pub pagination: PaginationState,
    pub lock: Option<SelectLockClause>,
    pub returning: Option<Arc<SqlQuery>>,
    pub compounds: Arc<Vec<CompoundPart>>,
    pub insert: InsertState,
    pub update: UpdateState,
    pub delete: DeleteState,
    /// Cached rendered query. Set on first output access; cleared implicitly when
    /// a new handle is created (mutations always produce a fresh `QueryParts`).
    pub cached_query: Option<Arc<SqlQuery>>,
    /// Cached validation result for this exact immutable builder state.
    ///
    /// Shared across cloned snapshots of the same handle state so repeated
    /// render attempts do not rerun dialect and structural validation.
    pub validation_cache: Arc<ValidationCache>,
    /// When `true`, fallback rewrites (e.g. ILIKE → LOWER LIKE) are promoted to
    /// hard errors instead of silently rewriting with a warning.
    pub dialect_strict: bool,
    /// Warnings accumulated during predicate / expression evaluation (builder-time).
    ///
    /// These are carried through to [`RenderResult`](crate::types::RenderResult)
    /// at render time.  When `dialect_strict` is `true` and this list is
    /// non-empty, `build_query` surfaces an error instead of emitting SQL.
    pub pending_warnings: Vec<crate::dialect::DialectWarning>,
}

impl Default for QueryParts {
    fn default() -> Self {
        Self {
            dialect: Dialect::default(),
            statement: None,
            ctes: Arc::new(vec![]),
            distinct: None,
            select: None,
            selected_columns: vec![],
            from: None,
            joins: Arc::new(vec![]),
            pending_join: None,
            where_clauses: Arc::new(vec![]),
            group_by: Arc::new(vec![]),
            having_clauses: Arc::new(vec![]),
            order_by: Arc::new(vec![]),
            pagination: PaginationState::default(),
            lock: None,
            returning: None,
            compounds: Arc::new(vec![]),
            insert: InsertState::default(),
            update: UpdateState::default(),
            delete: DeleteState::default(),
            cached_query: None,
            validation_cache: Arc::new(ValidationCache::new()),
            dialect_strict: false,
            pending_warnings: Vec::new(),
        }
    }
}
