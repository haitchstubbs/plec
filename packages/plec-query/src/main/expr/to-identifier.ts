import { identifier } from '#core';
import type { SqlIdentifier } from '#types';

export function toIdentifier(path: string): SqlIdentifier {
  return identifier(...path.split('.'));
}
