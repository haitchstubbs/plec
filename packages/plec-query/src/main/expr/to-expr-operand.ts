import type {
  Primitive,
  SelectExpressionInput,
  SqlIdentifier,
  SqlQuery,
  SqlValue,
  ValueLiteral,
} from '#types';
import { serializeSqlValue } from '../../runtime/bridge';
import { exprRef } from './expr-ref';
import { makeExprNode } from './make-expr-node';

export function toExprOperand<TAvailable extends string>(
  value: SelectExpressionInput<TAvailable> | '*' | SqlIdentifier,
): SqlQuery {
  if (value === null) return makeExprNode({ type: 'val', value: null });
  if (value === '*') return makeExprNode({ type: 'raw', text: '*' });
  if (typeof value === 'boolean' || typeof value === 'number')
    return makeExprNode({ type: 'val', value });
  if (typeof value === 'bigint' || value instanceof Date)
    return makeExprNode({
      type: 'val',
      value: serializeSqlValue(value as SqlValue),
    });
  if (typeof value === 'string') return exprRef(value);

  const obj = value as Record<string, unknown>;

  if (obj.__kind === 'value') {
    const prim = (value as ValueLiteral<Primitive>).value;
    return makeExprNode({
      type: 'val',
      value: serializeSqlValue(prim as SqlValue),
    });
  }
  if (obj.__kind === 'identifier') {
    return makeExprNode({
      type: 'ref',
      parts: (value as SqlIdentifier).parts,
    });
  }
  if (obj.__kind === 'raw') {
    return makeExprNode({ type: 'raw', text: obj.text as string });
  }
  if ('type' in obj && typeof obj.type === 'string') {
    return value as unknown as SqlQuery;
  }
  if ('text' in obj && 'raw' in obj && 'values' in obj) {
    const q = value as SqlQuery;
    return makeExprNode({
      type: 'query',
      query: {
        text: q.text,
        raw: q.raw,
        values: q.values.map((v) => serializeSqlValue(v as SqlValue)),
      },
    });
  }
  return makeExprNode({
    type: 'val',
    value: serializeSqlValue(value as SqlValue),
  });
}
