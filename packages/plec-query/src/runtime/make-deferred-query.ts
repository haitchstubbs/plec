import type { SqlQuery } from '#types';
import type { DeferredQueryNode } from './types';

export function makeDeferredQuery(
  node: DeferredQueryNode,
  materialize: () => SqlQuery,
): SqlQuery {
  let cached: SqlQuery | undefined;
  const query = { ...node } as Record<string, unknown>;
  const get = (): SqlQuery => {
    cached ??= materialize();
    return cached;
  };
  Object.defineProperties(query, {
    text: {
      enumerable: false,
      get: () => get().text,
    },
    raw: {
      enumerable: false,
      get: () => get().raw,
    },
    values: {
      enumerable: false,
      get: () => get().values,
    },
  });
  return query as unknown as SqlQuery;
}
