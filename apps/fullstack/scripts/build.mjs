import {
  copyFile,
  mkdir,
  rm,
  writeFile,
  readFile,
  readdir,
  stat,
} from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';
import { createHash } from 'node:crypto';
import { constants, brotliCompress } from 'node:zlib';
import { promisify } from 'node:util';
import { build } from 'esbuild';

const OPTIMIZE = true;

const appDir = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);

const require = createRequire(import.meta.url);
const repoRoot = path.resolve(appDir, '..', '..');

const distDir = path.join(appDir, 'dist');
const publicDir = path.join(distDir, 'public');
const assetsDir = path.join(publicDir, 'assets');
const graphsDir = path.join(publicDir, 'graphs');
const runtimeDir = path.join(publicDir, 'runtime');

const compressBrotli = promisify(brotliCompress);

const FORBIDDEN_BROWSER_DEPENDENCIES = [
  'zod',
  'typescript',
  '@swc/core',
  'plec-compiler',
];

const BUILD_DEFAULTS = {
  bundle: true,
  format: 'esm',
  minify: OPTIMIZE,
  treeShaking: true,
  legalComments: 'none',
};

//
// Revision
//

// Every browser asset and compiled artifact shares this revision. Keeping it
// content-derived prevents the long-lived dev server from serving an older
// client bundle after a rebuild.
const assetRevision = createHash('sha256')
  .update(await readFile(path.join(appDir, 'src/client.tsx')))
  .update(await readFile(path.join(appDir, 'src/router.tsx')))
  .digest('hex')
  .slice(0, 12);

//
// Clean
//

await rm(distDir, { recursive: true, force: true });

await Promise.all([
  mkdir(assetsDir, { recursive: true }),
  mkdir(graphsDir, { recursive: true }),
  mkdir(runtimeDir, { recursive: true }),
]);

//
// Compile routes
//

const rustArtifacts = JSON.parse(
  execFileSync(
    'cargo',
    [
      'run', '-q', '-p', 'plec-compiler', '--bin', 'plec-route-manifest', '--',
      path.join(appDir, 'src/router.tsx'), appDir, repoRoot, '--artifacts',
    ],
    { cwd: repoRoot, encoding: 'utf8' },
  ),
);

await Promise.all(
  rustArtifacts.graphs.map(({ graphId, graph }) =>
    writeFile(
      path.join(graphsDir, `${graphId}.json`),
      `${JSON.stringify(graph, null, 2)}\n`,
    ),
  ),
);

await writeFile(
  path.join(publicDir, 'route-manifest.json'),
  `${JSON.stringify(rustArtifacts.manifest, null, 2)}\n`,
);

//
// Copy WASM runtime from package
//

// Copy pre-built WASM artifacts from the plec-runtime package
const wasmSourceDir = path.join(
  repoRoot,
  'packages',
  'plec-runtime',
  'dist',
  'runtime',
);

// Ensure the package has built WASM
try {
  await stat(wasmSourceDir);
} catch {
  console.error(
    `WASM artifacts not found at ${wasmSourceDir}. ` +
      `Run 'yarn workspace plec-runtime build:wasm' first.`
  );
  process.exit(1);
}

// Copy WASM files
const wasmFiles = ['runtime.js', 'runtime_bg.wasm'];
for (const file of wasmFiles) {
  await copyFile(path.join(wasmSourceDir, file), path.join(runtimeDir, file));
}

// Copy .br files if they exist (from previous optimized builds)
try {
  await copyFile(
    path.join(wasmSourceDir, 'runtime_bg.wasm.br'),
    path.join(runtimeDir, 'runtime_bg.wasm.br'),
  );
  await copyFile(
    path.join(wasmSourceDir, 'runtime.js.br'),
    path.join(runtimeDir, 'runtime.js.br'),
  );
} catch {
  // .br files don't exist yet, will be created below
}

console.log('WASM output:', await readdir(runtimeDir));
const wasmPath = path.join(runtimeDir, 'runtime_bg.wasm');
console.log('WASM size:', (await stat(wasmPath)).size);

// Create brotli compressed versions for deployment
await Promise.all(
  ['runtime.js', 'runtime_bg.wasm'].map((file) =>
    writeBrotliAsset(path.join(runtimeDir, file)),
  ),
);
//
// Build browser client
//

const browserBuild = await build({
  ...BUILD_DEFAULTS,
  entryPoints: [path.join(appDir, 'src/client.tsx')],
  platform: 'browser',
  target: 'es2022',
  jsx: 'automatic',
  jsxImportSource: 'plec',
  outfile: path.join(assetsDir, 'client.js'),
  metafile: true,
});

await writeFile(
  path.join(distDir, 'client.meta.json'),
  `${JSON.stringify(browserBuild.metafile, null, 2)}\n`,
);

assertBrowserBundle(
  browserBuild.metafile,
  FORBIDDEN_BROWSER_DEPENDENCIES,
);

await writeBrotliAsset(path.join(assetsDir, 'client.js'));

//
// Build server
//

await build({
  ...BUILD_DEFAULTS,
  entryPoints: [path.join(appDir, 'src/server.ts')],
  platform: 'node',
  target: 'node20',
  outfile: path.join(distDir, 'server.mjs'),
  packages: 'external',
});

//
// Tailwind
//

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
  {
    cwd: appDir,
    stdio: 'inherit',
  },
);

//
// Fonts
//

await Promise.all([
  copyFontFiles('@fontsource-variable/outfit'),
  copyFontFiles('@fontsource-variable/raleway'),
]);

//
// HTML
//

await writeFile(
  path.join(publicDir, 'index.html'),
  `<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Plec fullstack playground</title>
  <link
    rel="stylesheet"
    href="/assets/styles.css?v=${assetRevision}"
  >
</head>
<body>
  <div id="app" aria-live="polite"></div>
  <script
    type="module"
    src="/assets/client.js?v=${assetRevision}"
  ></script>
</body>
</html>
`,
);

//
// Helpers
//

function assertBrowserBundle(metafile, forbiddenPackages) {
  const violations = [];

  for (const input of Object.keys(metafile.inputs)) {
    const normalized = input.replaceAll('\\', '/');

    for (const packageName of forbiddenPackages) {
      if (matchesPackage(normalized, packageName)) {
        violations.push({
          packageName,
          input,
        });
      }
    }
  }

  if (violations.length === 0) return;

  const grouped = Map.groupBy(
    violations,
    ({ packageName }) => packageName,
  );

  throw new Error(
    [
      '',
      'Forbidden dependencies leaked into the browser bundle.',
      '',
      ...Array.from(grouped, ([packageName, entries]) => [
        `${packageName}:`,
        ...entries.map(({ input }) => `  - ${input}`),
      ]).flat(),
      '',
      `Inspect ${path.relative(
        repoRoot,
        path.join(distDir, 'client.meta.json'),
      )} to trace the import path.`,
      '',
    ].join('\n'),
  );
}

function matchesPackage(input, packageName) {
  const escaped = packageName.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

  return new RegExp(
    [
      `(?:^|/)node_modules/${escaped}(?:/|$)`,
      `(?:^|/)packages/${escaped}(?:/|$)`,
    ].join('|'),
  ).test(input);
}

/**
 * Tailwind preserves Fontsource's relative `./files/*.woff2` URLs.
 * Publish those files next to styles.css so the CSS remains portable.
 */
async function copyFontFiles(packageName) {
  const sourceDir = path.join(
    path.dirname(require.resolve(`${packageName}/package.json`)),
    'files',
  );

  const destinationDir = path.join(assetsDir, 'files');

  await mkdir(destinationDir, { recursive: true });

  for (const file of await readdir(sourceDir)) {
    if (!file.endsWith('.woff2')) continue;

    await copyFile(
      path.join(sourceDir, file),
      path.join(destinationDir, file),
    );
  }
}

async function writeBrotliAsset(filePath) {
  const source = await readFile(filePath);

  const compressed = await compressBrotli(source, {
    params: {
      [constants.BROTLI_PARAM_QUALITY]: 11,
    },
  });

  await writeFile(`${filePath}.br`, compressed);
}
