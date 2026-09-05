import { afterEach, describe, expect, it, vi } from 'vitest';
import { readSsrBootstrap } from './index';

type FakeElement = { textContent?: string | null };

function setBootstrap(payload: FakeElement | null) {
  vi.stubGlobal('document', {
    querySelector: () => payload,
  });
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('ssr bootstrap reader', () => {
  it('returns null without a bootstrap script', () => {
    setBootstrap(null);
    expect(readSsrBootstrap()).toBeNull();
  });

  it('returns null for an empty bootstrap script', () => {
    setBootstrap({ textContent: '' });
    expect(readSsrBootstrap()).toBeNull();
  });

  it('flags a present but unparseable bootstrap', () => {
    setBootstrap({ textContent: '{not json' });
    expect(readSsrBootstrap()).toEqual({ kind: 'invalid' });
  });

  it('recognizes the v2 snapshot payload', () => {
    setBootstrap({
      textContent: JSON.stringify({
        version: 2,
        snapshot: {
          version: 2,
          revision: 'rev-1',
          routes: [
            { routeId: 'routes.tsx#Home', params: {}, phase: 'active' },
          ],
          public: { location: '/' },
          loaders: [],
          structure: { graphs: {} },
        },
      }),
    });
    expect(readSsrBootstrap()).toEqual({
      kind: 'snapshot',
      revision: 'rev-1',
      routeId: 'routes.tsx#Home',
      snapshot: expect.objectContaining({ version: 2 }),
    });
  });

  it('treats non-v2 payloads as absent so the page mounts fresh', () => {
    setBootstrap({
      textContent: JSON.stringify({
        revision: 'rev-1',
        routeId: 'routes.tsx#Home',
        public: { location: { pathname: '/', search: '' } },
      }),
    });
    expect(readSsrBootstrap()).toBeNull();
  });
});
