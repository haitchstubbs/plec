use crate::dialect::{classify, Dialect, DialectFeature, DialectWarning};
use crate::expressions;
use crate::sql;
use crate::types::{Primitive, SqlQuery, SqlValue, WindowOrderItem};
use serde::Deserialize;

// ─── Supporting types ─────────────────────────────────────────────────────────

/// Items for IN / NOT IN — either a primitive list or a pre-rendered subquery.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum InArrayItems {
    Primitives(Vec<Primitive>),
    Query(SqlQuery),
}

/// One WHEN/THEN branch inside a CASE expression.
#[derive(Debug, Clone, Deserialize)]
pub struct CaseBranch {
    pub when: ExprNode,
    #[serde(rename = "result")]
    pub result: ExprNode,
}

/// One ORDER BY item inside an OVER(...) clause.
#[derive(Debug, Clone, Deserialize)]
pub struct WindowOrderNode {
    pub expression: ExprNode,
    pub direction: Option<String>,
    pub nulls: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum SqlTemplateExpr {
    Node(ExprNode),
    Value(SqlValue),
}

// ─── ExprNode ─────────────────────────────────────────────────────────────────

/// A deferred expression tree.  TypeScript builds this as a plain JSON object
/// (zero FFI) and sends it to Rust in the batch-op payload.  Rust evaluates it
/// via `eval_expr`.
///
/// `#[serde(tag = "type", rename_all = "camelCase")]` makes the JSON
/// discriminant field `"type"` with camelCase variant names, e.g. `"isNull"`,
/// `"fnCall"`, etc.  This is the same name the TypeScript side uses.
///
/// Extra fields present in the JSON (e.g. `"text"`, `"raw"`, `"values"` added
/// by TypeScript so the objects satisfy the `SqlQuery` structural type) are
/// silently ignored because we do not use `#[serde(deny_unknown_fields)]`.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ExprNode {
    /// A quoted column / table reference.  `parts` mirrors a dotted path.
    Ref {
        parts: Vec<String>,
    },
    /// A pseudo-reference to the insert conflict row (`excluded` or equivalent).
    Excluded {
        column: String,
    },
    /// A bound scalar value (number, string, boolean, null, bigint, date).
    Val {
        value: Primitive,
    },
    /// An unquoted, unparameterised raw SQL fragment.
    Raw {
        text: String,
    },
    /// An already-rendered `SqlQuery` — escape hatch for legacy outputs from
    /// the `sql` template tag or standalone `Expressions.*` calls.
    Query {
        query: SqlQuery,
    },
    /// A deferred SQL template expression.
    SqlTemplate {
        strings: Vec<String>,
        exprs: Vec<SqlTemplateExpr>,
    },

    // ─── Comparisons ────────────────────────────────────────────────────────
    Cmp {
        left: Box<ExprNode>,
        op: String,
        right: Box<ExprNode>,
    },
    IsNull {
        value: Box<ExprNode>,
    },
    IsNotNull {
        value: Box<ExprNode>,
    },

    // ─── Array / range ──────────────────────────────────────────────────────
    InArray {
        value: Box<ExprNode>,
        items: InArrayItems,
    },
    NotInArray {
        value: Box<ExprNode>,
        items: InArrayItems,
    },
    Between {
        value: Box<ExprNode>,
        lower: Box<ExprNode>,
        upper: Box<ExprNode>,
    },
    NotBetween {
        value: Box<ExprNode>,
        lower: Box<ExprNode>,
        upper: Box<ExprNode>,
    },

    // ─── Pattern matching ────────────────────────────────────────────────────
    Like {
        value: Box<ExprNode>,
        pattern: Box<ExprNode>,
    },
    NotLike {
        value: Box<ExprNode>,
        pattern: Box<ExprNode>,
    },
    /// Case-insensitive LIKE.  On dialects that support `ILIKE` natively this
    /// renders as `value ILIKE pattern`.  On other dialects the renderer
    /// transparently rewrites it to `LOWER(value) LIKE LOWER(pattern)` and
    /// records a [`DialectWarning`](crate::dialect::DialectWarning).
    Ilike {
        value: Box<ExprNode>,
        pattern: Box<ExprNode>,
    },
    NotIlike {
        value: Box<ExprNode>,
        pattern: Box<ExprNode>,
    },

    // ─── Subquery predicates ────────────────────────────────────────────────
    Exists {
        query: SqlQuery,
    },
    NotExists {
        query: SqlQuery,
    },

    // ─── Logical combinators ─────────────────────────────────────────────────
    And {
        conditions: Vec<ExprNode>,
    },
    Or {
        conditions: Vec<ExprNode>,
    },

    // ─── Function call ───────────────────────────────────────────────────────
    FnCall {
        name: String,
        args: Vec<ExprNode>,
    },

    // ─── Arithmetic ──────────────────────────────────────────────────────────
    Arith {
        left: Box<ExprNode>,
        op: String,
        right: Box<ExprNode>,
    },

    // ─── CASE ────────────────────────────────────────────────────────────────
    Case {
        branches: Vec<CaseBranch>,
        /// `elseVal` in JSON — the camelCase rename is produced by
        /// `rename_all = "camelCase"` on the enum, which also applies to
        /// struct-variant fields.
        #[serde(rename = "elseVal")]
        else_val: Option<Box<ExprNode>>,
    },

    // ─── Window / OVER ───────────────────────────────────────────────────────
    Over {
        expr: Box<ExprNode>,
        #[serde(rename = "partitionBy")]
        partition_by: Vec<ExprNode>,
        #[serde(rename = "orderBy")]
        order_by: Vec<WindowOrderNode>,
        /// The query dialect — needed for window-function support validation.
        dialect: String,
    },
}

// ─── Evaluator ────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "expr_node.test.rs"]
mod expr_node_test;

fn validate_window_function_for_dialect(dialect: &Dialect, expr: &ExprNode) -> Result<(), String> {
    let ExprNode::FnCall { name, .. } = expr else {
        return Err("OVER clauses require a function expression in this builder.".to_string());
    };

    expressions::validate_window_function_for_dialect(dialect, name).map(|_| ())
}

/// Recursively evaluate an `ExprNode` tree into a `SqlQuery`.
pub fn eval_expr(node: ExprNode) -> Result<SqlQuery, String> {
    eval_expr_inner(node, None, &mut None)
}

/// Recursively evaluate an `ExprNode` tree using dialect-aware validation.
pub fn eval_expr_for_dialect(node: ExprNode, dialect: &Dialect) -> Result<SqlQuery, String> {
    eval_expr_inner(node, Some(dialect), &mut None)
}

/// Evaluate an `ExprNode` tree using dialect-aware validation, collecting
/// fallback-rewrite warnings into `warnings`.
///
/// This is the preferred entry point from the render pipeline.  Pass a mutable
/// reference to the `warnings` collector so ILIKE rewrites and similar can be
/// recorded without changing the function return type.
///
/// # Errors
///
/// Returns a string error for validation failures.  Callers that need
/// [`QueryError`](crate::error::QueryError) should wrap via `.into()`.
pub fn eval_expr_for_dialect_ctx(
    node: ExprNode,
    dialect: &Dialect,
    warnings: &mut Option<WarningsCtx<'_>>,
) -> Result<SqlQuery, String> {
    eval_expr_inner(node, Some(dialect), warnings)
}

/// A warnings collector passed through the expression evaluator.
///
/// Holds a mutable reference to the warnings list and the `strict` flag.
pub struct WarningsCtx<'a> {
    pub warnings: &'a mut Vec<DialectWarning>,
    pub strict: bool,
}

fn eval_expr_inner(
    node: ExprNode,
    dialect: Option<&Dialect>,
    ctx: &mut Option<WarningsCtx<'_>>,
) -> Result<SqlQuery, String> {
    match node {
        ExprNode::Ref { parts } => {
            let id = sql::identifier(parts)?;
            sql::to_query_fragment(&SqlValue::Identifier(id))
        }

        ExprNode::Excluded { column } => {
            let column_id = sql::identifier(vec![column])?;
            sql::sql(
                vec!["excluded.".to_string(), "".to_string()],
                vec![SqlValue::Identifier(column_id)],
            )
        }

        ExprNode::Val { value } => sql::to_query_fragment(&SqlValue::Primitive(value)),

        ExprNode::Raw { text } => Ok(SqlQuery {
            text: text.clone(),
            raw: text,
            values: vec![],
        }),

        ExprNode::Query { query } => Ok(query),

        ExprNode::SqlTemplate { strings, exprs } => {
            let exprs = exprs
                .into_iter()
                .map(|expr| match expr {
                    SqlTemplateExpr::Node(node) => {
                        eval_expr_inner(node, dialect, ctx).map(SqlValue::Query)
                    }
                    SqlTemplateExpr::Value(value) => Ok(value),
                })
                .collect::<Result<Vec<_>, String>>()?;
            sql::sql(strings, exprs)
        }

        ExprNode::Cmp { left, op, right } => {
            let left = SqlValue::Query(eval_expr_inner(*left, dialect, ctx)?);
            let right = SqlValue::Query(eval_expr_inner(*right, dialect, ctx)?);
            match dialect {
                Some(dialect) => expressions::cmp_for_dialect(left, op, right, dialect),
                None => expressions::cmp(left, op, right),
            }
        }

        ExprNode::IsNull { value } => {
            expressions::is_null(SqlValue::Query(eval_expr_inner(*value, dialect, ctx)?))
        }

        ExprNode::IsNotNull { value } => {
            expressions::is_not_null(SqlValue::Query(eval_expr_inner(*value, dialect, ctx)?))
        }

        ExprNode::InArray { value, items } => {
            let v = SqlValue::Query(eval_expr_inner(*value, dialect, ctx)?);
            match items {
                InArrayItems::Primitives(prims) => {
                    let vals: Vec<SqlValue> = prims.into_iter().map(SqlValue::Primitive).collect();
                    expressions::in_array(v, vals)
                }
                InArrayItems::Query(q) => sql::sql(
                    vec!["".to_string(), " IN (".to_string(), ")".to_string()],
                    vec![v, SqlValue::Query(q)],
                ),
            }
        }

        ExprNode::NotInArray { value, items } => {
            let v = SqlValue::Query(eval_expr_inner(*value, dialect, ctx)?);
            match items {
                InArrayItems::Primitives(prims) => {
                    let vals: Vec<SqlValue> = prims.into_iter().map(SqlValue::Primitive).collect();
                    expressions::not_in_array(v, vals)
                }
                InArrayItems::Query(q) => sql::sql(
                    vec!["".to_string(), " NOT IN (".to_string(), ")".to_string()],
                    vec![v, SqlValue::Query(q)],
                ),
            }
        }

        ExprNode::Between {
            value,
            lower,
            upper,
        } => expressions::between(
            SqlValue::Query(eval_expr_inner(*value, dialect, ctx)?),
            SqlValue::Query(eval_expr_inner(*lower, dialect, ctx)?),
            SqlValue::Query(eval_expr_inner(*upper, dialect, ctx)?),
        ),

        ExprNode::NotBetween {
            value,
            lower,
            upper,
        } => expressions::not_between(
            SqlValue::Query(eval_expr_inner(*value, dialect, ctx)?),
            SqlValue::Query(eval_expr_inner(*lower, dialect, ctx)?),
            SqlValue::Query(eval_expr_inner(*upper, dialect, ctx)?),
        ),

        ExprNode::Like { value, pattern } => expressions::like_sql(
            SqlValue::Query(eval_expr_inner(*value, dialect, ctx)?),
            SqlValue::Query(eval_expr_inner(*pattern, dialect, ctx)?),
        ),

        ExprNode::NotLike { value, pattern } => expressions::not_like_sql(
            SqlValue::Query(eval_expr_inner(*value, dialect, ctx)?),
            SqlValue::Query(eval_expr_inner(*pattern, dialect, ctx)?),
        ),

        ExprNode::Ilike { value, pattern } => {
            let val_q = eval_expr_inner(*value, dialect, ctx)?;
            let pat_q = eval_expr_inner(*pattern, dialect, ctx)?;
            let use_native = dialect
                .map(|d| {
                    matches!(
                        classify(d, DialectFeature::Ilike),
                        crate::dialect::DialectPolicy::DialectRender
                    )
                })
                .unwrap_or(true);
            if use_native {
                expressions::ilike_sql(SqlValue::Query(val_q), SqlValue::Query(pat_q))
            } else {
                // Fallback rewrite: LOWER(value) LIKE LOWER(pattern)
                if let (Some(d), Some(ctx_ref)) = (dialect, ctx.as_mut()) {
                    ctx_ref.warnings.push(DialectWarning {
                        feature: "ILIKE".to_string(),
                        dialect: d.as_str().to_string(),
                        message: format!(
                            "ILIKE is not supported by dialect \"{d}\"; \
                             rewritten as LOWER(value) LIKE LOWER(pattern)"
                        ),
                    });
                }
                expressions::ilike_rewrite_sql(SqlValue::Query(val_q), SqlValue::Query(pat_q))
            }
        }

        ExprNode::NotIlike { value, pattern } => {
            let val_q = eval_expr_inner(*value, dialect, ctx)?;
            let pat_q = eval_expr_inner(*pattern, dialect, ctx)?;
            let use_native = dialect
                .map(|d| {
                    matches!(
                        classify(d, DialectFeature::Ilike),
                        crate::dialect::DialectPolicy::DialectRender
                    )
                })
                .unwrap_or(true);
            if use_native {
                expressions::not_ilike_sql(SqlValue::Query(val_q), SqlValue::Query(pat_q))
            } else {
                if let (Some(d), Some(ctx_ref)) = (dialect, ctx.as_mut()) {
                    ctx_ref.warnings.push(DialectWarning {
                        feature: "ILIKE".to_string(),
                        dialect: d.as_str().to_string(),
                        message: format!(
                            "NOT ILIKE is not supported by dialect \"{d}\"; \
                             rewritten as LOWER(value) NOT LIKE LOWER(pattern)"
                        ),
                    });
                }
                expressions::not_ilike_rewrite_sql(SqlValue::Query(val_q), SqlValue::Query(pat_q))
            }
        }

        ExprNode::Exists { query } => expressions::exists_sql(query),

        ExprNode::NotExists { query } => expressions::not_exists_sql(query),

        ExprNode::And { conditions } => {
            let vals: Vec<SqlValue> = conditions
                .into_iter()
                .map(|c| eval_expr_inner(c, dialect, ctx).map(SqlValue::Query))
                .collect::<Result<Vec<_>, _>>()?;
            expressions::and(vals)
        }

        ExprNode::Or { conditions } => {
            let vals: Vec<SqlValue> = conditions
                .into_iter()
                .map(|c| eval_expr_inner(c, dialect, ctx).map(SqlValue::Query))
                .collect::<Result<Vec<_>, _>>()?;
            expressions::or(vals)
        }

        ExprNode::FnCall { name, args } => {
            let vals: Vec<SqlValue> = args
                .into_iter()
                .map(|a| eval_expr_inner(a, dialect, ctx).map(SqlValue::Query))
                .collect::<Result<Vec<_>, _>>()?;
            match dialect {
                Some(dialect) => expressions::fn_call_for_dialect(name, vals, dialect),
                None => expressions::fn_call(name, vals),
            }
        }

        ExprNode::Arith { left, op, right } => {
            let left = SqlValue::Query(eval_expr_inner(*left, dialect, ctx)?);
            let right = SqlValue::Query(eval_expr_inner(*right, dialect, ctx)?);
            match dialect {
                Some(dialect) => expressions::arith_binary_for_dialect(left, op, right, dialect),
                None => expressions::arith_binary(left, op, right),
            }
        }

        ExprNode::Case { branches, else_val } => {
            let branch_pairs: Vec<(SqlValue, SqlValue)> = branches
                .into_iter()
                .map(|b| {
                    Ok((
                        SqlValue::Query(eval_expr_inner(b.when, dialect, ctx)?),
                        SqlValue::Query(eval_expr_inner(b.result, dialect, ctx)?),
                    ))
                })
                .collect::<Result<Vec<_>, String>>()?;
            let else_sql = else_val
                .map(|v| eval_expr_inner(*v, dialect, ctx).map(SqlValue::Query))
                .transpose()?;
            expressions::scalar_case(branch_pairs, else_sql)
        }

        ExprNode::Over {
            expr,
            partition_by,
            order_by,
            dialect,
        } => {
            let dialect = Dialect::parse(&dialect);
            validate_window_function_for_dialect(&dialect, &expr)?;
            let fn_query = eval_expr_inner(*expr, Some(&dialect), &mut None)?;
            let partition_queries: Vec<SqlQuery> = partition_by
                .into_iter()
                .map(|expr| eval_expr_inner(expr, Some(&dialect), &mut None))
                .collect::<Result<Vec<_>, _>>()?;
            let order_items: Vec<WindowOrderItem> = order_by
                .into_iter()
                .map(|item| {
                    Ok(WindowOrderItem {
                        expression: eval_expr_inner(item.expression, Some(&dialect), &mut None)?,
                        direction: item.direction.map(|d| d.to_uppercase()),
                        nulls: item.nulls.map(|n| n.to_uppercase()),
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            expressions::over_clause(fn_query, partition_queries, order_items, &dialect)
        }
    }
}
