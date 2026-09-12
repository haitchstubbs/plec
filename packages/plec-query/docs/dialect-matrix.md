# node-query dialect matrix

This file is generated from Rust capability and policy metadata.

## Capability guarantees

- `DialectRender`: feature is supported and rendered with dialect-specific SQL.
- `FallbackRewrite`: feature is rewritten to an equivalent form and may emit a warning.
- `HardError`: feature has no valid equivalent and must fail validation/rendering.

## Supported dialect names

Canonical dialect names:

- `postgres`, `duckdb`, `sqlite`, `mysql`, `mssql`, `oracle`, `snowflake`, `googlesql`, `redshift`, `bigquery`, `clickhouse`

Dialect aliases accepted by parser:

- `mssqlserver` -> `mssql`

TypeScript `DIALECTS` entries that are currently non-contract inputs (unsupported unless Rust capability support exists):

- `cassandra`, `dynamodb`, `elasticsearch`, `mongodb`, `redis`

## Capability matrix (Rust core source of truth)

| Dialect      | Placeholder   | RETURNING | Window | NULLS FIRST/LAST | DISTINCT ON | CTE | ILIKE | Insert conflict | Pagination         | Offset requires ORDER BY | Offset-only allowed | LIMIT via TOP only | RIGHT JOIN | FULL OUTER JOIN | USING | LATERAL | Recursive CTE style      | Recursive CTE aliases required | Lock strengths        | Lock modifiers      |
| ------------ | ------------- | --------- | ------ | ---------------- | ----------- | --- | ----- | --------------- | ------------------ | ------------------------ | ------------------- | ------------------ | ---------- | --------------- | ----- | ------- | ------------------------ | ------------------------------ | --------------------- | ------------------- |
| `postgres`   | `$n`          | Yes       | Yes    | Yes              | Yes         | Yes | Yes   | `on_conflict`   | `limit_offset`     | No                       | Yes                 | No                 | Yes        | Yes             | Yes   | Yes     | `with_recursive_keyword` | No                             | FOR UPDATE, FOR SHARE | NOWAIT, SKIP LOCKED |
| `duckdb`     | `$n`          | Yes       | Yes    | Yes              | Yes         | Yes | Yes   | `on_conflict`   | `limit_offset`     | No                       | Yes                 | No                 | Yes        | Yes             | Yes   | Yes     | `with_recursive_keyword` | No                             | n/a                   | n/a                 |
| `sqlite`     | `?`           | Yes       | Yes    | Yes              | No          | Yes | No    | `on_conflict`   | `limit_offset`     | No                       | Yes                 | No                 | No         | No              | Yes   | No      | `with_recursive_keyword` | No                             | n/a                   | n/a                 |
| `mysql`      | `?`           | No        | Yes    | No               | No          | Yes | No    | `mysql`         | `limit_offset`     | No                       | No                  | No                 | Yes        | No              | Yes   | Yes     | `with_recursive_keyword` | No                             | n/a                   | n/a                 |
| `mssql`      | `@pN`         | No        | Yes    | No               | No          | Yes | No    | `unsupported`   | `top_offset_fetch` | Yes                      | Yes                 | Yes                | Yes        | Yes             | No    | Yes     | `with_only`              | No                             | n/a                   | n/a                 |
| `oracle`     | `unsupported` | No        | Yes    | Yes              | No          | Yes | No    | `unsupported`   | `unsupported`      | No                       | No                  | No                 | Yes        | Yes             | Yes   | Yes     | `with_only`              | Yes                            | n/a                   | n/a                 |
| `snowflake`  | `unsupported` | No        | Yes    | Yes              | No          | Yes | Yes   | `unsupported`   | `unsupported`      | No                       | No                  | No                 | Yes        | Yes             | Yes   | Yes     | `with_recursive_keyword` | Yes                            | n/a                   | n/a                 |
| `googlesql`  | `unsupported` | No        | Yes    | No               | No          | Yes | No    | `unsupported`   | `unsupported`      | No                       | No                  | No                 | Yes        | Yes             | Yes   | Yes     | `with_recursive_keyword` | No                             | n/a                   | n/a                 |
| `redshift`   | `unsupported` | No        | Yes    | No               | No          | Yes | Yes   | `unsupported`   | `unsupported`      | No                       | No                  | No                 | Yes        | Yes             | Yes   | Yes     | `with_recursive_keyword` | Yes                            | n/a                   | n/a                 |
| `bigquery`   | `unsupported` | No        | Yes    | No               | No          | Yes | No    | `unsupported`   | `unsupported`      | No                       | No                  | No                 | Yes        | Yes             | Yes   | Yes     | `with_recursive_keyword` | No                             | n/a                   | n/a                 |
| `clickhouse` | `unsupported` | No        | Yes    | No               | No          | Yes | No    | `unsupported`   | `unsupported`      | No                       | No                  | No                 | Yes        | Yes             | Yes   | No      | `with_recursive_keyword` | No                             | n/a                   | n/a                 |

## Policy classification matrix (M2-3)

| Dialect      | RETURNING       | ILIKE             | INTERSECT ALL   | DISTINCT ON     | LIMIT/OFFSET semantic family |
| ------------ | --------------- | ----------------- | --------------- | --------------- | ---------------------------- |
| `postgres`   | `DialectRender` | `DialectRender`   | `DialectRender` | `DialectRender` | `DialectRender`              |
| `duckdb`     | `DialectRender` | `DialectRender`   | `DialectRender` | `DialectRender` | `DialectRender`              |
| `sqlite`     | `DialectRender` | `FallbackRewrite` | `DialectRender` | `HardError`     | `DialectRender`              |
| `mysql`      | `HardError`     | `FallbackRewrite` | `HardError`     | `HardError`     | `DialectRender`              |
| `mssql`      | `HardError`     | `FallbackRewrite` | `DialectRender` | `HardError`     | `FallbackRewrite`            |
| `oracle`     | `HardError`     | `FallbackRewrite` | `DialectRender` | `HardError`     | `HardError`                  |
| `snowflake`  | `HardError`     | `DialectRender`   | `DialectRender` | `HardError`     | `HardError`                  |
| `googlesql`  | `HardError`     | `FallbackRewrite` | `DialectRender` | `HardError`     | `HardError`                  |
| `redshift`   | `HardError`     | `DialectRender`   | `DialectRender` | `HardError`     | `HardError`                  |
| `bigquery`   | `HardError`     | `FallbackRewrite` | `DialectRender` | `HardError`     | `HardError`                  |
| `clickhouse` | `HardError`     | `FallbackRewrite` | `DialectRender` | `HardError`     | `HardError`                  |

## Node/WASM/core parity expectations

- Rust core defines runtime capability, validation, and policy semantics.
- Node native and WASM bindings must expose equivalent behavior for the same dialect and query shape.
- TypeScript remains a typed facade (compile-time inference and API ergonomics), not a second runtime semantics engine.

## Maintenance note

1. Add or change dialect/feature capability policy in Rust first.
2. Regenerate this matrix via `yarn docs:generate-dialect-matrix`.
3. Keep Node native and WASM bindings parity-aligned with Rust behavior.
