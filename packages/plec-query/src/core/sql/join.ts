import type { SqlIdentifier, SqlQuery, SqlRaw } from '#types';
import { runtimeJoin } from '../../runtime/bridge';

/**
 * Join SQL fragments into a single query expression.
 *
 * @remarks
 * Uses a separator to combine identifiers, raw fragments, and nested queries.
 *
 * @param items - Query fragments, identifiers, or raw SQL segments.
 * @param separator - Text placed between joined fragments.
 * @returns Combined {@link SqlQuery} object.
 *
 * @example
 * ```ts
 * join([identifier("table"), raw("?")], ", ");
 * ```
 */
export function join(
  items: Array<SqlQuery | SqlIdentifier | SqlRaw>,
  separator = ', ',
): SqlQuery {
  return runtimeJoin(items, separator);
}
