import type { SqlQuery } from '#types';
import { makeExprNode } from './make-expr-node';

export function exprRef(col: string): SqlQuery {
  return makeExprNode({ type: 'ref', parts: col.split('.') });
}
