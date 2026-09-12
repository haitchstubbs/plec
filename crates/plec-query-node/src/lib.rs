use napi_derive::napi;

// ─── Helpers ──────────────────────────────────────────────────────────────────

#[inline]
fn sv(v: serde_json::Value) -> napi::Result<query_core::SqlValue> {
    serde_json::from_value(v).map_err(|e| napi::Error::from_reason(e.to_string()))
}

#[inline]
fn sq(v: serde_json::Value) -> napi::Result<query_core::SqlQuery> {
    serde_json::from_value(v).map_err(|e| napi::Error::from_reason(e.to_string()))
}

#[inline]
fn out<T: serde::Serialize>(value: T) -> napi::Result<serde_json::Value> {
    serde_json::to_value(value).map_err(|e| napi::Error::from_reason(e.to_string()))
}

#[inline]
fn core_err(e: String) -> napi::Error {
    napi::Error::from_reason(e)
}

// ─── Meta ─────────────────────────────────────────────────────────────────────

#[napi]
pub fn version() -> String {
    query_core::version().to_string()
}

#[napi]
pub fn create_engine_snapshot(dialect: Option<String>) -> String {
    query_core::EngineSnapshot::new(dialect.as_deref())
        .to_json()
        .unwrap_or_else(|_| "{\"dialect\":\"postgres\",\"stage\":\"start\",\"artifacts\":{\"text\":\"\",\"raw\":\"\",\"values_json\":[]}}".to_string())
}

// ─── SQL primitives ───────────────────────────────────────────────────────────

#[napi]
pub fn identifier(parts: Vec<String>) -> napi::Result<serde_json::Value> {
    out(query_core::identifier(parts).map_err(core_err)?)
}

#[napi]
pub fn raw(text: String) -> napi::Result<serde_json::Value> {
    out(query_core::raw(text))
}

#[napi]
pub fn join(
    items: serde_json::Value,
    separator: Option<String>,
) -> napi::Result<serde_json::Value> {
    let items: Vec<query_core::SqlValue> =
        serde_json::from_value(items).map_err(|e| napi::Error::from_reason(e.to_string()))?;
    out(query_core::join(items, separator).map_err(core_err)?)
}

#[napi]
pub fn sql(strings: Vec<String>, exprs: serde_json::Value) -> napi::Result<serde_json::Value> {
    let exprs: Vec<query_core::SqlValue> =
        serde_json::from_value(exprs).map_err(|e| napi::Error::from_reason(e.to_string()))?;
    out(query_core::sql(strings, exprs).map_err(core_err)?)
}

#[napi]
pub fn ref_identifier(parts: Vec<String>) -> napi::Result<serde_json::Value> {
    out(query_core::ref_identifier(parts).map_err(core_err)?)
}

#[napi]
pub fn compile_postgres(query: serde_json::Value) -> napi::Result<serde_json::Value> {
    out(query_core::compile_query(sq(query)?, "postgres").map_err(core_err)?)
}

#[napi]
pub fn compile_query(query: serde_json::Value, dialect: String) -> napi::Result<serde_json::Value> {
    out(query_core::compile_query(sq(query)?, &dialect).map_err(core_err)?)
}

// ─── Expressions ──────────────────────────────────────────────────────────────

#[napi]
pub fn cmp(
    left: serde_json::Value,
    operator: String,
    right: serde_json::Value,
    dialect: Option<String>,
) -> napi::Result<serde_json::Value> {
    let dialect = query_core::Dialect::parse(dialect.as_deref().unwrap_or("postgres"));
    out(
        query_core::cmp_for_dialect(sv(left)?, operator, sv(right)?, &dialect)
            .map_err(core_err)?,
    )
}

#[napi]
pub fn is_null(value: serde_json::Value) -> napi::Result<serde_json::Value> {
    out(query_core::is_null(sv(value)?).map_err(core_err)?)
}

#[napi]
pub fn is_not_null(value: serde_json::Value) -> napi::Result<serde_json::Value> {
    out(query_core::is_not_null(sv(value)?).map_err(core_err)?)
}

#[napi]
pub fn in_array(
    value: serde_json::Value,
    items: serde_json::Value,
) -> napi::Result<serde_json::Value> {
    let items: Vec<query_core::SqlValue> =
        serde_json::from_value(items).map_err(|e| napi::Error::from_reason(e.to_string()))?;
    out(query_core::in_array(sv(value)?, items).map_err(core_err)?)
}

#[napi]
pub fn not_in_array(
    value: serde_json::Value,
    items: serde_json::Value,
) -> napi::Result<serde_json::Value> {
    let items: Vec<query_core::SqlValue> =
        serde_json::from_value(items).map_err(|e| napi::Error::from_reason(e.to_string()))?;
    out(query_core::not_in_array(sv(value)?, items).map_err(core_err)?)
}

#[napi]
pub fn between(
    value: serde_json::Value,
    lower: serde_json::Value,
    upper: serde_json::Value,
) -> napi::Result<serde_json::Value> {
    out(query_core::between(sv(value)?, sv(lower)?, sv(upper)?).map_err(core_err)?)
}

#[napi]
pub fn not_between(
    value: serde_json::Value,
    lower: serde_json::Value,
    upper: serde_json::Value,
) -> napi::Result<serde_json::Value> {
    out(query_core::not_between(sv(value)?, sv(lower)?, sv(upper)?).map_err(core_err)?)
}

#[napi]
pub fn like_sql(
    value: serde_json::Value,
    pattern: serde_json::Value,
) -> napi::Result<serde_json::Value> {
    out(query_core::like_sql(sv(value)?, sv(pattern)?).map_err(core_err)?)
}

#[napi]
pub fn not_like_sql(
    value: serde_json::Value,
    pattern: serde_json::Value,
) -> napi::Result<serde_json::Value> {
    out(query_core::not_like_sql(sv(value)?, sv(pattern)?).map_err(core_err)?)
}

#[napi]
pub fn exists_sql(query: serde_json::Value) -> napi::Result<serde_json::Value> {
    out(query_core::exists_sql(sq(query)?).map_err(core_err)?)
}

#[napi]
pub fn not_exists_sql(query: serde_json::Value) -> napi::Result<serde_json::Value> {
    out(query_core::not_exists_sql(sq(query)?).map_err(core_err)?)
}

#[napi]
pub fn and(conditions: serde_json::Value) -> napi::Result<serde_json::Value> {
    let conditions: Vec<query_core::SqlValue> =
        serde_json::from_value(conditions).map_err(|e| napi::Error::from_reason(e.to_string()))?;
    out(query_core::and(conditions).map_err(core_err)?)
}

#[napi]
pub fn or(conditions: serde_json::Value) -> napi::Result<serde_json::Value> {
    let conditions: Vec<query_core::SqlValue> =
        serde_json::from_value(conditions).map_err(|e| napi::Error::from_reason(e.to_string()))?;
    out(query_core::or(conditions).map_err(core_err)?)
}

#[napi]
pub fn fn_call(
    name: String,
    args: serde_json::Value,
    dialect: Option<String>,
) -> napi::Result<serde_json::Value> {
    let args: Vec<query_core::SqlValue> =
        serde_json::from_value(args).map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let dialect = query_core::Dialect::parse(dialect.as_deref().unwrap_or("postgres"));
    out(query_core::fn_call_for_dialect(name, args, &dialect).map_err(core_err)?)
}

#[napi]
pub fn scalar_case(
    branches: serde_json::Value,
    else_val: Option<serde_json::Value>,
) -> napi::Result<serde_json::Value> {
    let branches: Vec<(query_core::SqlValue, query_core::SqlValue)> =
        serde_json::from_value(branches).map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let else_val: Option<query_core::SqlValue> = else_val
        .map(|v| serde_json::from_value(v).map_err(|e| napi::Error::from_reason(e.to_string())))
        .transpose()?;
    out(query_core::scalar_case(branches, else_val).map_err(core_err)?)
}

#[napi]
pub fn arith_binary(
    left: serde_json::Value,
    operator: String,
    right: serde_json::Value,
    dialect: Option<String>,
) -> napi::Result<serde_json::Value> {
    let dialect = query_core::Dialect::parse(dialect.as_deref().unwrap_or("postgres"));
    out(
        query_core::arith_binary_for_dialect(sv(left)?, operator, sv(right)?, &dialect)
            .map_err(core_err)?,
    )
}

#[napi]
pub fn over_clause(
    query: serde_json::Value,
    partition_by: serde_json::Value,
    order_by: serde_json::Value,
    dialect: String,
) -> napi::Result<serde_json::Value> {
    let partition_by: Vec<query_core::SqlQuery> = serde_json::from_value(partition_by)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let order_by: Vec<query_core::WindowOrderItem> =
        serde_json::from_value(order_by).map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let dialect = query_core::Dialect::parse(&dialect);
    out(
        query_core::over_clause(sq(query)?, partition_by, order_by, &dialect)
            .map_err(core_err)?,
    )
}

// ─── Builder — utility ────────────────────────────────────────────────────────

#[napi]
pub fn builder_new(dialect: Option<String>) -> String {
    query_core::builder_new(dialect.as_deref())
}

#[napi]
pub fn builder_clone(handle: String) -> napi::Result<String> {
    query_core::builder_clone(&handle).map_err(core_err)
}

#[napi]
pub fn builder_drop(handle: String) {
    query_core::builder_drop(&handle);
}

#[napi]
pub fn builder_clear(handle: String) -> napi::Result<String> {
    query_core::builder_clear(&handle).map_err(core_err)
}

// ─── Builder — FROM ───────────────────────────────────────────────────────────

#[napi]
pub fn builder_from_table(handle: String, table: String) -> napi::Result<String> {
    query_core::builder_from_table(&handle, table).map_err(core_err)
}

#[napi]
pub fn builder_from_table_alias(
    handle: String,
    table: String,
    alias: String,
) -> napi::Result<String> {
    query_core::builder_from_table_alias(&handle, table, alias).map_err(core_err)
}

#[napi]
pub fn builder_from_subquery(
    handle: String,
    alias: String,
    query_json: String,
) -> napi::Result<String> {
    query_core::builder_from_subquery(&handle, alias, query_json).map_err(core_err)
}

// ─── Builder — DISTINCT ───────────────────────────────────────────────────────

#[napi]
pub fn builder_distinct(handle: String) -> napi::Result<String> {
    query_core::builder_distinct(&handle).map_err(core_err)
}

#[napi]
pub fn builder_distinct_on_columns(handle: String, cols_json: String) -> napi::Result<String> {
    query_core::builder_distinct_on_columns(&handle, cols_json).map_err(core_err)
}

#[napi]
pub fn builder_distinct_on_exprs(handle: String, exprs_json: String) -> napi::Result<String> {
    query_core::builder_distinct_on_exprs(&handle, exprs_json).map_err(core_err)
}

// ─── Builder — SELECT ─────────────────────────────────────────────────────────

#[napi]
pub fn builder_select_columns(handle: String, cols_json: String) -> napi::Result<String> {
    query_core::builder_select_columns(&handle, cols_json).map_err(core_err)
}

#[napi]
pub fn builder_select_aliased(handle: String, aliases_json: String) -> napi::Result<String> {
    query_core::builder_select_aliased(&handle, aliases_json).map_err(core_err)
}

#[napi]
pub fn builder_select_fragment(
    handle: String,
    fragment_json: String,
    selected_cols_json: String,
) -> napi::Result<String> {
    query_core::builder_select_fragment(&handle, fragment_json, selected_cols_json)
        .map_err(core_err)
}

// ─── Builder — JOIN ───────────────────────────────────────────────────────────

#[napi]
pub fn builder_join_table(
    handle: String,
    join_type: String,
    table: String,
) -> napi::Result<String> {
    query_core::builder_join_table(&handle, join_type, table).map_err(core_err)
}

#[napi]
pub fn builder_join_table_alias(
    handle: String,
    join_type: String,
    table: String,
    alias: String,
) -> napi::Result<String> {
    query_core::builder_join_table_alias(&handle, join_type, table, alias).map_err(core_err)
}

#[napi]
pub fn builder_join_subquery(
    handle: String,
    join_type: String,
    alias: String,
    query_json: String,
) -> napi::Result<String> {
    query_core::builder_join_subquery(&handle, join_type, alias, query_json).map_err(core_err)
}

#[napi]
pub fn builder_on(handle: String, pred_json: String) -> napi::Result<String> {
    query_core::builder_on(&handle, pred_json).map_err(core_err)
}

#[napi]
pub fn builder_and_on(handle: String, pred_json: String) -> napi::Result<String> {
    query_core::builder_and_on(&handle, pred_json).map_err(core_err)
}

#[napi]
pub fn builder_or_on(handle: String, pred_json: String) -> napi::Result<String> {
    query_core::builder_or_on(&handle, pred_json).map_err(core_err)
}

#[napi]
pub fn builder_using_columns(handle: String, cols_json: String) -> napi::Result<String> {
    query_core::builder_using_columns(&handle, cols_json).map_err(core_err)
}

#[napi]
pub fn builder_on_columns(handle: String, pairs_json: String) -> napi::Result<String> {
    query_core::builder_on_columns(&handle, pairs_json).map_err(core_err)
}

// ─── Builder — WHERE / HAVING ─────────────────────────────────────────────────

#[napi]
pub fn builder_where(handle: String, pred_json: String) -> napi::Result<String> {
    query_core::builder_where(&handle, pred_json).map_err(core_err)
}

#[napi]
pub fn builder_and_where(handle: String, pred_json: String) -> napi::Result<String> {
    query_core::builder_and_where(&handle, pred_json).map_err(core_err)
}

#[napi]
pub fn builder_or_where(handle: String, pred_json: String) -> napi::Result<String> {
    query_core::builder_or_where(&handle, pred_json).map_err(core_err)
}

#[napi]
pub fn builder_having(handle: String, pred_json: String) -> napi::Result<String> {
    query_core::builder_having(&handle, pred_json).map_err(core_err)
}

#[napi]
pub fn builder_and_having(handle: String, pred_json: String) -> napi::Result<String> {
    query_core::builder_and_having(&handle, pred_json).map_err(core_err)
}

#[napi]
pub fn builder_or_having(handle: String, pred_json: String) -> napi::Result<String> {
    query_core::builder_or_having(&handle, pred_json).map_err(core_err)
}

// ─── Builder — GROUP BY ───────────────────────────────────────────────────────

#[napi]
pub fn builder_group_by_columns(handle: String, cols_json: String) -> napi::Result<String> {
    query_core::builder_group_by_columns(&handle, cols_json).map_err(core_err)
}

// ─── Builder — ORDER BY ───────────────────────────────────────────────────────

#[napi]
pub fn builder_order_by_column(
    handle: String,
    col: String,
    direction: Option<String>,
    null_order: Option<String>,
) -> napi::Result<String> {
    query_core::builder_order_by_column(&handle, col, direction, null_order).map_err(core_err)
}

#[napi]
pub fn builder_order_by_columns(handle: String, cols_json: String) -> napi::Result<String> {
    query_core::builder_order_by_columns(&handle, cols_json).map_err(core_err)
}

// ─── Builder — LIMIT / OFFSET ─────────────────────────────────────────────────

#[napi]
pub fn builder_limit(handle: String, count: u32) -> napi::Result<String> {
    query_core::builder_limit(&handle, count).map_err(core_err)
}

#[napi]
pub fn builder_offset(handle: String, count: u32) -> napi::Result<String> {
    query_core::builder_offset(&handle, count).map_err(core_err)
}

#[napi]
pub fn builder_for_update(handle: String) -> napi::Result<String> {
    query_core::builder_for_update(&handle).map_err(core_err)
}

#[napi]
pub fn builder_for_share(handle: String) -> napi::Result<String> {
    query_core::builder_for_share(&handle).map_err(core_err)
}

#[napi]
pub fn builder_no_wait(handle: String) -> napi::Result<String> {
    query_core::builder_no_wait(&handle).map_err(core_err)
}

#[napi]
pub fn builder_skip_locked(handle: String) -> napi::Result<String> {
    query_core::builder_skip_locked(&handle).map_err(core_err)
}

// ─── Builder — COMPOUND ───────────────────────────────────────────────────────

#[napi]
pub fn builder_union(handle: String, query_json: String) -> napi::Result<String> {
    query_core::builder_union(&handle, query_json).map_err(core_err)
}

#[napi]
pub fn builder_union_all(handle: String, query_json: String) -> napi::Result<String> {
    query_core::builder_union_all(&handle, query_json).map_err(core_err)
}

#[napi]
pub fn builder_intersect(handle: String, query_json: String) -> napi::Result<String> {
    query_core::builder_intersect(&handle, query_json).map_err(core_err)
}

#[napi]
pub fn builder_except(handle: String, query_json: String) -> napi::Result<String> {
    query_core::builder_except(&handle, query_json).map_err(core_err)
}

// ─── Builder — CTE ────────────────────────────────────────────────────────────

#[napi]
pub fn builder_with(handle: String, name: String, query_json: String) -> napi::Result<String> {
    query_core::builder_with(&handle, name, query_json).map_err(core_err)
}

#[napi]
pub fn builder_with_recursive(
    handle: String,
    name: String,
    query_json: String,
) -> napi::Result<String> {
    query_core::builder_with_recursive(&handle, name, query_json).map_err(core_err)
}

// ─── Builder — INSERT ─────────────────────────────────────────────────────────

#[napi]
pub fn builder_insert_into(handle: String, table: String) -> napi::Result<String> {
    query_core::builder_insert_into(&handle, table).map_err(core_err)
}

#[napi]
pub fn builder_columns(handle: String, cols_json: String) -> napi::Result<String> {
    query_core::builder_columns(&handle, cols_json).map_err(core_err)
}

#[napi]
pub fn builder_values_insert(handle: String, rows_json: String) -> napi::Result<String> {
    query_core::builder_values_insert(&handle, rows_json).map_err(core_err)
}

#[napi]
pub fn builder_insert_select(handle: String, query_json: String) -> napi::Result<String> {
    query_core::builder_insert_select(&handle, query_json).map_err(core_err)
}

#[napi]
pub fn builder_insert_select_handle(handle: String, rhs_handle: String) -> napi::Result<String> {
    query_core::builder_insert_select_handle(&handle, &rhs_handle).map_err(core_err)
}

#[napi]
pub fn builder_on_conflict_columns(handle: String, cols_json: String) -> napi::Result<String> {
    query_core::builder_on_conflict_columns(&handle, cols_json).map_err(core_err)
}

#[napi]
pub fn builder_on_conflict_constraint(handle: String, constraint: String) -> napi::Result<String> {
    query_core::builder_on_conflict_constraint(&handle, constraint).map_err(core_err)
}

#[napi]
pub fn builder_do_nothing(handle: String) -> napi::Result<String> {
    query_core::builder_do_nothing(&handle).map_err(core_err)
}

#[napi]
pub fn builder_do_update_set(handle: String, assignments_json: String) -> napi::Result<String> {
    query_core::builder_do_update_set(&handle, assignments_json).map_err(core_err)
}

#[napi]
pub fn builder_conflict_where(handle: String, pred_json: String) -> napi::Result<String> {
    query_core::builder_conflict_where(&handle, pred_json).map_err(core_err)
}

#[napi]
pub fn builder_returning_columns(handle: String, cols_json: String) -> napi::Result<String> {
    query_core::builder_returning_columns(&handle, cols_json).map_err(core_err)
}

#[napi]
pub fn builder_returning_aliased(handle: String, aliases_json: String) -> napi::Result<String> {
    query_core::builder_returning_aliased(&handle, aliases_json).map_err(core_err)
}

#[napi]
pub fn builder_returning_fragment(
    handle: String,
    fragment_json: String,
    selected_cols_json: String,
) -> napi::Result<String> {
    query_core::builder_returning_fragment(&handle, fragment_json, selected_cols_json)
        .map_err(core_err)
}

// ─── Builder — UPDATE ─────────────────────────────────────────────────────────

#[napi]
pub fn builder_update(handle: String, table: String) -> napi::Result<String> {
    query_core::builder_update(&handle, table).map_err(core_err)
}

#[napi]
pub fn builder_set(handle: String, assignments_json: String) -> napi::Result<String> {
    query_core::builder_set(&handle, assignments_json).map_err(core_err)
}

// ─── Builder — DELETE ─────────────────────────────────────────────────────────

#[napi]
pub fn builder_delete_from(handle: String, table: String) -> napi::Result<String> {
    query_core::builder_delete_from(&handle, table).map_err(core_err)
}

// ─── Builder — output (typed: no JSON strings cross the FFI boundary) ─────────

#[napi]
pub fn builder_query(handle: String) -> napi::Result<serde_json::Value> {
    out(query_core::builder_query_typed(&handle).map_err(core_err)?)
}

#[napi]
pub fn builder_text(handle: String) -> napi::Result<String> {
    query_core::builder_text(&handle).map_err(core_err)
}

#[napi]
pub fn builder_raw(handle: String) -> napi::Result<String> {
    query_core::builder_raw(&handle).map_err(core_err)
}

#[napi]
pub fn builder_values(handle: String) -> napi::Result<serde_json::Value> {
    out(query_core::builder_values_typed(&handle).map_err(core_err)?)
}

#[napi]
pub fn builder_compile_bundle(handle: String) -> napi::Result<serde_json::Value> {
    out(query_core::builder_compile_bundle_typed(&handle).map_err(core_err)?)
}

#[napi]
pub fn builder_selected_columns(handle: String) -> napi::Result<Vec<String>> {
    query_core::builder_selected_columns_typed(&handle).map_err(core_err)
}

#[napi]
pub fn builder_insert_columns(handle: String) -> napi::Result<Vec<String>> {
    query_core::builder_insert_columns_typed(&handle).map_err(core_err)
}

#[napi]
pub fn builder_conflict_target_kind(handle: String) -> napi::Result<String> {
    query_core::builder_conflict_target_kind_typed(&handle).map_err(core_err)
}

#[napi]
pub fn builder_as(handle: String, alias: String) -> napi::Result<serde_json::Value> {
    out(query_core::builder_as_typed(&handle, alias).map_err(core_err)?)
}

#[napi]
pub fn builder_canonical_ir_hash(handle: String) -> napi::Result<String> {
    query_core::builder_canonical_ir_hash(&handle).map_err(core_err)
}

// ─── Builder — handle-passing compound / CTE / subquery ──────────────────────

#[napi]
pub fn builder_union_handle(handle: String, rhs_handle: String) -> napi::Result<String> {
    query_core::builder_union_handle(&handle, &rhs_handle).map_err(core_err)
}

#[napi]
pub fn builder_union_all_handle(handle: String, rhs_handle: String) -> napi::Result<String> {
    query_core::builder_union_all_handle(&handle, &rhs_handle).map_err(core_err)
}

#[napi]
pub fn builder_intersect_handle(handle: String, rhs_handle: String) -> napi::Result<String> {
    query_core::builder_intersect_handle(&handle, &rhs_handle).map_err(core_err)
}

#[napi]
pub fn builder_except_handle(handle: String, rhs_handle: String) -> napi::Result<String> {
    query_core::builder_except_handle(&handle, &rhs_handle).map_err(core_err)
}

#[napi]
pub fn builder_with_handle(
    handle: String,
    name: String,
    rhs_handle: String,
) -> napi::Result<String> {
    query_core::builder_with_handle(&handle, name, &rhs_handle).map_err(core_err)
}

#[napi]
pub fn builder_with_recursive_handle(
    handle: String,
    name: String,
    rhs_handle: String,
) -> napi::Result<String> {
    query_core::builder_with_recursive_handle(&handle, name, &rhs_handle).map_err(core_err)
}

#[napi]
pub fn builder_from_subquery_handle(
    handle: String,
    alias: String,
    rhs_handle: String,
) -> napi::Result<String> {
    query_core::builder_from_subquery_handle(&handle, alias, &rhs_handle).map_err(core_err)
}

#[napi]
pub fn builder_join_subquery_handle(
    handle: String,
    join_type: String,
    alias: String,
    rhs_handle: String,
) -> napi::Result<String> {
    query_core::builder_join_subquery_handle(&handle, join_type, alias, &rhs_handle)
        .map_err(core_err)
}

// ─── Builder — batch ops ───────────────────────────────────────────

#[napi]
pub fn builder_apply_ops(handle: String, ops_json: String) -> napi::Result<String> {
    query_core::builder_apply_ops(&handle, ops_json).map_err(core_err)
}

#[napi]
pub fn builder_apply_ops_binary(
    handle: String,
    payload: napi::bindgen_prelude::Buffer,
) -> napi::Result<String> {
    query_core::builder_apply_ops_bin(&handle, payload.to_vec()).map_err(core_err)
}
