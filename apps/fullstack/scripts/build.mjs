import {
  copyFile,
  mkdir,
  rm,
  writeFile,
  readFile,
  readdir,
} from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';
import { createHash } from 'node:crypto';
import { constants, brotliCompress } from 'node:zlib';
import { promisify } from 'node:util';
import { build } from 'esbuild';
import { compileRouteEntry } from 'plec-compiler/node-entry';

const appDir = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);
const require = createRequire(import.meta.url);
const distDir = path.join(appDir, 'dist');
const publicDir = path.join(distDir, 'public');
const compressBrotli = promisify(brotliCompress);

// Every browser asset and compiled artifact shares this revision. Keeping it
// content-derived prevents the long-lived dev server from serving an older
// client bundle after a rebuild.
const assetRevision = createHash('sha256')
  .update(await readFile(path.join(appDir, 'src/client.tsx')))
  .update(await readFile(path.join(appDir, 'src/router.tsx')))
  .digest('hex')
  .slice(0, 12);

const BUILD_DEFAULTS = {
  bundle: true,
  format: 'esm',
  minify: true,
  treeShaking: true,
  legalComments: 'none',
};

await rm(distDir, { recursive: true, force: true });
await mkdir(path.join(publicDir, 'assets'), { recursive: true });
const repoRoot = path.resolve(appDir, '..', '..');
const compiledRoutes = await compileRouteEntry(path.join(appDir, 'src/router.tsx'), {
  rootDir: appDir,
  repoRootDir: repoRoot,
  mode: 'strict',
});
const graphsDir = path.join(publicDir, 'graphs');
await mkdir(graphsDir, { recursive: true });
const graphs = [
  compiledRoutes.rootGraph,
  ...compiledRoutes.routes.flatMap((route) => [
    route.graph,
    route.pendingGraph,
    route.errorGraph,
  ].filter(Boolean)),
];
for (const graph of graphs)
  await writeFile(
    path.join(graphsDir, `${graph.graphId}.json`),
    `${JSON.stringify(graph, null, 2)}\n`,
  );
const routeManifest = {
  version: 3,
  revision: compiledRoutes.revision,
  rootGraphId: compiledRoutes.rootGraph.graphId,
  routes: compiledRoutes.routes.map((route) => ({
    id: route.id,
    parentId: route.parentId,
    path: route.path,
    graphId: route.graph.graphId,
    pendingGraphId: route.pendingGraph?.graphId,
    errorGraphId: route.errorGraph?.graphId,
    loaderAction: route.loaderAction,
    outletId: route.outletId,
  })),
};
await writeFile(
  path.join(publicDir, 'route-manifest.json'),
  `${JSON.stringify(routeManifest, null, 2)}\n`,
);
// wasm-pack falls back to `cargo install wasm-bindgen` on hosts without a
// matching prebuilt binary. Keep that temporary install work inside the
// workspace so locked-down user temp directories do not make an otherwise
// successful Rust/WASM build fail.
const wasmTempDir = path.join(repoRoot, '.tmp', 'wasm-pack');
await mkdir(wasmTempDir, { recursive: true });
execFileSync(
  'wasm-pack',
  [
    'build',
    path.join(repoRoot, 'packages/plec-runtime/crates/runtime'),
    '--target',
    'web',
    '--out-dir',
    path.join(publicDir, 'runtime'),
    '--out-name',
    'runtime',
  ],
  {
    cwd: repoRoot,
    stdio: 'inherit',
    env: { ...process.env, TMP: wasmTempDir, TEMP: wasmTempDir },
  },
);
await Promise.all(
  ['runtime.js', 'runtime_bg.wasm'].map((file) =>
    writeBrotliAsset(path.join(publicDir, 'runtime', file)),
  ),
);

await build({
  ...BUILD_DEFAULTS,
  entryPoints: [path.join(appDir, 'src/client.tsx')],
  platform: 'browser',
  target: 'es2022',
  jsx: 'automatic',
  jsxImportSource: 'plec',
  outfile: path.join(publicDir, 'assets/client.js'),
});
await build({
  ...BUILD_DEFAULTS,
  entryPoints: [path.join(appDir, 'src/server.ts')],
  platform: 'node',
  target: 'node20',
  outfile: path.join(distDir, 'server.mjs'),
  packages: 'external',
});

const tailwindCli = path.join(
  path.dirname(require.resolve('@tailwindcss/cli/package.json')),
  'dist',
  'index.mjs',
);
execFileSync(
  process.execPath,
  [
    tailwindCli,
    '-i',
    'src/styles.css',
    '-o',
    'dist/public/assets/styles.css',
  ],
  { cwd: appDir, stdio: 'inherit' },
);
await copyFontFiles('@fontsource-variable/outfit');
await copyFontFiles('@fontsource-variable/raleway');
await writeFile(
  path.join(publicDir, 'index.html'),
  `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Plec fullstack playground</title><link rel="stylesheet" href="/assets/styles.css?v=${assetRevision}"></head><body><div id="app" aria-live="polite"></div><script type="module" src="/assets/client.js?v=${assetRevision}"></script></body></html>\n`,
);

/** Tailwind preserves Fontsource's relative `./files/*.woff2` URLs. Publish
 * those files next to styles.css so the CSS remains portable after the build. */
async function copyFontFiles(packageName) {
  const sourceDir = path.join(
    path.dirname(require.resolve(`${packageName}/package.json`)),
    'files',
  );
  const destinationDir = path.join(publicDir, 'assets', 'files');
  await mkdir(destinationDir, { recursive: true });
  for (const file of await readdir(sourceDir)) {
    if (file.endsWith('.woff2'))
      await copyFile(
        path.join(sourceDir, file),
        path.join(destinationDir, file),
      );
  }
}

async function writeBrotliAsset(filePath) {
  const source = await readFile(filePath);
  const compressed = await compressBrotli(source, {
    params: { [constants.BROTLI_PARAM_QUALITY]: 11 },
  });
  await writeFile(`${filePath}.br`, compressed);
}
