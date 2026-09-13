import { isObjectRecord as isRecord } from '../../utils/record';
import type { Primitive } from '#types';

export function revivePrimitive(value: unknown): Primitive {
  if (
    isRecord(value) &&
    '__kind' in value &&
    (value as { __kind?: string }).__kind === 'date'
  ) {
    return new Date((value as unknown as { value: string }).value);
  }

  if (
    isRecord(value) &&
    '__kind' in value &&
    (value as { __kind?: string }).__kind === 'bigint'
  ) {
    return BigInt((value as unknown as { value: string }).value);
  }

  return value as Primitive;
}
