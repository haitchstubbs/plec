import test from 'node:test';
import {
  mkdtemp,
  mkdir,
  readFile,
  rm,
  writeFile,
} from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { buildClient } from './build.js';
import assert from 'node:assert/strict';
import { plec, plecHtmlFinalizer } from './index.js';

test('Plec Vite adapter provides Vite client and stylesheet to SSR documents', () => {
  const plugin = plec({
    root: process.cwd(),
    source: 'src/router.tsx',
    outDir: 'dist',
    internalPort: 3001,
    cli: 'plec',
  });
  const pre = plugin.transformIndexHtml.handler(
    '<html><head><link rel="stylesheet" href="/_plec/assets/client-a1b2.js"></head><body><script type="module" src="/_plec/assets/client-a1b2.js"></script></body></html>',
  );
  assert.doesNotMatch(pre, /<script type="module" src="\/_plec\//);
  const html = plecHtmlFinalizer().transformIndexHtml.handler(
    pre.replace(
      '</head>',
      '<script type="module" src="/@vite/client"></script></head>',
    ),
  );
  assert.match(html, /href="\/src\/styles\.css"/);
  assert.match(html, /src="\/@vite\/client"/);
  assert.match(html, /src="\/_plec\/assets\/client-a1b2\.js"/);
});

test('production build emits hashed client, CSS, assets, and provider entries', async () => {
  const root = await mkdtemp(
    path.join(os.tmpdir(), 'plec-vite-build-'),
  );
  try {
    await mkdir(path.join(root, 'src'));
    await writeFile(
      path.join(root, 'src/client.ts'),
      "import './style.css'; import image from './pixel.svg'; document.body.dataset.image = image;",
    );
    await writeFile(
      path.join(root, 'src/style.css'),
      "@font-face { font-family: fixture; src: url('./font.woff2'); } body { background-image: url('./pixel.svg'); }",
    );
    await writeFile(
      path.join(root, 'src/pixel.svg'),
      '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"><path d="M0 0h1v1H0z"/></svg>',
    );
    await writeFile(path.join(root, 'src/font.woff2'), 'font');
    await writeFile(
      path.join(root, 'src/provider.js'),
      'export default (components) => components; export const Icon = () => null;',
    );
    const outDir = path.join(root, 'dist/client');
    const result = await buildClient({
      root,
      entry: 'src/client.ts',
      outDir,
      providers: {
        icons: { adapter: './src/provider.js', components: ['Icon'] },
      },
    });
    assert.match(result.entry, /^\/_plec\/assets\/client-[\w-]+\.js$/);
    assert.equal(result.styles.length, 1);
    assert.match(
      result.styles[0],
      /^\/_plec\/assets\/client-[\w-]+\.css$/,
    );
    assert.match(
      result.providers.icons,
      /^\/_plec\/assets\/plec_provider_69636f6e73-[\w-]+\.js$/,
    );
    for (const url of [
      result.entry,
      ...result.styles,
      result.providers.icons,
    ]) {
      await readFile(path.join(outDir, url.replace('/_plec/', '')));
    }
    const css = await readFile(
      path.join(outDir, result.styles[0].replace('/_plec/', '')),
      'utf8',
    );
    assert.match(css, /\.woff2/);
    assert.match(css, /\.svg/);
    const provider = await readFile(
      path.join(outDir, result.providers.icons.replace('/_plec/', '')),
      'utf8',
    );
    assert.notEqual(provider.trim(), '');
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('production build rejects a forbidden browser dependency from Rollup metadata', async () => {
  const root = await mkdtemp(
    path.join(os.tmpdir(), 'plec-vite-boundary-'),
  );
  try {
    await mkdir(path.join(root, 'node_modules/zod'), {
      recursive: true,
    });
    await writeFile(
      path.join(root, 'node_modules/zod/package.json'),
      '{"name":"zod","type":"module","exports":"./index.js"}',
    );
    await writeFile(
      path.join(root, 'node_modules/zod/index.js'),
      'export const serverOnly = true;',
    );
    await writeFile(
      path.join(root, 'client.js'),
      "import { serverOnly } from 'zod'; console.log(serverOnly);",
    );
    await assert.rejects(
      buildClient({
        root,
        entry: 'client.js',
        outDir: path.join(root, 'dist'),
      }),
      /PLEC-DEPENDENCY-VALIDATION.*zod/s,
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
