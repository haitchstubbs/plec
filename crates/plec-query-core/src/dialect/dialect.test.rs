use super::{
    classify, function_capability_for, normalize_dialect_name, operator_capability_for,
    supports_function, supports_operator, Dialect, DialectFeature, DialectPolicy, FunctionFamily,
    OperatorFamily, OperatorKind,
};

#[test]
fn capabilities_method_delegates_to_capabilities_for_dialect() {
    let pg = Dialect::Postgres.capabilities();
    assert!(pg.returning, "Postgres must support RETURNING");
    assert!(pg.distinct_on, "Postgres must support DISTINCT ON");
}

#[test]
fn supports_cte_true_for_all_known_dialects() {
    let known = [
        Dialect::Postgres,
        Dialect::DuckDb,
        Dialect::Sqlite,
        Dialect::Mysql,
        Dialect::Mssql,
        Dialect::Oracle,
        Dialect::Snowflake,
        Dialect::GoogleSql,
        Dialect::Redshift,
        Dialect::BigQuery,
        Dialect::ClickHouse,
    ];
    for dialect in &known {
        assert!(
            dialect.capabilities().supports_cte,
            "Expected supports_cte = true for dialect {dialect}"
        );
    }
    assert!(
        !Dialect::Unknown("nodb".to_string())
            .capabilities()
            .supports_cte,
        "Unknown dialect must have supports_cte = false"
    );
}

#[test]
fn ilike_true_for_postgres_duckdb_snowflake_redshift() {
    for dialect in &[
        Dialect::Postgres,
        Dialect::DuckDb,
        Dialect::Snowflake,
        Dialect::Redshift,
    ] {
        assert!(
            dialect.capabilities().ilike,
            "Expected ilike = true for dialect {dialect}"
        );
    }
}

#[test]
fn ilike_false_for_mysql_sqlite_mssql_oracle() {
    for dialect in &[
        Dialect::Mysql,
        Dialect::Sqlite,
        Dialect::Mssql,
        Dialect::Oracle,
    ] {
        assert!(
            !dialect.capabilities().ilike,
            "Expected ilike = false for dialect {dialect}"
        );
    }
}

#[test]
fn lateral_join_true_for_postgres_mysql_mssql_oracle_snowflake_googlesql_redshift_bigquery() {
    for dialect in &[
        Dialect::Postgres,
        Dialect::DuckDb,
        Dialect::Mysql,
        Dialect::Mssql,
        Dialect::Oracle,
        Dialect::Snowflake,
        Dialect::GoogleSql,
        Dialect::Redshift,
        Dialect::BigQuery,
    ] {
        assert!(
            dialect.capabilities().joins.lateral_join,
            "Expected lateral_join = true for dialect {dialect}"
        );
    }
}

#[test]
fn lateral_join_false_for_sqlite_clickhouse_unknown() {
    for dialect in &[
        Dialect::Sqlite,
        Dialect::ClickHouse,
        Dialect::Unknown("nodb".to_string()),
    ] {
        assert!(
            !dialect.capabilities().joins.lateral_join,
            "Expected lateral_join = false for dialect {dialect}"
        );
    }
}

#[test]
fn parse_dialect_normalizes_aliases_and_unknown_values() {
    assert_eq!(Dialect::parse(" postgres "), Dialect::Postgres);
    assert_eq!(Dialect::parse("MSSQLSERVER"), Dialect::Mssql);
    assert_eq!(
        Dialect::parse(" mongodb "),
        Dialect::Unknown("mongodb".to_string())
    );
    assert_eq!(normalize_dialect_name("  MySQL  "), "mysql");
}

#[test]
fn function_capability_lookup_is_dialect_aware() {
    assert!(supports_function(&Dialect::Postgres, "lower"));
    assert!(supports_function(&Dialect::Mysql, "COALESCE"));
    assert!(!supports_function(&Dialect::Postgres, "UNKNOWN_FUNC"));
    assert!(!supports_function(
        &Dialect::Unknown("mongodb".to_string()),
        "LOWER"
    ));

    let capability = function_capability_for(&Dialect::Postgres, "count").unwrap();
    assert_eq!(capability.family, FunctionFamily::Aggregate);
    assert!(capability.window);
}

#[test]
fn operator_capability_lookup_normalizes_input() {
    assert!(supports_operator(
        &Dialect::Postgres,
        OperatorKind::Comparison,
        " >= "
    ));
    assert!(supports_operator(
        &Dialect::Sqlite,
        OperatorKind::Arithmetic,
        "+"
    ));
    assert!(!supports_operator(
        &Dialect::Postgres,
        OperatorKind::Comparison,
        "~~"
    ));
    assert!(!supports_operator(
        &Dialect::Unknown("mongodb".to_string()),
        OperatorKind::Arithmetic,
        "+"
    ));

    let capability =
        operator_capability_for(&Dialect::Postgres, OperatorKind::Arithmetic, " + ").unwrap();
    assert_eq!(capability.family, OperatorFamily::Arithmetic);
}

#[test]
fn classify_returning_is_hard_error_for_dialects_without_returning() {
    for dialect in &[
        Dialect::Mysql,
        Dialect::Mssql,
        Dialect::Oracle,
        Dialect::Snowflake,
        Dialect::GoogleSql,
        Dialect::BigQuery,
        Dialect::ClickHouse,
    ] {
        assert_eq!(
            classify(dialect, DialectFeature::Returning),
            DialectPolicy::HardError,
            "Expected HardError for RETURNING on {dialect}"
        );
    }
}

#[test]
fn classify_returning_is_dialect_render_for_supporting_dialects() {
    for dialect in &[Dialect::Postgres, Dialect::DuckDb, Dialect::Sqlite] {
        assert_eq!(
            classify(dialect, DialectFeature::Returning),
            DialectPolicy::DialectRender,
            "Expected DialectRender for RETURNING on {dialect}"
        );
    }
}

#[test]
fn classify_ilike_is_fallback_rewrite_for_dialects_without_ilike() {
    for dialect in &[
        Dialect::Mysql,
        Dialect::Sqlite,
        Dialect::Mssql,
        Dialect::Oracle,
    ] {
        assert_eq!(
            classify(dialect, DialectFeature::Ilike),
            DialectPolicy::FallbackRewrite,
            "Expected FallbackRewrite for ILIKE on {dialect}"
        );
    }
}

#[test]
fn classify_ilike_is_dialect_render_for_postgres_duckdb() {
    for dialect in &[Dialect::Postgres, Dialect::DuckDb] {
        assert_eq!(
            classify(dialect, DialectFeature::Ilike),
            DialectPolicy::DialectRender,
            "Expected DialectRender for ILIKE on {dialect}"
        );
    }
}

#[test]
fn classify_intersect_all_is_hard_error_for_mysql() {
    assert_eq!(
        classify(&Dialect::Mysql, DialectFeature::IntersectAll),
        DialectPolicy::HardError
    );
    assert_eq!(
        classify(&Dialect::Postgres, DialectFeature::IntersectAll),
        DialectPolicy::DialectRender
    );
}

#[test]
fn classify_pagination_limit_offset_is_fallback_rewrite_for_mssql() {
    assert_eq!(
        classify(&Dialect::Mssql, DialectFeature::PaginationLimitOffset),
        DialectPolicy::FallbackRewrite
    );
    assert_eq!(
        classify(&Dialect::Postgres, DialectFeature::PaginationLimitOffset),
        DialectPolicy::DialectRender
    );
}
