import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { compileSourceEntry } from 'plec-compiler/node-entry';
import { validateApplicationIr } from 'plec-ir';

const pages = [
  ['home', 'HomePage'],
  ['about', 'AboutPage'],
  ['not-found', 'NotFoundPage'],
] as const;

describe('Plec page compilation', () => {
  for (const [name, component] of pages) {
    it(`compiles ${name} deterministically without diagnostics`, async () => {
      const options = {
        rootDir: path.resolve('.'),
        repoRootDir: path.resolve('..', '..'),
        mode: 'strict' as const,
        rootComponent: component,
      };
      const first = await compileSourceEntry(
        path.resolve('src/routes', `${name}.tsx`),
        options,
      );
      const second = await compileSourceEntry(
        path.resolve('src/routes', `${name}.tsx`),
        options,
      );
      expect(first.result.diagnostics).toEqual([]);
      expect(validateApplicationIr(first.result.ir).rootElementId).toBe(
        'e1',
      );
      expect(first.result.ir).toEqual(second.result.ir);
    });
  }
});
