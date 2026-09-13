import type { SqlQuery } from '#types';
import { serializeSqlValue } from '../../runtime/bridge';

export function buildPredicateForOp(q: SqlQuery): unknown {
  const obj = q as unknown as Record<string, unknown>;
  if (!('__kind' in obj) && q.values.length === 0) {
    return q;
  }
  return serializeSqlValue(q);
}
