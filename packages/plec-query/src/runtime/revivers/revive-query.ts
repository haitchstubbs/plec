import type { SqlQuery } from '#types';
import { revivePrimitive } from './revive-primitive';

export function reviveQuery(wire: {
  text: string;
  raw: string;
  values: unknown[];
}): SqlQuery {
  return {
    text: wire.text,
    raw: wire.raw,
    values: wire.values.map(revivePrimitive),
  };
}
