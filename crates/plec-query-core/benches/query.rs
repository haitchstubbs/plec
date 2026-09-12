// Rust guideline compliant 2026-02-21

use criterion::{criterion_group, criterion_main, Criterion};
use plec_query::builder::render::{build_query, registry_get_parts};
use plec_query::{
    builder_and_where, builder_canonical_ir_json, builder_distinct, builder_do_update_set,
    builder_drop, builder_from_table, builder_from_table_alias, builder_group_by_columns,
    builder_having, builder_insert_into, builder_join_subquery_handle, builder_join_table_alias,
    builder_limit, builder_new, builder_offset, builder_on, builder_on_conflict_columns,
    builder_or_where, builder_order_by_column, builder_select_columns, builder_select_fragment,
    builder_union_handle, builder_values_insert, builder_where, builder_with_handle,
    builder_with_recursive_handle, Primitive, SqlValue,
};
use std::hint::black_box;

const SELECT_COLUMNS_JSON: &str = r#"["col_1","col_2","col_3"]"#;
const WHERE_PREDICATE_JSON: &str = r#"{"text":"col_2 = ?","raw":"col_2 = 15","values":[15]}"#;
const JOIN_PREDICATE_JSON: &str =
    r#"{"text":"\"u\".\"team_id\" = \"t\".\"id\"","raw":"u.team_id = t.id","values":[]}"#;
const AGGREGATION_FRAGMENT_JSON: &str = r#"{"text":"\"u\".\"country\", count(*) AS member_count","raw":"u.country, count(*) AS member_count","values":[]}"#;
const AGGREGATION_SELECTED_COLUMNS_JSON: &str = r#"["u.country","member_count"]"#;
const HAVING_PREDICATE_JSON: &str = r#"{"text":"count(*) > ?","raw":"count(*) > 1","values":[1]}"#;
const APP_CORE_SELECT_FRAGMENT_JSON: &str = r#"{"text":"\"u\".\"id\" AS \"userId\", \"u\".\"name\" AS \"userName\", \"u\".\"country\" AS \"country\", \"t\".\"name\" AS \"teamName\"","raw":"u.id AS userId, u.name AS userName, u.country AS country, t.name AS teamName","values":[]}"#;
const APP_CORE_SELECTED_COLUMNS_JSON: &str = r#"["userId","userName","country","teamName"]"#;
const APP_CORE_AGE_PREDICATE_JSON: &str =
    r#"{"text":"\"u\".\"age\" > ?","raw":"u.age > 18","values":[18]}"#;
const APP_CORE_COUNTRY_PREDICATE_JSON: &str = r#"{"text":"\"u\".\"country\" IN (?, ?)","raw":"u.country IN ('AU', 'NZ')","values":["AU","NZ"]}"#;
const APP_CORE_NAME_PREDICATE_JSON: &str =
    r#"{"text":"\"u\".\"name\" LIKE ?","raw":"u.name LIKE 'A%'","values":["A%"]}"#;
const APP_HEAVY_CTE_SELECT_FRAGMENT_JSON: &str = r#"{"text":"\"u\".\"id\" AS \"userId\", \"u\".\"name\" AS \"userName\", \"u\".\"country\" AS \"country\", \"t\".\"name\" AS \"teamName\", (\"u\".\"age\" + ?) AS \"ageNextYear\", CASE WHEN \"u\".\"age\" >= ? THEN ? ELSE ? END AS \"ageBucket\", LAG(\"tm\".\"teamId\") OVER (PARTITION BY \"u\".\"id\" ORDER BY \"tm\".\"joinedAt\" ASC) AS \"previousTeamId\", COUNT(*) OVER (PARTITION BY \"u\".\"id\" ORDER BY \"tm\".\"joinedAt\" ASC) AS \"teamMembershipCount\"","raw":"u.id AS userId, u.name AS userName, u.country AS country, t.name AS teamName, (u.age + 1) AS ageNextYear, CASE WHEN u.age >= 18 THEN 'adult' ELSE 'minor' END AS ageBucket, LAG(tm.teamId) OVER (PARTITION BY u.id ORDER BY tm.joinedAt ASC) AS previousTeamId, COUNT(*) OVER (PARTITION BY u.id ORDER BY tm.joinedAt ASC) AS teamMembershipCount","values":[1,18,"adult","minor"]}"#;
const APP_HEAVY_CTE_SELECTED_COLUMNS_JSON: &str = r#"["userId","userName","country","teamName","ageNextYear","ageBucket","previousTeamId","teamMembershipCount"]"#;
const APP_HEAVY_CTE_TEAM_COUNT_PREDICATE_JSON: &str = r#"{"text":"\"rm\".\"teamMembershipCount\" > ?","raw":"rm.teamMembershipCount > 0","values":[0]}"#;
const ACTIVE_TEAMS_SELECT_COLUMNS_JSON: &str = r#"["t.id","t.name","t.country"]"#;
const ELIGIBLE_USERS_SELECT_COLUMNS_JSON: &str = r#"["u.id","u.name"]"#;
const ACTIVE_TEAMS_COUNTRY_PREDICATE_JSON: &str =
    r#"{"text":"\"t\".\"country\" = ?","raw":"t.country = 'AU'","values":["AU"]}"#;
const ELIGIBLE_ADULTS_PREDICATE_JSON: &str =
    r#"{"text":"\"u\".\"age\" >= ?","raw":"u.age >= 18","values":[18]}"#;
const ELIGIBLE_COACHES_PREDICATE_JSON: &str =
    r#"{"text":"\"u\".\"role\" = ?","raw":"u.role = 'coach'","values":["coach"]}"#;
const ORG_TREE_SELECT_COLUMNS_JSON: &str = r#"["ou.id","ou.name","ou.parentId"]"#;
const ORG_TREE_ROOT_PREDICATE_JSON: &str =
    r#"{"text":"\"ou\".\"parentId\" IS NULL","raw":"ou.parentId IS NULL","values":[]}"#;
const ACTIVE_TEAMS_JOIN_PREDICATE_JSON: &str =
    r#"{"text":"\"at\".\"country\" = ?","raw":"at.country = 'AU'","values":["AU"]}"#;
const PARITY_CTE_COMPOUND_SELECT_COLUMNS_JSON: &str = r#"["at.teamId","eligibleUsers.name"]"#;

fn rows_json(column_names: &[&str], rows: Vec<Vec<SqlValue>>) -> String {
    serde_json::json!({
        "columnNames": column_names,
        "rows": rows,
    })
    .to_string()
}

fn assignment_json(entries: Vec<(&str, SqlValue)>) -> String {
    let owned_entries = entries
        .into_iter()
        .map(|(column, value)| (column.to_string(), value))
        .collect::<Vec<_>>();
    serde_json::to_string(&owned_entries).expect("assignments should serialize")
}

struct Scenario {
    name: &'static str,
    build_handle: fn() -> String,
}

fn advance_handle<F>(handle: String, next: F, step: &'static str) -> String
where
    F: FnOnce(&str) -> Result<String, String>,
{
    let next_handle = next(&handle).unwrap_or_else(|err| panic!("{step}: {err}")); // opengrep:ignore opengrep.rust-panic-in-library-code -- bench harness only, not library code
    builder_drop(&handle);
    next_handle
}

fn build_simple_query_handle() -> String {
    let handle = builder_new(Some("postgres"));
    let handle = advance_handle(
        handle,
        |h| builder_from_table(h, "table".to_string()),
        "from should build",
    );
    let handle = advance_handle(
        handle,
        |h| builder_select_columns(h, SELECT_COLUMNS_JSON.to_string()),
        "select columns should build",
    );
    let handle = advance_handle(
        handle,
        |h| builder_where(h, WHERE_PREDICATE_JSON.to_string()),
        "where should build",
    );
    advance_handle(handle, |h| builder_limit(h, 5), "limit should build")
}

fn build_join_query_handle() -> String {
    let handle = builder_new(Some("postgres"));
    let handle = advance_handle(
        handle,
        |h| builder_from_table_alias(h, "users".to_string(), "u".to_string()),
        "from should build",
    );
    let handle = advance_handle(
        handle,
        |h| {
            builder_join_table_alias(
                h,
                "INNER JOIN".to_string(),
                "teams".to_string(),
                "t".to_string(),
            )
        },
        "join should build",
    );
    let handle = advance_handle(
        handle,
        |h| builder_on(h, JOIN_PREDICATE_JSON.to_string()),
        "on should build",
    );
    let handle = advance_handle(
        handle,
        |h| {
            builder_select_columns(
                h,
                serde_json::to_string(&vec!["u.id", "u.name", "t.name"])
                    .expect("columns should serialize"),
            )
        },
        "select columns should build",
    );
    let handle = advance_handle(
        handle,
        |h| {
            builder_where(
                h,
                r#"{"text":"\"u\".\"country\" = ?","raw":"u.country = 'AU'","values":["AU"]}"#
                    .to_string(),
            )
        },
        "where should build",
    );
    advance_handle(handle, |h| builder_limit(h, 10), "limit should build")
}

fn build_aggregation_query_handle() -> String {
    let handle = builder_new(Some("postgres"));
    let handle = advance_handle(
        handle,
        |h| builder_from_table_alias(h, "users".to_string(), "u".to_string()),
        "from should build",
    );
    let handle = advance_handle(
        handle,
        |h| {
            builder_select_fragment(
                h,
                AGGREGATION_FRAGMENT_JSON.to_string(),
                AGGREGATION_SELECTED_COLUMNS_JSON.to_string(),
            )
        },
        "select fragment should build",
    );
    let handle = advance_handle(
        handle,
        |h| {
            builder_where(
                h,
                r#"{"text":"\"u\".\"age\" >= ?","raw":"u.age >= 18","values":[18]}"#.to_string(),
            )
        },
        "where should build",
    );
    let handle = advance_handle(
        handle,
        |h| builder_group_by_columns(h, r#"["u.country"]"#.to_string()),
        "group by should build",
    );
    let handle = advance_handle(
        handle,
        |h| builder_having(h, HAVING_PREDICATE_JSON.to_string()),
        "having should build",
    );
    let handle = advance_handle(
        handle,
        |h| builder_order_by_column(h, "u.country".to_string(), Some("ASC".to_string()), None),
        "order by should build",
    );
    advance_handle(handle, |h| builder_limit(h, 20), "limit should build")
}

fn build_app_core_builder_flow_handle() -> String {
    let handle = builder_new(Some("postgres"));
    let handle = advance_handle(
        handle,
        |h| builder_from_table_alias(h, "users".to_string(), "u".to_string()),
        "from should build",
    );
    let handle = advance_handle(handle, builder_distinct, "distinct should build");
    let handle = advance_handle(
        handle,
        |h| {
            builder_join_table_alias(
                h,
                "LEFT JOIN".to_string(),
                "teamMembers".to_string(),
                "tm".to_string(),
            )
        },
        "teamMembers join should build",
    );
    let handle = advance_handle(
        handle,
        |h| {
            builder_on(h, r#"{"text":"\"u\".\"id\" = \"tm\".\"userId\"","raw":"u.id = tm.userId","values":[]}"#.to_string())
        },
        "teamMembers join predicate should build",
    );
    let handle = advance_handle(
        handle,
        |h| {
            builder_join_table_alias(
                h,
                "LEFT JOIN".to_string(),
                "teams".to_string(),
                "t".to_string(),
            )
        },
        "teams join should build",
    );
    let handle = advance_handle(
        handle,
        |h| {
            builder_on(h, r#"{"text":"\"tm\".\"teamId\" = \"t\".\"id\"","raw":"tm.teamId = t.id","values":[]}"#.to_string())
        },
        "teams join predicate should build",
    );
    let handle = advance_handle(
        handle,
        |h| {
            builder_select_fragment(
                h,
                APP_CORE_SELECT_FRAGMENT_JSON.to_string(),
                APP_CORE_SELECTED_COLUMNS_JSON.to_string(),
            )
        },
        "select fragment should build",
    );
    let handle = advance_handle(
        handle,
        |h| builder_where(h, APP_CORE_AGE_PREDICATE_JSON.to_string()),
        "where should build",
    );
    let handle = advance_handle(
        handle,
        |h| builder_and_where(h, APP_CORE_COUNTRY_PREDICATE_JSON.to_string()),
        "andWhere should build",
    );
    let handle = advance_handle(
        handle,
        |h| builder_or_where(h, APP_CORE_NAME_PREDICATE_JSON.to_string()),
        "orWhere should build",
    );
    let handle = advance_handle(
        handle,
        |h| builder_group_by_columns(h, r#"["u.id","u.name","u.country","t.name"]"#.to_string()),
        "group by should build",
    );
    let handle = advance_handle(
        handle,
        |h| builder_having(h, HAVING_PREDICATE_JSON.to_string()),
        "having should build",
    );
    let handle = advance_handle(
        handle,
        |h| builder_order_by_column(h, "userName".to_string(), Some("ASC".to_string()), None),
        "primary order by should build",
    );
    let handle = advance_handle(
        handle,
        |h| builder_order_by_column(h, "teamName".to_string(), Some("DESC".to_string()), None),
        "secondary order by should build",
    );
    let handle = advance_handle(handle, |h| builder_limit(h, 8), "limit should build");
    advance_handle(handle, |h| builder_offset(h, 4), "offset should build")
}

fn build_app_heavy_cte_handle() -> String {
    let ranked_memberships = {
        let handle = builder_new(Some("postgres"));
        let handle = advance_handle(
            handle,
            |h| builder_from_table_alias(h, "users".to_string(), "u".to_string()),
            "cte from should build",
        );
        let handle = advance_handle(
            handle,
            |h| {
                builder_join_table_alias(
                    h,
                    "LEFT JOIN".to_string(),
                    "teamMembers".to_string(),
                    "tm".to_string(),
                )
            },
            "cte teamMembers join should build",
        );
        let handle = advance_handle(
            handle,
            |h| {
                builder_on(h, r#"{"text":"\"u\".\"id\" = \"tm\".\"userId\"","raw":"u.id = tm.userId","values":[]}"#.to_string())
            },
            "cte teamMembers join predicate should build",
        );
        let handle = advance_handle(
            handle,
            |h| {
                builder_join_table_alias(
                    h,
                    "LEFT JOIN".to_string(),
                    "teams".to_string(),
                    "t".to_string(),
                )
            },
            "cte teams join should build",
        );
        let handle = advance_handle(
            handle,
            |h| {
                builder_on(h, r#"{"text":"\"tm\".\"teamId\" = \"t\".\"id\"","raw":"tm.teamId = t.id","values":[]}"#.to_string())
            },
            "cte teams join predicate should build",
        );
        let handle = advance_handle(
            handle,
            |h| {
                builder_select_fragment(
                    h,
                    APP_HEAVY_CTE_SELECT_FRAGMENT_JSON.to_string(),
                    APP_HEAVY_CTE_SELECTED_COLUMNS_JSON.to_string(),
                )
            },
            "cte select fragment should build",
        );
        let handle = advance_handle(
            handle,
            |h| {
                builder_where(
                    h,
                    r#"{"text":"\"u\".\"age\" >= ?","raw":"u.age >= 18","values":[18]}"#
                        .to_string(),
                )
            },
            "cte age filter should build",
        );
        advance_handle(
            handle,
            |h| builder_and_where(h, APP_CORE_COUNTRY_PREDICATE_JSON.to_string()),
            "cte country filter should build",
        )
    };

    let handle = builder_new(Some("postgres"));
    let handle = advance_handle(
        handle,
        |h| builder_with_handle(h, "rankedMemberships".to_string(), &ranked_memberships),
        "with handle should build",
    );
    builder_drop(&ranked_memberships);
    let handle = advance_handle(
        handle,
        |h| builder_from_table_alias(h, "rankedMemberships".to_string(), "rm".to_string()),
        "outer from should build",
    );
    let handle = advance_handle(
        handle,
        |h| {
            builder_select_columns(
                h,
                serde_json::to_string(&vec![
                    "rm.userId",
                    "rm.userName",
                    "rm.country",
                    "rm.teamName",
                    "rm.ageNextYear",
                    "rm.ageBucket",
                    "rm.previousTeamId",
                    "rm.teamMembershipCount",
                ])
                .expect("outer columns should serialize"),
            )
        },
        "outer select should build",
    );
    let handle = advance_handle(
        handle,
        |h| builder_where(h, APP_HEAVY_CTE_TEAM_COUNT_PREDICATE_JSON.to_string()),
        "outer where should build",
    );
    let handle = advance_handle(
        handle,
        |h| builder_order_by_column(h, "userName".to_string(), Some("ASC".to_string()), None),
        "outer primary order should build",
    );
    let handle = advance_handle(
        handle,
        |h| {
            builder_order_by_column(
                h,
                "teamMembershipCount".to_string(),
                Some("DESC".to_string()),
                None,
            )
        },
        "outer secondary order should build",
    );
    let handle = advance_handle(
        handle,
        |h| builder_order_by_column(h, "teamName".to_string(), Some("ASC".to_string()), None),
        "outer tertiary order should build",
    );
    advance_handle(handle, |h| builder_limit(h, 8), "outer limit should build")
}

fn build_parity_dml_conflict_handle() -> String {
    let handle = builder_new(Some("postgres"));
    let handle = advance_handle(
        handle,
        |h| builder_insert_into(h, "users".to_string()),
        "insert into should build",
    );
    let handle = advance_handle(
        handle,
        |h| {
            builder_values_insert(
                h,
                rows_json(
                    &["id", "name", "email", "age", "country", "role"],
                    vec![vec![
                        SqlValue::Primitive(Primitive::String("u_1".to_string())),
                        SqlValue::Primitive(Primitive::String("Ada".to_string())),
                        SqlValue::Primitive(Primitive::String("ada@example.com".to_string())),
                        SqlValue::Primitive(Primitive::Number(37.into())),
                        SqlValue::Primitive(Primitive::String("AU".to_string())),
                        SqlValue::Primitive(Primitive::String("coach".to_string())),
                    ]],
                ),
            )
        },
        "values insert should build",
    );
    let handle = advance_handle(
        handle,
        |h| {
            builder_on_conflict_columns(
                h,
                serde_json::to_string(&vec!["id"]).expect("conflict columns should serialize"),
            )
        },
        "on conflict should build",
    );
    advance_handle(
        handle,
        |h| {
            builder_do_update_set(
                h,
                assignment_json(vec![
                    (
                        "name",
                        SqlValue::Primitive(Primitive::String("Ada Updated".to_string())),
                    ),
                    (
                        "country",
                        SqlValue::Primitive(Primitive::String("NZ".to_string())),
                    ),
                ]),
            )
        },
        "do update set should build",
    )
}

fn build_parity_cte_compound_handle() -> String {
    let active_teams = {
        let handle = builder_new(Some("postgres"));
        let handle = advance_handle(
            handle,
            |h| builder_from_table_alias(h, "teams".to_string(), "t".to_string()),
            "activeTeams from should build",
        );
        let handle = advance_handle(
            handle,
            |h| builder_select_columns(h, ACTIVE_TEAMS_SELECT_COLUMNS_JSON.to_string()),
            "activeTeams select should build",
        );
        advance_handle(
            handle,
            |h| builder_where(h, ACTIVE_TEAMS_COUNTRY_PREDICATE_JSON.to_string()),
            "activeTeams where should build",
        )
    };

    let eligible_users = {
        let adults = {
            let handle = builder_new(Some("postgres"));
            let handle = advance_handle(
                handle,
                |h| builder_from_table_alias(h, "users".to_string(), "u".to_string()),
                "adults from should build",
            );
            let handle = advance_handle(
                handle,
                |h| builder_select_columns(h, ELIGIBLE_USERS_SELECT_COLUMNS_JSON.to_string()),
                "adults select should build",
            );
            advance_handle(
                handle,
                |h| builder_where(h, ELIGIBLE_ADULTS_PREDICATE_JSON.to_string()),
                "adults where should build",
            )
        };

        let coaches = {
            let handle = builder_new(Some("postgres"));
            let handle = advance_handle(
                handle,
                |h| builder_from_table_alias(h, "users".to_string(), "u".to_string()),
                "coaches from should build",
            );
            let handle = advance_handle(
                handle,
                |h| builder_select_columns(h, ELIGIBLE_USERS_SELECT_COLUMNS_JSON.to_string()),
                "coaches select should build",
            );
            advance_handle(
                handle,
                |h| builder_where(h, ELIGIBLE_COACHES_PREDICATE_JSON.to_string()),
                "coaches where should build",
            )
        };

        let handle = advance_handle(
            adults,
            |h| builder_union_handle(h, &coaches),
            "eligible union should build",
        );
        builder_drop(&coaches);
        handle
    };

    let org_tree = {
        let handle = builder_new(Some("postgres"));
        let handle = advance_handle(
            handle,
            |h| builder_from_table_alias(h, "orgUnits".to_string(), "ou".to_string()),
            "orgTree from should build",
        );
        let handle = advance_handle(
            handle,
            |h| builder_select_columns(h, ORG_TREE_SELECT_COLUMNS_JSON.to_string()),
            "orgTree select should build",
        );
        advance_handle(
            handle,
            |h| builder_where(h, ORG_TREE_ROOT_PREDICATE_JSON.to_string()),
            "orgTree where should build",
        )
    };

    let handle = builder_new(Some("postgres"));
    let handle = advance_handle(
        handle,
        |h| builder_with_handle(h, "activeTeams".to_string(), &active_teams),
        "with activeTeams should build",
    );
    builder_drop(&active_teams);
    let handle = advance_handle(
        handle,
        |h| builder_with_recursive_handle(h, "orgTree".to_string(), &org_tree),
        "with recursive orgTree should build",
    );
    builder_drop(&org_tree);
    let handle = advance_handle(
        handle,
        |h| builder_from_table_alias(h, "activeTeams".to_string(), "at".to_string()),
        "outer from should build",
    );
    let handle = advance_handle(
        handle,
        |h| {
            builder_join_subquery_handle(
                h,
                "INNER JOIN".to_string(),
                "eligibleUsers".to_string(),
                &eligible_users,
            )
        },
        "join eligibleUsers should build",
    );
    builder_drop(&eligible_users);
    let handle = advance_handle(
        handle,
        |h| builder_on(h, ACTIVE_TEAMS_JOIN_PREDICATE_JSON.to_string()),
        "join predicate should build",
    );
    let handle = advance_handle(
        handle,
        |h| builder_select_columns(h, PARITY_CTE_COMPOUND_SELECT_COLUMNS_JSON.to_string()),
        "outer select should build",
    );
    let handle = advance_handle(
        handle,
        |h| {
            builder_order_by_column(
                h,
                "eligibleUsers.name".to_string(),
                Some("ASC".to_string()),
                None,
            )
        },
        "outer order should build",
    );
    advance_handle(handle, |h| builder_limit(h, 5), "outer limit should build")
}

fn canonicalize_query(handle: &str) -> String {
    builder_canonical_ir_json(handle).expect("canonicalization should succeed")
}

fn render_sql(parts: &plec_query::builder::QueryParts) -> plec_query::SqlQuery {
    build_query(parts).expect("SQL rendering should succeed")
}

fn benchmark_scenario(c: &mut Criterion, scenario: &Scenario) {
    let mut group = c.benchmark_group(format!("query_phases/{}", scenario.name));

    group.bench_function("ast_build", |b| {
        b.iter(|| {
            let handle = (scenario.build_handle)();
            black_box(&handle);
            builder_drop(&handle);
        })
    });

    let canonical_handle = (scenario.build_handle)();
    group.bench_function("canonicalization", |b| {
        b.iter(|| canonicalize_query(black_box(&canonical_handle)))
    });

    let render_handle = (scenario.build_handle)();
    let render_parts = registry_get_parts(&render_handle).expect("parts snapshot should exist");
    group.bench_function("sql_generation", |b| {
        b.iter(|| {
            let query = render_sql(black_box(&render_parts));
            black_box(query)
        })
    });

    builder_drop(&canonical_handle);
    builder_drop(&render_handle);
    group.finish();
}

fn benchmark(c: &mut Criterion) {
    let scenarios = [
        Scenario {
            name: "simple",
            build_handle: build_simple_query_handle,
        },
        Scenario {
            name: "join",
            build_handle: build_join_query_handle,
        },
        Scenario {
            name: "aggregation",
            build_handle: build_aggregation_query_handle,
        },
        Scenario {
            name: "app_core_builder_flow",
            build_handle: build_app_core_builder_flow_handle,
        },
        Scenario {
            name: "app_heavy_cte",
            build_handle: build_app_heavy_cte_handle,
        },
        Scenario {
            name: "parity_dml_conflict",
            build_handle: build_parity_dml_conflict_handle,
        },
        Scenario {
            name: "parity_cte_compound",
            build_handle: build_parity_cte_compound_handle,
        },
    ];

    for scenario in &scenarios {
        benchmark_scenario(c, scenario);
    }
}

criterion_group!(benches, benchmark);
criterion_main!(benches);
