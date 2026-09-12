use crate::builder::{
    DistinctMode, InsertConflictAction, InsertConflictTarget, LockModifier, LockStrength,
    PaginationState, QueryParts, StatementKind,
};
use crate::types::{Primitive, SqlQuery};
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum Dialect {
    #[default]
    Postgres,
    DuckDb,
    Sqlite,
    Mysql,
    Mssql,
    Oracle,
    Snowflake,
    GoogleSql,
    Redshift,
    BigQuery,
    ClickHouse,
    Unknown(String),
}

impl Dialect {
    pub fn parse(input: &str) -> Self {
        let normalized = normalize_dialect_name(input);
        match normalized.as_str() {
            "postgres" => Self::Postgres,
            "duckdb" => Self::DuckDb,
            "sqlite" => Self::Sqlite,
            "mysql" => Self::Mysql,
            "mssql" | "mssqlserver" => Self::Mssql,
            "oracle" => Self::Oracle,
            "snowflake" => Self::Snowflake,
            "googlesql" => Self::GoogleSql,
            "redshift" => Self::Redshift,
            "bigquery" => Self::BigQuery,
            "clickhouse" => Self::ClickHouse,
            _ => Self::Unknown(normalized),
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Postgres => "postgres",
            Self::DuckDb => "duckdb",
            Self::Sqlite => "sqlite",
            Self::Mysql => "mysql",
            Self::Mssql => "mssql",
            Self::Oracle => "oracle",
            Self::Snowflake => "snowflake",
            Self::GoogleSql => "googlesql",
            Self::Redshift => "redshift",
            Self::BigQuery => "bigquery",
            Self::ClickHouse => "clickhouse",
            Self::Unknown(value) => value.as_str(),
        }
    }

    #[must_use]
    pub fn is_sql(&self) -> bool {
        !matches!(self, Self::Unknown(_))
    }

    /// Returns the canonical capability flags for this dialect.
    ///
    /// This is a thin convenience wrapper over [`capabilities_for_dialect`].
    /// Prefer calling this on a `Dialect` value directly rather than the
    /// free function where the dialect instance is already in scope.
    #[must_use]
    pub fn capabilities(&self) -> DialectCapabilities {
        capabilities_for_dialect(self)
    }
}

impl Display for Dialect {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

pub fn normalize_dialect_name(input: &str) -> String {
    input.trim().to_ascii_lowercase()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertConflictStyle {
    Unsupported,
    OnConflict,
    MySql,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaginationSyntaxFamily {
    Unsupported,
    LimitOffset,
    TopOffsetFetch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecursiveCteStyle {
    Unsupported,
    WithRecursiveKeyword,
    WithOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceholderStyle {
    Unsupported,
    DollarNumbered,
    QuestionMark,
    AtPNamed,
}

#[derive(Debug, Clone, Copy)]
pub struct RecursiveCteCapabilities {
    pub style: RecursiveCteStyle,
    pub column_aliases_required: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct PaginationCapabilities {
    pub family: PaginationSyntaxFamily,
    pub offset_requires_order_by: bool,
    pub offset_only_allowed: bool,
    pub limit_only_uses_top: bool,
}

#[derive(Debug, Clone, Default)]
pub struct RenderedPagination {
    pub select_prefix: Option<SqlQuery>,
    pub trailing_clause: Option<SqlQuery>,
}

/// Join-syntax capabilities for a specific dialect.
///
/// `INNER JOIN`, `LEFT JOIN`, and `CROSS JOIN` are assumed universally supported
/// and are therefore not gated here. Only the variants that vary across dialects
/// have explicit flags.
#[derive(Debug, Clone, Copy)]
pub struct JoinCapabilities {
    /// Whether `RIGHT [OUTER] JOIN` is supported.
    pub right_join: bool,
    /// Whether `FULL [OUTER] JOIN` is supported.
    pub full_outer_join: bool,
    /// Whether `USING (col, ...)` syntax is supported as an alternative to `ON`.
    pub using_syntax: bool,
    /// Whether `LATERAL` subquery joins are supported.
    pub lateral_join: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatorKind {
    Comparison,
    Arithmetic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionFamily {
    Aggregate,
    Scalar,
    Window,
}

#[derive(Debug, Clone, Copy)]
pub struct FunctionCapability {
    pub family: FunctionFamily,
    pub window: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatorFamily {
    Comparison,
    Arithmetic,
}

#[derive(Debug, Clone, Copy)]
pub struct OperatorCapability {
    pub family: OperatorFamily,
}

#[derive(Debug, Clone, Copy)]
struct FunctionRegistryEntry {
    canonical_name: &'static str,
    capability: FunctionCapability,
}

#[derive(Debug, Clone, Copy)]
struct OperatorRegistryEntry {
    canonical_operator: &'static str,
    capability: OperatorCapability,
}

#[derive(Debug, Clone, Copy)]
pub struct DialectCapabilities {
    pub placeholder_style: PlaceholderStyle,
    pub returning: bool,
    pub window_functions: bool,
    /// Whether the dialect accepts explicit `NULLS FIRST` / `NULLS LAST` in ORDER BY.
    pub null_ordering: bool,
    pub distinct_on: bool,
    /// Whether the dialect supports basic `WITH` (non-recursive) CTEs.
    pub supports_cte: bool,
    /// Whether the dialect supports the `ILIKE` case-insensitive pattern operator.
    pub ilike: bool,
    pub insert_conflict: InsertConflictStyle,
    pub pagination: PaginationCapabilities,
    pub lock_strengths: &'static [LockStrength],
    pub lock_modifiers: &'static [LockModifier],
    pub joins: JoinCapabilities,
    pub recursive_ctes: RecursiveCteCapabilities,
}

const NO_LOCK_STRENGTHS: &[LockStrength] = &[];
const NO_LOCK_MODIFIERS: &[LockModifier] = &[];
const POSTGRES_LOCK_STRENGTHS: &[LockStrength] = &[LockStrength::Update, LockStrength::Share];
const POSTGRES_LOCK_MODIFIERS: &[LockModifier] = &[LockModifier::NoWait, LockModifier::SkipLocked];
const UNSUPPORTED_PAGINATION: PaginationCapabilities = PaginationCapabilities {
    family: PaginationSyntaxFamily::Unsupported,
    offset_requires_order_by: false,
    offset_only_allowed: false,
    limit_only_uses_top: false,
};
const LIMIT_OFFSET_PAGINATION: PaginationCapabilities = PaginationCapabilities {
    family: PaginationSyntaxFamily::LimitOffset,
    offset_requires_order_by: false,
    offset_only_allowed: true,
    limit_only_uses_top: false,
};
const MYSQL_PAGINATION: PaginationCapabilities = PaginationCapabilities {
    family: PaginationSyntaxFamily::LimitOffset,
    offset_requires_order_by: false,
    offset_only_allowed: false,
    limit_only_uses_top: false,
};
const MSSQL_PAGINATION: PaginationCapabilities = PaginationCapabilities {
    family: PaginationSyntaxFamily::TopOffsetFetch,
    offset_requires_order_by: true,
    offset_only_allowed: true,
    limit_only_uses_top: true,
};
const RECURSIVE_CTES_UNSUPPORTED: RecursiveCteCapabilities = RecursiveCteCapabilities {
    style: RecursiveCteStyle::Unsupported,
    column_aliases_required: false,
};
const RECURSIVE_CTES_WITH_RECURSIVE: RecursiveCteCapabilities = RecursiveCteCapabilities {
    style: RecursiveCteStyle::WithRecursiveKeyword,
    column_aliases_required: false,
};
const RECURSIVE_CTES_WITH_RECURSIVE_COLUMNS: RecursiveCteCapabilities = RecursiveCteCapabilities {
    style: RecursiveCteStyle::WithRecursiveKeyword,
    column_aliases_required: true,
};
const RECURSIVE_CTES_WITH_ONLY: RecursiveCteCapabilities = RecursiveCteCapabilities {
    style: RecursiveCteStyle::WithOnly,
    column_aliases_required: false,
};
const RECURSIVE_CTES_WITH_ONLY_COLUMNS: RecursiveCteCapabilities = RecursiveCteCapabilities {
    style: RecursiveCteStyle::WithOnly,
    column_aliases_required: true,
};

// ─── Join capability constants ────────────────────────────────────────────────

/// Joins available on most full-featured RDBMS: all four join types, USING, and LATERAL.
const FULL_JOINS: JoinCapabilities = JoinCapabilities {
    right_join: true,
    full_outer_join: true,
    using_syntax: true,
    lateral_join: true,
};

/// MySQL supports RIGHT and USING but not FULL OUTER JOIN; supports LATERAL since 8.0.14.
const MYSQL_JOINS: JoinCapabilities = JoinCapabilities {
    right_join: true,
    full_outer_join: false,
    using_syntax: true,
    lateral_join: true,
};

/// MSSQL supports RIGHT and FULL OUTER but not USING syntax; LATERAL is exposed as CROSS APPLY.
const MSSQL_JOINS: JoinCapabilities = JoinCapabilities {
    right_join: true,
    full_outer_join: true,
    using_syntax: false,
    lateral_join: true,
};

/// SQLite: RIGHT and FULL OUTER were added in version 3.39.0 (2022-07-21).
/// Conservatively gate them until minimum version requirements are confirmed.
/// SQLite does not support LATERAL joins.
const SQLITE_JOINS: JoinCapabilities = JoinCapabilities {
    right_join: false,
    full_outer_join: false,
    using_syntax: true,
    lateral_join: false,
};

/// ClickHouse: no LATERAL join support.
const CLICKHOUSE_JOINS: JoinCapabilities = JoinCapabilities {
    right_join: true,
    full_outer_join: true,
    using_syntax: true,
    lateral_join: false,
};

/// Unknown / generic dialect: only guarantee INNER, LEFT, CROSS, and no USING.
const FALLBACK_JOINS: JoinCapabilities = JoinCapabilities {
    right_join: false,
    full_outer_join: false,
    using_syntax: false,
    lateral_join: false,
};

const FUNCTION_REGISTRY: &[FunctionRegistryEntry] = &[
    FunctionRegistryEntry {
        canonical_name: "COUNT",
        capability: FunctionCapability {
            family: FunctionFamily::Aggregate,
            window: true,
        },
    },
    FunctionRegistryEntry {
        canonical_name: "SUM",
        capability: FunctionCapability {
            family: FunctionFamily::Aggregate,
            window: true,
        },
    },
    FunctionRegistryEntry {
        canonical_name: "AVG",
        capability: FunctionCapability {
            family: FunctionFamily::Aggregate,
            window: true,
        },
    },
    FunctionRegistryEntry {
        canonical_name: "MIN",
        capability: FunctionCapability {
            family: FunctionFamily::Aggregate,
            window: true,
        },
    },
    FunctionRegistryEntry {
        canonical_name: "MAX",
        capability: FunctionCapability {
            family: FunctionFamily::Aggregate,
            window: true,
        },
    },
    FunctionRegistryEntry {
        canonical_name: "COALESCE",
        capability: FunctionCapability {
            family: FunctionFamily::Scalar,
            window: false,
        },
    },
    FunctionRegistryEntry {
        canonical_name: "LOWER",
        capability: FunctionCapability {
            family: FunctionFamily::Scalar,
            window: false,
        },
    },
    FunctionRegistryEntry {
        canonical_name: "UPPER",
        capability: FunctionCapability {
            family: FunctionFamily::Scalar,
            window: false,
        },
    },
    FunctionRegistryEntry {
        canonical_name: "LAG",
        capability: FunctionCapability {
            family: FunctionFamily::Window,
            window: true,
        },
    },
    FunctionRegistryEntry {
        canonical_name: "LEAD",
        capability: FunctionCapability {
            family: FunctionFamily::Window,
            window: true,
        },
    },
    FunctionRegistryEntry {
        canonical_name: "ROW_NUMBER",
        capability: FunctionCapability {
            family: FunctionFamily::Window,
            window: true,
        },
    },
    FunctionRegistryEntry {
        canonical_name: "RANK",
        capability: FunctionCapability {
            family: FunctionFamily::Window,
            window: true,
        },
    },
    FunctionRegistryEntry {
        canonical_name: "DENSE_RANK",
        capability: FunctionCapability {
            family: FunctionFamily::Window,
            window: true,
        },
    },
];

const COMPARISON_OPERATOR_REGISTRY: &[OperatorRegistryEntry] = &[
    OperatorRegistryEntry {
        canonical_operator: "=",
        capability: OperatorCapability {
            family: OperatorFamily::Comparison,
        },
    },
    OperatorRegistryEntry {
        canonical_operator: "!=",
        capability: OperatorCapability {
            family: OperatorFamily::Comparison,
        },
    },
    OperatorRegistryEntry {
        canonical_operator: "<>",
        capability: OperatorCapability {
            family: OperatorFamily::Comparison,
        },
    },
    OperatorRegistryEntry {
        canonical_operator: ">",
        capability: OperatorCapability {
            family: OperatorFamily::Comparison,
        },
    },
    OperatorRegistryEntry {
        canonical_operator: ">=",
        capability: OperatorCapability {
            family: OperatorFamily::Comparison,
        },
    },
    OperatorRegistryEntry {
        canonical_operator: "<",
        capability: OperatorCapability {
            family: OperatorFamily::Comparison,
        },
    },
    OperatorRegistryEntry {
        canonical_operator: "<=",
        capability: OperatorCapability {
            family: OperatorFamily::Comparison,
        },
    },
];

const ARITHMETIC_OPERATOR_REGISTRY: &[OperatorRegistryEntry] = &[
    OperatorRegistryEntry {
        canonical_operator: "+",
        capability: OperatorCapability {
            family: OperatorFamily::Arithmetic,
        },
    },
    OperatorRegistryEntry {
        canonical_operator: "-",
        capability: OperatorCapability {
            family: OperatorFamily::Arithmetic,
        },
    },
    OperatorRegistryEntry {
        canonical_operator: "*",
        capability: OperatorCapability {
            family: OperatorFamily::Arithmetic,
        },
    },
    OperatorRegistryEntry {
        canonical_operator: "/",
        capability: OperatorCapability {
            family: OperatorFamily::Arithmetic,
        },
    },
];

pub fn normalize_function_name(name: &str) -> String {
    name.trim().to_ascii_uppercase()
}

pub fn normalize_operator(operator: &str) -> String {
    operator.trim().to_string()
}

pub fn function_capability_for(dialect: &Dialect, name: &str) -> Option<FunctionCapability> {
    if !dialect.is_sql() {
        return None;
    }

    let normalized = normalize_function_name(name);
    FUNCTION_REGISTRY
        .iter()
        .find_map(|entry| (entry.canonical_name == normalized.as_str()).then_some(entry.capability))
}

pub fn supports_function(dialect: &Dialect, name: &str) -> bool {
    function_capability_for(dialect, name).is_some()
}

pub fn operator_capability_for(
    dialect: &Dialect,
    kind: OperatorKind,
    operator: &str,
) -> Option<OperatorCapability> {
    if !dialect.is_sql() {
        return None;
    }

    let normalized = normalize_operator(operator);
    let registry = match kind {
        OperatorKind::Comparison => COMPARISON_OPERATOR_REGISTRY,
        OperatorKind::Arithmetic => ARITHMETIC_OPERATOR_REGISTRY,
    };

    registry.iter().find_map(|entry| {
        (entry.canonical_operator == normalized.as_str()).then_some(entry.capability)
    })
}

pub fn supports_operator(dialect: &Dialect, kind: OperatorKind, operator: &str) -> bool {
    operator_capability_for(dialect, kind, operator).is_some()
}

pub fn capabilities_for_dialect(dialect: &Dialect) -> DialectCapabilities {
    match dialect {
        Dialect::Postgres | Dialect::DuckDb => DialectCapabilities {
            placeholder_style: PlaceholderStyle::DollarNumbered,
            returning: true,
            window_functions: true,
            null_ordering: true,
            distinct_on: true,
            supports_cte: true,
            ilike: true,
            insert_conflict: InsertConflictStyle::OnConflict,
            pagination: LIMIT_OFFSET_PAGINATION,
            lock_strengths: if matches!(dialect, Dialect::Postgres) {
                POSTGRES_LOCK_STRENGTHS
            } else {
                NO_LOCK_STRENGTHS
            },
            lock_modifiers: if matches!(dialect, Dialect::Postgres) {
                POSTGRES_LOCK_MODIFIERS
            } else {
                NO_LOCK_MODIFIERS
            },
            joins: FULL_JOINS,
            recursive_ctes: RECURSIVE_CTES_WITH_RECURSIVE,
        },
        Dialect::Sqlite => DialectCapabilities {
            placeholder_style: PlaceholderStyle::QuestionMark,
            returning: true,
            window_functions: true,
            null_ordering: true,
            distinct_on: false,
            supports_cte: true,
            ilike: false,
            insert_conflict: InsertConflictStyle::OnConflict,
            pagination: LIMIT_OFFSET_PAGINATION,
            lock_strengths: NO_LOCK_STRENGTHS,
            lock_modifiers: NO_LOCK_MODIFIERS,
            joins: SQLITE_JOINS,
            recursive_ctes: RECURSIVE_CTES_WITH_RECURSIVE,
        },
        Dialect::Mysql => DialectCapabilities {
            placeholder_style: PlaceholderStyle::QuestionMark,
            returning: false,
            window_functions: true,
            null_ordering: false,
            distinct_on: false,
            supports_cte: true,
            ilike: false,
            insert_conflict: InsertConflictStyle::MySql,
            pagination: MYSQL_PAGINATION,
            lock_strengths: NO_LOCK_STRENGTHS,
            lock_modifiers: NO_LOCK_MODIFIERS,
            joins: MYSQL_JOINS,
            recursive_ctes: RECURSIVE_CTES_WITH_RECURSIVE,
        },
        Dialect::Mssql => DialectCapabilities {
            placeholder_style: PlaceholderStyle::AtPNamed,
            returning: false,
            window_functions: true,
            null_ordering: false,
            distinct_on: false,
            supports_cte: true,
            ilike: false,
            insert_conflict: InsertConflictStyle::Unsupported,
            pagination: MSSQL_PAGINATION,
            lock_strengths: NO_LOCK_STRENGTHS,
            lock_modifiers: NO_LOCK_MODIFIERS,
            joins: MSSQL_JOINS,
            recursive_ctes: RECURSIVE_CTES_WITH_ONLY,
        },
        Dialect::Oracle | Dialect::Snowflake => DialectCapabilities {
            placeholder_style: PlaceholderStyle::Unsupported,
            returning: false,
            window_functions: true,
            null_ordering: true,
            distinct_on: false,
            supports_cte: true,
            ilike: matches!(dialect, Dialect::Snowflake),
            insert_conflict: InsertConflictStyle::Unsupported,
            pagination: UNSUPPORTED_PAGINATION,
            lock_strengths: NO_LOCK_STRENGTHS,
            lock_modifiers: NO_LOCK_MODIFIERS,
            joins: FULL_JOINS,
            recursive_ctes: if matches!(dialect, Dialect::Oracle) {
                RECURSIVE_CTES_WITH_ONLY_COLUMNS
            } else {
                RECURSIVE_CTES_WITH_RECURSIVE_COLUMNS
            },
        },
        Dialect::GoogleSql | Dialect::Redshift | Dialect::BigQuery => DialectCapabilities {
            placeholder_style: PlaceholderStyle::Unsupported,
            returning: false,
            window_functions: true,
            null_ordering: false,
            distinct_on: false,
            supports_cte: true,
            ilike: matches!(dialect, Dialect::Redshift),
            insert_conflict: InsertConflictStyle::Unsupported,
            pagination: UNSUPPORTED_PAGINATION,
            lock_strengths: NO_LOCK_STRENGTHS,
            lock_modifiers: NO_LOCK_MODIFIERS,
            joins: FULL_JOINS,
            recursive_ctes: if matches!(dialect, Dialect::Redshift) {
                RECURSIVE_CTES_WITH_RECURSIVE_COLUMNS
            } else {
                RECURSIVE_CTES_WITH_RECURSIVE
            },
        },
        Dialect::ClickHouse => DialectCapabilities {
            placeholder_style: PlaceholderStyle::Unsupported,
            returning: false,
            window_functions: true,
            null_ordering: false,
            distinct_on: false,
            supports_cte: true,
            ilike: false,
            insert_conflict: InsertConflictStyle::Unsupported,
            pagination: UNSUPPORTED_PAGINATION,
            lock_strengths: NO_LOCK_STRENGTHS,
            lock_modifiers: NO_LOCK_MODIFIERS,
            joins: CLICKHOUSE_JOINS,
            recursive_ctes: RECURSIVE_CTES_WITH_RECURSIVE,
        },
        Dialect::Unknown(_) => DialectCapabilities {
            placeholder_style: PlaceholderStyle::Unsupported,
            returning: false,
            window_functions: false,
            null_ordering: false,
            distinct_on: false,
            supports_cte: false,
            ilike: false,
            insert_conflict: InsertConflictStyle::Unsupported,
            pagination: UNSUPPORTED_PAGINATION,
            lock_strengths: NO_LOCK_STRENGTHS,
            lock_modifiers: NO_LOCK_MODIFIERS,
            joins: FALLBACK_JOINS,
            recursive_ctes: RECURSIVE_CTES_UNSUPPORTED,
        },
    }
}

pub fn capabilities_for(dialect: &str) -> DialectCapabilities {
    capabilities_for_dialect(&Dialect::parse(dialect))
}

// ─── Dialect policy ───────────────────────────────────────────────────────────

/// A SQL feature that may be handled differently across dialects.
///
/// Used as the input to [`classify`] to look up the [`DialectPolicy`] that
/// applies for a given dialect/feature combination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialectFeature {
    /// `RETURNING` clause on INSERT / UPDATE / DELETE.
    Returning,
    /// `ILIKE` case-insensitive pattern operator.
    Ilike,
    /// `INTERSECT ALL` compound set operator.
    IntersectAll,
    /// `DISTINCT ON (...)` projection-level deduplication.
    DistinctOn,
    /// `LIMIT x OFFSET y` — may be rewritten to `OFFSET … FETCH NEXT … ROWS ONLY`.
    PaginationLimitOffset,
}

/// How the query renderer should handle a dialect/feature combination.
///
/// The three variants map directly to the policy tiers defined in issue #17:
/// hard errors, per-dialect rendering, and transparent fallback rewrites.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialectPolicy {
    /// The feature has no valid SQL equivalent in this dialect.
    ///
    /// The renderer must surface a `QueryError` and must not emit silent
    /// incorrect SQL.
    HardError,
    /// The feature exists in this dialect but renders differently.
    ///
    /// Per-dialect render logic handles the output; no rewrite is needed.
    DialectRender,
    /// The feature can be semantically preserved via a transparent SQL rewrite.
    ///
    /// The renderer applies the rewrite and records a [`DialectWarning`] so the
    /// caller can opt into strict mode or inspect what was changed.
    FallbackRewrite,
}

/// Returns the handling policy for a given `(dialect, feature)` pair.
///
/// Use the returned [`DialectPolicy`] to decide whether to emit a hard error,
/// apply dialect-specific rendering, or apply a fallback rewrite with a warning.
#[must_use]
pub fn classify(dialect: &Dialect, feature: DialectFeature) -> DialectPolicy {
    let caps = capabilities_for_dialect(dialect);
    match feature {
        DialectFeature::Returning => {
            if caps.returning {
                DialectPolicy::DialectRender
            } else {
                DialectPolicy::HardError
            }
        }
        DialectFeature::Ilike => {
            if caps.ilike {
                DialectPolicy::DialectRender
            } else {
                DialectPolicy::FallbackRewrite
            }
        }
        DialectFeature::IntersectAll => {
            // MySQL does not support INTERSECT ALL; no semantic rewrite exists.
            match dialect {
                Dialect::Mysql => DialectPolicy::HardError,
                _ => DialectPolicy::DialectRender,
            }
        }
        DialectFeature::DistinctOn => {
            if caps.distinct_on {
                DialectPolicy::DialectRender
            } else {
                DialectPolicy::HardError
            }
        }
        DialectFeature::PaginationLimitOffset => match caps.pagination.family {
            PaginationSyntaxFamily::LimitOffset => DialectPolicy::DialectRender,
            PaginationSyntaxFamily::TopOffsetFetch => DialectPolicy::FallbackRewrite,
            PaginationSyntaxFamily::Unsupported => DialectPolicy::HardError,
        },
    }
}

/// Contextual information attached to a fallback SQL rewrite performed during rendering.
///
/// Callers can inspect `warnings` on [`RenderResult`](crate::types::RenderResult) to
/// discover what rewrites were applied, or enable `dialect_strict` to promote them to
/// errors instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialectWarning {
    /// Short identifier of the SQL feature that was rewritten (e.g. `"ILIKE"`).
    pub feature: String,
    /// The canonical name of the dialect that triggered the rewrite.
    pub dialect: String,
    /// Human-readable description of the rewrite that was applied.
    pub message: String,
}

fn pagination_count_fragment(count: u32) -> SqlQuery {
    SqlQuery {
        text: "?".to_string(),
        raw: count.to_string(),
        values: vec![Primitive::Number(serde_json::Number::from(count))],
    }
}

fn pagination_single_fragment(prefix: &str, count: u32, suffix: &str) -> SqlQuery {
    let count_fragment = pagination_count_fragment(count);
    let mut text = String::with_capacity(prefix.len() + count_fragment.text.len() + suffix.len());
    let mut raw = String::with_capacity(prefix.len() + count_fragment.raw.len() + suffix.len());

    text.push_str(prefix);
    text.push_str(&count_fragment.text);
    text.push_str(suffix);

    raw.push_str(prefix);
    raw.push_str(&count_fragment.raw);
    raw.push_str(suffix);

    SqlQuery {
        text,
        raw,
        values: count_fragment.values,
    }
}

fn pagination_pair_fragment(
    prefix: &str,
    first: u32,
    middle: &str,
    second: u32,
    suffix: &str,
) -> SqlQuery {
    let first_fragment = pagination_count_fragment(first);
    let second_fragment = pagination_count_fragment(second);
    let mut text = String::with_capacity(
        prefix.len()
            + first_fragment.text.len()
            + middle.len()
            + second_fragment.text.len()
            + suffix.len(),
    );
    let mut raw = String::with_capacity(
        prefix.len()
            + first_fragment.raw.len()
            + middle.len()
            + second_fragment.raw.len()
            + suffix.len(),
    );
    let mut values = Vec::with_capacity(first_fragment.values.len() + second_fragment.values.len());

    text.push_str(prefix);
    text.push_str(&first_fragment.text);
    text.push_str(middle);
    text.push_str(&second_fragment.text);
    text.push_str(suffix);

    raw.push_str(prefix);
    raw.push_str(&first_fragment.raw);
    raw.push_str(middle);
    raw.push_str(&second_fragment.raw);
    raw.push_str(suffix);

    values.extend(first_fragment.values);
    values.extend(second_fragment.values);

    SqlQuery { text, raw, values }
}

pub fn render_pagination(
    dialect: &Dialect,
    pagination: &PaginationState,
) -> Result<RenderedPagination, String> {
    let capabilities = capabilities_for_dialect(dialect).pagination;

    match (pagination.limit, pagination.offset) {
        (None, None) => Ok(RenderedPagination::default()),
        (None, Some(offset)) => match capabilities.family {
            PaginationSyntaxFamily::LimitOffset => Ok(RenderedPagination {
                select_prefix: None,
                trailing_clause: Some(pagination_single_fragment("OFFSET ", offset, "")),
            }),
            PaginationSyntaxFamily::TopOffsetFetch => Ok(RenderedPagination {
                select_prefix: None,
                trailing_clause: Some(pagination_single_fragment("OFFSET ", offset, " ROWS")),
            }),
            PaginationSyntaxFamily::Unsupported => Ok(RenderedPagination::default()),
        },
        (Some(limit), None) => match capabilities.family {
            PaginationSyntaxFamily::LimitOffset => Ok(RenderedPagination {
                select_prefix: None,
                trailing_clause: Some(pagination_single_fragment("LIMIT ", limit, "")),
            }),
            PaginationSyntaxFamily::TopOffsetFetch => Ok(RenderedPagination {
                select_prefix: Some(pagination_single_fragment("TOP (", limit, ")")),
                trailing_clause: None,
            }),
            PaginationSyntaxFamily::Unsupported => Ok(RenderedPagination::default()),
        },
        (Some(limit), Some(offset)) => match capabilities.family {
            PaginationSyntaxFamily::LimitOffset => Ok(RenderedPagination {
                select_prefix: None,
                trailing_clause: Some(pagination_pair_fragment(
                    "LIMIT ", limit, " OFFSET ", offset, "",
                )),
            }),
            PaginationSyntaxFamily::TopOffsetFetch => Ok(RenderedPagination {
                select_prefix: None,
                trailing_clause: Some(pagination_pair_fragment(
                    "OFFSET ",
                    offset,
                    " ROWS FETCH NEXT ",
                    limit,
                    " ROWS ONLY",
                )),
            }),
            PaginationSyntaxFamily::Unsupported => Ok(RenderedPagination::default()),
        },
    }
}

pub fn validate_query_parts(parts: &QueryParts) -> Result<(), String> {
    let dialect = parts.dialect.clone();
    let capabilities = capabilities_for_dialect(&dialect);

    if let Some(DistinctMode::On(expressions)) = &parts.distinct {
        if expressions.is_empty() {
            return Err("DISTINCT ON requires at least one expression.".to_string());
        }

        if !capabilities.distinct_on {
            return Err(format!(
                "Dialect \"{}\" does not support DISTINCT ON in this builder.",
                dialect
            ));
        }
    }

    for cte in parts.ctes.iter().filter(|cte| cte.recursive) {
        match capabilities.recursive_ctes.style {
            RecursiveCteStyle::Unsupported => {
                return Err(format!(
                    "Dialect \"{}\" does not support recursive CTEs in this builder.",
                    dialect
                ));
            }
            RecursiveCteStyle::WithRecursiveKeyword | RecursiveCteStyle::WithOnly => {
                if capabilities.recursive_ctes.column_aliases_required && cte.columns.is_empty() {
                    return Err(format!(
                        "Dialect \"{}\" requires column aliases for recursive CTE \"{}\".",
                        dialect, cte.name
                    ));
                }
            }
        }
    }

    if let Some(target) = &parts.insert.conflict.target {
        match target {
            InsertConflictTarget::Columns(columns) => {
                if columns.is_empty() {
                    return Err("Insert conflict columns cannot be empty.".to_string());
                }
                if columns.iter().any(|column| column.is_empty()) {
                    return Err("Insert conflict columns cannot contain empty names.".to_string());
                }
            }
            InsertConflictTarget::Constraint(constraint) => {
                if constraint.is_empty() {
                    return Err("Insert conflict constraint name cannot be empty.".to_string());
                }
            }
        }
    }

    if parts.insert.conflict.target.is_some() && parts.insert.conflict.action.is_none() {
        return Err("Insert conflict target requires an action.".to_string());
    }

    if let Some(action) = &parts.insert.conflict.action {
        match action {
            InsertConflictAction::DoNothing => match capabilities.insert_conflict {
                InsertConflictStyle::Unsupported => {
                    return Err(format!(
                        "Dialect \"{}\" does not support DO NOTHING insert conflict handling.",
                        dialect
                    ));
                }
                InsertConflictStyle::MySql => {
                    if parts.insert.conflict.target.is_some() {
                        return Err(format!(
                            "Dialect \"{}\" does not support explicit conflict targets for DO NOTHING inserts.",
                            dialect
                        ));
                    }
                }
                InsertConflictStyle::OnConflict => {}
            },
            InsertConflictAction::DoUpdate {
                assignments,
                predicate,
            } => {
                if assignments.is_empty() {
                    return Err(
                        "Insert conflict update requires at least one assignment.".to_string()
                    );
                }

                match capabilities.insert_conflict {
                    InsertConflictStyle::Unsupported => {
                        return Err(format!(
                            "Dialect \"{}\" does not support DO UPDATE insert conflict handling.",
                            dialect
                        ));
                    }
                    InsertConflictStyle::MySql => {
                        if parts.insert.conflict.target.is_some() {
                            return Err(format!(
                                "Dialect \"{}\" does not support explicit conflict targets for DO UPDATE inserts.",
                                dialect
                            ));
                        }
                        if predicate.is_some() {
                            return Err(format!(
                                "Dialect \"{}\" does not support conflict WHERE for DO UPDATE inserts.",
                                dialect
                            ));
                        }
                    }
                    InsertConflictStyle::OnConflict => {
                        if parts.insert.conflict.target.is_none() {
                            return Err(
                                "Insert conflict update requires a conflict target.".to_string()
                            );
                        }
                    }
                }
            }
        }
    }

    if parts.returning.is_some() {
        if !matches!(
            parts.statement,
            Some(StatementKind::Insert | StatementKind::Update | StatementKind::Delete)
        ) {
            return Err(
                "RETURNING is only supported for INSERT, UPDATE, and DELETE builders.".to_string(),
            );
        }

        if !capabilities.returning {
            return Err(format!(
                "Dialect \"{}\" does not support RETURNING in this builder.",
                dialect
            ));
        }
    }

    if let Some(lock) = &parts.lock {
        if !matches!(parts.statement, None | Some(StatementKind::Select)) {
            return Err("SELECT lock clauses are only supported for SELECT builders.".to_string());
        }

        if !capabilities.lock_strengths.contains(&lock.strength) {
            return Err(format!(
                "Dialect \"{}\" does not support {} in this builder.",
                dialect,
                lock.strength.as_sql()
            ));
        }

        if let Some(modifier) = lock.modifier {
            if !capabilities.lock_modifiers.contains(&modifier) {
                return Err(format!(
                    "Dialect \"{}\" does not support {} with SELECT lock clauses in this builder.",
                    dialect,
                    modifier.as_sql()
                ));
            }
        }
    }

    let has_limit = parts.pagination.limit.is_some();
    let has_offset = parts.pagination.offset.is_some();

    if has_limit || has_offset {
        let pagination = capabilities.pagination;

        if pagination.family == PaginationSyntaxFamily::Unsupported {
            return Err(format!(
                "Dialect \"{}\" does not support pagination in this builder.",
                dialect
            ));
        }

        if has_offset && !has_limit && !pagination.offset_only_allowed {
            return Err(format!(
                "Dialect \"{}\" does not support OFFSET without LIMIT in this builder.",
                dialect
            ));
        }

        if has_offset && pagination.offset_requires_order_by && parts.order_by.is_empty() {
            return Err(format!(
                "Dialect \"{}\" requires ORDER BY when using OFFSET/FETCH pagination in this builder.",
                dialect
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "dialect.test.rs"]
mod dialect_test;
