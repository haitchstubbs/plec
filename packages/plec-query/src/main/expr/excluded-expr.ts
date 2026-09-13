import type { SqlQuery } from '#types';
import { EMPTY_EXPR_VALUES } from './empty-expr-values';

export function excludedExpr(column: string): SqlQuery {
  return {
    type: 'excluded',
    column,
    text: '',
    raw: '',
    values: EMPTY_EXPR_VALUES,
  } as SqlQuery;
}
