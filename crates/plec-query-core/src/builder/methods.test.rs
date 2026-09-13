use crate::builder::canonical::{
    builder_canonical_ir_hash, canonical_ir_for_parts, canonical_ir_json_for_parts, fingerprint,
};
use crate::builder::registry::{test_set_structural_cache_capacity, test_structural_cache_len};
use crate::builder::render::{
    build_query_with_context, builder_compile_bundle_typed,
    get_or_build_query_arc_with_structural_cache, registry_get_parts,
    test_build_execution_count_for_handle, test_cache_lookup_count_for_handle,
    test_reset_build_execution_count,
};
use crate::builder::{
    builder_apply_ops, builder_apply_ops_bin, builder_columns, builder_conflict_where,
    builder_delete_from, builder_dialect_strict, builder_distinct, builder_distinct_on_columns,
    builder_distinct_on_exprs, builder_do_nothing, builder_do_update_set, builder_for_share,
    builder_for_update, builder_from_table_alias, builder_insert_into,
    builder_insert_select_handle, builder_join_table, builder_join_table_alias, builder_new,
    builder_no_wait, builder_on, builder_on_columns, builder_on_conflict_columns,
    builder_on_conflict_constraint, builder_query, builder_raw, builder_returning_aliased,
    builder_returning_columns, builder_select_columns, builder_select_fragment, builder_set,
    builder_skip_locked, builder_text, builder_update, builder_using_columns, builder_values,
    builder_values_insert, builder_where, builder_with, builder_with_recursive,
    builder_with_recursive_columns,
};
use crate::dialect::Dialect;
use crate::expressions::over_clause;
use crate::sql;
use crate::types::{Primitive, SqlQuery, SqlValue, WindowOrderItem};
use lru::LruCache;
use once_cell::sync::Lazy;
use parking_lot::{Mutex, MutexGuard};
use std::num::NonZeroUsize;
use std::sync::Arc;

static CACHE_TEST_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

fn cache_test_guard() -> MutexGuard<'static, ()> {
    CACHE_TEST_LOCK.lock()
}

fn cache_test_query(label: &str) -> Arc<SqlQuery> {
    Arc::new(SqlQuery {
        text: label.to_string(),
        raw: label.to_string(),
        values: vec![],
    })
}

fn rows_json(column_names: &[&str], rows: Vec<Vec<SqlValue>>) -> String {
    serde_json::json!({
        "columnNames": column_names,
        "rows": rows,
    })
    .to_string()
}

fn parse_query(json: String) -> SqlQuery {
    serde_json::from_str(&json).unwrap()
}

fn binary_ops_payload(ops: Vec<serde_json::Value>) -> Vec<u8> {
    let mut out = vec![1u8];
    out.extend_from_slice(&(ops.len() as u32).to_le_bytes());
    for op in ops {
        let payload = rmp_serde::to_vec_named(&op).unwrap();
        out.push(0);
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&payload);
    }
    out
}

fn simple_cte_query() -> SqlQuery {
    SqlQuery {
        text: "SELECT 1 AS id".to_string(),
        raw: "SELECT 1 AS id".to_string(),
        values: vec![],
    }
}

#[test]
fn insert_values_returning_columns_renders() {
    let handle = builder_new(Some("postgres"));
    let handle = builder_insert_into(&handle, "users".to_string()).unwrap();
    let handle = builder_values_insert(
        &handle,
        rows_json(
            &["id", "name"],
            vec![vec![
                SqlValue::Primitive(Primitive::String("u_1".to_string())),
                SqlValue::Primitive(Primitive::String("Ada".to_string())),
            ]],
        ),
    )
    .unwrap();
    let handle =
        builder_returning_columns(&handle, serde_json::to_string(&vec!["id", "name"]).unwrap())
            .unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert_eq!(
        query.text,
        r#"INSERT INTO "users" ("id", "name") VALUES (?, ?) RETURNING "id", "name""#
    );
    assert_eq!(
        query.raw,
        "INSERT INTO users (id, name) VALUES ('u_1', 'Ada') RETURNING id, name"
    );
}

#[test]
fn plain_distinct_still_renders() {
    let handle = builder_new(Some("postgres"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    let handle = builder_distinct(&handle).unwrap();
    let handle = builder_select_columns(
        &handle,
        serde_json::to_string(&vec!["u.id", "u.name"]).unwrap(),
    )
    .unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert_eq!(query.raw, "SELECT DISTINCT u.id, u.name FROM users u");
}

#[test]
fn select_star_renders_unquoted_wildcard() {
    let handle = builder_new(Some("duckdb"));
    let handle =
        builder_from_table_alias(&handle, "org_acme.users".to_string(), "u".to_string()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["*"]).unwrap()).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert_eq!(query.text, r#"SELECT * FROM "org_acme"."users" "u""#);
    assert_eq!(query.raw, "SELECT * FROM org_acme.users u");
}

#[test]
fn distinct_on_columns_renders_for_postgres() {
    let handle = builder_new(Some("postgres"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    let handle =
        builder_distinct_on_columns(&handle, serde_json::to_string(&vec!["u.country"]).unwrap())
            .unwrap();
    let handle = builder_select_columns(
        &handle,
        serde_json::to_string(&vec!["u.id", "u.country"]).unwrap(),
    )
    .unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert_eq!(
        query.text,
        r#"SELECT DISTINCT ON ("u"."country") "u"."id", "u"."country" FROM "users" "u""#
    );
    assert_eq!(
        query.raw,
        "SELECT DISTINCT ON (u.country) u.id, u.country FROM users u"
    );
}

#[test]
fn distinct_on_exprs_renders_for_duckdb() {
    let handle = builder_new(Some("duckdb"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    let exprs = serde_json::json!([
        { "type": "ref", "parts": ["u", "country"], "text": "", "raw": "", "values": [] },
        { "type": "fnCall", "name": "LOWER", "args": [
            { "type": "ref", "parts": ["u", "email"], "text": "", "raw": "", "values": [] }
        ], "text": "", "raw": "", "values": [] }
    ]);
    let handle = builder_distinct_on_exprs(&handle, exprs.to_string()).unwrap();
    let handle = builder_select_columns(
        &handle,
        serde_json::to_string(&vec!["u.id", "u.email"]).unwrap(),
    )
    .unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert_eq!(
        query.raw,
        "SELECT DISTINCT ON (u.country, LOWER(u.email)) u.id, u.email FROM users u"
    );
}

#[test]
fn unsupported_dialect_rejects_distinct_on_before_sql_emission() {
    let handle = builder_new(Some("sqlite"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    let handle =
        builder_distinct_on_columns(&handle, serde_json::to_string(&vec!["u.country"]).unwrap())
            .unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();

    let error = builder_query(&handle).unwrap_err();
    assert_eq!(
        error,
        r#"Dialect "sqlite" does not support DISTINCT ON in this builder."#
    );
}

#[test]
fn postgres_recursive_cte_renders_with_recursive_keyword() {
    let handle = builder_new(Some("postgres"));
    let handle = builder_with_recursive(
        &handle,
        "nums".to_string(),
        serde_json::to_string(&simple_cte_query()).unwrap(),
    )
    .unwrap();
    let handle = builder_from_table_alias(&handle, "nums".to_string(), "n".to_string()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["n.id"]).unwrap()).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert_eq!(
        query.raw,
        "WITH RECURSIVE nums AS (SELECT 1 AS id) SELECT n.id FROM nums n"
    );
}

#[test]
fn non_recursive_cte_renders_with_keyword_only() {
    let handle = builder_new(Some("postgres"));
    let handle = builder_with(
        &handle,
        "nums".to_string(),
        serde_json::to_string(&simple_cte_query()).unwrap(),
    )
    .unwrap();
    let handle = builder_from_table_alias(&handle, "nums".to_string(), "n".to_string()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["n.id"]).unwrap()).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert_eq!(
        query.raw,
        "WITH nums AS (SELECT 1 AS id) SELECT n.id FROM nums n"
    );
}

#[test]
fn mixed_ctes_render_one_recursive_with_keyword() {
    let handle = builder_new(Some("postgres"));
    let handle = builder_with(
        &handle,
        "seed".to_string(),
        serde_json::to_string(&simple_cte_query()).unwrap(),
    )
    .unwrap();
    let handle = builder_with_recursive(
        &handle,
        "nums".to_string(),
        serde_json::to_string(&simple_cte_query()).unwrap(),
    )
    .unwrap();
    let handle = builder_from_table_alias(&handle, "nums".to_string(), "n".to_string()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["n.id"]).unwrap()).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert_eq!(
        query.raw,
        "WITH RECURSIVE seed AS (SELECT 1 AS id), nums AS (SELECT 1 AS id) SELECT n.id FROM nums n"
    );
}

#[test]
fn unknown_dialect_rejects_recursive_cte_before_sql_emission() {
    let handle = builder_new(Some("unknown"));
    let handle = builder_with_recursive(
        &handle,
        "nums".to_string(),
        serde_json::to_string(&simple_cte_query()).unwrap(),
    )
    .unwrap();
    let handle = builder_from_table_alias(&handle, "nums".to_string(), "n".to_string()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["n.id"]).unwrap()).unwrap();

    let error = builder_query(&handle).unwrap_err();
    assert_eq!(
        error,
        r#"Dialect "unknown" does not support recursive CTEs in this builder."#
    );
}

#[test]
fn mssql_recursive_cte_renders_without_recursive_keyword() {
    let handle = builder_new(Some("mssql"));
    let handle = builder_with_recursive(
        &handle,
        "nums".to_string(),
        serde_json::to_string(&simple_cte_query()).unwrap(),
    )
    .unwrap();
    let handle = builder_from_table_alias(&handle, "nums".to_string(), "n".to_string()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["n.id"]).unwrap()).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert_eq!(
        query.raw,
        "WITH nums AS (SELECT 1 AS id) SELECT n.id FROM nums n"
    );
}

#[test]
fn recursive_cte_column_aliases_are_required_when_dialect_requires_them() {
    let handle = builder_new(Some("snowflake"));
    let handle = builder_with_recursive(
        &handle,
        "nums".to_string(),
        serde_json::to_string(&simple_cte_query()).unwrap(),
    )
    .unwrap();
    let handle = builder_from_table_alias(&handle, "nums".to_string(), "n".to_string()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["n.id"]).unwrap()).unwrap();

    let error = builder_query(&handle).unwrap_err();
    assert_eq!(
        error,
        r#"Dialect "snowflake" requires column aliases for recursive CTE "nums"."#
    );
}

#[test]
fn recursive_cte_column_aliases_render_when_dialect_requires_them() {
    let handle = builder_new(Some("snowflake"));
    let handle = builder_with_recursive_columns(
        &handle,
        "nums".to_string(),
        serde_json::to_string(&simple_cte_query()).unwrap(),
        serde_json::to_string(&vec!["id"]).unwrap(),
    )
    .unwrap();
    let handle = builder_from_table_alias(&handle, "nums".to_string(), "n".to_string()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["n.id"]).unwrap()).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert_eq!(
        query.raw,
        "WITH RECURSIVE nums (id) AS (SELECT 1 AS id) SELECT n.id FROM nums n"
    );
}

#[test]
fn batch_recursive_cte_query_columns_render_when_dialect_requires_them() {
    let handle = builder_new(Some("snowflake"));
    let ops = serde_json::json!([
        {
            "op": "withRecursiveQuery",
            "name": "nums",
            "query": simple_cte_query(),
            "columns": ["id"]
        },
        ["fta", "nums", "n"],
        ["sc", ["n.id"]]
    ]);
    let handle = builder_apply_ops(&handle, ops.to_string()).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert_eq!(
        query.raw,
        "WITH RECURSIVE nums (id) AS (SELECT 1 AS id) SELECT n.id FROM nums n"
    );
}

#[test]
fn batch_ops_support_distinct_on_with_ordering_sensitive_output() {
    let handle = builder_new(Some("postgres"));
    let ops = serde_json::json!([
        ["fta", "users", "u"],
        ["de", [
            { "type": "ref", "parts": ["u", "country"], "text": "", "raw": "", "values": [] },
            { "type": "fnCall", "name": "LOWER", "args": [
                { "type": "ref", "parts": ["u", "email"], "text": "", "raw": "", "values": [] }
            ], "text": "", "raw": "", "values": [] }
        ]],
        ["sc", ["u.id", "u.email", "u.country"]],
        ["ob", "u.country", "ASC", null],
        ["ob", "u.email", "DESC", null]
    ]);
    let handle = builder_apply_ops(&handle, ops.to_string()).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert_eq!(
        query.raw,
        "SELECT DISTINCT ON (u.country, LOWER(u.email)) u.id, u.email, u.country FROM users u ORDER BY u.country ASC, u.email DESC"
    );
}

#[test]
fn select_for_update_renders() {
    let handle = builder_new(Some("postgres"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();
    let handle = builder_for_update(&handle).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert_eq!(query.raw, "SELECT u.id FROM users u FOR UPDATE");
}

#[test]
fn select_for_share_renders() {
    let handle = builder_new(Some("postgres"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();
    let handle = builder_for_share(&handle).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert_eq!(query.raw, "SELECT u.id FROM users u FOR SHARE");
}

#[test]
fn select_lock_modifier_renders_after_order_and_pagination() {
    let handle = builder_new(Some("postgres"));
    let ops = serde_json::json!([
        ["fta", "users", "u"],
        ["sc", ["u.id"]],
        ["ob", "u.id", "DESC", null],
        ["l", 5],
        ["o", 10],
        ["fu"],
        ["sl"]
    ]);
    let handle = builder_apply_ops(&handle, ops.to_string()).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert_eq!(
        query.raw,
        "SELECT u.id FROM users u ORDER BY u.id DESC LIMIT 5 OFFSET 10 FOR UPDATE SKIP LOCKED"
    );
}

#[test]
fn postgres_limit_only_renders_with_structured_pagination() {
    let handle = builder_new(Some("postgres"));
    let ops = serde_json::json!([["fta", "users", "u"], ["sc", ["u.id"]], ["l", 5]]);
    let handle = builder_apply_ops(&handle, ops.to_string()).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert_eq!(query.text, r#"SELECT "u"."id" FROM "users" "u" LIMIT ?"#);
    assert_eq!(query.raw, "SELECT u.id FROM users u LIMIT 5");
    assert_eq!(
        query.values,
        vec![Primitive::Number(serde_json::Number::from(5))]
    );
}

#[test]
fn postgres_limit_offset_renders_with_structured_pagination() {
    let handle = builder_new(Some("postgres"));
    let ops = serde_json::json!([["fta", "users", "u"], ["sc", ["u.id"]], ["l", 5], ["o", 10]]);
    let handle = builder_apply_ops(&handle, ops.to_string()).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert_eq!(
        query.text,
        r#"SELECT "u"."id" FROM "users" "u" LIMIT ? OFFSET ?"#
    );
    assert_eq!(query.raw, "SELECT u.id FROM users u LIMIT 5 OFFSET 10");
    assert_eq!(
        query.values,
        vec![
            Primitive::Number(serde_json::Number::from(5)),
            Primitive::Number(serde_json::Number::from(10))
        ]
    );
}

#[test]
fn mysql_rejects_offset_without_limit_before_sql_emission() {
    let handle = builder_new(Some("mysql"));
    let ops = serde_json::json!([["fta", "users", "u"], ["sc", ["u.id"]], ["o", 10]]);
    let handle = builder_apply_ops(&handle, ops.to_string()).unwrap();

    let error = builder_query(&handle).unwrap_err();
    assert_eq!(
        error,
        r#"Dialect "mysql" does not support OFFSET without LIMIT in this builder."#
    );
}

#[test]
fn mssql_limit_only_renders_top() {
    let handle = builder_new(Some("mssql"));
    let ops = serde_json::json!([["fta", "users", "u"], ["sc", ["u.id"]], ["l", 5]]);
    let handle = builder_apply_ops(&handle, ops.to_string()).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert_eq!(query.text, r#"SELECT TOP (?) "u"."id" FROM "users" "u""#);
    assert_eq!(query.raw, "SELECT TOP (5) u.id FROM users u");
    assert_eq!(
        query.values,
        vec![Primitive::Number(serde_json::Number::from(5))]
    );
}

#[test]
fn mssql_offset_only_renders_offset_rows() {
    let handle = builder_new(Some("mssql"));
    let ops = serde_json::json!([
        ["fta", "users", "u"],
        ["sc", ["u.id"]],
        ["ob", "u.id", "ASC", null],
        ["o", 10]
    ]);
    let handle = builder_apply_ops(&handle, ops.to_string()).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert_eq!(
        query.text,
        r#"SELECT "u"."id" FROM "users" "u" ORDER BY "u"."id" ASC OFFSET ? ROWS"#
    );
    assert_eq!(
        query.raw,
        "SELECT u.id FROM users u ORDER BY u.id ASC OFFSET 10 ROWS"
    );
    assert_eq!(
        query.values,
        vec![Primitive::Number(serde_json::Number::from(10))]
    );
}

#[test]
fn mssql_limit_offset_renders_offset_fetch() {
    let handle = builder_new(Some("mssql"));
    let ops = serde_json::json!([
        ["fta", "users", "u"],
        ["sc", ["u.id"]],
        ["ob", "u.id", "DESC", null],
        ["l", 5],
        ["o", 10]
    ]);
    let handle = builder_apply_ops(&handle, ops.to_string()).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert_eq!(
        query.text,
        r#"SELECT "u"."id" FROM "users" "u" ORDER BY "u"."id" DESC OFFSET ? ROWS FETCH NEXT ? ROWS ONLY"#
    );
    assert_eq!(
        query.raw,
        "SELECT u.id FROM users u ORDER BY u.id DESC OFFSET 10 ROWS FETCH NEXT 5 ROWS ONLY"
    );
    assert_eq!(
        query.values,
        vec![
            Primitive::Number(serde_json::Number::from(10)),
            Primitive::Number(serde_json::Number::from(5))
        ]
    );
}

#[test]
fn mssql_rejects_offset_without_order_by_before_sql_emission() {
    let handle = builder_new(Some("mssql"));
    let ops = serde_json::json!([["fta", "users", "u"], ["sc", ["u.id"]], ["o", 10]]);
    let handle = builder_apply_ops(&handle, ops.to_string()).unwrap();

    let error = builder_query(&handle).unwrap_err();
    assert_eq!(
        error,
        r#"Dialect "mssql" requires ORDER BY when using OFFSET/FETCH pagination in this builder."#
    );
}

#[test]
fn select_lock_modifier_requires_strength() {
    let handle = builder_new(Some("postgres"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();

    let error = builder_no_wait(&handle).unwrap_err();
    assert_eq!(
        error,
        "Select lock modifier requires a lock strength first."
    );
}

#[test]
fn unsupported_dialect_rejects_select_lock_before_sql_emission() {
    let handle = builder_new(Some("sqlite"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();
    let handle = builder_for_update(&handle).unwrap();

    let error = builder_query(&handle).unwrap_err();
    assert_eq!(
        error,
        r#"Dialect "sqlite" does not support FOR UPDATE in this builder."#
    );
}

#[test]
fn replacing_lock_strength_clears_existing_modifier() {
    let handle = builder_new(Some("postgres"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();
    let handle = builder_for_update(&handle).unwrap();
    let handle = builder_skip_locked(&handle).unwrap();
    let handle = builder_for_share(&handle).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert_eq!(query.raw, "SELECT u.id FROM users u FOR SHARE");
}

#[test]
fn insert_select_returning_columns_renders() {
    let rhs = builder_new(Some("postgres"));
    let rhs = builder_from_table_alias(&rhs, "users".to_string(), "u".to_string()).unwrap();
    let rhs = builder_select_columns(
        &rhs,
        serde_json::to_string(&vec!["u.id", "u.name"]).unwrap(),
    )
    .unwrap();

    let handle = builder_new(Some("postgres"));
    let handle = builder_insert_into(&handle, "teams".to_string()).unwrap();
    let handle =
        builder_columns(&handle, serde_json::to_string(&vec!["id", "name"]).unwrap()).unwrap();
    let handle = builder_insert_select_handle(&handle, &rhs).unwrap();
    let handle =
        builder_returning_columns(&handle, serde_json::to_string(&vec!["id"]).unwrap()).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert_eq!(
        query.raw,
        "INSERT INTO teams (id, name) SELECT u.id, u.name FROM users u RETURNING id"
    );
}

#[test]
fn update_returning_aliased_renders() {
    let handle = builder_new(Some("postgres"));
    let handle = builder_update(&handle, "users".to_string()).unwrap();
    let handle = builder_set(
        &handle,
        serde_json::to_string(&vec![(
            "name".to_string(),
            SqlValue::Raw(sql::raw("UPPER(name)".to_string())),
        )])
        .unwrap(),
    )
    .unwrap();
    let predicate = sql::sql(
        vec!["".to_string(), " = ".to_string(), "".to_string()],
        vec![
            SqlValue::Identifier(sql::identifier(vec!["id".to_string()]).unwrap()),
            SqlValue::Primitive(Primitive::String("u_1".to_string())),
        ],
    )
    .unwrap();
    let handle = builder_where(&handle, serde_json::to_string(&predicate).unwrap()).unwrap();
    let entries = serde_json::json!([
        {
            "alias": "userId",
            "expr": { "type": "ref", "parts": ["id"], "text": "", "raw": "", "values": [] }
        }
    ]);
    let handle = builder_returning_aliased(&handle, entries.to_string()).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert_eq!(
        query.raw,
        "UPDATE users SET name = UPPER(name) WHERE id = 'u_1' RETURNING id AS userId"
    );
}

#[test]
fn delete_returning_columns_renders() {
    let handle = builder_new(Some("postgres"));
    let handle = builder_delete_from(&handle, "users".to_string()).unwrap();
    let predicate = sql::sql(
        vec!["".to_string(), " = ".to_string(), "".to_string()],
        vec![
            SqlValue::Identifier(sql::identifier(vec!["country".to_string()]).unwrap()),
            SqlValue::Primitive(Primitive::String("NZ".to_string())),
        ],
    )
    .unwrap();
    let handle = builder_where(&handle, serde_json::to_string(&predicate).unwrap()).unwrap();
    let handle =
        builder_returning_columns(&handle, serde_json::to_string(&vec!["id"]).unwrap()).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert_eq!(
        query.text,
        r#"DELETE FROM "users" WHERE "country" = ? RETURNING "id""#
    );
    assert_eq!(query.values, vec![Primitive::String("NZ".to_string())]);
}

#[test]
fn dml_qualified_table_names_render_as_identifier_parts() {
    let insert = builder_insert_into(&builder_new(Some("duckdb")), "platform.reps".to_string())
        .and_then(|handle| {
            builder_values_insert(
                &handle,
                rows_json(
                    &["rep_id"],
                    vec![vec![SqlValue::Primitive(Primitive::Number(
                        serde_json::Number::from(1),
                    ))]],
                ),
            )
        })
        .unwrap();
    let update = builder_update(&builder_new(Some("duckdb")), "platform.reps".to_string())
        .and_then(|handle| {
            builder_set(
                &handle,
                serde_json::to_string(&vec![(
                    "rep_name".to_string(),
                    SqlValue::Primitive(Primitive::String("Ada".to_string())),
                )])
                .unwrap(),
            )
        })
        .unwrap();
    let delete =
        builder_delete_from(&builder_new(Some("duckdb")), "platform.reps".to_string()).unwrap();

    assert_eq!(
        parse_query(builder_query(&insert).unwrap()).text,
        r#"INSERT INTO "platform"."reps" ("rep_id") VALUES (?)"#
    );
    assert_eq!(
        parse_query(builder_query(&update).unwrap()).text,
        r#"UPDATE "platform"."reps" SET "rep_name" = ?"#
    );
    assert_eq!(
        parse_query(builder_query(&delete).unwrap()).text,
        r#"DELETE FROM "platform"."reps""#
    );
}

#[test]
fn unsupported_dialect_rejects_returning_before_sql_emission() {
    let handle = builder_new(Some("mysql"));
    let handle = builder_insert_into(&handle, "users".to_string()).unwrap();
    let handle = builder_values_insert(
        &handle,
        rows_json(
            &["id"],
            vec![vec![SqlValue::Primitive(Primitive::String(
                "u_1".to_string(),
            ))]],
        ),
    )
    .unwrap();
    let handle =
        builder_returning_columns(&handle, serde_json::to_string(&vec!["id"]).unwrap()).unwrap();

    let error = builder_query(&handle).unwrap_err();
    assert_eq!(
        error,
        r#"Dialect "mysql" does not support RETURNING in this builder."#
    );
}

#[test]
fn insert_values_on_conflict_columns_do_nothing_renders() {
    let handle = builder_new(Some("postgres"));
    let handle = builder_insert_into(&handle, "users".to_string()).unwrap();
    let handle = builder_values_insert(
        &handle,
        rows_json(
            &["id", "name"],
            vec![vec![
                SqlValue::Primitive(Primitive::String("u_1".to_string())),
                SqlValue::Primitive(Primitive::String("Ada".to_string())),
            ]],
        ),
    )
    .unwrap();
    let handle =
        builder_on_conflict_columns(&handle, serde_json::to_string(&vec!["id"]).unwrap()).unwrap();
    let handle = builder_do_nothing(&handle).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert_eq!(
        query.raw,
        "INSERT INTO users (id, name) VALUES ('u_1', 'Ada') ON CONFLICT (id) DO NOTHING"
    );
}

#[test]
fn insert_values_on_conflict_constraint_do_nothing_renders() {
    let handle = builder_new(Some("postgres"));
    let handle = builder_insert_into(&handle, "users".to_string()).unwrap();
    let handle = builder_values_insert(
        &handle,
        rows_json(
            &["email"],
            vec![vec![SqlValue::Primitive(Primitive::String(
                "ada@example.com".to_string(),
            ))]],
        ),
    )
    .unwrap();
    let handle = builder_on_conflict_constraint(&handle, "users_email_key".to_string()).unwrap();
    let handle = builder_do_nothing(&handle).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert_eq!(
        query.text,
        r#"INSERT INTO "users" ("email") VALUES (?) ON CONFLICT ON CONSTRAINT "users_email_key" DO NOTHING"#
    );
}

#[test]
fn insert_select_on_conflict_do_nothing_renders() {
    let rhs = builder_new(Some("postgres"));
    let rhs = builder_from_table_alias(&rhs, "users".to_string(), "u".to_string()).unwrap();
    let rhs = builder_select_columns(
        &rhs,
        serde_json::to_string(&vec!["u.id", "u.name"]).unwrap(),
    )
    .unwrap();

    let handle = builder_new(Some("postgres"));
    let handle = builder_insert_into(&handle, "teams".to_string()).unwrap();
    let handle =
        builder_columns(&handle, serde_json::to_string(&vec!["id", "name"]).unwrap()).unwrap();
    let handle = builder_insert_select_handle(&handle, &rhs).unwrap();
    let handle =
        builder_on_conflict_columns(&handle, serde_json::to_string(&vec!["id"]).unwrap()).unwrap();
    let handle = builder_do_nothing(&handle).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert_eq!(
        query.raw,
        "INSERT INTO teams (id, name) SELECT u.id, u.name FROM users u ON CONFLICT (id) DO NOTHING"
    );
}

#[test]
fn mysql_targetless_do_nothing_renders_insert_ignore() {
    let handle = builder_new(Some("mysql"));
    let handle = builder_insert_into(&handle, "users".to_string()).unwrap();
    let handle = builder_values_insert(
        &handle,
        rows_json(
            &["id"],
            vec![vec![SqlValue::Primitive(Primitive::String(
                "u_1".to_string(),
            ))]],
        ),
    )
    .unwrap();
    let handle = builder_do_nothing(&handle).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert_eq!(query.raw, "INSERT IGNORE INTO users (id) VALUES ('u_1')");
}

#[test]
fn unsupported_dialect_rejects_do_nothing_before_sql_emission() {
    let handle = builder_new(Some("mssql"));
    let handle = builder_insert_into(&handle, "users".to_string()).unwrap();
    let handle = builder_values_insert(
        &handle,
        rows_json(
            &["id"],
            vec![vec![SqlValue::Primitive(Primitive::String(
                "u_1".to_string(),
            ))]],
        ),
    )
    .unwrap();
    let handle = builder_do_nothing(&handle).unwrap();

    let error = builder_query(&handle).unwrap_err();
    assert_eq!(
        error,
        r#"Dialect "mssql" does not support DO NOTHING insert conflict handling."#
    );
}

#[test]
fn mysql_rejects_explicit_conflict_target_for_do_nothing() {
    let handle = builder_new(Some("mysql"));
    let handle = builder_insert_into(&handle, "users".to_string()).unwrap();
    let handle = builder_values_insert(
        &handle,
        rows_json(
            &["id"],
            vec![vec![SqlValue::Primitive(Primitive::String(
                "u_1".to_string(),
            ))]],
        ),
    )
    .unwrap();
    let handle =
        builder_on_conflict_columns(&handle, serde_json::to_string(&vec!["id"]).unwrap()).unwrap();
    let handle = builder_do_nothing(&handle).unwrap();

    let error = builder_query(&handle).unwrap_err();
    assert_eq!(
        error,
        r#"Dialect "mysql" does not support explicit conflict targets for DO NOTHING inserts."#
    );
}

#[test]
fn insert_conflict_target_requires_action() {
    let handle = builder_new(Some("postgres"));
    let handle = builder_insert_into(&handle, "users".to_string()).unwrap();
    let handle = builder_values_insert(
        &handle,
        rows_json(
            &["id"],
            vec![vec![SqlValue::Primitive(Primitive::String(
                "u_1".to_string(),
            ))]],
        ),
    )
    .unwrap();
    let handle =
        builder_on_conflict_columns(&handle, serde_json::to_string(&vec!["id"]).unwrap()).unwrap();

    let error = builder_query(&handle).unwrap_err();
    assert_eq!(error, "Insert conflict target requires an action.");
}

#[test]
fn insert_conflict_rejects_mixed_target_kinds() {
    let handle = builder_new(Some("postgres"));
    let handle = builder_insert_into(&handle, "users".to_string()).unwrap();
    let handle = builder_values_insert(
        &handle,
        rows_json(
            &["id"],
            vec![vec![SqlValue::Primitive(Primitive::String(
                "u_1".to_string(),
            ))]],
        ),
    )
    .unwrap();
    let handle =
        builder_on_conflict_columns(&handle, serde_json::to_string(&vec!["id"]).unwrap()).unwrap();

    let error = builder_on_conflict_constraint(&handle, "users_email_key".to_string()).unwrap_err();
    assert_eq!(
        error,
        "Insert conflict target cannot mix columns and constraint targets."
    );
}

#[test]
fn insert_values_on_conflict_do_update_renders() {
    let handle = builder_new(Some("postgres"));
    let handle = builder_insert_into(&handle, "users".to_string()).unwrap();
    let handle = builder_values_insert(
        &handle,
        rows_json(
            &["id", "name"],
            vec![vec![
                SqlValue::Primitive(Primitive::String("u_1".to_string())),
                SqlValue::Primitive(Primitive::String("Ada".to_string())),
            ]],
        ),
    )
    .unwrap();
    let handle =
        builder_on_conflict_columns(&handle, serde_json::to_string(&vec!["id"]).unwrap()).unwrap();
    let assignments = serde_json::json!([
        [
            "name",
            { "type": "excluded", "column": "name", "text": "", "raw": "", "values": [] }
        ]
    ]);
    let handle = builder_do_update_set(&handle, assignments.to_string()).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert_eq!(
        query.raw,
        "INSERT INTO users (id, name) VALUES ('u_1', 'Ada') ON CONFLICT (id) DO UPDATE SET name = excluded.name"
    );
}

#[test]
fn insert_conflict_do_update_where_renders() {
    let handle = builder_new(Some("postgres"));
    let handle = builder_insert_into(&handle, "users".to_string()).unwrap();
    let handle = builder_values_insert(
        &handle,
        rows_json(
            &["id", "country"],
            vec![vec![
                SqlValue::Primitive(Primitive::String("u_1".to_string())),
                SqlValue::Primitive(Primitive::String("AU".to_string())),
            ]],
        ),
    )
    .unwrap();
    let handle =
        builder_on_conflict_columns(&handle, serde_json::to_string(&vec!["id"]).unwrap()).unwrap();
    let assignments = serde_json::json!([
        [
            "country",
            { "type": "excluded", "column": "country", "text": "", "raw": "", "values": [] }
        ]
    ]);
    let handle = builder_do_update_set(&handle, assignments.to_string()).unwrap();
    let predicate = serde_json::json!({
        "type": "cmp",
        "left": { "type": "ref", "parts": ["users", "country"], "text": "", "raw": "", "values": [] },
        "op": "<>",
        "right": { "type": "excluded", "column": "country", "text": "", "raw": "", "values": [] }
    });
    let handle = builder_conflict_where(&handle, predicate.to_string()).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert_eq!(
        query.raw,
        "INSERT INTO users (id, country) VALUES ('u_1', 'AU') ON CONFLICT (id) DO UPDATE SET country = excluded.country WHERE users.country <> excluded.country"
    );
}

#[test]
fn mysql_conflict_update_renders_on_duplicate_key() {
    let handle = builder_new(Some("mysql"));
    let handle = builder_insert_into(&handle, "users".to_string()).unwrap();
    let handle = builder_values_insert(
        &handle,
        rows_json(
            &["id", "name"],
            vec![vec![
                SqlValue::Primitive(Primitive::String("u_1".to_string())),
                SqlValue::Primitive(Primitive::String("Ada".to_string())),
            ]],
        ),
    )
    .unwrap();
    let assignments = serde_json::json!([
        [
            "name",
            { "type": "excluded", "column": "name", "text": "", "raw": "", "values": [] }
        ]
    ]);
    let handle = builder_do_update_set(&handle, assignments.to_string()).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert_eq!(
        query.raw,
        "INSERT INTO users (id, name) VALUES ('u_1', 'Ada') ON DUPLICATE KEY UPDATE name = VALUES(name)"
    );
}

#[test]
fn mysql_rejects_conflict_target_for_do_update() {
    let handle = builder_new(Some("mysql"));
    let handle = builder_insert_into(&handle, "users".to_string()).unwrap();
    let handle = builder_values_insert(
        &handle,
        rows_json(
            &["id"],
            vec![vec![SqlValue::Primitive(Primitive::String(
                "u_1".to_string(),
            ))]],
        ),
    )
    .unwrap();
    let handle =
        builder_on_conflict_columns(&handle, serde_json::to_string(&vec!["id"]).unwrap()).unwrap();
    let assignments = serde_json::json!([
        [
            "id",
            { "type": "excluded", "column": "id", "text": "", "raw": "", "values": [] }
        ]
    ]);
    let handle = builder_do_update_set(&handle, assignments.to_string()).unwrap();

    let error = builder_query(&handle).unwrap_err();
    assert_eq!(
        error,
        r#"Dialect "mysql" does not support explicit conflict targets for DO UPDATE inserts."#
    );
}

// ─── NULLS FIRST / LAST — top-level ORDER BY ─────────────────────────────────

#[test]
fn order_by_nulls_first_renders_for_postgres() {
    let handle = builder_new(Some("postgres"));
    let ops = serde_json::json!([
        ["fta", "users", "u"],
        ["sc", ["u.id"]],
        ["ob", "u.id", "ASC", "FIRST"]
    ]);
    let handle = builder_apply_ops(&handle, ops.to_string()).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert_eq!(
        query.raw,
        "SELECT u.id FROM users u ORDER BY u.id ASC NULLS FIRST"
    );
}

#[test]
fn order_by_nulls_last_renders_for_duckdb() {
    let handle = builder_new(Some("duckdb"));
    let ops = serde_json::json!([
        ["fta", "events", "e"],
        ["sc", ["e.ts"]],
        ["ob", "e.ts", "DESC", "LAST"]
    ]);
    let handle = builder_apply_ops(&handle, ops.to_string()).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert_eq!(
        query.raw,
        "SELECT e.ts FROM events e ORDER BY e.ts DESC NULLS LAST"
    );
}

#[test]
fn order_by_direction_only_unaffected_for_mysql() {
    let handle = builder_new(Some("mysql"));
    let ops = serde_json::json!([
        ["fta", "users", "u"],
        ["sc", ["u.id"]],
        ["ob", "u.id", "ASC", null]
    ]);
    let handle = builder_apply_ops(&handle, ops.to_string()).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert_eq!(query.raw, "SELECT u.id FROM users u ORDER BY u.id ASC");
}

#[test]
fn order_by_nulls_rejected_for_mysql() {
    let handle = builder_new(Some("mysql"));
    let ops = serde_json::json!([
        ["fta", "users", "u"],
        ["sc", ["u.id"]],
        ["ob", "u.id", "ASC", "FIRST"]
    ]);

    let error = builder_apply_ops(&handle, ops.to_string()).unwrap_err();
    assert_eq!(
        error,
        r#"Dialect "mysql" does not support NULLS FIRST/LAST in ORDER BY."#
    );
}

// ─── Join type and dialect validation ────────────────────────────────────────

#[test]
fn unknown_join_type_is_rejected() {
    let handle = builder_new(Some("postgres"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    let err =
        builder_join_table(&handle, "BANANA JOIN".to_string(), "orders".to_string()).unwrap_err();
    assert!(err.contains("Unknown join type"), "got: {err}");
}

#[test]
fn cross_join_commits_without_predicate() {
    let handle = builder_new(Some("postgres"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    let handle = builder_join_table(&handle, "CROSS JOIN".to_string(), "tags".to_string()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert!(query.raw.contains("CROSS JOIN tags"), "got: {}", query.raw);
}

#[test]
fn inner_join_with_on_renders_for_postgres() {
    let pred = serde_json::json!({"text": "? = ?", "raw": "u.id = o.user_id", "values": []});
    let handle = builder_new(Some("postgres"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    let handle = builder_join_table_alias(
        &handle,
        "INNER JOIN".to_string(),
        "orders".to_string(),
        "o".to_string(),
    )
    .unwrap();
    let handle = builder_on(&handle, pred.to_string()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert!(
        query.raw.contains("INNER JOIN orders o ON"),
        "got: {}",
        query.raw
    );
}

#[test]
fn left_join_with_using_renders_for_postgres() {
    let handle = builder_new(Some("postgres"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    let handle = builder_join_table_alias(
        &handle,
        "LEFT JOIN".to_string(),
        "orders".to_string(),
        "o".to_string(),
    )
    .unwrap();
    let handle =
        builder_using_columns(&handle, serde_json::to_string(&vec!["user_id"]).unwrap()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert!(
        query.raw.contains("LEFT JOIN orders o USING (user_id)"),
        "got: {}",
        query.raw
    );
}

#[test]
fn right_join_accepted_for_mysql() {
    let pred = serde_json::json!({"text": "? = ?", "raw": "u.id = o.user_id", "values": []});
    let handle = builder_new(Some("mysql"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    let handle = builder_join_table_alias(
        &handle,
        "RIGHT JOIN".to_string(),
        "orders".to_string(),
        "o".to_string(),
    )
    .unwrap();
    let handle = builder_on(&handle, pred.to_string()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert!(
        query.raw.contains("RIGHT JOIN orders o ON"),
        "got: {}",
        query.raw
    );
}

#[test]
fn right_join_rejected_for_sqlite() {
    let handle = builder_new(Some("sqlite"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    let handle = builder_join_table_alias(
        &handle,
        "RIGHT JOIN".to_string(),
        "orders".to_string(),
        "o".to_string(),
    )
    .unwrap();
    let pred = serde_json::json!({"text": "? = ?", "raw": "u.id = o.user_id", "values": []});
    let handle = builder_on(&handle, pred.to_string()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();

    let err = builder_query(&handle).unwrap_err();
    assert!(
        err.contains("RIGHT JOIN") && err.contains("sqlite"),
        "got: {err}"
    );
}

#[test]
fn full_outer_join_accepted_for_postgres() {
    let pred = serde_json::json!({"text": "? = ?", "raw": "u.id = o.user_id", "values": []});
    let handle = builder_new(Some("postgres"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    let handle = builder_join_table_alias(
        &handle,
        "FULL OUTER JOIN".to_string(),
        "orders".to_string(),
        "o".to_string(),
    )
    .unwrap();
    let handle = builder_on(&handle, pred.to_string()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert!(
        query.raw.contains("FULL OUTER JOIN orders o ON"),
        "got: {}",
        query.raw
    );
}

#[test]
fn full_outer_join_rejected_for_mysql() {
    let pred = serde_json::json!({"text": "? = ?", "raw": "u.id = o.user_id", "values": []});
    let handle = builder_new(Some("mysql"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    let handle = builder_join_table_alias(
        &handle,
        "FULL OUTER JOIN".to_string(),
        "orders".to_string(),
        "o".to_string(),
    )
    .unwrap();
    let handle = builder_on(&handle, pred.to_string()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();

    let err = builder_query(&handle).unwrap_err();
    assert!(
        err.contains("FULL OUTER JOIN") && err.contains("mysql"),
        "got: {err}"
    );
}

#[test]
fn using_syntax_rejected_for_mssql() {
    let handle = builder_new(Some("mssql"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    let handle = builder_join_table_alias(
        &handle,
        "INNER JOIN".to_string(),
        "orders".to_string(),
        "o".to_string(),
    )
    .unwrap();
    let handle =
        builder_using_columns(&handle, serde_json::to_string(&vec!["user_id"]).unwrap()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();

    let err = builder_query(&handle).unwrap_err();
    assert!(err.contains("USING") && err.contains("mssql"), "got: {err}");
}

#[test]
fn using_syntax_accepted_for_postgres() {
    let handle = builder_new(Some("postgres"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    let handle = builder_join_table_alias(
        &handle,
        "INNER JOIN".to_string(),
        "orders".to_string(),
        "o".to_string(),
    )
    .unwrap();
    let handle =
        builder_using_columns(&handle, serde_json::to_string(&vec!["user_id"]).unwrap()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert!(query.raw.contains("USING (user_id)"), "got: {}", query.raw);
}

#[test]
fn join_without_predicate_fails_at_render() {
    let handle = builder_new(Some("postgres"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    // Open a join but never call on() or using()
    let handle = builder_join_table_alias(
        &handle,
        "INNER JOIN".to_string(),
        "orders".to_string(),
        "o".to_string(),
    )
    .unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();

    let err = builder_query(&handle).unwrap_err();
    assert!(err.contains("Join predicate is required"), "got: {err}");
}

#[test]
fn join_aliases_left_outer_join_keyword() {
    let pred = serde_json::json!({"text": "? = ?", "raw": "u.id = o.user_id", "values": []});
    let handle = builder_new(Some("postgres"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    // "LEFT OUTER JOIN" is an accepted alias for LEFT JOIN
    let handle = builder_join_table_alias(
        &handle,
        "LEFT OUTER JOIN".to_string(),
        "orders".to_string(),
        "o".to_string(),
    )
    .unwrap();
    let handle = builder_on(&handle, pred.to_string()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    // Canonical output should be "LEFT JOIN", not "LEFT OUTER JOIN"
    assert!(
        query.raw.contains("LEFT JOIN orders o ON"),
        "got: {}",
        query.raw
    );
}

#[test]
fn on_columns_renders_inner_join_predicate() {
    let handle = builder_new(Some("postgres"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    let handle = builder_join_table_alias(
        &handle,
        "INNER JOIN".to_string(),
        "orders".to_string(),
        "o".to_string(),
    )
    .unwrap();
    let handle = builder_on_columns(
        &handle,
        serde_json::to_string(&vec![["u.id", "o.user_id"]]).unwrap(),
    )
    .unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert!(
        query.raw.contains("INNER JOIN orders o ON"),
        "got: {}",
        query.raw
    );
}

#[test]
fn order_by_nulls_rejected_for_mssql() {
    let handle = builder_new(Some("mssql"));
    let ops = serde_json::json!([
        ["fta", "users", "u"],
        ["sc", ["u.id"]],
        ["ob", "u.id", "DESC", "LAST"]
    ]);

    let error = builder_apply_ops(&handle, ops.to_string()).unwrap_err();
    assert_eq!(
        error,
        r#"Dialect "mssql" does not support NULLS FIRST/LAST in ORDER BY."#
    );
}

// ─── NULLS FIRST / LAST — window ORDER BY ────────────────────────────────────

fn window_count_expr() -> SqlQuery {
    SqlQuery {
        text: "COUNT(*)".to_string(),
        raw: "COUNT(*)".to_string(),
        values: vec![],
    }
}

fn window_col_expr(col: &str) -> SqlQuery {
    SqlQuery {
        text: col.to_string(),
        raw: col.to_string(),
        values: vec![],
    }
}

#[test]
fn window_order_by_nulls_first_renders_for_postgres() {
    let result = over_clause(
        window_count_expr(),
        vec![],
        vec![WindowOrderItem {
            expression: window_col_expr("score"),
            direction: Some("DESC".to_string()),
            nulls: Some("FIRST".to_string()),
        }],
        &Dialect::Postgres,
    )
    .unwrap();

    assert!(
        result.raw.contains("NULLS FIRST"),
        "expected NULLS FIRST in: {}",
        result.raw
    );
    assert_eq!(
        result.raw,
        "COUNT(*) OVER (ORDER BY score DESC NULLS FIRST)"
    );
}

#[test]
fn window_order_by_nulls_rejected_for_mssql() {
    let error = over_clause(
        window_count_expr(),
        vec![],
        vec![WindowOrderItem {
            expression: window_col_expr("score"),
            direction: Some("DESC".to_string()),
            nulls: Some("LAST".to_string()),
        }],
        &Dialect::Mssql,
    )
    .unwrap_err();

    assert_eq!(
        error,
        r#"Dialect "mssql" does not support NULLS FIRST/LAST in ORDER BY."#
    );
}

#[test]
fn window_order_by_direction_only_unaffected_for_mssql() {
    let result = over_clause(
        window_count_expr(),
        vec![],
        vec![WindowOrderItem {
            expression: window_col_expr("score"),
            direction: Some("DESC".to_string()),
            nulls: None,
        }],
        &Dialect::Mssql,
    )
    .unwrap();

    assert_eq!(result.raw, "COUNT(*) OVER (ORDER BY score DESC)");
}

// ─── Dialect policy — hard errors ────────────────────────────────────────────

#[test]
fn returning_on_mysql_is_hard_error() {
    let handle = builder_new(Some("mysql"));
    let handle = builder_insert_into(&handle, "users".to_string()).unwrap();
    let handle = builder_values_insert(
        &handle,
        rows_json(
            &["name"],
            vec![vec![SqlValue::Primitive(Primitive::String(
                "Alice".to_string(),
            ))]],
        ),
    )
    .unwrap();
    let handle =
        builder_returning_columns(&handle, serde_json::to_string(&vec!["id"]).unwrap()).unwrap();

    let err = builder_query(&handle).unwrap_err();
    assert!(
        err.contains("RETURNING"),
        "error should mention RETURNING, got: {err}"
    );
    assert!(
        err.contains("mysql"),
        "error should mention the dialect, got: {err}"
    );
}

#[test]
fn returning_on_postgres_still_renders() {
    let handle = builder_new(Some("postgres"));
    let handle = builder_insert_into(&handle, "users".to_string()).unwrap();
    let handle = builder_values_insert(
        &handle,
        rows_json(
            &["name"],
            vec![vec![SqlValue::Primitive(Primitive::String(
                "Alice".to_string(),
            ))]],
        ),
    )
    .unwrap();
    let handle =
        builder_returning_columns(&handle, serde_json::to_string(&vec!["id"]).unwrap()).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert!(
        query.raw.contains("RETURNING id"),
        "expected RETURNING in: {}",
        query.raw
    );
}

// ─── ILIKE — dialect-specific rendering and fallback rewrite ─────────────────

#[test]
fn ilike_on_postgres_renders_natively() {
    let handle = builder_new(Some("postgres"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();
    let ilike_expr = serde_json::json!({
        "type": "ilike",
        "value": { "type": "ref", "parts": ["u", "email"] },
        "pattern": { "type": "val", "value": "%@example.com" }
    });
    let handle = builder_where(&handle, ilike_expr.to_string()).unwrap();

    let parts = registry_get_parts(&handle).unwrap();
    let result = build_query_with_context(&parts).unwrap();
    assert!(
        result.query.raw.contains("ILIKE"),
        "expected native ILIKE on postgres, got: {}",
        result.query.raw
    );
    assert!(
        result.warnings.is_empty(),
        "expected no warnings on postgres"
    );
}

#[test]
fn ilike_on_mysql_rewrites_to_lower_like_lower_with_warning() {
    let handle = builder_new(Some("mysql"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();
    let ilike_expr = serde_json::json!({
        "type": "ilike",
        "value": { "type": "ref", "parts": ["u", "email"] },
        "pattern": { "type": "val", "value": "%@example.com" }
    });
    let handle = builder_where(&handle, ilike_expr.to_string()).unwrap();

    let parts = registry_get_parts(&handle).unwrap();
    let result = build_query_with_context(&parts).unwrap();
    assert!(
        result.query.raw.contains("LOWER") && result.query.raw.contains("LIKE"),
        "expected LOWER(...) LIKE rewrite on mysql, got: {}",
        result.query.raw
    );
    assert!(
        !result.query.raw.contains("ILIKE"),
        "ILIKE must not appear in rewritten output"
    );
    assert_eq!(result.warnings.len(), 1, "expected exactly one warning");
    let w = &result.warnings[0];
    assert_eq!(w.feature, "ILIKE");
    assert_eq!(w.dialect, "mysql");
}

#[test]
fn dialect_strict_promotes_ilike_rewrite_to_error() {
    let handle = builder_new(Some("mysql"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();
    let ilike_expr = serde_json::json!({
        "type": "ilike",
        "value": { "type": "ref", "parts": ["u", "email"] },
        "pattern": { "type": "val", "value": "%@example.com" }
    });
    let handle = builder_where(&handle, ilike_expr.to_string()).unwrap();
    let handle = builder_dialect_strict(&handle, true).unwrap();

    let err = builder_query(&handle).unwrap_err();
    assert!(
        err.contains("ILIKE") && err.contains("mysql"),
        "expected strict error mentioning ILIKE and mysql, got: {err}"
    );
}

#[test]
fn render_result_has_empty_warnings_on_clean_query() {
    let handle = builder_new(Some("postgres"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();

    let parts = registry_get_parts(&handle).unwrap();
    let result = build_query_with_context(&parts).unwrap();
    assert!(result.warnings.is_empty());
}

// ── Phase 1: SqlBackend trait tracer bullet ──────────────────────────────────

#[test]
fn postgres_backend_reports_returning_support() {
    use crate::backend::backend_for_dialect_name;
    let backend = backend_for_dialect_name("postgres");
    assert!(
        backend.capabilities().returning,
        "Postgres backend must report RETURNING support"
    );
}

#[test]
fn unknown_backend_reports_no_returning_support() {
    use crate::backend::backend_for_dialect_name;
    let backend = backend_for_dialect_name("cassandra");
    assert!(
        !backend.capabilities().returning,
        "Unknown/cassandra backend must not claim RETURNING support"
    );
}

// ── Phase 2: MySQL backend capabilities ──────────────────────────────────────

#[test]
fn mysql_backend_uses_mysql_conflict_style() {
    use crate::backend::backend_for_dialect_name;
    use crate::dialect::InsertConflictStyle;
    let backend = backend_for_dialect_name("mysql");
    assert_eq!(
        backend.capabilities().insert_conflict,
        InsertConflictStyle::MySql,
        "MySQL backend must report MySql insert conflict style"
    );
}

#[test]
fn mysql_backend_reports_no_returning_support() {
    use crate::backend::backend_for_dialect_name;
    let backend = backend_for_dialect_name("mysql");
    assert!(
        !backend.capabilities().returning,
        "MySQL backend must not claim RETURNING support"
    );
}

// ── Phase 3: SQLite backend capabilities ─────────────────────────────────────

#[test]
fn sqlite_backend_reports_no_right_join_support() {
    use crate::backend::backend_for_dialect_name;
    let backend = backend_for_dialect_name("sqlite");
    assert!(
        !backend.capabilities().joins.right_join,
        "SQLite backend must not claim RIGHT JOIN support"
    );
}

#[test]
fn sqlite_backend_reports_returning_support() {
    use crate::backend::backend_for_dialect_name;
    let backend = backend_for_dialect_name("sqlite");
    assert!(
        backend.capabilities().returning,
        "SQLite backend must report RETURNING support"
    );
}

// ── Phase 4: backend-driven placeholder compilation ──────────────────────────

#[test]
fn postgres_backend_compiles_dollar_numbered_placeholders() {
    use crate::backend::backend_for_dialect_name;
    let backend = backend_for_dialect_name("postgres");
    let query = SqlQuery {
        text: "a = ? AND b = ?".to_string(),
        raw: "a = 1 AND b = 2".to_string(),
        values: vec![
            Primitive::Number(serde_json::Number::from(1)),
            Primitive::Number(serde_json::Number::from(2)),
        ],
    };
    let compiled = backend.compile_placeholders(query).unwrap();
    assert_eq!(compiled.text, "a = $1 AND b = $2");
    assert_eq!(compiled.raw, "a = 1 AND b = 2");
}

#[test]
fn duckdb_backend_compiles_dollar_numbered_placeholders() {
    use crate::backend::backend_for_dialect_name;
    let backend = backend_for_dialect_name("duckdb");
    let query = SqlQuery {
        text: "LIMIT ? OFFSET ?".to_string(),
        raw: "LIMIT 5 OFFSET 10".to_string(),
        values: vec![
            Primitive::Number(serde_json::Number::from(5)),
            Primitive::Number(serde_json::Number::from(10)),
        ],
    };
    let compiled = backend.compile_placeholders(query).unwrap();
    assert_eq!(compiled.text, "LIMIT $1 OFFSET $2");
}

#[test]
fn mysql_backend_keeps_question_mark_placeholders() {
    use crate::backend::backend_for_dialect_name;
    let backend = backend_for_dialect_name("mysql");
    let query = SqlQuery {
        text: "a = ? AND b = ?".to_string(),
        raw: "a = 1 AND b = 2".to_string(),
        values: vec![
            Primitive::Number(serde_json::Number::from(1)),
            Primitive::Number(serde_json::Number::from(2)),
        ],
    };
    let compiled = backend.compile_placeholders(query).unwrap();
    assert_eq!(compiled.text, "a = ? AND b = ?");
}

#[test]
fn mssql_backend_compiles_at_p_named_placeholders() {
    use crate::backend::backend_for_dialect_name;
    let backend = backend_for_dialect_name("mssql");
    let query = SqlQuery {
        text: "a = ? AND b = ?".to_string(),
        raw: "a = 1 AND b = 2".to_string(),
        values: vec![
            Primitive::Number(serde_json::Number::from(1)),
            Primitive::Number(serde_json::Number::from(2)),
        ],
    };
    let compiled = backend.compile_placeholders(query).unwrap();
    assert_eq!(compiled.text, "a = @p1 AND b = @p2");
}

#[test]
fn unknown_backend_rejects_placeholder_compilation() {
    use crate::backend::backend_for_dialect_name;
    let backend = backend_for_dialect_name("mongodb");
    let query = SqlQuery {
        text: "a = ?".to_string(),
        raw: "a = 1".to_string(),
        values: vec![Primitive::Number(serde_json::Number::from(1))],
    };
    let error = backend.compile_placeholders(query).unwrap_err();
    assert_eq!(
        error,
        "Dialect \"mongodb\" does not support SQL placeholder compilation in this builder."
    );
}

#[test]
fn same_handle_output_accessors_compile_once_then_reuse_cache() {
    let _guard = cache_test_guard();
    test_reset_build_execution_count();

    let handle = builder_new(Some("postgres"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();

    let query_json = builder_query(&handle).unwrap();
    let query = parse_query(query_json);
    assert_eq!(query.raw, "SELECT u.id FROM users u");
    let text = builder_text(&handle).unwrap();
    let raw = builder_raw(&handle).unwrap();
    let values_json = builder_values(&handle).unwrap();
    let values: Vec<Primitive> = serde_json::from_str(&values_json).unwrap();

    assert_eq!(text, r#"SELECT "u"."id" FROM "users" "u""#);
    assert_eq!(raw, "SELECT u.id FROM users u");
    assert!(values.is_empty());
    assert_eq!(test_build_execution_count_for_handle(&handle).unwrap(), 1);
}

#[test]
fn mutated_handle_does_not_reuse_cached_query_from_parent_handle() {
    let _guard = cache_test_guard();
    test_reset_build_execution_count();

    let base = builder_new(Some("postgres"));
    let base = builder_from_table_alias(&base, "users".to_string(), "u".to_string()).unwrap();
    let base =
        builder_select_columns(&base, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();

    let base_first = builder_raw(&base).unwrap();
    assert_eq!(base_first, "SELECT u.id FROM users u");
    assert_eq!(test_build_execution_count_for_handle(&base).unwrap(), 1);

    let pred = serde_json::json!({"text":"? = ?","raw":"u.country = 'AU'","values":[]});
    let extended = builder_where(&base, pred.to_string()).unwrap();
    let extended_raw = builder_raw(&extended).unwrap();

    assert!(extended_raw.contains("WHERE"));
    assert_eq!(test_build_execution_count_for_handle(&base).unwrap(), 1);
    assert_eq!(test_build_execution_count_for_handle(&extended).unwrap(), 1);
}

#[test]
fn parent_and_branch_handles_cache_independently_after_branching() {
    let _guard = cache_test_guard();
    test_reset_build_execution_count();

    let base = builder_new(Some("postgres"));
    let base = builder_from_table_alias(&base, "users".to_string(), "u".to_string()).unwrap();
    let base =
        builder_select_columns(&base, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();

    let pred = serde_json::json!({"text":"? = ?","raw":"u.country = 'AU'","values":[]});
    let branch = builder_where(&base, pred.to_string()).unwrap();

    let base_first = builder_text(&base).unwrap();
    assert_eq!(base_first, r#"SELECT "u"."id" FROM "users" "u""#);
    assert_eq!(test_build_execution_count_for_handle(&base).unwrap(), 1);

    let branch_first = builder_text(&branch).unwrap();
    assert!(branch_first.contains("WHERE"));
    assert_eq!(test_build_execution_count_for_handle(&base).unwrap(), 1);
    assert_eq!(test_build_execution_count_for_handle(&branch).unwrap(), 1);

    let base_second = builder_raw(&base).unwrap();
    let branch_second = builder_values(&branch).unwrap();
    let branch_values: Vec<Primitive> = serde_json::from_str(&branch_second).unwrap();

    assert_eq!(base_second, "SELECT u.id FROM users u");
    assert!(branch_values.is_empty());
    assert_eq!(test_build_execution_count_for_handle(&base).unwrap(), 1);
    assert_eq!(test_build_execution_count_for_handle(&branch).unwrap(), 1);
}

#[test]
fn structural_cache_reuses_compiled_query_across_equivalent_handles() {
    let _guard = cache_test_guard();
    test_reset_build_execution_count();
    test_set_structural_cache_capacity(1_000_000);

    let first = builder_new(Some("postgres"));
    let first = builder_from_table_alias(&first, "users".to_string(), "u".to_string()).unwrap();
    let first =
        builder_select_columns(&first, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();
    let first_query = get_or_build_query_arc_with_structural_cache(&first).unwrap();
    assert!(test_structural_cache_len() >= 1);

    let second = builder_new(Some("postgres"));
    let second = builder_from_table_alias(&second, "users".to_string(), "u".to_string()).unwrap();
    let second =
        builder_select_columns(&second, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();
    let second_query = get_or_build_query_arc_with_structural_cache(&second).unwrap();
    let second_raw_again = builder_raw(&second).unwrap();

    assert_eq!(first_query.raw, second_query.raw);
    assert_eq!(second_raw_again, second_query.raw);
    let total_for_test = test_build_execution_count_for_handle(&first).unwrap()
        + test_build_execution_count_for_handle(&second).unwrap();
    assert!(
        (1..=2).contains(&total_for_test),
        "expected one structural-cache build under isolation, or two under concurrent test pressure; got {}",
        total_for_test
    );
}

#[test]
fn structural_cache_does_not_reuse_value_bearing_query_output() {
    let _guard = cache_test_guard();
    test_reset_build_execution_count();
    test_set_structural_cache_capacity(1_000_000);

    let first = builder_new(Some("postgres"));
    let first = builder_from_table_alias(&first, "users".to_string(), "u".to_string()).unwrap();
    let first =
        builder_select_columns(&first, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();
    let first = builder_where(
        &first,
        serde_json::json!({
            "text": "\"u\".\"age\" > ?",
            "raw": "u.age > 18",
            "values": [18],
        })
        .to_string(),
    )
    .unwrap();
    let first_query = get_or_build_query_arc_with_structural_cache(&first).unwrap();

    let second = builder_new(Some("postgres"));
    let second = builder_from_table_alias(&second, "users".to_string(), "u".to_string()).unwrap();
    let second =
        builder_select_columns(&second, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();
    let second = builder_where(
        &second,
        serde_json::json!({
            "text": "\"u\".\"age\" > ?",
            "raw": "u.age > 21",
            "values": [21],
        })
        .to_string(),
    )
    .unwrap();
    let second_query = get_or_build_query_arc_with_structural_cache(&second).unwrap();

    assert_eq!(first_query.values, vec![Primitive::Number(18.into())]);
    assert_eq!(second_query.values, vec![Primitive::Number(21.into())]);
    assert_eq!(first_query.raw, "SELECT u.id FROM users u WHERE u.age > 18");
    assert_eq!(
        second_query.raw,
        "SELECT u.id FROM users u WHERE u.age > 21"
    );
    assert_eq!(test_build_execution_count_for_handle(&first).unwrap(), 1);
    assert_eq!(test_build_execution_count_for_handle(&second).unwrap(), 1);
}

#[test]
fn compile_bundle_uses_per_handle_cache_without_populating_structural_cache() {
    let _guard = cache_test_guard();
    test_reset_build_execution_count();
    test_set_structural_cache_capacity(1_000_000);

    let first = builder_new(Some("postgres"));
    let first = builder_from_table_alias(&first, "users".to_string(), "u".to_string()).unwrap();
    let first =
        builder_select_columns(&first, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();
    let first_bundle = builder_compile_bundle_typed(&first).unwrap();
    let first_bundle_again = builder_compile_bundle_typed(&first).unwrap();

    let second = builder_new(Some("postgres"));
    let second = builder_from_table_alias(&second, "users".to_string(), "u".to_string()).unwrap();
    let second =
        builder_select_columns(&second, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();
    let second_bundle = builder_compile_bundle_typed(&second).unwrap();

    assert_eq!(first_bundle.raw, second_bundle.raw);
    assert_eq!(first_bundle.raw, first_bundle_again.raw);
    assert_eq!(test_structural_cache_len(), 0);
    assert_eq!(test_build_execution_count_for_handle(&first).unwrap(), 1);
    assert_eq!(
        test_build_execution_count_for_handle(&second).unwrap(),
        1,
        "expected an equivalent handle to render independently on the default compile path"
    );
}

#[test]
fn normal_output_path_uses_per_handle_cache_without_populating_structural_cache() {
    let _guard = cache_test_guard();
    test_reset_build_execution_count();
    test_set_structural_cache_capacity(1_000_000);

    let handle = builder_new(Some("postgres"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();

    let raw = builder_raw(&handle).unwrap();
    let text = builder_text(&handle).unwrap();

    assert_eq!(raw, "SELECT u.id FROM users u");
    assert_eq!(text, "SELECT \"u\".\"id\" FROM \"users\" \"u\"");
    assert_eq!(test_structural_cache_len(), 0);
    assert_eq!(test_build_execution_count_for_handle(&handle).unwrap(), 1);
}

#[test]
fn structural_cache_misses_for_different_query_shape() {
    let _guard = cache_test_guard();
    test_reset_build_execution_count();

    let first = builder_new(Some("postgres"));
    let first = builder_from_table_alias(&first, "users".to_string(), "u".to_string()).unwrap();
    let first =
        builder_select_columns(&first, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();
    let _ = builder_raw(&first).unwrap();

    let second = builder_new(Some("postgres"));
    let second = builder_from_table_alias(&second, "users".to_string(), "u".to_string()).unwrap();
    let second =
        builder_select_columns(&second, serde_json::to_string(&vec!["u.name"]).unwrap()).unwrap();
    let _ = builder_raw(&second).unwrap();

    let total_for_test = test_build_execution_count_for_handle(&first).unwrap()
        + test_build_execution_count_for_handle(&second).unwrap();
    assert_eq!(total_for_test, 2);
}

#[test]
fn structural_cache_misses_for_same_shape_with_different_dialects() {
    let _guard = cache_test_guard();
    test_reset_build_execution_count();

    let postgres = builder_new(Some("postgres"));
    let postgres =
        builder_from_table_alias(&postgres, "users".to_string(), "u".to_string()).unwrap();
    let postgres =
        builder_select_columns(&postgres, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();
    let _ = builder_raw(&postgres).unwrap();

    let mysql = builder_new(Some("mysql"));
    let mysql = builder_from_table_alias(&mysql, "users".to_string(), "u".to_string()).unwrap();
    let mysql =
        builder_select_columns(&mysql, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();
    let _ = builder_raw(&mysql).unwrap();

    let total_for_test = test_build_execution_count_for_handle(&postgres).unwrap()
        + test_build_execution_count_for_handle(&mysql).unwrap();
    assert_eq!(total_for_test, 2);
}

#[test]
fn lru_structural_cache_respects_max_size() {
    let mut cache = LruCache::new(NonZeroUsize::new(2).unwrap());
    cache.put("a".to_string(), cache_test_query("a"));
    cache.put("b".to_string(), cache_test_query("b"));
    cache.put("c".to_string(), cache_test_query("c"));

    assert_eq!(cache.len(), 2);
    assert!(cache.get("a").is_none());
    assert!(cache.get("b").is_some());
    assert!(cache.get("c").is_some());
}

#[test]
fn lru_structural_cache_evicts_least_recently_used_entry() {
    let mut cache = LruCache::new(NonZeroUsize::new(2).unwrap());
    cache.put("hot".to_string(), cache_test_query("hot"));
    cache.put("cold".to_string(), cache_test_query("cold"));
    assert!(cache.get("hot").is_some());
    cache.put("new".to_string(), cache_test_query("new"));

    assert!(cache.get("cold").is_none());
    assert!(cache.get("hot").is_some());
    assert!(cache.get("new").is_some());
    assert_eq!(cache.len(), 2);
}

#[test]
fn lru_structural_cache_keeps_frequently_used_queries_cached() {
    let mut cache = LruCache::new(NonZeroUsize::new(2).unwrap());
    cache.put("hot".to_string(), cache_test_query("hot"));
    cache.put("cold".to_string(), cache_test_query("cold"));
    assert!(cache.get("hot").is_some());
    assert!(cache.get("hot").is_some());
    cache.put("new".to_string(), cache_test_query("new"));

    assert!(cache.get("hot").is_some());
    assert!(cache.get("cold").is_none());
    assert!(cache.get("new").is_some());
    assert_eq!(cache.len(), 2);
}

#[test]
fn canonical_ir_is_identical_for_equivalent_queries_built_from_independent_handles() {
    let first = builder_new(Some("postgres"));
    let first = builder_from_table_alias(&first, "users".to_string(), "u".to_string()).unwrap();
    let first = builder_select_columns(
        &first,
        serde_json::to_string(&vec!["u.id", "u.country"]).unwrap(),
    )
    .unwrap();
    let pred = serde_json::json!({"text":"? = ?","raw":"u.country = 'AU'","values":[]});
    let first = builder_where(&first, pred.to_string()).unwrap();

    let second = builder_new(Some("postgres"));
    let second = builder_from_table_alias(&second, "users".to_string(), "u".to_string()).unwrap();
    let second = builder_select_columns(
        &second,
        serde_json::to_string(&vec!["u.id", "u.country"]).unwrap(),
    )
    .unwrap();
    let second = builder_where(&second, pred.to_string()).unwrap();

    let first_ir = canonical_ir_json_for_parts(&registry_get_parts(&first).unwrap()).unwrap();
    let second_ir = canonical_ir_json_for_parts(&registry_get_parts(&second).unwrap()).unwrap();
    assert_eq!(first_ir, second_ir);
}

#[test]
fn canonical_ir_normalizes_update_assignments_by_column_name() {
    let first = builder_new(Some("postgres"));
    let first = builder_update(&first, "users".to_string()).unwrap();
    let first = builder_set(
        &first,
        serde_json::to_string(&vec![
            (
                "name".to_string(),
                SqlValue::Primitive(Primitive::String("Ada".to_string())),
            ),
            (
                "country".to_string(),
                SqlValue::Primitive(Primitive::String("AU".to_string())),
            ),
        ])
        .unwrap(),
    )
    .unwrap();

    let second = builder_new(Some("postgres"));
    let second = builder_update(&second, "users".to_string()).unwrap();
    let second = builder_set(
        &second,
        serde_json::to_string(&vec![
            (
                "country".to_string(),
                SqlValue::Primitive(Primitive::String("AU".to_string())),
            ),
            (
                "name".to_string(),
                SqlValue::Primitive(Primitive::String("Ada".to_string())),
            ),
        ])
        .unwrap(),
    )
    .unwrap();

    let first_ir = canonical_ir_json_for_parts(&registry_get_parts(&first).unwrap()).unwrap();
    let second_ir = canonical_ir_json_for_parts(&registry_get_parts(&second).unwrap()).unwrap();
    assert_eq!(first_ir, second_ir);
}

#[test]
fn canonical_ir_changes_when_query_semantics_change() {
    let first = builder_new(Some("postgres"));
    let first = builder_from_table_alias(&first, "users".to_string(), "u".to_string()).unwrap();
    let first =
        builder_select_columns(&first, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();
    let pred_first = serde_json::json!({"text":"? = ?","raw":"u.country = 'AU'","values":[]});
    let first = builder_where(&first, pred_first.to_string()).unwrap();

    let second = builder_new(Some("postgres"));
    let second = builder_from_table_alias(&second, "users".to_string(), "u".to_string()).unwrap();
    let second =
        builder_select_columns(&second, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();
    let pred_second = serde_json::json!({"text":"? <> ?","raw":"u.country <> 'NZ'","values":[]});
    let second = builder_where(&second, pred_second.to_string()).unwrap();

    let first_ir = canonical_ir_json_for_parts(&registry_get_parts(&first).unwrap()).unwrap();
    let second_ir = canonical_ir_json_for_parts(&registry_get_parts(&second).unwrap()).unwrap();
    assert_ne!(first_ir, second_ir);
}

#[test]
fn canonical_ir_is_unchanged_before_and_after_render_cache_population() {
    let handle = builder_new(Some("postgres"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();

    let before = canonical_ir_json_for_parts(&registry_get_parts(&handle).unwrap()).unwrap();
    let _ = builder_query(&handle).unwrap();
    let after = canonical_ir_json_for_parts(&registry_get_parts(&handle).unwrap()).unwrap();

    assert_eq!(before, after);
}

#[test]
fn canonical_ir_serialization_is_deterministic_for_same_parts() {
    let handle = builder_new(Some("postgres"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();

    let parts = registry_get_parts(&handle).unwrap();
    let first = canonical_ir_json_for_parts(&parts).unwrap();
    let second = canonical_ir_json_for_parts(&parts).unwrap();

    assert_eq!(first, second);
}

#[test]
fn canonical_ir_preserves_parameter_structure_and_order() {
    let first = builder_new(Some("postgres"));
    let first = builder_from_table_alias(&first, "users".to_string(), "u".to_string()).unwrap();
    let first =
        builder_select_columns(&first, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();
    let pred_first = serde_json::json!({
        "text":"? = ?",
        "raw":"u.created_at = '2024-01-01T00:00:00.000Z'",
        "values":[{"__kind":"date","value":"2024-01-01T00:00:00.000Z"}]
    });
    let first = builder_where(&first, pred_first.to_string()).unwrap();

    let second = builder_new(Some("postgres"));
    let second = builder_from_table_alias(&second, "users".to_string(), "u".to_string()).unwrap();
    let second =
        builder_select_columns(&second, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();
    let pred_second = serde_json::json!({
        "text":"? = ?",
        "raw":"u.created_at = '2024-01-02T00:00:00.000Z'",
        "values":[{"__kind":"date","value":"2024-01-02T00:00:00.000Z"}]
    });
    let second = builder_where(&second, pred_second.to_string()).unwrap();

    let first_ir = canonical_ir_json_for_parts(&registry_get_parts(&first).unwrap()).unwrap();
    let second_ir = canonical_ir_json_for_parts(&registry_get_parts(&second).unwrap()).unwrap();

    assert!(first_ir.contains(r#""value_count":1"#));
    assert!(second_ir.contains(r#""value_count":1"#));
    assert_eq!(first_ir, second_ir);
}

#[test]
fn canonical_hash_same_ir_same_hash() {
    let first = builder_new(Some("postgres"));
    let first = builder_from_table_alias(&first, "users".to_string(), "u".to_string()).unwrap();
    let first =
        builder_select_columns(&first, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();

    let second = builder_new(Some("postgres"));
    let second = builder_from_table_alias(&second, "users".to_string(), "u".to_string()).unwrap();
    let second =
        builder_select_columns(&second, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();

    let first_hash = builder_canonical_ir_hash(&first).unwrap();
    let second_hash = builder_canonical_ir_hash(&second).unwrap();
    assert_eq!(first_hash, second_hash);
}

#[test]
fn canonical_hash_ignores_bound_values_for_same_query_shape() {
    let first = builder_new(Some("postgres"));
    let first = builder_from_table_alias(&first, "users".to_string(), "u".to_string()).unwrap();
    let first =
        builder_select_columns(&first, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();
    let first = builder_where(
        &first,
        serde_json::json!({
            "text": "\"u\".\"age\" > ?",
            "raw": "u.age > 18",
            "values": [18],
        })
        .to_string(),
    )
    .unwrap();

    let second = builder_new(Some("postgres"));
    let second = builder_from_table_alias(&second, "users".to_string(), "u".to_string()).unwrap();
    let second =
        builder_select_columns(&second, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();
    let second = builder_where(
        &second,
        serde_json::json!({
            "text": "\"u\".\"age\" > ?",
            "raw": "u.age > 21",
            "values": [21],
        })
        .to_string(),
    )
    .unwrap();

    let first_hash = builder_canonical_ir_hash(&first).unwrap();
    let second_hash = builder_canonical_ir_hash(&second).unwrap();
    assert_eq!(first_hash, second_hash);
}

#[test]
fn canonical_fingerprint_ignores_bound_values_for_same_query_shape() {
    let first = builder_new(Some("postgres"));
    let first = builder_from_table_alias(&first, "users".to_string(), "u".to_string()).unwrap();
    let first =
        builder_select_columns(&first, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();
    let first = builder_where(
        &first,
        serde_json::json!({
            "text": "\"u\".\"age\" > ?",
            "raw": "u.age > 18",
            "values": [18],
        })
        .to_string(),
    )
    .unwrap();

    let second = builder_new(Some("postgres"));
    let second = builder_from_table_alias(&second, "users".to_string(), "u".to_string()).unwrap();
    let second =
        builder_select_columns(&second, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();
    let second = builder_where(
        &second,
        serde_json::json!({
            "text": "\"u\".\"age\" > ?",
            "raw": "u.age > 21",
            "values": [21],
        })
        .to_string(),
    )
    .unwrap();

    let first_fingerprint =
        fingerprint(&canonical_ir_for_parts(&registry_get_parts(&first).unwrap()).unwrap())
            .unwrap();
    let second_fingerprint =
        fingerprint(&canonical_ir_for_parts(&registry_get_parts(&second).unwrap()).unwrap())
            .unwrap();
    assert_eq!(first_fingerprint, second_fingerprint);
}

#[test]
fn canonical_hash_different_ir_different_hash() {
    let first = builder_new(Some("postgres"));
    let first = builder_from_table_alias(&first, "users".to_string(), "u".to_string()).unwrap();
    let first =
        builder_select_columns(&first, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();

    let second = builder_new(Some("mysql"));
    let second = builder_from_table_alias(&second, "users".to_string(), "u".to_string()).unwrap();
    let second =
        builder_select_columns(&second, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();

    let first_hash = builder_canonical_ir_hash(&first).unwrap();
    let second_hash = builder_canonical_ir_hash(&second).unwrap();
    assert_ne!(first_hash, second_hash);
}

#[test]
fn canonical_hash_stable_across_repeated_calls() {
    let handle = builder_new(Some("postgres"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();

    let first = builder_canonical_ir_hash(&handle).unwrap();
    let second = builder_canonical_ir_hash(&handle).unwrap();
    assert_eq!(first, second);
}

#[test]
fn canonical_hash_ignores_transient_render_cache() {
    let handle = builder_new(Some("postgres"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();

    let before = builder_canonical_ir_hash(&handle).unwrap();
    let _ = builder_query(&handle).unwrap();
    let after = builder_canonical_ir_hash(&handle).unwrap();
    assert_eq!(before, after);
}

#[test]
fn canonical_hash_no_collisions_in_test_corpus() {
    let mut handles: Vec<String> = Vec::new();

    let select_basic = {
        let h = builder_new(Some("postgres"));
        let h = builder_from_table_alias(&h, "users".to_string(), "u".to_string()).unwrap();
        builder_select_columns(&h, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap()
    };
    handles.push(select_basic);

    let select_predicate = {
        let h = builder_new(Some("postgres"));
        let h = builder_from_table_alias(&h, "users".to_string(), "u".to_string()).unwrap();
        let h = builder_select_columns(&h, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();
        let pred = serde_json::json!({"text":"? = ?","raw":"u.country = 'AU'","values":[]});
        builder_where(&h, pred.to_string()).unwrap()
    };
    handles.push(select_predicate);

    let select_join = {
        let h = builder_new(Some("postgres"));
        let h = builder_from_table_alias(&h, "users".to_string(), "u".to_string()).unwrap();
        let h = builder_join_table_alias(
            &h,
            "INNER JOIN".to_string(),
            "teams".to_string(),
            "t".to_string(),
        )
        .unwrap();
        let pred = serde_json::json!({"text":"? = ?","raw":"u.country = t.country","values":[]});
        let h = builder_on(&h, pred.to_string()).unwrap();
        builder_select_columns(&h, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap()
    };
    handles.push(select_join);

    let insert_values = {
        let h = builder_new(Some("postgres"));
        let h = builder_insert_into(&h, "users".to_string()).unwrap();
        builder_values_insert(
            &h,
            rows_json(
                &["id", "name"],
                vec![vec![
                    SqlValue::Primitive(Primitive::String("u_1".to_string())),
                    SqlValue::Primitive(Primitive::String("Ada".to_string())),
                ]],
            ),
        )
        .unwrap()
    };
    handles.push(insert_values);

    let update_set = {
        let h = builder_new(Some("postgres"));
        let h = builder_update(&h, "users".to_string()).unwrap();
        builder_set(
            &h,
            serde_json::to_string(&vec![(
                "country".to_string(),
                SqlValue::Primitive(Primitive::String("NZ".to_string())),
            )])
            .unwrap(),
        )
        .unwrap()
    };
    handles.push(update_set);

    let delete_where = {
        let h = builder_new(Some("postgres"));
        let h = builder_delete_from(&h, "users".to_string()).unwrap();
        let pred = serde_json::json!({"text":"? = ?","raw":"country = 'NZ'","values":[]});
        builder_where(&h, pred.to_string()).unwrap()
    };
    handles.push(delete_where);

    let mysql_variant = {
        let h = builder_new(Some("mysql"));
        let h = builder_from_table_alias(&h, "users".to_string(), "u".to_string()).unwrap();
        builder_select_columns(&h, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap()
    };
    handles.push(mysql_variant);

    let mut seen = std::collections::HashSet::new();
    for handle in handles {
        let hash = builder_canonical_ir_hash(&handle).unwrap();
        assert!(
            seen.insert(hash.clone()),
            "collision detected for hash: {}",
            hash
        );
    }
}

#[test]
fn batch_compound_handle_ops_reuse_rhs_render_cache_path() {
    let _guard = cache_test_guard();
    test_reset_build_execution_count();
    test_set_structural_cache_capacity(1_000_000);

    let rhs = builder_new(Some("postgres"));
    let rhs = builder_from_table_alias(&rhs, "users".to_string(), "u".to_string()).unwrap();
    let rhs = builder_select_columns(&rhs, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();

    let lhs = builder_new(Some("postgres"));
    let lhs = builder_from_table_alias(&lhs, "users".to_string(), "u".to_string()).unwrap();
    let lhs = builder_select_columns(&lhs, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();

    let ops = serde_json::json!([["unh", rhs.clone()], ["uah", rhs.clone()]]);
    let lhs = builder_apply_ops(&lhs, ops.to_string()).unwrap();

    assert_eq!(
        test_cache_lookup_count_for_handle(&rhs).unwrap(),
        2,
        "RHS handle should be resolved through the cache-aware lookup for each handle-based compound op"
    );
    let rhs_builds = test_build_execution_count_for_handle(&rhs).unwrap();
    assert!(
        rhs_builds <= 1,
        "RHS handle should compile at most once (or zero on structural-cache hit); got {}",
        rhs_builds
    );

    // Materializing the parent query must not trigger extra RHS rebuilds.
    let _ = builder_query(&lhs).unwrap();
    let rhs_builds_after_parent = test_build_execution_count_for_handle(&rhs).unwrap();
    assert!(
        rhs_builds_after_parent <= 1,
        "Materializing parent must not force additional RHS rebuilds; got {}",
        rhs_builds_after_parent
    );
}

#[test]
fn batch_apply_ops_bin_matches_json_apply_semantics() {
    let handle_json = builder_new(Some("postgres"));
    let handle_bin = builder_new(Some("postgres"));
    let ops = serde_json::json!([
        ["fta", "users", "u"],
        ["sc", ["u.id", "u.name"]],
        ["ob", "u.name", "ASC", serde_json::Value::Null],
        ["l", 5]
    ]);

    let after_json = builder_apply_ops(&handle_json, ops.to_string()).unwrap();
    let after_bin = builder_apply_ops_bin(
        &handle_bin,
        binary_ops_payload(ops.as_array().unwrap().clone()),
    )
    .unwrap();

    assert_eq!(
        builder_query(&after_json).unwrap(),
        builder_query(&after_bin).unwrap()
    );
}

#[test]
fn batch_apply_ops_bin_rejects_truncated_payload() {
    let handle = builder_new(Some("postgres"));
    let err = builder_apply_ops_bin(&handle, vec![1, 0, 0]).unwrap_err();
    assert!(err.contains("too short"));
}

#[test]
fn compile_bundle_matches_output_accessors_and_hash() {
    let _guard = cache_test_guard();
    test_reset_build_execution_count();

    let handle = builder_new(Some("postgres"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();

    let bundle = builder_compile_bundle_typed(&handle).unwrap();

    assert_eq!(test_build_execution_count_for_handle(&handle).unwrap(), 1);
    assert_eq!(bundle.text, builder_text(&handle).unwrap());
    assert_eq!(bundle.raw, builder_raw(&handle).unwrap());
    assert_eq!(
        bundle.values,
        crate::builder::render::builder_values_typed(&handle).unwrap()
    );
    assert_eq!(test_build_execution_count_for_handle(&handle).unwrap(), 1);
    assert!(!builder_canonical_ir_hash(&handle).unwrap().is_empty());
}

// ─── Dialect-aware validation pipeline (issues #20, #21, #22, #30) ───────────

#[test]
fn sqlite_right_join_detected_by_dialect_validator() {
    let handle = builder_new(Some("sqlite"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    let handle = builder_join_table_alias(
        &handle,
        "RIGHT JOIN".to_string(),
        "orders".to_string(),
        "o".to_string(),
    )
    .unwrap();
    let pred = serde_json::json!({"text": "? = ?", "raw": "u.id = o.user_id", "values": []});
    let handle = builder_on(&handle, pred.to_string()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();

    let err = builder_query(&handle).unwrap_err();
    assert!(
        err.contains("RightJoin"),
        "expected 'RightJoin' feature name in error, got: {err}"
    );
    assert!(
        err.contains("sqlite"),
        "expected dialect name in error, got: {err}"
    );
}

#[test]
fn sqlite_full_join_detected_by_dialect_validator() {
    let pred = serde_json::json!({"text": "? = ?", "raw": "u.id = o.user_id", "values": []});
    let handle = builder_new(Some("sqlite"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    let handle = builder_join_table_alias(
        &handle,
        "FULL OUTER JOIN".to_string(),
        "orders".to_string(),
        "o".to_string(),
    )
    .unwrap();
    let handle = builder_on(&handle, pred.to_string()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();

    let err = builder_query(&handle).unwrap_err();
    assert!(
        err.contains("FullJoin"),
        "expected 'FullJoin' feature name in error, got: {err}"
    );
    assert!(
        err.contains("sqlite"),
        "expected dialect name in error, got: {err}"
    );
}

#[test]
fn sqlite_returning_on_update_detected_by_dialect_validator() {
    let handle = builder_new(Some("sqlite"));
    let handle = builder_update(&handle, "users".to_string()).unwrap();
    let handle = builder_set(
        &handle,
        serde_json::to_string(&vec![(
            "name".to_string(),
            SqlValue::Raw(sql::raw("UPPER(name)".to_string())),
        )])
        .unwrap(),
    )
    .unwrap();
    let handle =
        builder_returning_columns(&handle, serde_json::to_string(&vec!["id"]).unwrap()).unwrap();

    let err = builder_query(&handle).unwrap_err();
    assert!(
        err.contains("ReturningOnUpdate"),
        "expected 'ReturningOnUpdate' feature name in error, got: {err}"
    );
    assert!(
        err.contains("sqlite"),
        "expected dialect name in error, got: {err}"
    );
}

#[test]
fn sqlite_window_function_detected_by_dialect_validator() {
    // Inject raw SQL containing "OVER (" directly via builder_select_fragment.
    // This bypasses ExprNode-level dialect checks so our validator is the
    // only gate that fires.
    let fragment = serde_json::json!({
        "text": "ROW_NUMBER() OVER ()",
        "raw":  "ROW_NUMBER() OVER ()",
        "values": []
    });
    let selected_cols = serde_json::json!(["row_num"]);

    let handle = builder_new(Some("sqlite"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    let handle =
        builder_select_fragment(&handle, fragment.to_string(), selected_cols.to_string()).unwrap();

    let err = builder_query(&handle).unwrap_err();
    assert!(
        err.contains("WindowFunction"),
        "expected 'WindowFunction' feature name in error, got: {err}"
    );
    assert!(
        err.contains("sqlite"),
        "expected dialect name in error, got: {err}"
    );
}

#[test]
fn postgres_right_join_passes_dialect_validation() {
    let pred = serde_json::json!({"text": "? = ?", "raw": "u.id = o.user_id", "values": []});
    let handle = builder_new(Some("postgres"));
    let handle = builder_from_table_alias(&handle, "users".to_string(), "u".to_string()).unwrap();
    let handle = builder_join_table_alias(
        &handle,
        "RIGHT JOIN".to_string(),
        "orders".to_string(),
        "o".to_string(),
    )
    .unwrap();
    let handle = builder_on(&handle, pred.to_string()).unwrap();
    let handle =
        builder_select_columns(&handle, serde_json::to_string(&vec!["u.id"]).unwrap()).unwrap();

    let query = parse_query(builder_query(&handle).unwrap());
    assert!(
        query.raw.contains("RIGHT JOIN orders o ON"),
        "expected RIGHT JOIN in rendered SQL, got: {}",
        query.raw
    );
}
