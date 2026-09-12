#![cfg_attr(not(test), allow(dead_code))]

use super::registry::registry_get_cloned;
use super::render::normalize_pending_join;
use super::{
    AssignmentValue, CompoundOperator, DistinctMode, InsertConflict, InsertConflictAction,
    InsertConflictTarget, InsertState, LockModifier, LockStrength, LogicalOp, QueryParts,
    SelectLockClause, StatementKind, UpdateAssignment, UpdateState, WhereEntry,
};
use crate::dialect::Dialect;
use crate::expr_node::eval_expr_for_dialect_ctx;
use crate::types::{SqlIdentifier, SqlQuery, SqlValue};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CanonicalQueryIr {
    dialect: String,
    statement: Option<String>,
    ctes: Vec<CanonicalCte>,
    distinct: Option<CanonicalDistinctMode>,
    select: Option<CanonicalSqlQuery>,
    selected_columns: Vec<String>,
    from: Option<CanonicalSqlQuery>,
    joins: Vec<CanonicalSqlQuery>,
    where_clauses: Vec<CanonicalClause>,
    group_by: Vec<SqlIdentifier>,
    having_clauses: Vec<CanonicalClause>,
    order_by: Vec<CanonicalSqlQuery>,
    pagination: CanonicalPaginationState,
    lock: Option<CanonicalSelectLockClause>,
    returning: Option<CanonicalSqlQuery>,
    compounds: Vec<CanonicalCompoundPart>,
    insert: CanonicalInsertState,
    update: CanonicalUpdateState,
    delete_from_table: Option<String>,
    dialect_strict: bool,
}

pub(crate) type CanonicalQuery = CanonicalQueryIr;

#[derive(Debug, Clone, Serialize)]
struct CanonicalSqlQuery {
    text: String,
    value_count: usize,
}

#[derive(Debug, Clone, Serialize)]
struct CanonicalCte {
    name: String,
    recursive: bool,
    columns: Vec<String>,
    query: CanonicalSqlQuery,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum CanonicalDistinctMode {
    Distinct,
    On { expressions: Vec<CanonicalSqlQuery> },
}

#[derive(Debug, Clone, Serialize)]
struct CanonicalClause {
    op: String,
    pred: CanonicalSqlQuery,
}

#[derive(Debug, Clone, Serialize)]
struct CanonicalPaginationState {
    limit: Option<u32>,
    offset: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
struct CanonicalSelectLockClause {
    strength: String,
    modifier: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct CanonicalCompoundPart {
    operator: String,
    query: CanonicalSqlQuery,
}

#[derive(Debug, Clone, Serialize)]
struct CanonicalInsertState {
    into: Option<String>,
    column_names: Vec<String>,
    rows: Vec<Vec<CanonicalSqlValue>>,
    select: Option<CanonicalSqlQuery>,
    conflict: CanonicalInsertConflict,
}

#[derive(Debug, Clone, Serialize)]
struct CanonicalInsertConflict {
    target: Option<CanonicalInsertConflictTarget>,
    action: Option<CanonicalInsertConflictAction>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum CanonicalInsertConflictTarget {
    Columns { columns: Vec<String> },
    Constraint { name: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum CanonicalInsertConflictAction {
    DoNothing,
    DoUpdate {
        assignments: Vec<CanonicalUpdateAssignment>,
        predicate: Option<CanonicalSqlQuery>,
    },
}

#[derive(Debug, Clone, Serialize)]
struct CanonicalUpdateState {
    table: Option<String>,
    assignments: Vec<CanonicalUpdateAssignment>,
}

#[derive(Debug, Clone, Serialize)]
struct CanonicalUpdateAssignment {
    column: String,
    value: CanonicalSqlValue,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
enum CanonicalPrimitiveKind {
    Null,
    Bool,
    Number,
    String,
    BigInt,
    Date,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum CanonicalSqlValue {
    Array {
        len: usize,
        element_kinds: Vec<CanonicalPrimitiveKind>,
    },
    Query(CanonicalSqlQuery),
    Identifier {
        parts: Vec<String>,
    },
    Raw {
        text: String,
    },
    Primitive {
        primitive: CanonicalPrimitiveKind,
    },
}

fn canonical_sql_query(query: &SqlQuery) -> CanonicalSqlQuery {
    CanonicalSqlQuery {
        text: query.text.clone(),
        value_count: query.values.len(),
    }
}

fn canonical_primitive_kind(value: &crate::types::Primitive) -> CanonicalPrimitiveKind {
    match value {
        crate::types::Primitive::Null => CanonicalPrimitiveKind::Null,
        crate::types::Primitive::Bool(_) => CanonicalPrimitiveKind::Bool,
        crate::types::Primitive::Number(_) => CanonicalPrimitiveKind::Number,
        crate::types::Primitive::String(_) => CanonicalPrimitiveKind::String,
        crate::types::Primitive::BigInt(_) => CanonicalPrimitiveKind::BigInt,
        crate::types::Primitive::Date(_) => CanonicalPrimitiveKind::Date,
    }
}

fn canonical_sql_value(value: &SqlValue) -> CanonicalSqlValue {
    match value {
        SqlValue::Array(values) => CanonicalSqlValue::Array {
            len: values.len(),
            element_kinds: values.iter().map(canonical_primitive_kind).collect(),
        },
        SqlValue::Query(query) => CanonicalSqlValue::Query(canonical_sql_query(query)),
        SqlValue::Identifier(identifier) => CanonicalSqlValue::Identifier {
            parts: identifier.parts.clone(),
        },
        SqlValue::Raw(raw) => CanonicalSqlValue::Raw {
            text: raw.text.clone(),
        },
        SqlValue::Primitive(primitive) => CanonicalSqlValue::Primitive {
            primitive: canonical_primitive_kind(primitive),
        },
    }
}

fn statement_to_canonical(statement: Option<&StatementKind>) -> Option<String> {
    statement.map(|value| match value {
        StatementKind::Select => "select".to_string(),
        StatementKind::Insert => "insert".to_string(),
        StatementKind::Update => "update".to_string(),
        StatementKind::Delete => "delete".to_string(),
    })
}

fn distinct_to_canonical(distinct: Option<&DistinctMode>) -> Option<CanonicalDistinctMode> {
    distinct.map(|value| match value {
        DistinctMode::Distinct => CanonicalDistinctMode::Distinct,
        DistinctMode::On(expressions) => CanonicalDistinctMode::On {
            expressions: expressions.iter().map(canonical_sql_query).collect(),
        },
    })
}

fn op_to_canonical(op: LogicalOp) -> String {
    match op {
        LogicalOp::And => "and".to_string(),
        LogicalOp::Or => "or".to_string(),
    }
}

fn clauses_to_canonical(clauses: &[WhereEntry]) -> Vec<CanonicalClause> {
    clauses
        .iter()
        .map(|entry| CanonicalClause {
            op: op_to_canonical(entry.op),
            pred: canonical_sql_query(&entry.pred),
        })
        .collect()
}

fn lock_strength_to_canonical(strength: LockStrength) -> String {
    match strength {
        LockStrength::Update => "update".to_string(),
        LockStrength::Share => "share".to_string(),
    }
}

fn lock_modifier_to_canonical(modifier: LockModifier) -> String {
    match modifier {
        LockModifier::NoWait => "noWait".to_string(),
        LockModifier::SkipLocked => "skipLocked".to_string(),
    }
}

fn lock_to_canonical(lock: Option<SelectLockClause>) -> Option<CanonicalSelectLockClause> {
    lock.map(|value| CanonicalSelectLockClause {
        strength: lock_strength_to_canonical(value.strength),
        modifier: value.modifier.map(lock_modifier_to_canonical),
    })
}

fn compounds_to_canonical(compounds: &[super::CompoundPart]) -> Vec<CanonicalCompoundPart> {
    compounds
        .iter()
        .map(|part| CanonicalCompoundPart {
            operator: match part.operator {
                CompoundOperator::Union => "union".to_string(),
                CompoundOperator::UnionAll => "unionAll".to_string(),
                CompoundOperator::Intersect => "intersect".to_string(),
                CompoundOperator::Except => "except".to_string(),
            },
            query: canonical_sql_query(part.query.as_ref()),
        })
        .collect()
}

fn assignment_value_to_sql(
    value: &AssignmentValue,
    dialect: &Dialect,
) -> Result<CanonicalSqlValue, String> {
    match value {
        AssignmentValue::Sql(sql_value) => Ok(canonical_sql_value(sql_value)),
        AssignmentValue::Expr(expr) => {
            let mut warnings = None;
            eval_expr_for_dialect_ctx(expr.clone(), dialect, &mut warnings)
                .map(|query| CanonicalSqlValue::Query(canonical_sql_query(&query)))
        }
    }
}

fn assignments_to_canonical(
    assignments: &[UpdateAssignment],
    dialect: &Dialect,
) -> Result<Vec<CanonicalUpdateAssignment>, String> {
    let mut canonical = assignments
        .iter()
        .map(|assignment| {
            Ok(CanonicalUpdateAssignment {
                column: assignment.column.clone(),
                value: assignment_value_to_sql(&assignment.value, dialect)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    canonical.sort_by(|left, right| left.column.cmp(&right.column));
    Ok(canonical)
}

fn insert_conflict_to_canonical(
    conflict: &InsertConflict,
    dialect: &Dialect,
) -> Result<CanonicalInsertConflict, String> {
    let target = conflict.target.as_ref().map(|value| match value {
        InsertConflictTarget::Columns(columns) => CanonicalInsertConflictTarget::Columns {
            columns: columns.clone(),
        },
        InsertConflictTarget::Constraint(name) => {
            CanonicalInsertConflictTarget::Constraint { name: name.clone() }
        }
    });
    let action = match conflict.action.as_ref() {
        None => None,
        Some(InsertConflictAction::DoNothing) => Some(CanonicalInsertConflictAction::DoNothing),
        Some(InsertConflictAction::DoUpdate {
            assignments,
            predicate,
        }) => Some(CanonicalInsertConflictAction::DoUpdate {
            assignments: assignments_to_canonical(assignments, dialect)?,
            predicate: predicate.as_ref().map(canonical_sql_query),
        }),
    };

    Ok(CanonicalInsertConflict { target, action })
}

fn insert_to_canonical(
    insert: &InsertState,
    dialect: &Dialect,
) -> Result<CanonicalInsertState, String> {
    Ok(CanonicalInsertState {
        into: insert.into.clone(),
        column_names: insert.column_names.clone(),
        rows: insert
            .rows
            .iter()
            .map(|row| row.iter().map(canonical_sql_value).collect())
            .collect(),
        select: insert.select.as_ref().map(canonical_sql_query),
        conflict: insert_conflict_to_canonical(&insert.conflict, dialect)?,
    })
}

fn update_to_canonical(
    update: &UpdateState,
    dialect: &Dialect,
) -> Result<CanonicalUpdateState, String> {
    Ok(CanonicalUpdateState {
        table: update.table.clone(),
        assignments: assignments_to_canonical(&update.assignments, dialect)?,
    })
}

fn normalize_parts_for_canonical(parts: &QueryParts) -> Result<QueryParts, String> {
    if parts.pending_join.is_some() {
        normalize_pending_join(parts.clone())
    } else {
        Ok(parts.clone())
    }
}

/// Build a deterministic canonical representation of query state.
///
/// This projection is intentionally independent from registry handle identity and
/// transient runtime fields such as render caches.
pub(crate) fn canonical_ir_for_parts(parts: &QueryParts) -> Result<CanonicalQueryIr, String> {
    let normalized = normalize_parts_for_canonical(parts)?;
    let dialect = normalized.dialect.clone();

    Ok(CanonicalQueryIr {
        dialect: dialect.as_str().to_string(),
        statement: statement_to_canonical(normalized.statement.as_ref()),
        ctes: normalized
            .ctes
            .iter()
            .map(|cte| CanonicalCte {
                name: cte.name.clone(),
                recursive: cte.recursive,
                columns: cte.columns.clone(),
                query: canonical_sql_query(&cte.query),
            })
            .collect(),
        distinct: distinct_to_canonical(normalized.distinct.as_ref()),
        select: normalized.select.as_deref().map(canonical_sql_query),
        selected_columns: normalized.selected_columns.clone(),
        from: normalized.from.as_deref().map(canonical_sql_query),
        joins: normalized.joins.iter().map(canonical_sql_query).collect(),
        where_clauses: clauses_to_canonical(&normalized.where_clauses),
        group_by: normalized.group_by.iter().cloned().collect(),
        having_clauses: clauses_to_canonical(&normalized.having_clauses),
        order_by: normalized.order_by.iter().map(canonical_sql_query).collect(),
        pagination: CanonicalPaginationState {
            limit: normalized.pagination.limit,
            offset: normalized.pagination.offset,
        },
        lock: lock_to_canonical(normalized.lock),
        returning: normalized.returning.as_deref().map(canonical_sql_query),
        compounds: compounds_to_canonical(&normalized.compounds),
        insert: insert_to_canonical(&normalized.insert, &dialect)?,
        update: update_to_canonical(&normalized.update, &dialect)?,
        delete_from_table: normalized.delete.from_table.clone(),
        dialect_strict: normalized.dialect_strict,
    })
}

/// Serialize canonical query state as stable JSON.
pub(crate) fn canonical_ir_json_for_parts(parts: &QueryParts) -> Result<String, String> {
    let ir = canonical_ir_for_parts(parts)?;
    serde_json::to_string(&ir).map_err(|error| error.to_string())
}

/// Compute a deterministic in-memory fingerprint for canonical query shape.
///
/// # Errors
///
/// Returns an error if the canonical query cannot be serialized to JSON.
pub(crate) fn fingerprint(query: &CanonicalQuery) -> Result<u64, String> {
    let bytes = serde_json::to_vec(query).map_err(|e| e.to_string())?;
    let hash = blake3::hash(&bytes);
    let mut prefix = [0_u8; 8];
    prefix.copy_from_slice(&hash.as_bytes()[..8]);
    Ok(u64::from_le_bytes(prefix))
}

pub(crate) fn canonical_fingerprint_for_parts(parts: &QueryParts) -> Result<u64, String> {
    let ir = canonical_ir_for_parts(parts)?;
    fingerprint(&ir)
}

/// Hash canonical query state with a deterministic BLAKE3 hex digest.
pub(crate) fn canonical_ir_hash_for_parts(parts: &QueryParts) -> Result<String, String> {
    let canonical_json = canonical_ir_json_for_parts(parts)?;
    Ok(blake3::hash(canonical_json.as_bytes()).to_hex().to_string())
}

/// Serialize canonical query state from a builder handle.
///
/// This is primarily useful for tests and benchmarks that need to isolate
/// canonicalization cost from SQL rendering.
///
/// # Errors
///
/// Returns an error string when `handle` does not exist in the registry or the
/// underlying query state cannot be canonicalized.
pub fn builder_canonical_ir_json(handle: &str) -> Result<String, String> {
    let parts = registry_get_cloned(handle)?;
    canonical_ir_json_for_parts(&parts)
}

/// Compute a deterministic canonical hash from a builder handle.
pub fn builder_canonical_ir_hash(handle: &str) -> Result<String, String> {
    let parts = registry_get_cloned(handle)?;
    canonical_ir_hash_for_parts(&parts)
}
