import {
  mkdtemp,
  mkdir,
  rm,
  symlink,
  writeFile,
} from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { loadSsrProviders } from './providers.js';

const roots: string[] = [];

afterEach(async () => {
  await Promise.all(
    roots
      .splice(0)
      .map((root) => rm(root, { recursive: true, force: true })),
  );
});

describe('Node host-provider SSR registry', () => {
  it('loads only opted-in providers, applies the component allowlist, and invokes renderers', async () => {
    const { root, manifest } = await fixture(
      {
        id: 'icons',
        module: '/_plec/assets/provider.mjs',
        components: ['Badge', 'NoRenderer'],
        ssr: true,
      },
      `export default () => ({ Badge: { render: ({ label }) => '<b>' + label + '</b>' } });`,
    );
    const renderHost = await loadSsrProviders(root, manifest);
    expect(
      await renderHost(
        JSON.stringify({
          provider: 'icons',
          component: 'Badge',
          props: { label: 'Plec' },
        }),
      ),
    ).toBe('<b>Plec</b>');
    expect(
      await renderHost(
        JSON.stringify({
          provider: 'icons',
          component: 'Other',
          props: {},
        }),
      ),
    ).toBe('');
    expect(
      await renderHost(
        JSON.stringify({
          provider: 'icons',
          component: 'NoRenderer',
          props: {},
        }),
      ),
    ).toBe('');
  });

  it('keeps browser-only providers inert without importing their modules', async () => {
    const { root, manifest } = await fixture({
      id: 'browser-only',
      module: '/_plec/assets/missing.mjs',
      components: ['Widget'],
      ssr: false,
    });
    const renderHost = await loadSsrProviders(root, manifest);
    expect(
      await renderHost(
        JSON.stringify({
          provider: 'browser-only',
          component: 'Widget',
          props: {},
        }),
      ),
    ).toBe('');
  });

  it('rejects module traversal and symlink escapes during startup', async () => {
    const traversal = await fixture({
      id: 'icons',
      module: '/_plec/assets/../../outside.mjs',
      components: ['Icon'],
      ssr: true,
    });
    await expect(
      loadSsrProviders(traversal.root, traversal.manifest),
    ).rejects.toThrow('invalid host provider module');

    const escape = await fixture({
      id: 'icons',
      module: '/_plec/assets/provider.mjs',
      components: ['Icon'],
      ssr: true,
    });
    const outside = path.join(escape.root, 'outside.mjs');
    await writeFile(outside, 'export default () => ({})');
    await rm(path.join(escape.root, 'client/assets/provider.mjs'));
    await symlink(
      outside,
      path.join(escape.root, 'client/assets/provider.mjs'),
    );
    await expect(
      loadSsrProviders(escape.root, escape.manifest),
    ).rejects.toThrow('invalid host provider module');
  });

  it('rejects invalid factories, renderer failures, and oversized markup', async () => {
    const invalid = await fixture(
      {
        id: 'bad',
        module: '/_plec/assets/provider.mjs',
        components: ['Bad'],
        ssr: true,
      },
      'export default 1;',
    );
    await expect(
      loadSsrProviders(invalid.root, invalid.manifest),
    ).rejects.toThrow('no default factory');

    const failing = await fixture(
      {
        id: 'bad',
        module: '/_plec/assets/provider.mjs',
        components: ['Bad'],
        ssr: true,
      },
      `export default () => ({ Bad: { render: () => { throw new Error('failure'); } } });`,
    );
    const renderFail = await loadSsrProviders(
      failing.root,
      failing.manifest,
    );
    await expect(
      renderFail(
        JSON.stringify({
          provider: 'bad',
          component: 'Bad',
          props: {},
        }),
      ),
    ).rejects.toThrow('failure');

    const large = await fixture(
      {
        id: 'large',
        module: '/_plec/assets/provider.mjs',
        components: ['Large'],
        ssr: true,
      },
      `export default () => ({ Large: { render: () => 'x'.repeat(1024 * 1024 + 1) } });`,
    );
    const renderLarge = await loadSsrProviders(
      large.root,
      large.manifest,
    );
    await expect(
      renderLarge(
        JSON.stringify({
          provider: 'large',
          component: 'Large',
          props: {},
        }),
      ),
    ).rejects.toThrow('invalid host render response');
  });
});

async function fixture(
  provider: {
    id: string;
    module: string;
    components: string[];
    ssr: boolean;
  },
  moduleSource = `export default () => ({})`,
) {
  const root = await mkdtemp(path.join(os.tmpdir(), 'plec-provider-'));
  roots.push(root);
  await mkdir(path.join(root, 'client/assets'), { recursive: true });
  const manifest = path.join(root, 'client/host-providers.json');
  await writeFile(
    path.join(root, 'client/assets/provider.mjs'),
    moduleSource,
  );
  await writeFile(
    manifest,
    JSON.stringify({
      version: 2,
      revision: 'test',
      providers: [provider],
    }),
  );
  return { root, manifest };
}
