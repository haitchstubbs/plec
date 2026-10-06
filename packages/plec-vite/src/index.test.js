import test from 'node:test';
import { execFileSync } from 'node:child_process';
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
  assert.match(pre, /href="\/src\/styles\.css"/);
  assert.match(
    pre,
    /data-plec-client-src="\/_plec\/assets\/client-a1b2\.js"/,
  );
  const html = plecHtmlFinalizer().transformIndexHtml.handler(
    '<html><head><script type="module" src="/@vite/client"></script></head><body><template data-plec-client-src="/_plec/assets/client-a1b2.js"></template></body></html>',
  );
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
  for (const dependency of ['zod', 'typescript', '@swc/core']) {
    const root = await mkdtemp(
      path.join(os.tmpdir(), 'plec-vite-boundary-'),
    );
    try {
      const dependencyDir = path.join(root, 'node_modules', dependency);
      await mkdir(dependencyDir, { recursive: true });
      await writeFile(
        path.join(dependencyDir, 'package.json'),
        JSON.stringify({
          name: dependency,
          type: 'module',
          exports: './index.js',
        }),
      );
      await writeFile(
        path.join(dependencyDir, 'index.js'),
        'export const serverOnly = true;',
      );
      await writeFile(
        path.join(root, 'client.js'),
        `import { serverOnly } from '${dependency}'; console.log(serverOnly);`,
      );
      await assert.rejects(
        buildClient({
          root,
          entry: 'client.js',
          outDir: path.join(root, 'dist'),
        }),
        (error) =>
          String(error).includes('[PLEC-DEPENDENCY-VALIDATION]') &&
          String(error).includes(dependency),
      );
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  }
});

test('published packages contain the Vite production adapter and dependency chain', async () => {
  const outputDir = await mkdtemp(
    path.join(os.tmpdir(), 'plec-package-artifacts-'),
  );
  try {
    const packages = [
      {
        name: '@plec/core',
        directory: path.resolve(import.meta.dirname, '../../plec'),
        requiredFile: 'scripts/vite-build.mjs',
        dependency: '@plec/vite',
      },
      {
        name: '@plec/vite',
        directory: path.resolve(import.meta.dirname, '..'),
        requiredFile: 'src/build.js',
        dependency: 'vite',
      },
    ];
    for (const expected of packages) {
      const output = execFileSync(
        'npm',
        [
          'pack',
          '--json',
          '--ignore-scripts',
          '--pack-destination',
          outputDir,
        ],
        { cwd: expected.directory, encoding: 'utf8' },
      );
      const [packed] = JSON.parse(output);
      assert(
        packed.files.some(
          (file) => file.path === expected.requiredFile,
        ),
        `${expected.name} archive must include ${expected.requiredFile}`,
      );
      const packedPackageJson = JSON.parse(
        execFileSync(
          'tar',
          [
            '-xOf',
            path.join(outputDir, packed.filename),
            'package/package.json',
          ],
          { encoding: 'utf8' },
        ),
      );
      assert(
        packedPackageJson.dependencies?.[expected.dependency],
        `${expected.name} must declare ${expected.dependency}`,
      );
    }
  } finally {
    await rm(outputDir, { recursive: true, force: true });
  }
});
