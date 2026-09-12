use super::{
    canonical::canonical_fingerprint_for_parts,
    registry::{
        registry_get_cloned, registry_get_cloned_key, registry_parse_handle,
        registry_set_cached_key, registry_set_structural_cached, registry_try_get_cached_key,
        registry_try_get_structural_cached,
    },
    AssignmentValue, DistinctMode, InsertConflictAction, InsertConflictTarget, InsertState,
    JoinType, QueryParts, SelectLockClause, StatementKind, UpdateAssignment, UpdateState,
    WhereEntry,
};
use crate::backend::backend_for_dialect;
use crate::dialect::{
    render_pagination, validate_query_parts, Dialect, InsertConflictStyle, RecursiveCteStyle,
};
use crate::error::QueryError;
use crate::expr_node::eval_expr_for_dialect_ctx;
use crate::sql;
use crate::types::{Primitive, RenderResult, SqlIdentifier, SqlQuery, SqlValue};
use crate::validation::validator_for_dialect;
use serde::Serialize;
use std::sync::Arc;

// ─── Pending join normalization ───────────────────────────────────────────────

/// Commit a pending join (with ON or USING) into the joins list.
///
/// When a dialect has been set on `parts`, this function validates the join
/// type and `USING` syntax against the dialect's [`JoinCapabilities`] before
/// rendering.  Unsupported combinations produce a deterministic error with the
/// dialect name included in the message.
///
/// # Errors
///
/// Returns an error string when:
/// - The pending join carries no predicate and no USING columns.
/// - The join type is not supported by the active dialect.
/// - USING syntax is not supported by the active dialect.
pub fn normalize_pending_join(mut parts: QueryParts) -> Result<QueryParts, String> {
    let pj = match parts.pending_join.take() {
        None => return Ok(parts),
        Some(pj) => pj,
    };

    if pj.predicate.is_none() && pj.using.is_empty() {
        return Err(
            "Join predicate is required. Call on() or using() before continuing the query."
                .to_string(),
        );
    }

    // Validate join type and USING support against dialect capabilities.
    {
        let caps = backend_for_dialect(&parts.dialect).capabilities();
        match pj.join_type {
            JoinType::Right if !caps.joins.right_join => {
                return Err(format!(
                    "RIGHT JOIN is not supported for the '{}' dialect",
                    parts.dialect
                ));
            }
            JoinType::FullOuter if !caps.joins.full_outer_join => {
                return Err(format!(
                    "FULL OUTER JOIN is not supported for the '{}' dialect",
                    parts.dialect
                ));
            }
            _ => {}
        }
        if !pj.using.is_empty() && !caps.joins.using_syntax {
            return Err(format!(
                "USING syntax is not supported for the '{}' dialect",
                parts.dialect
            ));
        }
    }

    let join_keyword = format!("{} ", pj.join_type.as_sql());
    let join_clause = if let Some(pred) = pj.predicate {
        sql::sql(
            vec![join_keyword, " ON ".to_string(), "".to_string()],
            vec![SqlValue::Query(pj.source), SqlValue::Query(pred)],
        )?
    } else {
        // USING
        let using_joined = sql::join(
            pj.using.into_iter().map(SqlValue::Identifier).collect(),
            Some(", ".to_string()),
        )?;
        sql::sql(
            vec![join_keyword, " USING (".to_string(), ")".to_string()],
            vec![SqlValue::Query(pj.source), SqlValue::Query(using_joined)],
        )?
    };

    Arc::make_mut(&mut parts.joins).push(join_clause);
    Ok(parts)
}

fn sql_value_contains_runtime_values(value: &SqlValue) -> bool {
    match value {
        SqlValue::Array(values) => !values.is_empty(),
        SqlValue::Query(query) => !query.values.is_empty(),
        SqlValue::Identifier(_) | SqlValue::Raw(_) => false,
        SqlValue::Primitive(_) => true,
    }
}

fn assignment_contains_runtime_values(value: &AssignmentValue) -> bool {
    match value {
        AssignmentValue::Sql(sql_value) => sql_value_contains_runtime_values(sql_value),
        AssignmentValue::Expr(_) => true,
    }
}

fn query_parts_contain_runtime_values(parts: &QueryParts) -> bool {
    let select_has_values = parts
        .select
        .as_ref()
        .is_some_and(|query| !query.values.is_empty());
    let from_has_values = parts
        .from
        .as_ref()
        .is_some_and(|query| !query.values.is_empty());
    let join_has_values = parts.joins.iter().any(|query| !query.values.is_empty());
    let pending_join_has_values = parts.pending_join.as_ref().is_some_and(|pending| {
        !pending.source.values.is_empty()
            || pending
                .predicate
                .as_ref()
                .is_some_and(|query| !query.values.is_empty())
    });
    let where_has_values = parts
        .where_clauses
        .iter()
        .any(|entry| !entry.pred.values.is_empty());
    let having_has_values = parts
        .having_clauses
        .iter()
        .any(|entry| !entry.pred.values.is_empty());
    let order_has_values = parts.order_by.iter().any(|query| !query.values.is_empty());
    let returning_has_values = parts
        .returning
        .as_ref()
        .is_some_and(|query| !query.values.is_empty());
    let cte_has_values = parts.ctes.iter().any(|cte| !cte.query.values.is_empty());
    let compound_has_values = parts
        .compounds
        .iter()
        .any(|part| !part.query.values.is_empty());
    let distinct_has_values = parts
        .distinct
        .as_ref()
        .is_some_and(|distinct| match distinct {
            DistinctMode::Distinct => false,
            DistinctMode::On(expressions) => {
                expressions.iter().any(|query| !query.values.is_empty())
            }
        });
    let insert_has_values = parts
        .insert
        .rows
        .iter()
        .flatten()
        .any(sql_value_contains_runtime_values)
        || parts
            .insert
            .select
            .as_ref()
            .is_some_and(|query| !query.values.is_empty())
        || parts
            .insert
            .conflict
            .action
            .as_ref()
            .is_some_and(|action| match action {
                InsertConflictAction::DoNothing => false,
                InsertConflictAction::DoUpdate {
                    assignments,
                    predicate,
                } => {
                    assignments
                        .iter()
                        .any(|assignment| assignment_contains_runtime_values(&assignment.value))
                        || predicate
                            .as_ref()
                            .is_some_and(|query| !query.values.is_empty())
                }
            });
    let update_has_values = parts
        .update
        .assignments
        .iter()
        .any(|assignment| assignment_contains_runtime_values(&assignment.value));

    select_has_values
        || from_has_values
        || join_has_values
        || pending_join_has_values
        || where_has_values
        || having_has_values
        || order_has_values
        || returning_has_values
        || cte_has_values
        || compound_has_values
        || distinct_has_values
        || insert_has_values
        || update_has_values
}

// ─── INSERT rendering ─────────────────────────────────────────────────────────

fn render_assignment_value(value: &AssignmentValue, dialect: &Dialect) -> Result<SqlValue, String> {
    let is_mysql_conflict = matches!(
        backend_for_dialect(dialect).capabilities().insert_conflict,
        InsertConflictStyle::MySql
    );
    match value {
        AssignmentValue::Sql(value) => Ok(value.clone()),
        AssignmentValue::Expr(expr) => match expr {
            crate::expr_node::ExprNode::Excluded { column } if is_mysql_conflict => {
                let column_id = sql::identifier(vec![column.clone()])?;
                sql::sql(
                    vec!["VALUES(".to_string(), ")".to_string()],
                    vec![SqlValue::Identifier(column_id)],
                )
                .map(SqlValue::Query)
            }
            _ => eval_expr_for_dialect_ctx(expr.clone(), dialect, &mut None).map(SqlValue::Query),
        },
    }
}

fn render_assignments(
    assignments: &[UpdateAssignment],
    dialect: &Dialect,
) -> Result<SqlQuery, String> {
    let assignment_sqls: Vec<SqlValue> = assignments
        .iter()
        .map(|a| {
            let col_id = sql::identifier(vec![a.column.clone()])?;
            sql::sql(
                vec!["".to_string(), " = ".to_string(), "".to_string()],
                vec![
                    SqlValue::Identifier(col_id),
                    render_assignment_value(&a.value, dialect)?,
                ],
            )
            .map(SqlValue::Query)
        })
        .collect::<Result<Vec<_>, _>>()?;

    sql::join(assignment_sqls, Some(", ".to_string()))
}

fn render_insert_conflict(
    insert: &InsertState,
    dialect: &Dialect,
) -> Result<Option<SqlQuery>, String> {
    let action = match insert.conflict.action {
        None => return Ok(None),
        Some(ref action) => action,
    };

    let target = match &insert.conflict.target {
        None => None,
        Some(InsertConflictTarget::Columns(columns)) => {
            let col_idents: Vec<SqlValue> = columns
                .iter()
                .map(|column| sql::identifier(vec![column.clone()]).map(SqlValue::Identifier))
                .collect::<Result<Vec<_>, _>>()?;
            let joined = sql::join(col_idents, Some(", ".to_string()))?;
            Some(sql::sql(
                vec!["(".to_string(), ")".to_string()],
                vec![SqlValue::Query(joined)],
            )?)
        }
        Some(InsertConflictTarget::Constraint(constraint)) => {
            let constraint_id = sql::identifier(vec![constraint.clone()])?;
            Some(sql::sql(
                vec!["ON CONSTRAINT ".to_string(), "".to_string()],
                vec![SqlValue::Identifier(constraint_id)],
            )?)
        }
    };

    match action {
        InsertConflictAction::DoNothing => match target {
            Some(target) => sql::sql(
                vec!["ON CONFLICT ".to_string(), " DO NOTHING".to_string()],
                vec![SqlValue::Query(target)],
            )
            .map(Some),
            None => sql::sql(vec!["ON CONFLICT DO NOTHING".to_string()], vec![]).map(Some),
        },
        InsertConflictAction::DoUpdate {
            assignments,
            predicate,
        } => {
            let assignments_sql = render_assignments(assignments, dialect)?;
            let conflict_style = backend_for_dialect(dialect).capabilities().insert_conflict;

            if matches!(conflict_style, InsertConflictStyle::MySql) {
                return sql::sql(
                    vec!["ON DUPLICATE KEY UPDATE ".to_string(), "".to_string()],
                    vec![SqlValue::Query(assignments_sql)],
                )
                .map(Some);
            }

            let target_sql = target.ok_or("Insert conflict update requires a conflict target.")?;
            let mut conflict = sql::sql(
                vec![
                    "ON CONFLICT ".to_string(),
                    " DO UPDATE SET ".to_string(),
                    "".to_string(),
                ],
                vec![
                    SqlValue::Query(target_sql),
                    SqlValue::Query(assignments_sql),
                ],
            )?;

            if let Some(predicate) = predicate {
                buf_push(&mut conflict, "WHERE ", predicate);
            }

            Ok(Some(conflict))
        }
    }
}

fn build_insert_query(insert: &InsertState, dialect: &Dialect) -> Result<SqlQuery, String> {
    let capabilities = backend_for_dialect(dialect).capabilities();
    let table = insert
        .into
        .as_deref()
        .ok_or("Cannot build INSERT query without a target table")?;

    let table_id = table_identifier(table)?;

    let column_fragment = if !insert.column_names.is_empty() {
        let col_idents: Vec<SqlValue> = insert
            .column_names
            .iter()
            .map(|c| sql::identifier(vec![c.clone()]).map(SqlValue::Identifier))
            .collect::<Result<Vec<_>, _>>()?;
        let joined = sql::join(col_idents, Some(", ".to_string()))?;
        sql::sql(
            vec![" (".to_string(), ")".to_string()],
            vec![SqlValue::Query(joined)],
        )?
    } else {
        SqlQuery::default()
    };

    let insert_keyword = if matches!(capabilities.insert_conflict, InsertConflictStyle::MySql)
        && matches!(
            insert.conflict.action,
            Some(InsertConflictAction::DoNothing)
        )
        && insert.conflict.target.is_none()
    {
        "INSERT IGNORE INTO "
    } else {
        "INSERT INTO "
    };

    if let Some(ref sel) = insert.select {
        let mut query = sql::sql(
            vec![
                insert_keyword.to_string(),
                "".to_string(),
                " ".to_string(),
                "".to_string(),
            ],
            vec![
                SqlValue::Identifier(table_id),
                SqlValue::Query(column_fragment),
                SqlValue::Query(sel.clone()),
            ],
        )?;
        if !matches!(capabilities.insert_conflict, InsertConflictStyle::MySql)
            || matches!(
                insert.conflict.action,
                Some(InsertConflictAction::DoUpdate { .. })
            )
        {
            if let Some(conflict) = render_insert_conflict(insert, dialect)? {
                buf_push(&mut query, "", &conflict);
            }
        }
        return Ok(query);
    }

    if insert.rows.is_empty() {
        return Err("Cannot build INSERT query without values or select()".to_string());
    }

    let row_sqls: Vec<SqlValue> = insert
        .rows
        .iter()
        .map(|row| {
            let vals: Vec<SqlValue> = row
                .iter()
                .map(|v| sql::to_query_fragment(v).map(SqlValue::Query))
                .collect::<Result<Vec<_>, _>>()?;
            let joined = sql::join(vals, Some(", ".to_string()))?;
            sql::sql(
                vec!["(".to_string(), ")".to_string()],
                vec![SqlValue::Query(joined)],
            )
            .map(SqlValue::Query)
        })
        .collect::<Result<Vec<_>, _>>()?;

    let values_list = sql::join(row_sqls, Some(", ".to_string()))?;
    let mut query = sql::sql(
        vec![
            insert_keyword.to_string(),
            "".to_string(),
            " VALUES ".to_string(),
            "".to_string(),
        ],
        vec![
            SqlValue::Identifier(table_id),
            SqlValue::Query(column_fragment),
            SqlValue::Query(values_list),
        ],
    )?;
    if !matches!(capabilities.insert_conflict, InsertConflictStyle::MySql)
        || matches!(
            insert.conflict.action,
            Some(InsertConflictAction::DoUpdate { .. })
        )
    {
        if let Some(conflict) = render_insert_conflict(insert, dialect)? {
            buf_push(&mut query, "", &conflict);
        }
    }
    Ok(query)
}

// ─── UPDATE rendering ──────────────────────────────────────────────────────────

fn build_update_query(
    update: &UpdateState,
    where_clauses: &[WhereEntry],
) -> Result<SqlQuery, String> {
    let table = update
        .table
        .as_deref()
        .ok_or("Cannot build UPDATE query without a target table")?;

    if update.assignments.is_empty() {
        return Err("Cannot build UPDATE query without set()".to_string());
    }

    let table_id = table_identifier(table)?;

    let assignments_joined = render_assignments(&update.assignments, &Dialect::Postgres)?;

    let mut clauses: Vec<SqlValue> = vec![sql::sql(
        vec!["UPDATE ".to_string(), " SET ".to_string(), "".to_string()],
        vec![
            SqlValue::Identifier(table_id),
            SqlValue::Query(assignments_joined),
        ],
    )
    .map(SqlValue::Query)?];

    if let Some(ref w) = render_clause_list(where_clauses)? {
        clauses.push(SqlValue::Query(sql::sql(
            vec!["WHERE ".to_string(), "".to_string()],
            vec![SqlValue::Query(w.clone())],
        )?));
    }

    sql::join(clauses, Some(" ".to_string()))
}

// ─── DELETE rendering ──────────────────────────────────────────────────────────

fn build_delete_query(from_table: &str, where_clauses: &[WhereEntry]) -> Result<SqlQuery, String> {
    let table_id = table_identifier(from_table)?;
    let mut clauses: Vec<SqlValue> = vec![sql::sql(
        vec!["DELETE FROM ".to_string(), "".to_string()],
        vec![SqlValue::Identifier(table_id)],
    )
    .map(SqlValue::Query)?];

    if let Some(ref w) = render_clause_list(where_clauses)? {
        clauses.push(SqlValue::Query(sql::sql(
            vec!["WHERE ".to_string(), "".to_string()],
            vec![SqlValue::Query(w.clone())],
        )?));
    }

    sql::join(clauses, Some(" ".to_string()))
}

fn table_identifier(table: &str) -> Result<SqlIdentifier, String> {
    sql::identifier(table.split('.').map(str::to_string).collect())
}

// ─── SELECT base query rendering ──────────────────────────────────────────────

/// Write a SQL fragment into `buf`, inserting a space separator if the buffer
/// is non-empty.  Avoids any intermediate `Vec<SqlValue>` + `sql::join` pass.
fn buf_push(buf: &mut SqlQuery, prefix: &str, frag: &SqlQuery) {
    if !buf.text.is_empty() {
        buf.text.push(' ');
        buf.raw.push(' ');
    }
    buf.text.push_str(prefix);
    buf.raw.push_str(prefix);
    buf.text.push_str(&frag.text);
    buf.raw.push_str(&frag.raw);
    buf.values.extend_from_slice(&frag.values);
}

/// Render a flat list of WHERE/HAVING predicates into a single composed SqlQuery.
///
/// Produces the same parenthesisation as the previous nested-compose approach:
/// - one clause  → `pred`
/// - two clauses → `(pred1) OP (pred2)`
/// - three       → `((pred1) OP1 (pred2)) OP2 (pred3)`
fn render_clause_list(clauses: &[WhereEntry]) -> Result<Option<SqlQuery>, String> {
    let mut iter = clauses.iter();
    let first = match iter.next() {
        None => return Ok(None),
        Some(e) => e,
    };
    let mut result = first.pred.clone();
    for entry in iter {
        result = sql::sql(
            vec![
                "(".to_string(),
                format!(") {} (", entry.op.as_str()),
                ")".to_string(),
            ],
            vec![SqlValue::Query(result), SqlValue::Query(entry.pred.clone())],
        )?;
    }
    Ok(Some(result))
}

fn render_select_lock_clause(lock: SelectLockClause) -> SqlQuery {
    let mut raw = lock.strength.as_sql().to_string();
    if let Some(modifier) = lock.modifier {
        raw.push(' ');
        raw.push_str(modifier.as_sql());
    }

    SqlQuery {
        text: raw.clone(),
        raw,
        values: vec![],
    }
}

fn build_prefixed_query(prefix: &str, middle: Option<&SqlQuery>, tail: &SqlQuery) -> SqlQuery {
    let middle_text_len = middle.map_or(0, |query| query.text.len());
    let middle_raw_len = middle.map_or(0, |query| query.raw.len());
    let separator_len = usize::from(middle.is_some());
    let middle_values_len = middle.map_or(0, |query| query.values.len());

    let mut text =
        String::with_capacity(prefix.len() + middle_text_len + separator_len + tail.text.len());
    let mut raw =
        String::with_capacity(prefix.len() + middle_raw_len + separator_len + tail.raw.len());
    let mut values = Vec::with_capacity(middle_values_len + tail.values.len());

    text.push_str(prefix);
    raw.push_str(prefix);

    if let Some(query) = middle {
        text.push_str(&query.text);
        raw.push_str(&query.raw);
        text.push(' ');
        raw.push(' ');
        values.extend_from_slice(&query.values);
    }

    text.push_str(&tail.text);
    raw.push_str(&tail.raw);
    values.extend_from_slice(&tail.values);

    SqlQuery { text, raw, values }
}

fn build_select_projection(
    parts: &QueryParts,
    select_prefix: Option<&SqlQuery>,
) -> Result<SqlQuery, String> {
    let select = parts
        .select
        .as_ref()
        .ok_or_else(|| "Cannot build SELECT query without a projection".to_string())?;

    match &parts.distinct {
        None => Ok(build_prefixed_query("SELECT ", select_prefix, select)),
        Some(DistinctMode::Distinct) => Ok(build_prefixed_query(
            "SELECT DISTINCT ",
            select_prefix,
            select,
        )),
        Some(DistinctMode::On(expressions)) => {
            let distinct_exprs = sql::join(
                expressions.iter().cloned().map(SqlValue::Query).collect(),
                Some(", ".to_string()),
            )?;
            sql::sql(
                vec![
                    "SELECT DISTINCT ON (".to_string(),
                    ") ".to_string(),
                    "".to_string(),
                ],
                vec![
                    SqlValue::Query(distinct_exprs),
                    SqlValue::Query((**select).clone()),
                ],
            )
        }
    }
}

fn build_base_query(parts: &QueryParts) -> Result<SqlQuery, String> {
    let mut buf = SqlQuery {
        text: String::with_capacity(256),
        raw: String::with_capacity(256),
        values: Vec::new(),
    };
    let pagination = render_pagination(&parts.dialect, &parts.pagination)?;

    // SELECT [DISTINCT] <content>
    let select_projection = build_select_projection(parts, pagination.select_prefix.as_ref())?;
    buf_push(&mut buf, "", &select_projection);

    if let Some(ref from) = parts.from {
        buf_push(&mut buf, "", from);
    }

    for join in parts.joins.iter() {
        buf_push(&mut buf, "", join);
    }

    if !parts.where_clauses.is_empty() {
        if let Some(ref w) = render_clause_list(&parts.where_clauses)? {
            buf_push(&mut buf, "WHERE ", w);
        }
    }

    if !parts.group_by.is_empty() {
        let gb_idents: Vec<SqlValue> = parts
            .group_by
            .iter()
            .map(|id| SqlValue::Identifier(id.clone()))
            .collect();
        let gb_joined = sql::join(gb_idents, Some(", ".to_string()))?;
        buf_push(&mut buf, "GROUP BY ", &gb_joined);
    }

    if !parts.having_clauses.is_empty() {
        if let Some(ref h) = render_clause_list(&parts.having_clauses)? {
            buf_push(&mut buf, "HAVING ", h);
        }
    }

    if !parts.order_by.is_empty() {
        let ob: Vec<SqlValue> = parts
            .order_by
            .iter()
            .map(|q| SqlValue::Query(q.clone()))
            .collect();
        let ob_joined = sql::join(ob, Some(", ".to_string()))?;
        buf_push(&mut buf, "ORDER BY ", &ob_joined);
    }

    if let Some(ref trailing_clause) = pagination.trailing_clause {
        buf_push(&mut buf, "", trailing_clause);
    }

    if let Some(lock) = parts.lock {
        let lock_clause = render_select_lock_clause(lock);
        buf_push(&mut buf, "", &lock_clause);
    }

    Ok(buf)
}

fn validate_query_cached(parts: &QueryParts) -> Result<(), String> {
    parts
        .validation_cache
        .get_or_try_init(|| validate_query_uncached(parts))
}

fn validate_query_uncached(parts: &QueryParts) -> Result<(), String> {
    #[cfg(test)]
    render_test::record_validation_execution();

    if let Err(errs) = validator_for_dialect(&parts.dialect).validate(parts) {
        let payload = serde_json::json!({
            "kind": "ValidationError",
            "errors": errs
        });
        return Err(payload.to_string());
    }

    let normalized;
    let parts: &QueryParts = if parts.pending_join.is_some() {
        normalized = normalize_pending_join(parts.clone())?;
        &normalized
    } else {
        parts
    };

    validate_query_parts(parts)
}

// ─── Full query build ──────────────────────────────────────────────────────────

pub fn build_query(parts: &QueryParts) -> Result<SqlQuery, String> {
    validate_query_cached(parts)?;

    let normalized;
    let parts: &QueryParts = if parts.pending_join.is_some() {
        normalized = normalize_pending_join(parts.clone())?;
        &normalized
    } else {
        parts
    };

    // Strict dialect mode: pending warnings from builder-time expression rewrites
    // (e.g. ILIKE → LOWER LIKE LOWER on MySQL) are promoted to hard errors.
    if parts.dialect_strict {
        if let Some(w) = parts.pending_warnings.first() {
            return Err(format!(
                "Dialect \"{}\" does not support {} (strict mode): {}",
                w.dialect, w.feature, w.message
            ));
        }
    }

    let mut buf = SqlQuery {
        text: String::with_capacity(512),
        raw: String::with_capacity(512),
        values: Vec::new(),
    };

    // CTEs
    if !parts.ctes.is_empty() {
        let has_recursive = parts.ctes.iter().any(|c| c.recursive);
        let dialect = parts.dialect.clone();
        let recursive_ctes = backend_for_dialect(&dialect).capabilities().recursive_ctes;
        let keyword = match (has_recursive, recursive_ctes.style) {
            (true, RecursiveCteStyle::WithRecursiveKeyword) => "WITH RECURSIVE ",
            _ => "WITH ",
        };
        let cte_sqls: Vec<SqlValue> = parts
            .ctes
            .iter()
            .map(|cte| {
                let mut name_query =
                    sql::to_query_fragment(&SqlValue::Identifier(sql::identifier(vec![cte
                        .name
                        .clone()])?))?;

                if cte.recursive
                    && recursive_ctes.column_aliases_required
                    && !cte.columns.is_empty()
                {
                    let column_idents: Vec<SqlValue> = cte
                        .columns
                        .iter()
                        .map(|column| {
                            sql::identifier(vec![column.clone()]).map(SqlValue::Identifier)
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    let columns = sql::join(column_idents, Some(", ".to_string()))?;
                    buf_push(&mut name_query, "(", &columns);
                    name_query.text.push(')');
                    name_query.raw.push(')');
                }

                sql::sql(
                    vec!["".to_string(), " AS (".to_string(), ")".to_string()],
                    vec![
                        SqlValue::Query(name_query),
                        SqlValue::Query(cte.query.clone()),
                    ],
                )
                .map(SqlValue::Query)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let ctes_joined = sql::join(cte_sqls, Some(", ".to_string()))?;
        buf_push(&mut buf, keyword, &ctes_joined);
    }

    // Statement body
    let mut body = match parts.statement.as_ref().unwrap_or(&StatementKind::Select) {
        StatementKind::Insert => build_insert_query(&parts.insert, &parts.dialect)?,
        StatementKind::Update => build_update_query(&parts.update, &parts.where_clauses)?,
        StatementKind::Delete => {
            let table = parts
                .delete
                .from_table
                .as_deref()
                .ok_or("Cannot build DELETE query without a target table")?;
            build_delete_query(table, &parts.where_clauses)?
        }
        StatementKind::Select => build_base_query(parts)?,
    };
    if let Some(ref returning) = parts.returning {
        buf_push(&mut body, "RETURNING ", returning);
    }
    buf_push(&mut buf, "", &body);

    // Compound operators (UNION, INTERSECT, EXCEPT)
    for compound in parts.compounds.iter() {
        if !buf.text.is_empty() {
            buf.text.push(' ');
            buf.raw.push(' ');
        }
        buf.text.push_str(compound.operator.as_sql());
        buf.raw.push_str(compound.operator.as_sql());
        buf.text.push_str(" (");
        buf.raw.push_str(" (");
        let query = compound.query.as_ref();
        buf.text.push_str(&query.text);
        buf.raw.push_str(&query.raw);
        buf.values.extend_from_slice(&query.values);
        buf.text.push(')');
        buf.raw.push(')');
    }

    Ok(buf)
}

// ─── Output functions ─────────────────────────────────────────────────────────

fn get_or_build_query_arc_internal(
    handle: &str,
    use_structural_cache: bool,
) -> Result<Arc<SqlQuery>, String> {
    let key = registry_parse_handle(handle)?;

    #[cfg(test)]
    render_test::record_cache_lookup(key);

    if let Some(cached) = registry_try_get_cached_key(key) {
        return Ok(cached);
    }

    let parts = registry_get_cloned_key(key)?;

    if use_structural_cache && !query_parts_contain_runtime_values(&parts) {
        let canonical_fingerprint = canonical_fingerprint_for_parts(&parts).ok();

        if let Some(canonical_fingerprint) = canonical_fingerprint {
            if let Some(shared_cached) = registry_try_get_structural_cached(canonical_fingerprint) {
                registry_set_cached_key(key, Arc::clone(&shared_cached));
                return Ok(shared_cached);
            }

            let query = build_and_cache_query_arc(key, &parts)?;
            registry_set_structural_cached(canonical_fingerprint, Arc::clone(&query));
            return Ok(query);
        }

        return build_and_cache_query_arc(key, &parts);
    }

    build_and_cache_query_arc(key, &parts)
}

pub(crate) fn get_or_build_query_arc(handle: &str) -> Result<Arc<SqlQuery>, String> {
    get_or_build_query_arc_internal(handle, false)
}

fn build_and_cache_query_arc(key: u64, parts: &QueryParts) -> Result<Arc<SqlQuery>, String> {
    #[cfg(test)]
    render_test::record_build_execution(key);

    let query = Arc::new(build_query(parts)?);
    registry_set_cached_key(key, Arc::clone(&query));
    Ok(query)
}

#[derive(Serialize, Clone)]
pub struct CompiledBundle {
    pub text: String,
    pub raw: String,
    pub values: Vec<Primitive>,
}

pub fn builder_compile_bundle_typed(handle: &str) -> Result<CompiledBundle, String> {
    let query = get_or_build_query_arc(handle)?;
    Ok(CompiledBundle {
        text: query.text.clone(),
        raw: query.raw.clone(),
        values: query.values.clone(),
    })
}

pub fn builder_compile_bundle(handle: &str) -> Result<String, String> {
    serde_json::to_string(&builder_compile_bundle_typed(handle)?).map_err(|e| e.to_string())
}

pub fn builder_query(handle: &str) -> Result<String, String> {
    let query = get_or_build_query_arc(handle)?;
    serde_json::to_string(query.as_ref()).map_err(|e| e.to_string())
}

pub fn builder_text(handle: &str) -> Result<String, String> {
    Ok(get_or_build_query_arc(handle)?.text.clone())
}

pub fn builder_raw(handle: &str) -> Result<String, String> {
    Ok(get_or_build_query_arc(handle)?.raw.clone())
}

pub fn builder_values(handle: &str) -> Result<String, String> {
    let query = get_or_build_query_arc(handle)?;
    serde_json::to_string(&query.values).map_err(|e| e.to_string())
}

pub fn builder_selected_columns(handle: &str) -> Result<String, String> {
    let parts = registry_get_cloned(handle)?;
    serde_json::to_string(&parts.selected_columns).map_err(|e| e.to_string())
}

pub fn builder_insert_columns(handle: &str) -> Result<String, String> {
    let parts = registry_get_cloned(handle)?;
    serde_json::to_string(&parts.insert.column_names).map_err(|e| e.to_string())
}

/// Build the query and wrap it as an AliasedQuery JSON object.
pub fn builder_as(handle: &str, alias: String) -> Result<String, String> {
    let query = get_or_build_query_arc(handle)?;
    let parts = registry_get_cloned(handle)?;
    let selected_columns = parts.selected_columns;

    #[derive(Serialize)]
    struct AliasedQuery<'a> {
        #[serde(rename = "__kind")]
        kind: &'static str,
        alias: &'a str,
        #[serde(rename = "__handle")]
        handle: &'a str,
        query: &'a SqlQuery,
        text: &'a str,
        raw: &'a str,
        values: &'a [Primitive],
        #[serde(rename = "selectedColumns")]
        selected_columns: &'a [String],
    }

    let obj = AliasedQuery {
        kind: "aliased-query",
        alias: &alias,
        handle,
        query: &query,
        text: &query.text,
        raw: &query.raw,
        values: &query.values,
        selected_columns: &selected_columns,
    };

    serde_json::to_string(&obj).map_err(|e| e.to_string())
}

// ─── Typed output helpers (no JSON strings cross the FFI boundary) ────────────

/// Typed output of [`builder_as`] — same shape, owned and serializable.
#[derive(Clone, Serialize)]
pub struct AliasedQueryData {
    #[serde(rename = "__kind")]
    pub kind: &'static str,
    pub alias: String,
    #[serde(rename = "__handle")]
    pub handle: String,
    pub query: SqlQuery,
    pub text: String,
    pub raw: String,
    pub values: Vec<Primitive>,
    #[serde(rename = "selectedColumns")]
    pub selected_columns: Vec<String>,
}

pub fn builder_query_typed(handle: &str) -> Result<SqlQuery, String> {
    Ok(get_or_build_query_arc(handle)?.as_ref().clone())
}

pub fn builder_values_typed(handle: &str) -> Result<Vec<Primitive>, String> {
    Ok(get_or_build_query_arc(handle)?.values.clone())
}

pub fn builder_selected_columns_typed(handle: &str) -> Result<Vec<String>, String> {
    Ok(registry_get_cloned(handle)?.selected_columns.to_vec())
}

pub fn builder_insert_columns_typed(handle: &str) -> Result<Vec<String>, String> {
    Ok(registry_get_cloned(handle)?.insert.column_names.to_vec())
}

pub fn builder_conflict_target_kind_typed(handle: &str) -> Result<String, String> {
    let parts = registry_get_cloned(handle)?;
    let kind = match parts.insert.conflict.target {
        Some(crate::builder::InsertConflictTarget::Columns(_)) => "columns",
        Some(crate::builder::InsertConflictTarget::Constraint(_)) => "constraint",
        None => "none",
    };
    Ok(kind.to_string())
}

pub fn builder_as_typed(handle: &str, alias: String) -> Result<AliasedQueryData, String> {
    let query = get_or_build_query_arc(handle)?;
    let parts = registry_get_cloned(handle)?;
    Ok(AliasedQueryData {
        kind: "aliased-query",
        text: query.text.clone(),
        raw: query.raw.clone(),
        values: query.values.clone(),
        query: query.as_ref().clone(),
        alias,
        handle: handle.to_string(),
        selected_columns: parts.selected_columns.to_vec(),
    })
}

// ─── Context-aware rendering ──────────────────────────────────────────────────

/// Build a query and return it together with any dialect-rewrite warnings.
///
/// Unlike [`build_query`], which only returns the [`SqlQuery`], this function
/// surfaces the [`DialectWarning`]s collected during builder-method evaluation
/// (e.g. ILIKE → LOWER LIKE LOWER rewrites on MySQL).
///
/// # Errors
///
/// Returns [`QueryError::HardError`] when `parts.dialect_strict` is set and
/// one or more pending warnings exist.  Returns [`QueryError::Validation`] for
/// all other build failures.
pub fn build_query_with_context(parts: &QueryParts) -> Result<RenderResult, QueryError> {
    let query = build_query(parts).map_err(QueryError::from)?;
    Ok(RenderResult {
        query,
        warnings: parts.pending_warnings.clone(),
    })
}

/// Retrieve a cloned [`QueryParts`] snapshot from the registry by handle.
///
/// Useful in tests and higher-level bindings that need access to the raw query
/// state (e.g. pending warnings, dialect settings) before rendering.
///
/// # Errors
///
/// Returns an error string when `handle` does not exist in the registry.
pub fn registry_get_parts(handle: &str) -> Result<QueryParts, String> {
    registry_get_cloned(handle)
}

#[cfg(test)]
pub(crate) use render_test::{
    get_or_build_query_arc_with_structural_cache, test_build_execution_count_for_handle,
    test_cache_lookup_count_for_handle, test_reset_build_execution_count,
};

#[cfg(test)]
#[path = "render.test.rs"]
mod render_test;
