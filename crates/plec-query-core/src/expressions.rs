use crate::dialect::{
    function_capability_for, normalize_function_name, normalize_operator, operator_capability_for,
    Dialect, OperatorKind,
};
use crate::sql;
use crate::types::{SqlQuery, SqlValue, WindowOrderItem};

// ─── Helper ──────────────────────────────────────────────────────────────────

/// Wrap a SqlValue in a single-expression sql template: `(value)`.
fn wrap(value: SqlValue) -> Result<SqlQuery, String> {
    sql::sql(vec!["(".to_string(), ")".to_string()], vec![value])
}

// ─── Comparison ──────────────────────────────────────────────────────────────

pub fn cmp(left: SqlValue, operator: String, right: SqlValue) -> Result<SqlQuery, String> {
    sql::sql(
        vec![
            "".to_string(),
            " ".to_string() + &operator + " ",
            "".to_string(),
        ],
        vec![left, right],
    )
}

pub fn cmp_for_dialect(
    left: SqlValue,
    operator: String,
    right: SqlValue,
    dialect: &Dialect,
) -> Result<SqlQuery, String> {
    let normalized = normalize_operator(&operator);
    if operator_capability_for(dialect, OperatorKind::Comparison, &normalized).is_none() {
        return Err(format!(
            "Dialect \"{}\" does not support operator \"{}\" in this builder.",
            dialect, normalized
        ));
    }

    cmp(left, normalized, right)
}

pub fn is_null(value: SqlValue) -> Result<SqlQuery, String> {
    sql::sql(vec!["".to_string(), " IS NULL".to_string()], vec![value])
}

pub fn is_not_null(value: SqlValue) -> Result<SqlQuery, String> {
    sql::sql(
        vec!["".to_string(), " IS NOT NULL".to_string()],
        vec![value],
    )
}

// ─── List / range ─────────────────────────────────────────────────────────────

pub fn in_array(value: SqlValue, items: Vec<SqlValue>) -> Result<SqlQuery, String> {
    let list = sql::join(items, Some(", ".to_string()))?;
    sql::sql(
        vec!["".to_string(), " IN (".to_string(), ")".to_string()],
        vec![value, SqlValue::Query(list)],
    )
}

pub fn not_in_array(value: SqlValue, items: Vec<SqlValue>) -> Result<SqlQuery, String> {
    let list = sql::join(items, Some(", ".to_string()))?;
    sql::sql(
        vec!["".to_string(), " NOT IN (".to_string(), ")".to_string()],
        vec![value, SqlValue::Query(list)],
    )
}

pub fn between(value: SqlValue, lower: SqlValue, upper: SqlValue) -> Result<SqlQuery, String> {
    sql::sql(
        vec![
            "".to_string(),
            " BETWEEN ".to_string(),
            " AND ".to_string(),
            "".to_string(),
        ],
        vec![value, lower, upper],
    )
}

pub fn not_between(value: SqlValue, lower: SqlValue, upper: SqlValue) -> Result<SqlQuery, String> {
    sql::sql(
        vec![
            "".to_string(),
            " NOT BETWEEN ".to_string(),
            " AND ".to_string(),
            "".to_string(),
        ],
        vec![value, lower, upper],
    )
}

// ─── Pattern matching ────────────────────────────────────────────────────────

pub fn like_sql(value: SqlValue, pattern: SqlValue) -> Result<SqlQuery, String> {
    sql::sql(
        vec!["".to_string(), " LIKE ".to_string(), "".to_string()],
        vec![value, pattern],
    )
}

pub fn not_like_sql(value: SqlValue, pattern: SqlValue) -> Result<SqlQuery, String> {
    sql::sql(
        vec!["".to_string(), " NOT LIKE ".to_string(), "".to_string()],
        vec![value, pattern],
    )
}

/// Renders `value ILIKE pattern` for dialects that support native `ILIKE`.
///
/// For dialects that do not support `ILIKE`, use [`ilike_rewrite_sql`] instead.
pub fn ilike_sql(value: SqlValue, pattern: SqlValue) -> Result<SqlQuery, String> {
    sql::sql(
        vec!["".to_string(), " ILIKE ".to_string(), "".to_string()],
        vec![value, pattern],
    )
}

pub fn not_ilike_sql(value: SqlValue, pattern: SqlValue) -> Result<SqlQuery, String> {
    sql::sql(
        vec!["".to_string(), " NOT ILIKE ".to_string(), "".to_string()],
        vec![value, pattern],
    )
}

/// Renders the ILIKE fallback rewrite: `LOWER(value) LIKE LOWER(pattern)`.
///
/// Used when the active dialect does not support native `ILIKE`.
pub fn ilike_rewrite_sql(value: SqlValue, pattern: SqlValue) -> Result<SqlQuery, String> {
    // LOWER(value) LIKE LOWER(pattern)
    let lower_value = fn_call("LOWER".to_string(), vec![value])?;
    let lower_pattern = fn_call("LOWER".to_string(), vec![pattern])?;
    sql::sql(
        vec!["".to_string(), " LIKE ".to_string(), "".to_string()],
        vec![SqlValue::Query(lower_value), SqlValue::Query(lower_pattern)],
    )
}

/// Renders the NOT ILIKE fallback rewrite: `LOWER(value) NOT LIKE LOWER(pattern)`.
pub fn not_ilike_rewrite_sql(value: SqlValue, pattern: SqlValue) -> Result<SqlQuery, String> {
    let lower_value = fn_call("LOWER".to_string(), vec![value])?;
    let lower_pattern = fn_call("LOWER".to_string(), vec![pattern])?;
    sql::sql(
        vec!["".to_string(), " NOT LIKE ".to_string(), "".to_string()],
        vec![SqlValue::Query(lower_value), SqlValue::Query(lower_pattern)],
    )
}

// ─── Subquery predicates ─────────────────────────────────────────────────────

pub fn exists_sql(query: SqlQuery) -> Result<SqlQuery, String> {
    sql::sql(
        vec!["EXISTS (".to_string(), ")".to_string()],
        vec![SqlValue::Query(query)],
    )
}

pub fn not_exists_sql(query: SqlQuery) -> Result<SqlQuery, String> {
    sql::sql(
        vec!["NOT EXISTS (".to_string(), ")".to_string()],
        vec![SqlValue::Query(query)],
    )
}

// ─── Logical combinators ─────────────────────────────────────────────────────

pub fn and(conditions: Vec<SqlValue>) -> Result<SqlQuery, String> {
    if conditions.is_empty() {
        return Err("and() requires at least one condition".to_string());
    }
    // Each condition wrapped in parens: (cond1) AND (cond2) AND ...
    // then the whole joined result wrapped in outer parens: ((cond1) AND (cond2))
    let wrapped: Vec<SqlValue> = conditions
        .into_iter()
        .map(|c| wrap(c).map(SqlValue::Query))
        .collect::<Result<Vec<_>, _>>()?;
    let inner = sql::join(wrapped, Some(" AND ".to_string()))?;
    sql::sql(
        vec!["(".to_string(), ")".to_string()],
        vec![SqlValue::Query(inner)],
    )
}

pub fn or(conditions: Vec<SqlValue>) -> Result<SqlQuery, String> {
    if conditions.is_empty() {
        return Err("or() requires at least one condition".to_string());
    }
    let wrapped: Vec<SqlValue> = conditions
        .into_iter()
        .map(|c| wrap(c).map(SqlValue::Query))
        .collect::<Result<Vec<_>, _>>()?;
    let inner = sql::join(wrapped, Some(" OR ".to_string()))?;
    sql::sql(
        vec!["(".to_string(), ")".to_string()],
        vec![SqlValue::Query(inner)],
    )
}

// ─── Function calls (aggregates, scalars) ────────────────────────────────────

/// Generic SQL function call: NAME(arg1, arg2, ...) or NAME()
pub fn fn_call(name: String, args: Vec<SqlValue>) -> Result<SqlQuery, String> {
    if args.is_empty() {
        return Ok(SqlQuery {
            text: format!("{}()", name),
            raw: format!("{}()", name),
            values: vec![],
        });
    }
    let rendered = sql::join(args, Some(", ".to_string()))?;
    sql::sql(
        vec![format!("{}(", name), ")".to_string()],
        vec![SqlValue::Query(rendered)],
    )
}

pub fn fn_call_for_dialect(
    name: String,
    args: Vec<SqlValue>,
    dialect: &Dialect,
) -> Result<SqlQuery, String> {
    let normalized = normalize_function_name(&name);
    if function_capability_for(dialect, &normalized).is_none() {
        return Err(format!(
            "Dialect \"{}\" does not support function \"{}\" in this builder.",
            dialect, normalized
        ));
    }

    fn_call(normalized, args)
}

/// CASE WHEN when1 THEN then1 ... [ELSE else_val] END
pub fn scalar_case(
    branches: Vec<(SqlValue, SqlValue)>,
    else_val: Option<SqlValue>,
) -> Result<SqlQuery, String> {
    if branches.is_empty() {
        return Err("scalar_case() requires at least one branch".to_string());
    }
    let mut branch_parts: Vec<SqlValue> = Vec::new();
    for (when, then) in branches {
        let clause = sql::sql(
            vec!["WHEN ".to_string(), " THEN ".to_string(), "".to_string()],
            vec![when, then],
        )?;
        branch_parts.push(SqlValue::Query(clause));
    }
    let branches_joined = sql::join(branch_parts, Some(" ".to_string()))?;

    match else_val {
        None => sql::sql(
            vec!["CASE ".to_string(), " END".to_string()],
            vec![SqlValue::Query(branches_joined)],
        ),
        Some(ev) => sql::sql(
            vec![
                "CASE ".to_string(),
                " ELSE ".to_string(),
                " END".to_string(),
            ],
            vec![SqlValue::Query(branches_joined), ev],
        ),
    }
}

/// Binary arithmetic: (left op right)  e.g. (a + b)
pub fn arith_binary(left: SqlValue, operator: String, right: SqlValue) -> Result<SqlQuery, String> {
    sql::sql(
        vec![
            "(".to_string(),
            " ".to_string() + &operator + " ",
            ")".to_string(),
        ],
        vec![left, right],
    )
}

pub fn arith_binary_for_dialect(
    left: SqlValue,
    operator: String,
    right: SqlValue,
    dialect: &Dialect,
) -> Result<SqlQuery, String> {
    let normalized = normalize_operator(&operator);
    if operator_capability_for(dialect, OperatorKind::Arithmetic, &normalized).is_none() {
        return Err(format!(
            "Dialect \"{}\" does not support operator \"{}\" in this builder.",
            dialect, normalized
        ));
    }

    arith_binary(left, normalized, right)
}

pub fn validate_window_function_for_dialect(
    dialect: &Dialect,
    function_name: &str,
) -> Result<String, String> {
    if !crate::backend::backend_for_dialect(dialect)
        .capabilities()
        .window_functions
    {
        return Err(format!(
            "Dialect \"{}\" does not support window functions in this builder.",
            dialect
        ));
    }

    let normalized = normalize_function_name(function_name);
    let capability = function_capability_for(dialect, &normalized).ok_or_else(|| {
        format!(
            "Dialect \"{}\" does not support function \"{}\" in this builder.",
            dialect, normalized
        )
    })?;

    if capability.window {
        Ok(normalized)
    } else {
        Err(format!(
            "Function \"{}\" cannot be used as a window function in dialect \"{}\".",
            normalized, dialect
        ))
    }
}

pub fn over_clause(
    query: SqlQuery,
    partition_by: Vec<SqlQuery>,
    order_by: Vec<WindowOrderItem>,
    dialect: &Dialect,
) -> Result<SqlQuery, String> {
    if !crate::backend::backend_for_dialect(dialect)
        .capabilities()
        .window_functions
    {
        return Err(format!(
            "Dialect \"{}\" does not support window functions in this builder.",
            dialect
        ));
    }

    if order_by.iter().any(|item| item.nulls.is_some())
        && !crate::backend::backend_for_dialect(dialect)
            .capabilities()
            .null_ordering
    {
        return Err(format!(
            "Dialect \"{}\" does not support NULLS FIRST/LAST in ORDER BY.",
            dialect
        ));
    }

    let mut clauses: Vec<SqlValue> = Vec::new();

    if !partition_by.is_empty() {
        let partition = sql::join(
            partition_by.into_iter().map(SqlValue::Query).collect(),
            Some(", ".to_string()),
        )?;
        clauses.push(SqlValue::Query(sql::sql(
            vec!["PARTITION BY ".to_string(), "".to_string()],
            vec![SqlValue::Query(partition)],
        )?));
    }

    if !order_by.is_empty() {
        let items: Vec<SqlValue> = order_by
            .into_iter()
            .map(|item| {
                let mut parts = vec![SqlValue::Query(item.expression)];
                if let Some(direction) = item.direction {
                    parts.push(SqlValue::Raw(sql::raw(direction)));
                }
                if let Some(nulls) = item.nulls {
                    parts.push(SqlValue::Query(sql::sql(
                        vec!["NULLS ".to_string(), "".to_string()],
                        vec![SqlValue::Raw(sql::raw(nulls))],
                    )?));
                }
                sql::join(parts, Some(" ".to_string())).map(SqlValue::Query)
            })
            .collect::<Result<Vec<_>, _>>()?;

        let joined = sql::join(items, Some(", ".to_string()))?;
        clauses.push(SqlValue::Query(sql::sql(
            vec!["ORDER BY ".to_string(), "".to_string()],
            vec![SqlValue::Query(joined)],
        )?));
    }

    let over_inner = if clauses.is_empty() {
        SqlQuery {
            text: String::new(),
            raw: String::new(),
            values: vec![],
        }
    } else {
        sql::join(clauses, Some(" ".to_string()))?
    };

    sql::sql(
        vec!["".to_string(), " OVER (".to_string(), ")".to_string()],
        vec![SqlValue::Query(query), SqlValue::Query(over_inner)],
    )
}

#[cfg(test)]
#[path = "expressions.test.rs"]
mod expressions_test;
