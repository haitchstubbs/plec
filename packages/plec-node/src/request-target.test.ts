import cases from '../../../testdata/node-host/request-targets.json';
import { describe, expect, it } from 'vitest';
import {
  canonicalRequestTarget,
  classifyPlecPath,
} from './request-target';

describe('canonical request target contract', () => {
  for (const fixture of cases) {
    it(`handles ${JSON.stringify(fixture.target)}`, () => {
      if ('invalid' in fixture) {
        expect(() => canonicalRequestTarget(fixture.target)).toThrow();
        return;
      }
      const parsed = canonicalRequestTarget(fixture.target);
      expect(parsed.path).toBe(fixture.path);
      expect(parsed.query).toBe(
        'query' in fixture ? fixture.query : '',
      );
      expect(classifyPlecPath(parsed.path)).toBe(fixture.class);
    });
  }
});
