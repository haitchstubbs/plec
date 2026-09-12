use super::render::{build_query, get_or_build_query_arc, normalize_pending_join};
use super::{
    registry::{
        registry_clone, registry_drop, registry_get_cloned, registry_get_dialect, registry_insert,
        registry_new,
    },
    AssignmentValue, CompoundOperator, CompoundPart, CteDefinition, DeleteState, DistinctMode,
    InsertConflictAction, InsertConflictTarget, InsertState, JoinType, LockModifier, LockStrength,
    LogicalOp, PendingJoinState, QueryParts, SelectLockClause, StatementKind, UpdateAssignment,
    UpdateState, WhereEntry,
};
use crate::dialect::{Dialect, DialectWarning};
use crate::expr_node::{eval_expr_for_dialect, eval_expr_for_dialect_ctx, ExprNode, WarningsCtx};
use crate::sql;
use crate::types::{SqlIdentifier, SqlQuery, SqlValue};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

// ─── Predicate ────────────────────────────────────────────────────────────────

/// A predicate value in a WHERE / HAVING clause.  Either a deferred ExprNode
/// (built in TypeScript with zero FFI) or a pre-rendered SqlQuery (from the
/// `sql` template tag or standalone `Expressions.*` exports).
///
/// Untagged deserialization works because ExprNode requires a `"type"` JSON
/// field (via `#[serde(tag = "type")]`) while SqlQuery does not.  ExprNode is
/// tried first; if the `"type"` field is absent the fallback to SqlQuery fires.
#[derive(Deserialize)]
#[serde(untagged)]
enum Predicate {
    Node(ExprNode),
    Query(SqlQuery),
}

impl Predicate {
    fn eval(
        self,
        dialect: &Dialect,
        warnings: &mut Vec<DialectWarning>,
    ) -> Result<SqlQuery, String> {
        match self {
            Predicate::Node(node) => {
                let mut ctx = Some(WarningsCtx {
                    warnings,
                    strict: false,
                });
                eval_expr_for_dialect_ctx(node, dialect, &mut ctx)
            }
            Predicate::Query(query) => Ok(query),
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum AssignmentInputValue {
    Node(ExprNode),
    Sql(SqlValue),
}

impl AssignmentInputValue {
    fn into_assignment_value(self) -> AssignmentValue {
        match self {
            AssignmentInputValue::Node(node) => AssignmentValue::Expr(node),
            AssignmentInputValue::Sql(value) => AssignmentValue::Sql(value),
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum DistinctOnInput {
    Node(ExprNode),
    Query(SqlQuery),
}

impl DistinctOnInput {
    fn eval(self, dialect: &Dialect) -> Result<SqlQuery, String> {
        match self {
            DistinctOnInput::Node(node) => eval_expr_for_dialect(node, dialect),
            DistinctOnInput::Query(query) => Ok(query),
        }
    }
}

#[derive(Deserialize)]
struct InsertRowsInput {
    #[serde(rename = "columnNames")]
    column_names: Vec<String>,
    rows: Vec<Vec<SqlValue>>,
}

// ─── SelectAliased entry ─────────────────────────────────────────────────────

/// One entry in a `selectAliased` batch op.
/// When `bare` is true the expression is rendered as-is without `AS alias`
/// (used for plain column references in mixed array projections).
#[derive(Deserialize)]
struct SelectAliasedEntry {
    alias: String,
    expr: ExprNode,
    #[serde(default)]
    bare: bool,
}

#[derive(Debug, Clone, Copy)]
enum ProjectionTarget {
    Select,
    Returning,
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn active_dialect(parts: &QueryParts) -> Dialect {
    parts.dialect.clone()
}

/// Append a predicate to a flat WHERE/HAVING clause list.
///
/// The first entry uses op=AND by convention (rendered with no preceding operator),
/// subsequent entries use the supplied `op`.
fn push_clause(clauses: &mut Arc<Vec<WhereEntry>>, op: LogicalOp, pred: SqlQuery) {
    Arc::make_mut(clauses).push(WhereEntry { pred, op });
}

/// Compose JOIN ON predicates with AND or OR (nested binary tree, unlike WHERE).
/// Used only for building join predicates where nesting semantics matter.
fn compose_logical(
    existing: Option<SqlQuery>,
    operator: &str,
    next: SqlQuery,
) -> Result<SqlQuery, String> {
    match existing {
        None => Ok(next),
        Some(ex) => sql::sql(
            vec![
                "(".to_string(),
                format!(") {} (", operator),
                ")".to_string(),
            ],
            vec![SqlValue::Query(ex), SqlValue::Query(next)],
        ),
    }
}

/// Split a dotted column path into identifier parts.
fn split_col(col: &str) -> Vec<String> {
    col.split('.').map(str::to_string).collect()
}

/// Build a quoted identifier from a dotted column path.
fn col_to_identifier(col: &str) -> Result<SqlIdentifier, String> {
    sql::identifier(split_col(col))
}

fn col_to_query(col: &str) -> Result<SqlQuery, String> {
    sql::sql(
        vec!["".to_string(), "".to_string()],
        vec![SqlValue::Identifier(col_to_identifier(col)?)],
    )
}

fn col_to_projection_value(col: &str) -> Result<SqlValue, String> {
    if col == "*" {
        Ok(SqlValue::Raw(sql::raw("*".to_string())))
    } else {
        col_to_identifier(col).map(SqlValue::Identifier)
    }
}

/// Extract the local column name (last part after the last dot).
fn col_to_local(col: &str) -> String {
    col.split('.').next_back().unwrap_or(col).to_string()
}

fn set_distinct_mode(parts: &mut QueryParts, distinct: DistinctMode) {
    parts.distinct = Some(distinct);
}

fn ensure_select_lock_builder(parts: &QueryParts) -> Result<(), String> {
    if matches!(
        parts.statement,
        Some(StatementKind::Insert | StatementKind::Update | StatementKind::Delete)
    ) {
        return Err("SELECT lock clauses are only supported for SELECT builders.".to_string());
    }

    Ok(())
}

fn set_lock_strength(parts: &mut QueryParts, strength: LockStrength) {
    parts.lock = Some(SelectLockClause {
        strength,
        modifier: None,
    });
}

fn set_lock_modifier(parts: &mut QueryParts, modifier: LockModifier) -> Result<(), String> {
    let existing = parts
        .lock
        .ok_or_else(|| "Select lock modifier requires a lock strength first.".to_string())?;
    parts.lock = Some(SelectLockClause {
        strength: existing.strength,
        modifier: Some(modifier),
    });
    Ok(())
}

fn apply_projection(
    parts: &mut QueryParts,
    content: SqlQuery,
    selected_columns: Vec<String>,
    target: ProjectionTarget,
) {
    match target {
        ProjectionTarget::Select => {
            parts.select = Some(Arc::new(content));
            parts.statement = Some(StatementKind::Select);
        }
        ProjectionTarget::Returning => {
            parts.returning = Some(Arc::new(content));
        }
    }
    parts.selected_columns = selected_columns;
}

/// Build a FROM source fragment for simple `FROM "table"` or `FROM "table" "alias"`.
fn from_table_fragment(table: &str, alias: Option<&str>) -> Result<SqlQuery, String> {
    let table_id = col_to_identifier(table)?;
    match alias {
        None => sql::sql(
            vec!["FROM ".to_string(), "".to_string()],
            vec![SqlValue::Identifier(table_id)],
        ),
        Some(a) => {
            let alias_id = sql::identifier(vec![a.to_string()])?;
            sql::sql(
                vec!["FROM ".to_string(), " ".to_string(), "".to_string()],
                vec![
                    SqlValue::Identifier(table_id),
                    SqlValue::Identifier(alias_id),
                ],
            )
        }
    }
}

/// Build a source fragment (no FROM keyword) for a table with optional alias.
fn source_fragment(table: &str, alias: Option<&str>) -> Result<SqlQuery, String> {
    let table_id = col_to_identifier(table)?;
    match alias {
        None => sql::sql(
            vec!["".to_string(), "".to_string()],
            vec![SqlValue::Identifier(table_id)],
        ),
        Some(a) => {
            let alias_id = sql::identifier(vec![a.to_string()])?;
            sql::sql(
                vec!["".to_string(), " ".to_string(), "".to_string()],
                vec![
                    SqlValue::Identifier(table_id),
                    SqlValue::Identifier(alias_id),
                ],
            )
        }
    }
}

/// Build a subquery source fragment: `(query) "alias"`.
fn subquery_source_fragment(query: SqlQuery, alias: &str) -> Result<SqlQuery, String> {
    let alias_id = sql::identifier(vec![alias.to_string()])?;
    sql::sql(
        vec!["(".to_string(), ") ".to_string(), "".to_string()],
        vec![SqlValue::Query(query), SqlValue::Identifier(alias_id)],
    )
}

// ─── Utility ─────────────────────────────────────────────────────────────────

fn apply_single_builder_op(handle: &str, op: BuilderOp) -> Result<String, String> {
    builder_apply_parsed_ops(handle, vec![op])
}

pub fn builder_new(dialect: Option<&str>) -> String {
    registry_new(dialect)
}

pub fn builder_clone(handle: &str) -> Result<String, String> {
    registry_clone(handle)
}

pub fn builder_drop(handle: &str) {
    registry_drop(handle);
}

pub fn builder_clear(handle: &str) -> Result<String, String> {
    let dialect = registry_get_dialect(handle);
    Ok(registry_insert(QueryParts {
        dialect,
        ..Default::default()
    }))
}

/// Enables or disables strict dialect mode for this builder.
///
/// When `strict` is `true`, fallback rewrites (e.g. `ILIKE` → `LOWER … LIKE`)
/// are promoted to hard errors at render time.  Defaults to `false`.
///
/// # Errors
///
/// Returns an error if `handle` does not refer to a valid builder.
pub fn builder_dialect_strict(handle: &str, strict: bool) -> Result<String, String> {
    let mut parts = registry_get_cloned(handle)?;
    parts.dialect_strict = strict;
    parts.cached_query = None;
    Ok(registry_insert(parts))
}

// ─── FROM ────────────────────────────────────────────────────────────────────

pub fn builder_from_table(handle: &str, table: String) -> Result<String, String> {
    apply_single_builder_op(handle, BuilderOp::FromTable { table })
}

pub fn builder_from_table_alias(
    handle: &str,
    table: String,
    alias: String,
) -> Result<String, String> {
    apply_single_builder_op(handle, BuilderOp::FromTableAlias { table, alias })
}

pub fn builder_from_subquery(
    handle: &str,
    alias: String,
    query_json: String,
) -> Result<String, String> {
    let query: SqlQuery = serde_json::from_str(&query_json).map_err(|e| e.to_string())?;
    apply_single_builder_op(handle, BuilderOp::FromSubquery { alias, query })
}

// ─── DISTINCT ────────────────────────────────────────────────────────────────

pub fn builder_distinct(handle: &str) -> Result<String, String> {
    apply_single_builder_op(handle, BuilderOp::Distinct)
}

pub fn builder_distinct_on_columns(handle: &str, cols_json: String) -> Result<String, String> {
    let cols: Vec<String> = serde_json::from_str(&cols_json).map_err(|e| e.to_string())?;
    apply_single_builder_op(handle, BuilderOp::DistinctOnColumns { cols })
}

pub fn builder_distinct_on_exprs(handle: &str, exprs_json: String) -> Result<String, String> {
    let inputs: Vec<DistinctOnInput> =
        serde_json::from_str(&exprs_json).map_err(|e| e.to_string())?;
    apply_single_builder_op(handle, BuilderOp::DistinctOnExprs { exprs: inputs })
}

// ─── SELECT ──────────────────────────────────────────────────────────────────

/// SELECT with simple column list: ["users.id", "name"] → stores joined identifiers.
pub fn builder_select_columns(handle: &str, cols_json: String) -> Result<String, String> {
    let cols: Vec<String> = serde_json::from_str(&cols_json).map_err(|e| e.to_string())?;
    apply_single_builder_op(handle, BuilderOp::SelectColumns { cols })
}

/// SELECT with alias map: [[alias, col_or_expr_json], ...].
/// Value can be a column name (string) or a pre-built SqlValue JSON.
#[derive(Deserialize)]
#[serde(untagged)]
enum SelectAliasValue {
    ColumnName(String),
    Expression(SqlValue),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum SelectAliasedInputEntry {
    Tuple((String, SelectAliasValue)),
    Entry(SelectAliasedEntry),
}

pub fn builder_select_aliased(handle: &str, aliases_json: String) -> Result<String, String> {
    let entries: Vec<SelectAliasedInputEntry> =
        serde_json::from_str(&aliases_json).map_err(|e| e.to_string())?;
    let mut parts = registry_get_cloned(handle)?;
    let dialect = active_dialect(&parts);

    let selections: Vec<SqlValue> = entries
        .iter()
        .map(|entry| match entry {
            SelectAliasedInputEntry::Tuple((alias, val)) => {
                let selection = match val {
                    SelectAliasValue::ColumnName(col) => {
                        SqlValue::Identifier(sql::identifier(split_col(col))?)
                    }
                    SelectAliasValue::Expression(sv) => sv.clone(),
                };
                let alias_id = sql::identifier(vec![alias.clone()])?;
                sql::sql(
                    vec!["".to_string(), " AS ".to_string(), "".to_string()],
                    vec![selection, SqlValue::Identifier(alias_id)],
                )
                .map(SqlValue::Query)
            }
            SelectAliasedInputEntry::Entry(entry) => {
                let expr_query = eval_expr_for_dialect(entry.expr.clone(), &dialect)?;
                if entry.bare {
                    Ok(SqlValue::Query(expr_query))
                } else {
                    let alias_id = sql::identifier(vec![entry.alias.clone()])?;
                    sql::sql(
                        vec!["".to_string(), " AS ".to_string(), "".to_string()],
                        vec![SqlValue::Query(expr_query), SqlValue::Identifier(alias_id)],
                    )
                    .map(SqlValue::Query)
                }
            }
        })
        .collect::<Result<Vec<_>, _>>()?;

    let content = sql::join(selections, Some(", ".to_string()))?;
    let selected_columns = entries
        .iter()
        .map(|entry| match entry {
            SelectAliasedInputEntry::Tuple((alias, _)) => alias.clone(),
            SelectAliasedInputEntry::Entry(entry) => entry.alias.clone(),
        })
        .collect();

    apply_projection(
        &mut parts,
        content,
        selected_columns,
        ProjectionTarget::Select,
    );
    Ok(registry_insert(parts))
}

/// SELECT with a pre-built fragment (from BuilderContext callback).
/// fragment_json: JSON of SqlQuery, selected_cols_json: JSON of Vec<String>.
pub fn builder_select_fragment(
    handle: &str,
    fragment_json: String,
    selected_cols_json: String,
) -> Result<String, String> {
    let fragment: SqlQuery = serde_json::from_str(&fragment_json).map_err(|e| e.to_string())?;
    let selected_cols: Vec<String> =
        serde_json::from_str(&selected_cols_json).map_err(|e| e.to_string())?;
    let mut parts = registry_get_cloned(handle)?;
    apply_projection(
        &mut parts,
        fragment,
        selected_cols,
        ProjectionTarget::Select,
    );
    Ok(registry_insert(parts))
}

pub fn builder_returning_columns(handle: &str, cols_json: String) -> Result<String, String> {
    let cols: Vec<String> = serde_json::from_str(&cols_json).map_err(|e| e.to_string())?;
    apply_single_builder_op(handle, BuilderOp::ReturningColumns { cols })
}

pub fn builder_returning_aliased(handle: &str, aliases_json: String) -> Result<String, String> {
    let entries: Vec<SelectAliasedEntry> =
        serde_json::from_str(&aliases_json).map_err(|e| e.to_string())?;
    let mut parts = registry_get_cloned(handle)?;

    let selected_cols: Vec<String> = entries.iter().map(|e| e.alias.clone()).collect();
    let dialect = active_dialect(&parts);
    let selections: Vec<SqlValue> = entries
        .into_iter()
        .map(|entry| {
            let expr_query = eval_expr_for_dialect(entry.expr, &dialect)?;
            if entry.bare {
                Ok(SqlValue::Query(expr_query))
            } else {
                let alias_id = sql::identifier(vec![entry.alias])?;
                sql::sql(
                    vec!["".to_string(), " AS ".to_string(), "".to_string()],
                    vec![SqlValue::Query(expr_query), SqlValue::Identifier(alias_id)],
                )
                .map(SqlValue::Query)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    let content = sql::join(selections, Some(", ".to_string()))?;

    apply_projection(
        &mut parts,
        content,
        selected_cols,
        ProjectionTarget::Returning,
    );
    Ok(registry_insert(parts))
}

pub fn builder_returning_fragment(
    handle: &str,
    fragment_json: String,
    selected_cols_json: String,
) -> Result<String, String> {
    let fragment: SqlQuery = serde_json::from_str(&fragment_json).map_err(|e| e.to_string())?;
    let selected_cols: Vec<String> =
        serde_json::from_str(&selected_cols_json).map_err(|e| e.to_string())?;
    let mut parts = registry_get_cloned(handle)?;
    apply_projection(
        &mut parts,
        fragment,
        selected_cols,
        ProjectionTarget::Returning,
    );
    Ok(registry_insert(parts))
}

// ─── JOIN ─────────────────────────────────────────────────────────────────────

fn open_join(mut parts: QueryParts, join_type: JoinType, source: SqlQuery) -> QueryParts {
    parts.pending_join = Some(PendingJoinState {
        join_type,
        source,
        predicate: None,
        using: vec![],
    });
    parts
}

fn commit_cross_join(mut parts: QueryParts, source: SqlQuery) -> Result<QueryParts, String> {
    let join_fragment = sql::sql(
        vec!["CROSS JOIN ".to_string(), "".to_string()],
        vec![SqlValue::Query(source)],
    )?;
    Arc::make_mut(&mut parts.joins).push(join_fragment);
    Ok(parts)
}

/// Parse `join_type` then route to [`commit_cross_join`] or [`open_join`].
fn apply_join(parts: QueryParts, join_type_str: &str, src: SqlQuery) -> Result<QueryParts, String> {
    let jt = JoinType::parse(join_type_str)?;
    if jt == JoinType::Cross {
        commit_cross_join(parts, src)
    } else {
        Ok(open_join(parts, jt, src))
    }
}

pub fn builder_join_table(
    handle: &str,
    join_type: String,
    table: String,
) -> Result<String, String> {
    apply_single_builder_op(handle, BuilderOp::JoinTable { join_type, table })
}

pub fn builder_join_table_alias(
    handle: &str,
    join_type: String,
    table: String,
    alias: String,
) -> Result<String, String> {
    apply_single_builder_op(
        handle,
        BuilderOp::JoinTableAlias {
            join_type,
            table,
            alias,
        },
    )
}

pub fn builder_join_subquery(
    handle: &str,
    join_type: String,
    alias: String,
    query_json: String,
) -> Result<String, String> {
    let query: SqlQuery = serde_json::from_str(&query_json).map_err(|e| e.to_string())?;
    let parts = normalize_pending_join(registry_get_cloned(handle)?)?;
    let src = subquery_source_fragment(query, &alias)?;
    Ok(registry_insert(apply_join(parts, &join_type, src)?))
}

// ─── JOIN predicates ─────────────────────────────────────────────────────────

fn update_pending_join_predicate(
    mut parts: QueryParts,
    operator: &str,
    condition: SqlQuery,
    require_existing: bool,
) -> Result<QueryParts, String> {
    let pj = parts
        .pending_join
        .as_mut()
        .ok_or("No pending join — call join() first")?;

    if require_existing && pj.predicate.is_none() {
        return Err("Call on() before andOn() or orOn()".to_string());
    }
    if !pj.using.is_empty() {
        return Err("Cannot combine using() with on() / andOn() / orOn()".to_string());
    }

    pj.predicate = Some(compose_logical(pj.predicate.take(), operator, condition)?);
    Ok(parts)
}

fn apply_using_columns(mut parts: QueryParts, cols: Vec<String>) -> Result<QueryParts, String> {
    let pj = parts
        .pending_join
        .as_mut()
        .ok_or("No pending join — call join() first")?;

    if pj.predicate.is_some() {
        return Err("Cannot combine using() with on()".to_string());
    }

    pj.using = cols
        .iter()
        .map(|c| sql::identifier(split_col(c)))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(parts)
}

fn apply_on_columns(parts: QueryParts, pairs: Vec<[String; 2]>) -> Result<QueryParts, String> {
    let conditions: Vec<SqlValue> = pairs
        .iter()
        .map(|[left, right]| {
            let l = sql::identifier(split_col(left))?;
            let r = sql::identifier(split_col(right))?;
            crate::expressions::cmp(
                SqlValue::Identifier(l),
                "=".to_string(),
                SqlValue::Identifier(r),
            )
            .map(SqlValue::Query)
        })
        .collect::<Result<Vec<_>, _>>()?;

    let condition = crate::expressions::and(conditions)?;
    update_pending_join_predicate(parts, "AND", condition, false)
}

pub fn builder_on(handle: &str, pred_json: String) -> Result<String, String> {
    let pred: Predicate = serde_json::from_str(&pred_json).map_err(|e| e.to_string())?;
    apply_single_builder_op(handle, BuilderOp::On { pred })
}

pub fn builder_and_on(handle: &str, pred_json: String) -> Result<String, String> {
    let pred: Predicate = serde_json::from_str(&pred_json).map_err(|e| e.to_string())?;
    apply_single_builder_op(handle, BuilderOp::AndOn { pred })
}

pub fn builder_or_on(handle: &str, pred_json: String) -> Result<String, String> {
    let pred: Predicate = serde_json::from_str(&pred_json).map_err(|e| e.to_string())?;
    apply_single_builder_op(handle, BuilderOp::OrOn { pred })
}

pub fn builder_using_columns(handle: &str, cols_json: String) -> Result<String, String> {
    let cols: Vec<String> = serde_json::from_str(&cols_json).map_err(|e| e.to_string())?;
    apply_single_builder_op(handle, BuilderOp::UsingColumns { cols })
}

pub fn builder_on_columns(handle: &str, pairs_json: String) -> Result<String, String> {
    let pairs: Vec<[String; 2]> = serde_json::from_str(&pairs_json).map_err(|e| e.to_string())?;
    apply_single_builder_op(handle, BuilderOp::OnColumns { pairs })
}

// ─── WHERE / HAVING ──────────────────────────────────────────────────────────

pub fn builder_where(handle: &str, pred_json: String) -> Result<String, String> {
    let pred: Predicate = serde_json::from_str(&pred_json).map_err(|e| e.to_string())?;
    apply_single_builder_op(handle, BuilderOp::Where { pred })
}

pub fn builder_and_where(handle: &str, pred_json: String) -> Result<String, String> {
    let pred: Predicate = serde_json::from_str(&pred_json).map_err(|e| e.to_string())?;
    apply_single_builder_op(handle, BuilderOp::AndWhere { pred })
}

pub fn builder_or_where(handle: &str, pred_json: String) -> Result<String, String> {
    let pred: Predicate = serde_json::from_str(&pred_json).map_err(|e| e.to_string())?;
    apply_single_builder_op(handle, BuilderOp::OrWhere { pred })
}

pub fn builder_having(handle: &str, pred_json: String) -> Result<String, String> {
    let pred: Predicate = serde_json::from_str(&pred_json).map_err(|e| e.to_string())?;
    apply_single_builder_op(handle, BuilderOp::Having { pred })
}

pub fn builder_and_having(handle: &str, pred_json: String) -> Result<String, String> {
    let pred: Predicate = serde_json::from_str(&pred_json).map_err(|e| e.to_string())?;
    apply_single_builder_op(handle, BuilderOp::AndHaving { pred })
}

pub fn builder_or_having(handle: &str, pred_json: String) -> Result<String, String> {
    let pred: Predicate = serde_json::from_str(&pred_json).map_err(|e| e.to_string())?;
    apply_single_builder_op(handle, BuilderOp::OrHaving { pred })
}

// ─── GROUP BY ────────────────────────────────────────────────────────────────

pub fn builder_group_by_columns(handle: &str, cols_json: String) -> Result<String, String> {
    let cols: Vec<String> = serde_json::from_str(&cols_json).map_err(|e| e.to_string())?;
    apply_single_builder_op(handle, BuilderOp::GroupBy { cols })
}

// ─── ORDER BY ────────────────────────────────────────────────────────────────

pub fn builder_order_by_column(
    handle: &str,
    col: String,
    direction: Option<String>,
    null_order: Option<String>,
) -> Result<String, String> {
    apply_single_builder_op(
        handle,
        BuilderOp::OrderBy {
            col,
            direction,
            null_order,
        },
    )
}

pub fn builder_order_by_columns(handle: &str, cols_json: String) -> Result<String, String> {
    let cols: Vec<String> = serde_json::from_str(&cols_json).map_err(|e| e.to_string())?;
    apply_single_builder_op(handle, BuilderOp::OrderByColumns { cols })
}

fn render_order_by(
    col: &str,
    direction: Option<&str>,
    null_order: Option<&str>,
    dialect: Dialect,
) -> Result<SqlQuery, String> {
    let normalized_dir = direction.map(|d| d.to_uppercase());
    let normalized_null = null_order.map(|n| n.to_uppercase());

    if normalized_null.is_some()
        && !crate::backend::backend_for_dialect(&dialect)
            .capabilities()
            .null_ordering
    {
        return Err(format!(
            "Dialect \"{}\" does not support NULLS FIRST/LAST in ORDER BY.",
            dialect
        ));
    }

    let id = col_to_identifier(col)?;

    if normalized_dir.is_none() && normalized_null.is_none() {
        return sql::sql(
            vec!["".to_string(), "".to_string()],
            vec![SqlValue::Identifier(id)],
        );
    }

    let mut parts_list: Vec<SqlQuery> = vec![sql::sql(
        vec!["".to_string(), "".to_string()],
        vec![SqlValue::Identifier(id)],
    )?];

    if let Some(dir) = &normalized_dir {
        parts_list.push(SqlQuery {
            text: dir.clone(),
            raw: dir.clone(),
            values: vec![],
        });
    }

    if let Some(null_ord) = &normalized_null {
        parts_list.push(SqlQuery {
            text: format!("NULLS {}", null_ord),
            raw: format!("NULLS {}", null_ord),
            values: vec![],
        });
    }

    sql::join(
        parts_list.into_iter().map(SqlValue::Query).collect(),
        Some(" ".to_string()),
    )
}

// ─── LIMIT / OFFSET ──────────────────────────────────────────────────────────

pub fn builder_limit(handle: &str, count: u32) -> Result<String, String> {
    apply_single_builder_op(handle, BuilderOp::Limit { count })
}

pub fn builder_offset(handle: &str, count: u32) -> Result<String, String> {
    apply_single_builder_op(handle, BuilderOp::Offset { count })
}

// ─── SELECT lock clauses ─────────────────────────────────────────────────────

pub fn builder_for_update(handle: &str) -> Result<String, String> {
    apply_single_builder_op(handle, BuilderOp::ForUpdate)
}

pub fn builder_for_share(handle: &str) -> Result<String, String> {
    apply_single_builder_op(handle, BuilderOp::ForShare)
}

pub fn builder_no_wait(handle: &str) -> Result<String, String> {
    apply_single_builder_op(handle, BuilderOp::NoWait)
}

pub fn builder_skip_locked(handle: &str) -> Result<String, String> {
    apply_single_builder_op(handle, BuilderOp::SkipLocked)
}

// ─── COMPOUND operators ───────────────────────────────────────────────────────

pub fn builder_union(handle: &str, query_json: String) -> Result<String, String> {
    let query: SqlQuery = serde_json::from_str(&query_json).map_err(|e| e.to_string())?;
    apply_single_builder_op(handle, BuilderOp::Union { query })
}

pub fn builder_union_all(handle: &str, query_json: String) -> Result<String, String> {
    let query: SqlQuery = serde_json::from_str(&query_json).map_err(|e| e.to_string())?;
    apply_single_builder_op(handle, BuilderOp::UnionAll { query })
}

pub fn builder_intersect(handle: &str, query_json: String) -> Result<String, String> {
    let query: SqlQuery = serde_json::from_str(&query_json).map_err(|e| e.to_string())?;
    apply_single_builder_op(handle, BuilderOp::Intersect { query })
}

pub fn builder_except(handle: &str, query_json: String) -> Result<String, String> {
    let query: SqlQuery = serde_json::from_str(&query_json).map_err(|e| e.to_string())?;
    apply_single_builder_op(handle, BuilderOp::Except { query })
}

// ─── CTE ─────────────────────────────────────────────────────────────────────

pub fn builder_with(handle: &str, name: String, query_json: String) -> Result<String, String> {
    let query: SqlQuery = serde_json::from_str(&query_json).map_err(|e| e.to_string())?;
    apply_single_builder_op(handle, BuilderOp::WithQuery { name, query })
}

pub fn builder_with_recursive(
    handle: &str,
    name: String,
    query_json: String,
) -> Result<String, String> {
    let query: SqlQuery = serde_json::from_str(&query_json).map_err(|e| e.to_string())?;
    apply_single_builder_op(
        handle,
        BuilderOp::WithRecursiveQuery {
            name,
            query,
            columns: vec![],
        },
    )
}

pub fn builder_with_recursive_columns(
    handle: &str,
    name: String,
    query_json: String,
    columns_json: String,
) -> Result<String, String> {
    let columns: Vec<String> = serde_json::from_str(&columns_json).map_err(|e| e.to_string())?;
    let query: SqlQuery = serde_json::from_str(&query_json).map_err(|e| e.to_string())?;
    apply_single_builder_op(
        handle,
        BuilderOp::WithRecursiveQuery {
            name,
            query,
            columns,
        },
    )
}

fn apply_cte_query(
    mut parts: QueryParts,
    name: String,
    query: SqlQuery,
    recursive: bool,
    columns: Vec<String>,
) -> QueryParts {
    Arc::make_mut(&mut parts.ctes).push(CteDefinition {
        name,
        recursive,
        columns,
        query,
    });
    parts
}

fn apply_from_table(
    parts: &mut QueryParts,
    table: &str,
    alias: Option<&str>,
) -> Result<(), String> {
    parts.from = Some(Arc::new(from_table_fragment(table, alias)?));
    parts.statement = Some(StatementKind::Select);
    parts.selected_columns = vec![];
    parts.select = None;
    parts.returning = None;
    parts.distinct = None;
    parts.lock = None;
    Ok(())
}

// ─── INSERT ──────────────────────────────────────────────────────────────────

fn parse_conflict_columns(cols_json: String) -> Result<Vec<String>, String> {
    let cols: Vec<String> = serde_json::from_str(&cols_json).map_err(|e| e.to_string())?;
    if cols.is_empty() {
        return Err("onConflict(...) requires at least one column.".to_string());
    }
    if cols.iter().any(|col| col.is_empty()) {
        return Err("onConflict(...) columns cannot be empty.".to_string());
    }
    Ok(cols)
}

fn apply_conflict_target(
    parts: &mut QueryParts,
    target: InsertConflictTarget,
) -> Result<(), String> {
    if matches!(
        (&parts.insert.conflict.target, &target),
        (
            Some(InsertConflictTarget::Columns(_)),
            InsertConflictTarget::Constraint(_)
        ) | (
            Some(InsertConflictTarget::Constraint(_)),
            InsertConflictTarget::Columns(_)
        )
    ) {
        return Err(
            "Insert conflict target cannot mix columns and constraint targets.".to_string(),
        );
    }

    parts.insert.conflict.target = Some(target);
    Ok(())
}

fn apply_insert_into(parts: &mut QueryParts, table: String) {
    parts.statement = Some(StatementKind::Insert);
    parts.insert = InsertState {
        into: Some(table),
        column_names: vec![],
        rows: vec![],
        select: None,
        conflict: Default::default(),
    };
    parts.update = UpdateState::default();
    parts.delete = DeleteState::default();
    parts.where_clauses = Arc::new(vec![]);
    parts.returning = None;
    parts.selected_columns = vec![];
}

fn apply_values_insert(parts: &mut QueryParts, input: InsertRowsInput) -> Result<(), String> {
    if input.rows.is_empty() {
        return Err("values() requires at least one row".to_string());
    }

    if !input.column_names.is_empty() {
        parts.insert.column_names = input.column_names;
    }
    parts.insert.rows = input.rows;
    parts.insert.select = None;
    parts.returning = None;
    parts.selected_columns = vec![];
    Ok(())
}

fn apply_insert_columns(parts: &mut QueryParts, cols: Vec<String>) {
    parts.insert.column_names = cols;
    parts.insert.rows = vec![];
    parts.insert.select = None;
    parts.returning = None;
    parts.selected_columns = vec![];
}

pub fn builder_insert_into(handle: &str, table: String) -> Result<String, String> {
    apply_single_builder_op(handle, BuilderOp::InsertInto { table })
}

pub fn builder_columns(handle: &str, cols_json: String) -> Result<String, String> {
    let cols: Vec<String> = serde_json::from_str(&cols_json).map_err(|e| e.to_string())?;
    apply_single_builder_op(handle, BuilderOp::InsertColumns { cols })
}

/// rows_json: { "columnNames": [...], "rows": [[SqlValue, ...], ...] }
pub fn builder_values_insert(handle: &str, rows_json: String) -> Result<String, String> {
    let input: InsertRowsInput = serde_json::from_str(&rows_json).map_err(|e| e.to_string())?;
    apply_single_builder_op(
        handle,
        BuilderOp::ValuesInsert {
            column_names: input.column_names,
            rows: input.rows,
        },
    )
}

pub fn builder_insert_select(handle: &str, query_json: String) -> Result<String, String> {
    let query: SqlQuery = serde_json::from_str(&query_json).map_err(|e| e.to_string())?;
    apply_single_builder_op(handle, BuilderOp::InsertSelect { query })
}

pub fn builder_insert_select_handle(handle: &str, rhs_handle: &str) -> Result<String, String> {
    apply_single_builder_op(
        handle,
        BuilderOp::InsertSelectHandle {
            rhs_handle: rhs_handle.to_string(),
        },
    )
}

pub fn builder_on_conflict_columns(handle: &str, cols_json: String) -> Result<String, String> {
    let cols = parse_conflict_columns(cols_json)?;
    apply_single_builder_op(handle, BuilderOp::OnConflictColumns { cols })
}

pub fn builder_on_conflict_constraint(handle: &str, constraint: String) -> Result<String, String> {
    apply_single_builder_op(handle, BuilderOp::OnConflictConstraint { name: constraint })
}

pub fn builder_do_nothing(handle: &str) -> Result<String, String> {
    apply_single_builder_op(handle, BuilderOp::DoNothing)
}

pub fn builder_do_update_set(handle: &str, assignments_json: String) -> Result<String, String> {
    let raw_entries: Vec<(String, AssignmentInputValue)> =
        serde_json::from_str(&assignments_json).map_err(|e| e.to_string())?;
    apply_single_builder_op(
        handle,
        BuilderOp::DoUpdateSet {
            assignments: raw_entries,
        },
    )
}

pub fn builder_conflict_where(handle: &str, pred_json: String) -> Result<String, String> {
    let pred: Predicate = serde_json::from_str(&pred_json).map_err(|e| e.to_string())?;
    apply_single_builder_op(handle, BuilderOp::ConflictWhere { pred })
}

// ─── UPDATE ──────────────────────────────────────────────────────────────────

fn apply_update(parts: &mut QueryParts, table: String) {
    parts.statement = Some(StatementKind::Update);
    parts.update = UpdateState {
        table: Some(table),
        assignments: vec![],
    };
    parts.insert = InsertState::default();
    parts.delete = DeleteState::default();
    parts.where_clauses = Arc::new(vec![]);
    parts.returning = None;
    parts.selected_columns = vec![];
}

pub fn builder_update(handle: &str, table: String) -> Result<String, String> {
    apply_single_builder_op(handle, BuilderOp::Update { table })
}

/// assignments_json: [[column_name, SqlValue], ...]
pub fn builder_set(handle: &str, assignments_json: String) -> Result<String, String> {
    let raw_entries: Vec<(String, AssignmentInputValue)> =
        serde_json::from_str(&assignments_json).map_err(|e| e.to_string())?;
    apply_single_builder_op(
        handle,
        BuilderOp::Set {
            assignments: raw_entries,
        },
    )
}

// ─── DELETE ──────────────────────────────────────────────────────────────────

fn apply_delete_from(parts: &mut QueryParts, table: String) {
    parts.statement = Some(StatementKind::Delete);
    parts.delete = DeleteState {
        from_table: Some(table),
    };
    parts.insert = InsertState::default();
    parts.update = UpdateState::default();
    parts.where_clauses = Arc::new(vec![]);
    parts.returning = None;
    parts.selected_columns = vec![];
}

pub fn builder_delete_from(handle: &str, table: String) -> Result<String, String> {
    apply_single_builder_op(handle, BuilderOp::DeleteFrom { table })
}

// ─── Handle-passing compound / CTE / subquery ops ────────────────────────────
//
// These bypass the JSON serialisation round-trip when the RHS is another
// builder instance whose handle is already live in the registry.

pub fn builder_union_handle(handle: &str, rhs_handle: &str) -> Result<String, String> {
    apply_single_builder_op(
        handle,
        BuilderOp::UnionHandle {
            rhs_handle: rhs_handle.to_string(),
        },
    )
}

pub fn builder_union_all_handle(handle: &str, rhs_handle: &str) -> Result<String, String> {
    apply_single_builder_op(
        handle,
        BuilderOp::UnionAllHandle {
            rhs_handle: rhs_handle.to_string(),
        },
    )
}

pub fn builder_intersect_handle(handle: &str, rhs_handle: &str) -> Result<String, String> {
    apply_single_builder_op(
        handle,
        BuilderOp::IntersectHandle {
            rhs_handle: rhs_handle.to_string(),
        },
    )
}

pub fn builder_except_handle(handle: &str, rhs_handle: &str) -> Result<String, String> {
    apply_single_builder_op(
        handle,
        BuilderOp::ExceptHandle {
            rhs_handle: rhs_handle.to_string(),
        },
    )
}

pub fn builder_with_handle(handle: &str, name: String, rhs_handle: &str) -> Result<String, String> {
    apply_single_builder_op(
        handle,
        BuilderOp::WithHandle {
            name,
            rhs_handle: rhs_handle.to_string(),
        },
    )
}

pub fn builder_with_recursive_handle(
    handle: &str,
    name: String,
    rhs_handle: &str,
) -> Result<String, String> {
    apply_single_builder_op(
        handle,
        BuilderOp::WithRecursiveHandle {
            name,
            rhs_handle: rhs_handle.to_string(),
        },
    )
}

pub fn builder_from_subquery_handle(
    handle: &str,
    alias: String,
    rhs_handle: &str,
) -> Result<String, String> {
    apply_single_builder_op(
        handle,
        BuilderOp::FromSubqueryHandle {
            alias,
            rhs_handle: rhs_handle.to_string(),
        },
    )
}

pub fn builder_join_subquery_handle(
    handle: &str,
    join_type: String,
    alias: String,
    rhs_handle: &str,
) -> Result<String, String> {
    apply_single_builder_op(
        handle,
        BuilderOp::JoinSubqueryHandle {
            join_type,
            alias,
            rhs_handle: rhs_handle.to_string(),
        },
    )
}

// ─── Batch ops ───────────────────────────────────────────────────────────────
//
// `builder_apply_ops` accepts a JSON array of operation descriptors and
// applies them sequentially.  One FFI crossing for N mutations.
//
// Supported ops (same field names as the individual bridge functions):
//   { "op": "where",     "pred": <SqlQuery JSON>  }
//   { "op": "andWhere",  "pred": <SqlQuery JSON>  }
//   { "op": "orWhere",   "pred": <SqlQuery JSON>  }
//   { "op": "having",    "pred": <SqlQuery JSON>  }
//   { "op": "andHaving", "pred": <SqlQuery JSON>  }
//   { "op": "orHaving",  "pred": <SqlQuery JSON>  }
//   { "op": "fromTable", "table": <string> }
//   { "op": "fromTableAlias", "table": <string>, "alias": <string> }
//   { "op": "joinTable", "joinType": <string>, "table": <string> }
//   { "op": "joinTableAlias", "joinType": <string>, "table": <string>, "alias": <string> }
//   { "op": "on", "pred": <SqlQuery JSON> }
//   { "op": "andOn", "pred": <SqlQuery JSON> }
//   { "op": "orOn", "pred": <SqlQuery JSON> }
//   { "op": "usingColumns", "cols": [<string>] }
//   { "op": "onColumns", "pairs": [[<string>, <string>], ...] }
//   { "op": "withHandle", "name": <string>, "rhsHandle": <string> }
//   { "op": "withQuery", "name": <string>, "query": <SqlQuery JSON> }
//   { "op": "withRecursiveHandle", "name": <string>, "rhsHandle": <string> }
//   { "op": "withRecursiveQuery", "name": <string>, "query": <SqlQuery JSON>, "columns"?: [<string>] }
//   { "op": "orderBy",   "col": <string>, "direction"?: <string>, "nullOrder"?: <string> }
//   { "op": "orderByColumns", "cols": [<string>]  }
//   { "op": "groupBy",   "cols": [<string>]       }
//   { "op": "limit",     "count": <u32>           }
//   { "op": "offset",    "count": <u32>           }

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
enum BuilderOp {
    Distinct,
    DistinctOnColumns {
        cols: Vec<String>,
    },
    DistinctOnExprs {
        exprs: Vec<DistinctOnInput>,
    },
    Where {
        pred: Predicate,
    },
    AndWhere {
        pred: Predicate,
    },
    OrWhere {
        pred: Predicate,
    },
    Having {
        pred: Predicate,
    },
    AndHaving {
        pred: Predicate,
    },
    OrHaving {
        pred: Predicate,
    },
    On {
        pred: Predicate,
    },
    AndOn {
        pred: Predicate,
    },
    OrOn {
        pred: Predicate,
    },
    UsingColumns {
        cols: Vec<String>,
    },
    OnColumns {
        pairs: Vec<[String; 2]>,
    },
    FromTable {
        table: String,
    },
    FromTableAlias {
        table: String,
        alias: String,
    },
    JoinTable {
        #[serde(rename = "joinType")]
        join_type: String,
        table: String,
    },
    JoinTableAlias {
        #[serde(rename = "joinType")]
        join_type: String,
        table: String,
        alias: String,
    },
    WithHandle {
        name: String,
        #[serde(rename = "rhsHandle")]
        rhs_handle: String,
    },
    WithQuery {
        name: String,
        query: SqlQuery,
    },
    WithRecursiveHandle {
        name: String,
        #[serde(rename = "rhsHandle")]
        rhs_handle: String,
    },
    WithRecursiveQuery {
        name: String,
        query: SqlQuery,
        #[serde(default)]
        columns: Vec<String>,
    },
    InsertInto {
        table: String,
    },
    ValuesInsert {
        #[serde(rename = "columnNames")]
        column_names: Vec<String>,
        rows: Vec<Vec<SqlValue>>,
    },
    InsertColumns {
        cols: Vec<String>,
    },
    Update {
        table: String,
    },
    DeleteFrom {
        table: String,
    },
    Set {
        assignments: Vec<(String, AssignmentInputValue)>,
    },
    OnConflictColumns {
        cols: Vec<String>,
    },
    OnConflictConstraint {
        name: String,
    },
    DoNothing,
    DoUpdateSet {
        assignments: Vec<(String, AssignmentInputValue)>,
    },
    ConflictWhere {
        pred: Predicate,
    },
    OrderBy {
        col: String,
        direction: Option<String>,
        #[serde(rename = "nullOrder")]
        null_order: Option<String>,
    },
    OrderByColumns {
        cols: Vec<String>,
    },
    GroupBy {
        cols: Vec<String>,
    },
    Limit {
        count: u32,
    },
    Offset {
        count: u32,
    },
    ForUpdate,
    ForShare,
    NoWait,
    SkipLocked,
    /// SELECT alias list built from ExprNode trees — zero FFI from TypeScript.
    SelectAliased {
        entries: Vec<SelectAliasedEntry>,
    },
    /// SELECT plain column list — batched alongside where/having ops.
    SelectColumns {
        cols: Vec<String>,
    },
    ReturningAliased {
        entries: Vec<SelectAliasedEntry>,
    },
    ReturningColumns {
        cols: Vec<String>,
    },
    // ── Phase 2: compound ops ─────────────────────────────────────────────────
    /// UNION (rhs is a registered registry handle).
    UnionHandle {
        rhs_handle: String,
    },
    /// UNION (rhs is a pre-built SqlQuery).
    Union {
        query: SqlQuery,
    },
    /// UNION ALL (rhs is a registered registry handle).
    UnionAllHandle {
        rhs_handle: String,
    },
    /// UNION ALL (rhs is a pre-built SqlQuery).
    UnionAll {
        query: SqlQuery,
    },
    /// INTERSECT (rhs is a registered registry handle).
    IntersectHandle {
        rhs_handle: String,
    },
    /// INTERSECT (rhs is a pre-built SqlQuery).
    Intersect {
        query: SqlQuery,
    },
    /// EXCEPT (rhs is a registered registry handle).
    ExceptHandle {
        rhs_handle: String,
    },
    /// EXCEPT (rhs is a pre-built SqlQuery).
    Except {
        query: SqlQuery,
    },
    // ── Phase 3: subquery source ops ──────────────────────────────────────────
    /// FROM (subquery) — rhs comes from a registry handle.
    FromSubqueryHandle {
        alias: String,
        rhs_handle: String,
    },
    /// FROM (subquery) — rhs is a pre-built SqlQuery.
    FromSubquery {
        alias: String,
        query: SqlQuery,
    },
    /// JOIN (subquery) — rhs comes from a registry handle.
    JoinSubqueryHandle {
        join_type: String,
        alias: String,
        rhs_handle: String,
    },
    /// JOIN (subquery) — rhs is a pre-built SqlQuery.
    JoinSubquery {
        join_type: String,
        alias: String,
        query: SqlQuery,
    },
    // ── Phase 4: INSERT … SELECT ──────────────────────────────────────────────
    /// INSERT … SELECT — rhs comes from a registry handle.
    InsertSelectHandle {
        rhs_handle: String,
    },
    /// INSERT … SELECT — rhs is a pre-built SqlQuery.
    InsertSelect {
        query: SqlQuery,
    },
    // ── Inline child ops: single-FFI-crossing variants ────────────────────────
    /// CTE (WITH) — child ops inlined; no prior child FFI call needed.
    WithInline {
        name: String,
        base_handle: String,
        child_ops: Vec<BuilderOp>,
    },
    /// CTE (WITH RECURSIVE) — child ops inlined.
    WithRecursiveInline {
        name: String,
        base_handle: String,
        child_ops: Vec<BuilderOp>,
    },
    /// FROM (subquery) — child ops inlined.
    FromSubqueryInline {
        alias: String,
        base_handle: String,
        child_ops: Vec<BuilderOp>,
    },
    /// JOIN (subquery) — child ops inlined.
    JoinSubqueryInline {
        join_type: String,
        alias: String,
        base_handle: String,
        child_ops: Vec<BuilderOp>,
    },
    /// UNION — child ops inlined.
    UnionInline {
        base_handle: String,
        child_ops: Vec<BuilderOp>,
    },
    /// UNION ALL — child ops inlined.
    UnionAllInline {
        base_handle: String,
        child_ops: Vec<BuilderOp>,
    },
    /// INTERSECT — child ops inlined.
    IntersectInline {
        base_handle: String,
        child_ops: Vec<BuilderOp>,
    },
    /// EXCEPT — child ops inlined.
    ExceptInline {
        base_handle: String,
        child_ops: Vec<BuilderOp>,
    },
}

fn parse_compact_field<T: serde::de::DeserializeOwned>(
    op: &str,
    field: &str,
    value: Value,
) -> Result<T, String> {
    serde_json::from_value(value).map_err(|e| format!("Invalid {field} for compact op '{op}': {e}"))
}

fn parse_compact_string(op: &str, field: &str, value: Value) -> Result<String, String> {
    match value {
        Value::String(value) => Ok(value),
        other => Err(format!(
            "Invalid {field} for compact op '{op}': expected string, got {other}"
        )),
    }
}

fn parse_compact_optional_string(
    op: &str,
    field: &str,
    value: Value,
) -> Result<Option<String>, String> {
    match value {
        Value::Null => Ok(None),
        Value::String(value) => Ok(Some(value)),
        other => Err(format!(
            "Invalid {field} for compact op '{op}': expected string or null, got {other}"
        )),
    }
}

fn parse_compact_u32(op: &str, field: &str, value: Value) -> Result<u32, String> {
    match value {
        Value::Number(number) => number
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| {
                format!("Invalid {field} for compact op '{op}': expected u32-compatible number")
            }),
        other => Err(format!(
            "Invalid {field} for compact op '{op}': expected number, got {other}"
        )),
    }
}

fn parse_compact_string_vec(op: &str, field: &str, value: Value) -> Result<Vec<String>, String> {
    match value {
        Value::Array(values) => values
            .into_iter()
            .enumerate()
            .map(|(index, value)| parse_compact_string(op, &format!("{field}[{index}]"), value))
            .collect(),
        other => Err(format!(
            "Invalid {field} for compact op '{op}': expected array, got {other}"
        )),
    }
}

fn next_compact_value<I>(items: &mut I, op: &str, field: &str) -> Result<Value, String>
where
    I: Iterator<Item = Value>,
{
    items
        .next()
        .ok_or_else(|| format!("Missing {field} for compact op '{op}'"))
}

/// Parse inline child ops from a msgpack `Value::Array` of compact op arrays.
fn parse_inline_child_ops(tag: &str, ops_value: Value) -> Result<Vec<BuilderOp>, String> {
    match ops_value {
        Value::Array(ops_array) => ops_array
            .into_iter()
            .enumerate()
            .map(|(i, v)| match v {
                Value::Array(items) => parse_compact_op(items),
                _ => Err(format!("Inline child op {i} for '{tag}' must be an array")),
            })
            .collect(),
        _ => Err(format!("Inline child ops for '{tag}' must be an array")),
    }
}

fn parse_compact_op(items: Vec<Value>) -> Result<BuilderOp, String> {
    let mut items = items.into_iter();
    let tag = match items.next() {
        Some(Value::String(tag)) => tag,
        Some(_) => return Err("Compact op tag must be a string".to_string()),
        None => return Err("Compact op cannot be empty".to_string()),
    };

    let op = match tag.as_str() {
        "w" => BuilderOp::Where {
            pred: parse_compact_field(&tag, "pred", next_compact_value(&mut items, &tag, "pred")?)?,
        },
        "d" => BuilderOp::Distinct,
        "dc" => BuilderOp::DistinctOnColumns {
            cols: parse_compact_string_vec(
                &tag,
                "cols",
                next_compact_value(&mut items, &tag, "cols")?,
            )?,
        },
        "de" => BuilderOp::DistinctOnExprs {
            exprs: parse_compact_field(
                &tag,
                "exprs",
                next_compact_value(&mut items, &tag, "exprs")?,
            )?,
        },
        "aw" => BuilderOp::AndWhere {
            pred: parse_compact_field(&tag, "pred", next_compact_value(&mut items, &tag, "pred")?)?,
        },
        "ow" => BuilderOp::OrWhere {
            pred: parse_compact_field(&tag, "pred", next_compact_value(&mut items, &tag, "pred")?)?,
        },
        "h" => BuilderOp::Having {
            pred: parse_compact_field(&tag, "pred", next_compact_value(&mut items, &tag, "pred")?)?,
        },
        "ah" => BuilderOp::AndHaving {
            pred: parse_compact_field(&tag, "pred", next_compact_value(&mut items, &tag, "pred")?)?,
        },
        "oh" => BuilderOp::OrHaving {
            pred: parse_compact_field(&tag, "pred", next_compact_value(&mut items, &tag, "pred")?)?,
        },
        "ft" => BuilderOp::FromTable {
            table: parse_compact_string(
                &tag,
                "table",
                next_compact_value(&mut items, &tag, "table")?,
            )?,
        },
        "fta" => BuilderOp::FromTableAlias {
            table: parse_compact_string(
                &tag,
                "table",
                next_compact_value(&mut items, &tag, "table")?,
            )?,
            alias: parse_compact_string(
                &tag,
                "alias",
                next_compact_value(&mut items, &tag, "alias")?,
            )?,
        },
        "jt" => BuilderOp::JoinTable {
            join_type: parse_compact_string(
                &tag,
                "joinType",
                next_compact_value(&mut items, &tag, "joinType")?,
            )?,
            table: parse_compact_string(
                &tag,
                "table",
                next_compact_value(&mut items, &tag, "table")?,
            )?,
        },
        "jta" => BuilderOp::JoinTableAlias {
            join_type: parse_compact_string(
                &tag,
                "joinType",
                next_compact_value(&mut items, &tag, "joinType")?,
            )?,
            table: parse_compact_string(
                &tag,
                "table",
                next_compact_value(&mut items, &tag, "table")?,
            )?,
            alias: parse_compact_string(
                &tag,
                "alias",
                next_compact_value(&mut items, &tag, "alias")?,
            )?,
        },
        "on" => BuilderOp::On {
            pred: parse_compact_field(&tag, "pred", next_compact_value(&mut items, &tag, "pred")?)?,
        },
        "aon" => BuilderOp::AndOn {
            pred: parse_compact_field(&tag, "pred", next_compact_value(&mut items, &tag, "pred")?)?,
        },
        "oon" => BuilderOp::OrOn {
            pred: parse_compact_field(&tag, "pred", next_compact_value(&mut items, &tag, "pred")?)?,
        },
        "uc" => BuilderOp::UsingColumns {
            cols: parse_compact_string_vec(
                &tag,
                "cols",
                next_compact_value(&mut items, &tag, "cols")?,
            )?,
        },
        "oc" => BuilderOp::OnColumns {
            pairs: parse_compact_field(
                &tag,
                "pairs",
                next_compact_value(&mut items, &tag, "pairs")?,
            )?,
        },
        "wh" => BuilderOp::WithHandle {
            name: parse_compact_string(
                &tag,
                "name",
                next_compact_value(&mut items, &tag, "name")?,
            )?,
            rhs_handle: parse_compact_string(
                &tag,
                "rhsHandle",
                next_compact_value(&mut items, &tag, "rhsHandle")?,
            )?,
        },
        "wq" => BuilderOp::WithQuery {
            name: parse_compact_string(
                &tag,
                "name",
                next_compact_value(&mut items, &tag, "name")?,
            )?,
            query: parse_compact_field(
                &tag,
                "query",
                next_compact_value(&mut items, &tag, "query")?,
            )?,
        },
        "wrh" => BuilderOp::WithRecursiveHandle {
            name: parse_compact_string(
                &tag,
                "name",
                next_compact_value(&mut items, &tag, "name")?,
            )?,
            rhs_handle: parse_compact_string(
                &tag,
                "rhsHandle",
                next_compact_value(&mut items, &tag, "rhsHandle")?,
            )?,
        },
        "wrq" => BuilderOp::WithRecursiveQuery {
            name: parse_compact_string(
                &tag,
                "name",
                next_compact_value(&mut items, &tag, "name")?,
            )?,
            query: parse_compact_field(
                &tag,
                "query",
                next_compact_value(&mut items, &tag, "query")?,
            )?,
            columns: parse_compact_field::<Option<Vec<String>>>(
                &tag,
                "columns",
                items.next().unwrap_or(Value::Null),
            )?
            .unwrap_or_default(),
        },
        "ii" => BuilderOp::InsertInto {
            table: parse_compact_string(
                &tag,
                "table",
                next_compact_value(&mut items, &tag, "table")?,
            )?,
        },
        "vi" => BuilderOp::ValuesInsert {
            column_names: parse_compact_field(
                &tag,
                "columnNames",
                next_compact_value(&mut items, &tag, "columnNames")?,
            )?,
            rows: parse_compact_field(&tag, "rows", next_compact_value(&mut items, &tag, "rows")?)?,
        },
        "ic" => BuilderOp::InsertColumns {
            cols: parse_compact_string_vec(
                &tag,
                "cols",
                next_compact_value(&mut items, &tag, "cols")?,
            )?,
        },
        "upd" => BuilderOp::Update {
            table: parse_compact_string(
                &tag,
                "table",
                next_compact_value(&mut items, &tag, "table")?,
            )?,
        },
        "del" => BuilderOp::DeleteFrom {
            table: parse_compact_string(
                &tag,
                "table",
                next_compact_value(&mut items, &tag, "table")?,
            )?,
        },
        "set" => BuilderOp::Set {
            assignments: parse_compact_field(
                &tag,
                "assignments",
                next_compact_value(&mut items, &tag, "assignments")?,
            )?,
        },
        "ict" => BuilderOp::OnConflictColumns {
            cols: parse_compact_string_vec(
                &tag,
                "cols",
                next_compact_value(&mut items, &tag, "cols")?,
            )?,
        },
        "icn" => BuilderOp::OnConflictConstraint {
            name: parse_compact_string(
                &tag,
                "name",
                next_compact_value(&mut items, &tag, "name")?,
            )?,
        },
        "idn" => BuilderOp::DoNothing,
        "idu" => BuilderOp::DoUpdateSet {
            assignments: parse_compact_field(
                &tag,
                "assignments",
                next_compact_value(&mut items, &tag, "assignments")?,
            )?,
        },
        "icw" => BuilderOp::ConflictWhere {
            pred: parse_compact_field(&tag, "pred", next_compact_value(&mut items, &tag, "pred")?)?,
        },
        "sa" => BuilderOp::SelectAliased {
            entries: parse_compact_field(
                &tag,
                "entries",
                next_compact_value(&mut items, &tag, "entries")?,
            )?,
        },
        "sc" => BuilderOp::SelectColumns {
            cols: parse_compact_string_vec(
                &tag,
                "cols",
                next_compact_value(&mut items, &tag, "cols")?,
            )?,
        },
        "ra" => BuilderOp::ReturningAliased {
            entries: parse_compact_field(
                &tag,
                "entries",
                next_compact_value(&mut items, &tag, "entries")?,
            )?,
        },
        "rc" => BuilderOp::ReturningColumns {
            cols: parse_compact_string_vec(
                &tag,
                "cols",
                next_compact_value(&mut items, &tag, "cols")?,
            )?,
        },
        "ob" => BuilderOp::OrderBy {
            col: parse_compact_string(&tag, "col", next_compact_value(&mut items, &tag, "col")?)?,
            direction: parse_compact_optional_string(
                &tag,
                "direction",
                items.next().unwrap_or(Value::Null),
            )?,
            null_order: parse_compact_optional_string(
                &tag,
                "nullOrder",
                items.next().unwrap_or(Value::Null),
            )?,
        },
        "obc" => BuilderOp::OrderByColumns {
            cols: parse_compact_string_vec(
                &tag,
                "cols",
                next_compact_value(&mut items, &tag, "cols")?,
            )?,
        },
        "gb" => BuilderOp::GroupBy {
            cols: parse_compact_string_vec(
                &tag,
                "cols",
                next_compact_value(&mut items, &tag, "cols")?,
            )?,
        },
        "l" => BuilderOp::Limit {
            count: parse_compact_u32(
                &tag,
                "count",
                next_compact_value(&mut items, &tag, "count")?,
            )?,
        },
        "o" => BuilderOp::Offset {
            count: parse_compact_u32(
                &tag,
                "count",
                next_compact_value(&mut items, &tag, "count")?,
            )?,
        },
        "fu" => BuilderOp::ForUpdate,
        "fs" => BuilderOp::ForShare,
        "nw" => BuilderOp::NoWait,
        "sl" => BuilderOp::SkipLocked,
        // ── Phase 2: compound ops ─────────────────────────────────────────────
        "unh" => BuilderOp::UnionHandle {
            rhs_handle: parse_compact_string(
                &tag,
                "rhsHandle",
                next_compact_value(&mut items, &tag, "rhsHandle")?,
            )?,
        },
        "un" => BuilderOp::Union {
            query: parse_compact_field(
                &tag,
                "query",
                next_compact_value(&mut items, &tag, "query")?,
            )?,
        },
        "uah" => BuilderOp::UnionAllHandle {
            rhs_handle: parse_compact_string(
                &tag,
                "rhsHandle",
                next_compact_value(&mut items, &tag, "rhsHandle")?,
            )?,
        },
        "ua" => BuilderOp::UnionAll {
            query: parse_compact_field(
                &tag,
                "query",
                next_compact_value(&mut items, &tag, "query")?,
            )?,
        },
        "ixh" => BuilderOp::IntersectHandle {
            rhs_handle: parse_compact_string(
                &tag,
                "rhsHandle",
                next_compact_value(&mut items, &tag, "rhsHandle")?,
            )?,
        },
        "ix" => BuilderOp::Intersect {
            query: parse_compact_field(
                &tag,
                "query",
                next_compact_value(&mut items, &tag, "query")?,
            )?,
        },
        "exh" => BuilderOp::ExceptHandle {
            rhs_handle: parse_compact_string(
                &tag,
                "rhsHandle",
                next_compact_value(&mut items, &tag, "rhsHandle")?,
            )?,
        },
        "ex" => BuilderOp::Except {
            query: parse_compact_field(
                &tag,
                "query",
                next_compact_value(&mut items, &tag, "query")?,
            )?,
        },
        // ── Phase 3: subquery source ops ──────────────────────────────────────
        "fsqh" => BuilderOp::FromSubqueryHandle {
            alias: parse_compact_string(
                &tag,
                "alias",
                next_compact_value(&mut items, &tag, "alias")?,
            )?,
            rhs_handle: parse_compact_string(
                &tag,
                "rhsHandle",
                next_compact_value(&mut items, &tag, "rhsHandle")?,
            )?,
        },
        "fsq" => BuilderOp::FromSubquery {
            alias: parse_compact_string(
                &tag,
                "alias",
                next_compact_value(&mut items, &tag, "alias")?,
            )?,
            query: parse_compact_field(
                &tag,
                "query",
                next_compact_value(&mut items, &tag, "query")?,
            )?,
        },
        "jsqh" => BuilderOp::JoinSubqueryHandle {
            join_type: parse_compact_string(
                &tag,
                "joinType",
                next_compact_value(&mut items, &tag, "joinType")?,
            )?,
            alias: parse_compact_string(
                &tag,
                "alias",
                next_compact_value(&mut items, &tag, "alias")?,
            )?,
            rhs_handle: parse_compact_string(
                &tag,
                "rhsHandle",
                next_compact_value(&mut items, &tag, "rhsHandle")?,
            )?,
        },
        "jsq" => BuilderOp::JoinSubquery {
            join_type: parse_compact_string(
                &tag,
                "joinType",
                next_compact_value(&mut items, &tag, "joinType")?,
            )?,
            alias: parse_compact_string(
                &tag,
                "alias",
                next_compact_value(&mut items, &tag, "alias")?,
            )?,
            query: parse_compact_field(
                &tag,
                "query",
                next_compact_value(&mut items, &tag, "query")?,
            )?,
        },
        // ── Phase 4: INSERT … SELECT ──────────────────────────────────────────
        "ish" => BuilderOp::InsertSelectHandle {
            rhs_handle: parse_compact_string(
                &tag,
                "rhsHandle",
                next_compact_value(&mut items, &tag, "rhsHandle")?,
            )?,
        },
        "is" => BuilderOp::InsertSelect {
            query: parse_compact_field(
                &tag,
                "query",
                next_compact_value(&mut items, &tag, "query")?,
            )?,
        },
        // ── Inline child ops ──────────────────────────────────────────────────
        "whi" => BuilderOp::WithInline {
            name: parse_compact_string(
                &tag,
                "name",
                next_compact_value(&mut items, &tag, "name")?,
            )?,
            base_handle: parse_compact_string(
                &tag,
                "baseHandle",
                next_compact_value(&mut items, &tag, "baseHandle")?,
            )?,
            child_ops: parse_inline_child_ops(
                &tag,
                next_compact_value(&mut items, &tag, "childOps")?,
            )?,
        },
        "wrhi" => BuilderOp::WithRecursiveInline {
            name: parse_compact_string(
                &tag,
                "name",
                next_compact_value(&mut items, &tag, "name")?,
            )?,
            base_handle: parse_compact_string(
                &tag,
                "baseHandle",
                next_compact_value(&mut items, &tag, "baseHandle")?,
            )?,
            child_ops: parse_inline_child_ops(
                &tag,
                next_compact_value(&mut items, &tag, "childOps")?,
            )?,
        },
        "fsqi" => BuilderOp::FromSubqueryInline {
            alias: parse_compact_string(
                &tag,
                "alias",
                next_compact_value(&mut items, &tag, "alias")?,
            )?,
            base_handle: parse_compact_string(
                &tag,
                "baseHandle",
                next_compact_value(&mut items, &tag, "baseHandle")?,
            )?,
            child_ops: parse_inline_child_ops(
                &tag,
                next_compact_value(&mut items, &tag, "childOps")?,
            )?,
        },
        "jsqi" => BuilderOp::JoinSubqueryInline {
            join_type: parse_compact_string(
                &tag,
                "joinType",
                next_compact_value(&mut items, &tag, "joinType")?,
            )?,
            alias: parse_compact_string(
                &tag,
                "alias",
                next_compact_value(&mut items, &tag, "alias")?,
            )?,
            base_handle: parse_compact_string(
                &tag,
                "baseHandle",
                next_compact_value(&mut items, &tag, "baseHandle")?,
            )?,
            child_ops: parse_inline_child_ops(
                &tag,
                next_compact_value(&mut items, &tag, "childOps")?,
            )?,
        },
        "uni" => BuilderOp::UnionInline {
            base_handle: parse_compact_string(
                &tag,
                "baseHandle",
                next_compact_value(&mut items, &tag, "baseHandle")?,
            )?,
            child_ops: parse_inline_child_ops(
                &tag,
                next_compact_value(&mut items, &tag, "childOps")?,
            )?,
        },
        "uai" => BuilderOp::UnionAllInline {
            base_handle: parse_compact_string(
                &tag,
                "baseHandle",
                next_compact_value(&mut items, &tag, "baseHandle")?,
            )?,
            child_ops: parse_inline_child_ops(
                &tag,
                next_compact_value(&mut items, &tag, "childOps")?,
            )?,
        },
        "ixi" => BuilderOp::IntersectInline {
            base_handle: parse_compact_string(
                &tag,
                "baseHandle",
                next_compact_value(&mut items, &tag, "baseHandle")?,
            )?,
            child_ops: parse_inline_child_ops(
                &tag,
                next_compact_value(&mut items, &tag, "childOps")?,
            )?,
        },
        "exi" => BuilderOp::ExceptInline {
            base_handle: parse_compact_string(
                &tag,
                "baseHandle",
                next_compact_value(&mut items, &tag, "baseHandle")?,
            )?,
            child_ops: parse_inline_child_ops(
                &tag,
                next_compact_value(&mut items, &tag, "childOps")?,
            )?,
        },
        _ => return Err(format!("Unknown compact builder op tag '{tag}'")),
    };

    if items.next().is_some() {
        return Err(format!("Compact op '{tag}' had trailing fields"));
    }

    Ok(op)
}

fn parse_builder_ops(ops_payload: &str) -> Result<Vec<BuilderOp>, String> {
    let raw_ops: Vec<Value> = serde_json::from_str(ops_payload).map_err(|e| e.to_string())?;
    raw_ops
        .into_iter()
        .map(|raw| match raw {
            Value::Array(items) => parse_compact_op(items),
            other => serde_json::from_value(other).map_err(|e| e.to_string()),
        })
        .collect()
}

fn read_u32_le(payload: &[u8], offset: &mut usize, field: &str) -> Result<u32, String> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| format!("Binary builder ops payload {field} offset overflow"))?;
    let bytes = payload
        .get(*offset..end)
        .ok_or_else(|| format!("Binary builder ops payload too short while reading {field}"))?;
    *offset = end;
    Ok(u32::from_le_bytes(
        bytes
            .try_into()
            // opengrep:ignore opengrep.rust-expect-in-production
            // `bytes` is a sub-slice proven to be exactly 4 bytes by the `get(*offset..end)` bounds check above.
            .expect("slice length is fixed by checked range"),
    ))
}

fn parse_builder_ops_bin(payload: &[u8]) -> Result<Vec<BuilderOp>, String> {
    if payload.len() < 5 {
        return Err("Binary builder ops payload too short".to_string());
    }

    let version = payload[0];
    if version != 1 {
        return Err(format!(
            "Unsupported binary builder ops payload version {version}"
        ));
    }

    let mut offset = 1usize;
    let count = read_u32_le(payload, &mut offset, "op count")? as usize;
    let mut ops = Vec::with_capacity(count);

    for index in 0..count {
        let encoding = *payload
            .get(offset)
            .ok_or_else(|| format!("Binary builder ops payload too short before op {index}"))?;
        offset += 1;
        if encoding != 0 {
            return Err(format!(
                "Unsupported binary builder op encoding {encoding} at index {index}"
            ));
        }

        let len = read_u32_le(payload, &mut offset, "op length")? as usize;
        let end = offset
            .checked_add(len)
            .ok_or_else(|| format!("Binary builder op {index} length overflow"))?;
        let bytes = payload
            .get(offset..end)
            .ok_or_else(|| format!("Binary builder ops payload too short for op {index}"))?;
        offset = end;

        let items: Vec<Value> = rmp_serde::from_slice(bytes).map_err(|e| e.to_string())?;
        ops.push(parse_compact_op(items)?);
    }

    if offset != payload.len() {
        return Err("Binary builder ops payload had trailing bytes".to_string());
    }

    Ok(ops)
}

pub fn builder_apply_ops(handle: &str, ops_payload: String) -> Result<String, String> {
    let ops = parse_builder_ops(&ops_payload)?;
    builder_apply_parsed_ops(handle, ops)
}

pub fn builder_apply_ops_bin(handle: &str, payload: Vec<u8>) -> Result<String, String> {
    let ops = parse_builder_ops_bin(&payload)?;
    builder_apply_parsed_ops(handle, ops)
}

fn apply_ops_to_parts(mut parts: QueryParts, ops: Vec<BuilderOp>) -> Result<QueryParts, String> {
    for op in ops {
        match op {
            BuilderOp::Distinct => {
                set_distinct_mode(&mut parts, DistinctMode::Distinct);
            }
            BuilderOp::DistinctOnColumns { cols } => {
                if cols.is_empty() {
                    return Err("distinctOn(...) requires at least one expression.".to_string());
                }
                let expressions = cols
                    .iter()
                    .map(|col| col_to_query(col))
                    .collect::<Result<Vec<_>, _>>()?;
                set_distinct_mode(&mut parts, DistinctMode::On(expressions));
            }
            BuilderOp::DistinctOnExprs { exprs } => {
                if exprs.is_empty() {
                    return Err("distinctOn(...) requires at least one expression.".to_string());
                }
                let dialect = active_dialect(&parts);
                let expressions = exprs
                    .into_iter()
                    .map(|input| input.eval(&dialect))
                    .collect::<Result<Vec<_>, _>>()?;
                set_distinct_mode(&mut parts, DistinctMode::On(expressions));
            }
            BuilderOp::Where { pred } | BuilderOp::AndWhere { pred } => {
                let dialect = active_dialect(&parts);
                let sql = pred.eval(&dialect, &mut parts.pending_warnings)?;
                push_clause(&mut parts.where_clauses, LogicalOp::And, sql);
            }
            BuilderOp::OrWhere { pred } => {
                let dialect = active_dialect(&parts);
                let sql = pred.eval(&dialect, &mut parts.pending_warnings)?;
                push_clause(&mut parts.where_clauses, LogicalOp::Or, sql);
            }
            BuilderOp::Having { pred } | BuilderOp::AndHaving { pred } => {
                let dialect = active_dialect(&parts);
                let sql = pred.eval(&dialect, &mut parts.pending_warnings)?;
                push_clause(&mut parts.having_clauses, LogicalOp::And, sql);
            }
            BuilderOp::OrHaving { pred } => {
                let dialect = active_dialect(&parts);
                let sql = pred.eval(&dialect, &mut parts.pending_warnings)?;
                push_clause(&mut parts.having_clauses, LogicalOp::Or, sql);
            }
            BuilderOp::On { pred } => {
                let dialect = active_dialect(&parts);
                let sql = pred.eval(&dialect, &mut parts.pending_warnings)?;
                parts = update_pending_join_predicate(parts, "AND", sql, false)?;
            }
            BuilderOp::AndOn { pred } => {
                let dialect = active_dialect(&parts);
                let sql = pred.eval(&dialect, &mut parts.pending_warnings)?;
                parts = update_pending_join_predicate(parts, "AND", sql, true)?;
            }
            BuilderOp::OrOn { pred } => {
                let dialect = active_dialect(&parts);
                let sql = pred.eval(&dialect, &mut parts.pending_warnings)?;
                parts = update_pending_join_predicate(parts, "OR", sql, true)?;
            }
            BuilderOp::UsingColumns { cols } => {
                parts = apply_using_columns(parts, cols)?;
            }
            BuilderOp::OnColumns { pairs } => {
                parts = apply_on_columns(parts, pairs)?;
            }
            BuilderOp::FromTable { table } => {
                apply_from_table(&mut parts, &table, None)?;
            }
            BuilderOp::FromTableAlias { table, alias } => {
                apply_from_table(&mut parts, &table, Some(&alias))?;
            }
            BuilderOp::JoinTable { join_type, table } => {
                parts = normalize_pending_join(parts)?;
                let src = source_fragment(&table, None)?;
                parts = apply_join(parts, &join_type, src)?;
            }
            BuilderOp::JoinTableAlias {
                join_type,
                table,
                alias,
            } => {
                parts = normalize_pending_join(parts)?;
                let src = source_fragment(&table, Some(&alias))?;
                parts = apply_join(parts, &join_type, src)?;
            }
            BuilderOp::WithHandle { name, rhs_handle } => {
                let rhs_query = get_or_build_query_arc(&rhs_handle)?;
                parts = apply_cte_query(parts, name, rhs_query.as_ref().clone(), false, vec![]);
            }
            BuilderOp::WithQuery { name, query } => {
                parts = apply_cte_query(parts, name, query, false, vec![]);
            }
            BuilderOp::WithRecursiveHandle { name, rhs_handle } => {
                let rhs_parts = registry_get_cloned(&rhs_handle)?;
                let columns = rhs_parts.selected_columns.clone();
                let rhs_query = get_or_build_query_arc(&rhs_handle)?;
                parts = apply_cte_query(parts, name, rhs_query.as_ref().clone(), true, columns);
            }
            BuilderOp::WithRecursiveQuery {
                name,
                query,
                columns,
            } => {
                parts = apply_cte_query(parts, name, query, true, columns);
            }
            BuilderOp::InsertInto { table } => {
                apply_insert_into(&mut parts, table);
            }
            BuilderOp::InsertColumns { cols } => {
                apply_insert_columns(&mut parts, cols);
            }
            BuilderOp::ValuesInsert { column_names, rows } => {
                apply_values_insert(&mut parts, InsertRowsInput { column_names, rows })?;
            }
            BuilderOp::Update { table } => {
                apply_update(&mut parts, table);
            }
            BuilderOp::DeleteFrom { table } => {
                apply_delete_from(&mut parts, table);
            }
            BuilderOp::Set { assignments } => {
                if assignments.is_empty() {
                    return Err("set() requires at least one column".to_string());
                }
                parts.update.assignments = assignments
                    .into_iter()
                    .map(|(col, val)| UpdateAssignment {
                        column: col,
                        value: val.into_assignment_value(),
                    })
                    .collect();
                parts.returning = None;
                parts.selected_columns = vec![];
            }
            BuilderOp::OnConflictColumns { cols } => {
                if cols.is_empty() {
                    return Err("onConflict(...) requires at least one column.".to_string());
                }
                if cols.iter().any(|c| c.is_empty()) {
                    return Err("onConflict(...) columns cannot be empty.".to_string());
                }
                apply_conflict_target(&mut parts, InsertConflictTarget::Columns(cols))?;
            }
            BuilderOp::OnConflictConstraint { name } => {
                if name.is_empty() {
                    return Err(
                        "onConflictOnConstraint(...) requires a non-empty constraint name."
                            .to_string(),
                    );
                }
                apply_conflict_target(&mut parts, InsertConflictTarget::Constraint(name))?;
            }
            BuilderOp::DoNothing => {
                parts.insert.conflict.action = Some(InsertConflictAction::DoNothing);
            }
            BuilderOp::DoUpdateSet { assignments } => {
                if assignments.is_empty() {
                    return Err("doUpdateSet(...) requires at least one column.".to_string());
                }
                let update_assignments: Vec<UpdateAssignment> = assignments
                    .into_iter()
                    .map(|(col, val)| UpdateAssignment {
                        column: col,
                        value: val.into_assignment_value(),
                    })
                    .collect();
                parts.insert.conflict.action = Some(InsertConflictAction::DoUpdate {
                    assignments: update_assignments,
                    predicate: None,
                });
            }
            BuilderOp::ConflictWhere { pred } => {
                let dialect = active_dialect(&parts);
                let predicate_sql = pred.eval(&dialect, &mut parts.pending_warnings)?;
                match parts.insert.conflict.action.take() {
                    Some(InsertConflictAction::DoUpdate { assignments, .. }) => {
                        parts.insert.conflict.action = Some(InsertConflictAction::DoUpdate {
                            assignments,
                            predicate: Some(predicate_sql),
                        });
                    }
                    Some(action) => {
                        parts.insert.conflict.action = Some(action);
                        return Err("conflictWhere() requires doUpdateSet(...) first.".to_string());
                    }
                    None => {
                        return Err("conflictWhere() requires doUpdateSet(...) first.".to_string());
                    }
                }
            }
            BuilderOp::SelectAliased { entries } => {
                let selected_cols: Vec<String> = entries.iter().map(|e| e.alias.clone()).collect();
                let dialect = active_dialect(&parts);
                let selections: Vec<SqlValue> = entries
                    .into_iter()
                    .map(|entry| {
                        let expr_query = eval_expr_for_dialect(entry.expr, &dialect)?;
                        if entry.bare {
                            // Plain column reference — render without AS alias.
                            Ok(SqlValue::Query(expr_query))
                        } else {
                            let alias_id = sql::identifier(vec![entry.alias])?;
                            sql::sql(
                                vec!["".to_string(), " AS ".to_string(), "".to_string()],
                                vec![SqlValue::Query(expr_query), SqlValue::Identifier(alias_id)],
                            )
                            .map(SqlValue::Query)
                        }
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let content = sql::join(selections, Some(", ".to_string()))?;
                apply_projection(&mut parts, content, selected_cols, ProjectionTarget::Select);
            }
            BuilderOp::SelectColumns { cols } => {
                let idents: Vec<SqlValue> = cols
                    .iter()
                    .map(|col| col_to_projection_value(col))
                    .collect::<Result<Vec<_>, _>>()?;
                let content = sql::join(idents, Some(", ".to_string()))?;
                apply_projection(
                    &mut parts,
                    content,
                    cols.iter().map(|c| col_to_local(c)).collect(),
                    ProjectionTarget::Select,
                );
            }
            BuilderOp::ReturningAliased { entries } => {
                let selected_cols: Vec<String> = entries.iter().map(|e| e.alias.clone()).collect();
                let dialect = active_dialect(&parts);
                let selections: Vec<SqlValue> = entries
                    .into_iter()
                    .map(|entry| {
                        let expr_query = eval_expr_for_dialect(entry.expr, &dialect)?;
                        if entry.bare {
                            Ok(SqlValue::Query(expr_query))
                        } else {
                            let alias_id = sql::identifier(vec![entry.alias])?;
                            sql::sql(
                                vec!["".to_string(), " AS ".to_string(), "".to_string()],
                                vec![SqlValue::Query(expr_query), SqlValue::Identifier(alias_id)],
                            )
                            .map(SqlValue::Query)
                        }
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let content = sql::join(selections, Some(", ".to_string()))?;
                apply_projection(
                    &mut parts,
                    content,
                    selected_cols,
                    ProjectionTarget::Returning,
                );
            }
            BuilderOp::ReturningColumns { cols } => {
                let idents: Vec<SqlValue> = cols
                    .iter()
                    .map(|col| sql::identifier(split_col(col)).map(SqlValue::Identifier))
                    .collect::<Result<Vec<_>, _>>()?;
                let content = sql::join(idents, Some(", ".to_string()))?;
                apply_projection(
                    &mut parts,
                    content,
                    cols.iter().map(|c| col_to_local(c)).collect(),
                    ProjectionTarget::Returning,
                );
            }
            BuilderOp::OrderBy {
                col,
                direction,
                null_order,
            } => {
                let dialect = parts.dialect.clone();
                let frag =
                    render_order_by(&col, direction.as_deref(), null_order.as_deref(), dialect)?;
                Arc::make_mut(&mut parts.order_by).push(frag);
            }
            BuilderOp::OrderByColumns { cols } => {
                let dialect = parts.dialect.clone();
                for col in &cols {
                    Arc::make_mut(&mut parts.order_by).push(render_order_by(
                        col,
                        None,
                        None,
                        dialect.clone(),
                    )?);
                }
            }
            BuilderOp::GroupBy { cols } => {
                let idents: Vec<SqlIdentifier> = cols
                    .iter()
                    .map(|c| sql::identifier(split_col(c)))
                    .collect::<Result<Vec<_>, _>>()?;
                Arc::make_mut(&mut parts.group_by).extend(idents);
            }
            BuilderOp::Limit { count } => {
                parts.pagination.limit = Some(count);
            }
            BuilderOp::Offset { count } => {
                parts.pagination.offset = Some(count);
            }
            BuilderOp::ForUpdate => {
                ensure_select_lock_builder(&parts)?;
                set_lock_strength(&mut parts, LockStrength::Update);
            }
            BuilderOp::ForShare => {
                ensure_select_lock_builder(&parts)?;
                set_lock_strength(&mut parts, LockStrength::Share);
            }
            BuilderOp::NoWait => {
                ensure_select_lock_builder(&parts)?;
                set_lock_modifier(&mut parts, LockModifier::NoWait)?;
            }
            BuilderOp::SkipLocked => {
                ensure_select_lock_builder(&parts)?;
                set_lock_modifier(&mut parts, LockModifier::SkipLocked)?;
            }
            // ── Phase 2: compound ops ─────────────────────────────────────────
            BuilderOp::UnionHandle { rhs_handle } => {
                let rhs_query = get_or_build_query_arc(&rhs_handle)?;
                parts = normalize_pending_join(parts)?;
                Arc::<Vec<CompoundPart>>::make_mut(&mut parts.compounds).push(CompoundPart {
                    operator: CompoundOperator::Union,
                    query: rhs_query,
                });
            }
            BuilderOp::Union { query } => {
                parts = normalize_pending_join(parts)?;
                Arc::<Vec<CompoundPart>>::make_mut(&mut parts.compounds).push(CompoundPart {
                    operator: CompoundOperator::Union,
                    query: Arc::new(query),
                });
            }
            BuilderOp::UnionAllHandle { rhs_handle } => {
                let rhs_query = get_or_build_query_arc(&rhs_handle)?;
                parts = normalize_pending_join(parts)?;
                Arc::<Vec<CompoundPart>>::make_mut(&mut parts.compounds).push(CompoundPart {
                    operator: CompoundOperator::UnionAll,
                    query: rhs_query,
                });
            }
            BuilderOp::UnionAll { query } => {
                parts = normalize_pending_join(parts)?;
                Arc::<Vec<CompoundPart>>::make_mut(&mut parts.compounds).push(CompoundPart {
                    operator: CompoundOperator::UnionAll,
                    query: Arc::new(query),
                });
            }
            BuilderOp::IntersectHandle { rhs_handle } => {
                let rhs_query = get_or_build_query_arc(&rhs_handle)?;
                parts = normalize_pending_join(parts)?;
                Arc::<Vec<CompoundPart>>::make_mut(&mut parts.compounds).push(CompoundPart {
                    operator: CompoundOperator::Intersect,
                    query: rhs_query,
                });
            }
            BuilderOp::Intersect { query } => {
                parts = normalize_pending_join(parts)?;
                Arc::<Vec<CompoundPart>>::make_mut(&mut parts.compounds).push(CompoundPart {
                    operator: CompoundOperator::Intersect,
                    query: Arc::new(query),
                });
            }
            BuilderOp::ExceptHandle { rhs_handle } => {
                let rhs_query = get_or_build_query_arc(&rhs_handle)?;
                parts = normalize_pending_join(parts)?;
                Arc::<Vec<CompoundPart>>::make_mut(&mut parts.compounds).push(CompoundPart {
                    operator: CompoundOperator::Except,
                    query: rhs_query,
                });
            }
            BuilderOp::Except { query } => {
                parts = normalize_pending_join(parts)?;
                Arc::<Vec<CompoundPart>>::make_mut(&mut parts.compounds).push(CompoundPart {
                    operator: CompoundOperator::Except,
                    query: Arc::new(query),
                });
            }
            // ── Phase 3: subquery source ops ──────────────────────────────────
            BuilderOp::FromSubqueryHandle { alias, rhs_handle } => {
                let rhs_query = get_or_build_query_arc(&rhs_handle)?;
                parts.from = Some(Arc::new(sql::sql(
                    vec!["FROM ".to_string(), "".to_string()],
                    vec![SqlValue::Query(subquery_source_fragment(
                        rhs_query.as_ref().clone(),
                        &alias,
                    )?)],
                )?));
                parts.statement = Some(StatementKind::Select);
                parts.selected_columns = vec![];
                parts.select = None;
                parts.returning = None;
                parts.distinct = None;
                parts.lock = None;
            }
            BuilderOp::FromSubquery { alias, query } => {
                parts.from = Some(Arc::new(sql::sql(
                    vec!["FROM ".to_string(), "".to_string()],
                    vec![SqlValue::Query(subquery_source_fragment(query, &alias)?)],
                )?));
                parts.statement = Some(StatementKind::Select);
                parts.selected_columns = vec![];
                parts.select = None;
                parts.returning = None;
                parts.distinct = None;
                parts.lock = None;
            }
            BuilderOp::JoinSubqueryHandle {
                join_type,
                alias,
                rhs_handle,
            } => {
                let rhs_query = get_or_build_query_arc(&rhs_handle)?;
                parts = normalize_pending_join(parts)?;
                let src = subquery_source_fragment(rhs_query.as_ref().clone(), &alias)?;
                parts = apply_join(parts, &join_type, src)?;
            }
            BuilderOp::JoinSubquery {
                join_type,
                alias,
                query,
            } => {
                parts = normalize_pending_join(parts)?;
                let src = subquery_source_fragment(query, &alias)?;
                parts = apply_join(parts, &join_type, src)?;
            }
            // ── Phase 4: INSERT … SELECT ──────────────────────────────────────
            BuilderOp::InsertSelectHandle { rhs_handle } => {
                let rhs_query = get_or_build_query_arc(&rhs_handle)?;
                parts.insert.select = Some(rhs_query.as_ref().clone());
                parts.insert.rows = vec![];
                parts.returning = None;
                parts.selected_columns = vec![];
            }
            BuilderOp::InsertSelect { query } => {
                parts.insert.select = Some(query);
                parts.insert.rows = vec![];
                parts.returning = None;
                parts.selected_columns = vec![];
            }
            // ── Inline child ops: single-FFI-crossing variants ────────────────
            BuilderOp::WithInline {
                name,
                base_handle,
                child_ops,
            } => {
                let base_parts = registry_get_cloned(&base_handle)?;
                let child_parts = apply_ops_to_parts(base_parts, child_ops)?;
                let rhs_query = build_query(&child_parts)?;
                parts = apply_cte_query(parts, name, rhs_query, false, vec![]);
            }
            BuilderOp::WithRecursiveInline {
                name,
                base_handle,
                child_ops,
            } => {
                let base_parts = registry_get_cloned(&base_handle)?;
                let child_parts = apply_ops_to_parts(base_parts, child_ops)?;
                let columns = child_parts.selected_columns.clone();
                let rhs_query = build_query(&child_parts)?;
                parts = apply_cte_query(parts, name, rhs_query, true, columns);
            }
            BuilderOp::FromSubqueryInline {
                alias,
                base_handle,
                child_ops,
            } => {
                let base_parts = registry_get_cloned(&base_handle)?;
                let child_parts = apply_ops_to_parts(base_parts, child_ops)?;
                let rhs_query = build_query(&child_parts)?;
                parts.from = Some(Arc::new(sql::sql(
                    vec!["FROM ".to_string(), "".to_string()],
                    vec![SqlValue::Query(subquery_source_fragment(
                        rhs_query, &alias,
                    )?)],
                )?));
                parts.statement = Some(StatementKind::Select);
                parts.selected_columns = vec![];
                parts.select = None;
                parts.returning = None;
                parts.distinct = None;
                parts.lock = None;
            }
            BuilderOp::JoinSubqueryInline {
                join_type,
                alias,
                base_handle,
                child_ops,
            } => {
                let base_parts = registry_get_cloned(&base_handle)?;
                let child_parts = apply_ops_to_parts(base_parts, child_ops)?;
                let rhs_query = build_query(&child_parts)?;
                parts = normalize_pending_join(parts)?;
                let src = subquery_source_fragment(rhs_query, &alias)?;
                parts = apply_join(parts, &join_type, src)?;
            }
            BuilderOp::UnionInline {
                base_handle,
                child_ops,
            } => {
                let base_parts = registry_get_cloned(&base_handle)?;
                let child_parts = apply_ops_to_parts(base_parts, child_ops)?;
                let rhs_query = build_query(&child_parts)?;
                parts = normalize_pending_join(parts)?;
                Arc::<Vec<CompoundPart>>::make_mut(&mut parts.compounds).push(CompoundPart {
                    operator: CompoundOperator::Union,
                    query: Arc::new(rhs_query),
                });
            }
            BuilderOp::UnionAllInline {
                base_handle,
                child_ops,
            } => {
                let base_parts = registry_get_cloned(&base_handle)?;
                let child_parts = apply_ops_to_parts(base_parts, child_ops)?;
                let rhs_query = build_query(&child_parts)?;
                parts = normalize_pending_join(parts)?;
                Arc::<Vec<CompoundPart>>::make_mut(&mut parts.compounds).push(CompoundPart {
                    operator: CompoundOperator::UnionAll,
                    query: Arc::new(rhs_query),
                });
            }
            BuilderOp::IntersectInline {
                base_handle,
                child_ops,
            } => {
                let base_parts = registry_get_cloned(&base_handle)?;
                let child_parts = apply_ops_to_parts(base_parts, child_ops)?;
                let rhs_query = build_query(&child_parts)?;
                parts = normalize_pending_join(parts)?;
                Arc::<Vec<CompoundPart>>::make_mut(&mut parts.compounds).push(CompoundPart {
                    operator: CompoundOperator::Intersect,
                    query: Arc::new(rhs_query),
                });
            }
            BuilderOp::ExceptInline {
                base_handle,
                child_ops,
            } => {
                let base_parts = registry_get_cloned(&base_handle)?;
                let child_parts = apply_ops_to_parts(base_parts, child_ops)?;
                let rhs_query = build_query(&child_parts)?;
                parts = normalize_pending_join(parts)?;
                Arc::<Vec<CompoundPart>>::make_mut(&mut parts.compounds).push(CompoundPart {
                    operator: CompoundOperator::Except,
                    query: Arc::new(rhs_query),
                });
            }
        }
    }

    Ok(parts)
}

fn builder_apply_parsed_ops(handle: &str, ops: Vec<BuilderOp>) -> Result<String, String> {
    let parts = registry_get_cloned(handle)?;
    let parts = apply_ops_to_parts(parts, ops)?;
    Ok(registry_insert(parts))
}

#[cfg(test)]
#[path = "methods.test.rs"]
mod methods_test;
