const dialect_list = [
  'postgres',
  'mysql',
  'sqlite',
  'mssql',
  'duckdb',
  'googlesql',
  'oracle',
  'mssqlserver',
  'snowflake',
  'redshift',
  'bigquery',
  'cassandra',
  'mongodb',
  'dynamodb',
  'redis',
  'elasticsearch',
  'sqlite',
  'clickhouse',
] as const;

/**
 * Supported SQL dialect identifiers.
 *
 * @remarks
 * Used when validating or normalizing configured dialect values.
 *
 * @example
 * ```ts
 * DIALECTS.has("postgres");
 * ```
 */
export const DIALECTS = new Set(dialect_list);

/**
 * Union type of supported SQL dialect names.
 *
 * @remarks
 * Matches the values contained in {@link DIALECTS}.
 *
 * @example
 * ```ts
 * const dialect: Dialect = "mysql";
 * ```
 */
export type Dialect = (typeof dialect_list)[number];
