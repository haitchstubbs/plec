import type { SqlIdentifier } from "#types";

/**
 * Create a SQL identifier from path segments.
 *
 * @remarks
 * Joins parts into an identifier object for query SQL generation.
 *
 * @example
 * ```ts
 * identifier("schema", "table", "column");
 * ```
 */
export function identifier(...parts: string[]): SqlIdentifier {
  return { __kind: "identifier", parts } as SqlIdentifier;
}
