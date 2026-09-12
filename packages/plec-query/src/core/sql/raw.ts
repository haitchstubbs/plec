import type { SqlRaw } from "#types";

/**
 * Create a SQL raw expression.
 *
 * @remarks
 * Wraps raw SQL text so it can be used in query builder expressions.
 *
 * @param text - The raw SQL string.
 * @returns A {@link SqlRaw} wrapper object.
 *
 * @example
 * ```ts
 * raw("CURRENT_TIMESTAMP");
 * ```
 */
export function raw(text: string): SqlRaw {
  return { __kind: "raw", text } as SqlRaw;
}
