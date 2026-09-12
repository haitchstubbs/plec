import type { SqlQuery, SqlValue } from "#types";
import { runtimeDeferredSql } from "../../runtime/bridge";

/**
 * Create a deferred SQL query from template strings.
 *
 * @remarks
 * Captures template fragments and expressions as a {@link SqlQuery}.
 *
 * @param strings - Template literal segments.
 * @param exprs - Embedded SQL values.
 * @returns Combined SQL query object.
 *
 * @example
 * ```ts
 * sql`SELECT * FROM ${identifier("users")}`;
 * ```
 */
export function sql(
  strings: TemplateStringsArray,
  ...exprs: SqlValue[]
): SqlQuery {
  return runtimeDeferredSql(strings, exprs);
}
