import { sql } from '#core';
import type {
  ComparisonOperator,
  Primitive,
  SqlIdentifier,
  SqlQuery,
  SqlRaw,
} from '#types';
import {
  getActiveRuntimeDialect,
  makeDeferredQuery,
  runtimeAnd,
  runtimeBetween,
  runtimeCmp,
  runtimeExistsSql,
  runtimeInArray,
  runtimeIsNotNull,
  runtimeIsNull,
  runtimeLikeSql,
  runtimeNotBetween,
  runtimeNotExistsSql,
  runtimeNotInArray,
  runtimeNotLikeSql,
  runtimeOr,
  serializeSqlValue,
} from '../runtime/bridge';

type PredicateInput = SqlQuery | string;
type ExpressionInput = Primitive | SqlIdentifier | SqlQuery | SqlRaw;
const COMPARISON_OPERATORS = new Set([
  '=',
  '!=',
  '<>',
  '>',
  '>=',
  '<',
  '<=',
]);
const SQL_DIALECTS = new Set([
  'postgres',
  'duckdb',
  'sqlite',
  'mysql',
  'mssql',
  'mssqlserver',
  'oracle',
  'snowflake',
  'googlesql',
  'redshift',
  'bigquery',
  'clickhouse',
]);

function withSqlShape(fields: Record<string, unknown>): SqlQuery {
  return makeDeferredQuery(fields, () => {
    throw new Error('Deferred expression is missing a materializer.');
  });
}

function exprNode(value: ExpressionInput): SqlQuery {
  if (value === null) return withSqlShape({ type: 'val', value: null });
  if (typeof value === 'bigint' || value instanceof Date) {
    return withSqlShape({
      type: 'val',
      value: serializeSqlValue(value),
    });
  }
  if (
    typeof value === 'boolean' ||
    typeof value === 'number' ||
    typeof value === 'string'
  ) {
    return withSqlShape({ type: 'val', value });
  }
  const obj = value as Record<string, unknown>;
  if ('type' in obj && typeof obj.type === 'string') {
    return value as SqlQuery;
  }
  if (obj.__kind === 'identifier') {
    return withSqlShape({
      type: 'ref',
      parts: (value as SqlIdentifier).parts,
    });
  }
  if (obj.__kind === 'raw') {
    return withSqlShape({ type: 'raw', text: obj.text as string });
  }
  return withSqlShape({
    type: 'query',
    query: serializeSqlValue(value),
  });
}

function predicateNode(condition: PredicateInput): SqlQuery {
  if (typeof condition === 'string') {
    return withSqlShape({ type: 'raw', text: condition });
  }
  return exprNode(condition);
}

function deferredExpression(
  node: Record<string, unknown>,
  materialize: () => SqlQuery,
): SqlQuery {
  return makeDeferredQuery(node, materialize);
}

export function ref(...parts: string[]): SqlIdentifier {
  if (parts.length === 1) {
    return { __kind: 'identifier', parts: (parts[0] ?? '').split('.') };
  }

  return { __kind: 'identifier', parts };
}

export function excluded(column: string): SqlQuery {
  return {
    type: 'excluded',
    column,
    text: '',
    raw: '',
    values: [],
  } as SqlQuery;
}

export function cmp(
  left: ExpressionInput,
  operator: ComparisonOperator,
  right: ExpressionInput,
): SqlQuery {
  const normalized = operator.trim();
  const dialect = (getActiveRuntimeDialect() ?? 'postgres')
    .trim()
    .toLowerCase();
  if (
    !SQL_DIALECTS.has(dialect) ||
    !COMPARISON_OPERATORS.has(normalized)
  ) {
    throw new Error(
      `Dialect "${dialect}" does not support operator "${normalized}" in this builder.`,
    );
  }
  return deferredExpression(
    {
      type: 'cmp',
      left: exprNode(left),
      op: normalized,
      right: exprNode(right),
    },
    () => runtimeCmp(left, normalized, right),
  );
}

export function eq(
  left: ExpressionInput,
  right: ExpressionInput,
): SqlQuery {
  return cmp(left, '=', right);
}

export function ne(
  left: ExpressionInput,
  right: ExpressionInput,
): SqlQuery {
  return cmp(left, '<>', right);
}

export function gt(
  left: ExpressionInput,
  right: ExpressionInput,
): SqlQuery {
  return cmp(left, '>', right);
}

export function gte(
  left: ExpressionInput,
  right: ExpressionInput,
): SqlQuery {
  return cmp(left, '>=', right);
}

export function lt(
  left: ExpressionInput,
  right: ExpressionInput,
): SqlQuery {
  return cmp(left, '<', right);
}

export function lte(
  left: ExpressionInput,
  right: ExpressionInput,
): SqlQuery {
  return cmp(left, '<=', right);
}

export function isNull(value: ExpressionInput): SqlQuery {
  return deferredExpression(
    { type: 'isNull', value: exprNode(value) },
    () => runtimeIsNull(value),
  );
}

export function isNotNull(value: ExpressionInput): SqlQuery {
  return deferredExpression(
    { type: 'isNotNull', value: exprNode(value) },
    () => runtimeIsNotNull(value),
  );
}

export function inArray(
  value: ExpressionInput,
  items: Primitive[] | SqlQuery,
): SqlQuery {
  if (Array.isArray(items)) {
    if (items.length === 0) {
      throw new Error('Cannot interpolate an empty array');
    }
    return deferredExpression(
      {
        type: 'inArray',
        value: exprNode(value),
        items: items.map((item) => serializeSqlValue(item)),
      },
      () => runtimeInArray(value, items),
    );
  }
  return sql`${value} IN (${items})`;
}

export function notInArray(
  value: ExpressionInput,
  items: Primitive[] | SqlQuery,
): SqlQuery {
  if (Array.isArray(items)) {
    if (items.length === 0) {
      throw new Error('Cannot interpolate an empty array');
    }
    return deferredExpression(
      {
        type: 'notInArray',
        value: exprNode(value),
        items: items.map((item) => serializeSqlValue(item)),
      },
      () => runtimeNotInArray(value, items),
    );
  }
  return sql`${value} NOT IN (${items})`;
}

export function between(
  value: ExpressionInput,
  lower: ExpressionInput,
  upper: ExpressionInput,
): SqlQuery {
  return deferredExpression(
    {
      type: 'between',
      value: exprNode(value),
      lower: exprNode(lower),
      upper: exprNode(upper),
    },
    () => runtimeBetween(value, lower, upper),
  );
}

export function notBetween(
  value: ExpressionInput,
  lower: ExpressionInput,
  upper: ExpressionInput,
): SqlQuery {
  return deferredExpression(
    {
      type: 'notBetween',
      value: exprNode(value),
      lower: exprNode(lower),
      upper: exprNode(upper),
    },
    () => runtimeNotBetween(value, lower, upper),
  );
}

export function like(
  value: ExpressionInput,
  pattern: ExpressionInput,
): SqlQuery {
  return deferredExpression(
    {
      type: 'like',
      value: exprNode(value),
      pattern: exprNode(pattern),
    },
    () => runtimeLikeSql(value, pattern),
  );
}

export function notLike(
  value: ExpressionInput,
  pattern: ExpressionInput,
): SqlQuery {
  return deferredExpression(
    {
      type: 'notLike',
      value: exprNode(value),
      pattern: exprNode(pattern),
    },
    () => runtimeNotLikeSql(value, pattern),
  );
}

export function exists(query: SqlQuery): SqlQuery {
  return deferredExpression(
    {
      type: 'exists',
      query: {
        text: query.text,
        raw: query.raw,
        values: query.values.map((value) => serializeSqlValue(value)),
      },
    },
    () => runtimeExistsSql(query),
  );
}

export function notExists(query: SqlQuery): SqlQuery {
  return deferredExpression(
    {
      type: 'notExists',
      query: {
        text: query.text,
        raw: query.raw,
        values: query.values.map((value) => serializeSqlValue(value)),
      },
    },
    () => runtimeNotExistsSql(query),
  );
}

export function and(...conditions: PredicateInput[]): SqlQuery {
  return deferredExpression(
    { type: 'and', conditions: conditions.map(predicateNode) },
    () => runtimeAnd(conditions),
  );
}

export function or(...conditions: PredicateInput[]): SqlQuery {
  return deferredExpression(
    { type: 'or', conditions: conditions.map(predicateNode) },
    () => runtimeOr(conditions),
  );
}

export const Expressions = {
  and,
  between,
  cmp,
  eq,
  excluded,
  exists,
  gt,
  gte,
  inArray,
  isNotNull,
  isNull,
  like,
  lt,
  lte,
  ne,
  notBetween,
  notExists,
  notInArray,
  notLike,
  or,
  ref,
};
