#!/usr/bin/env node
/**
 * Assembles the `plec` release artifact: one self-contained package folder
 * consumable without the monorepo. The artifact is the package folder itself;
 * this script fills `dist/` with everything it must carry:
 *
 * - compiled JS (tsc) plus bundled `./server` / `./browser` entries
 *   (the browser package's built output is inlined so no bare workspace
 *   specifier survives)
 * - validated WASM runtime assets published into dist/runtime
 * - the cargo-built release-variant CLI binary (PLEC_CLI_VERSION=release,
 *   the public-build variant per `.env.plec` semantics)
 *
 * A final audit fails the build if the folder still references the
 * monorepo (workspace protocol, bare workspace-package specifiers) or if a
 * browser entry can pull Node builtins into a client bundle.
 */
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import esbuild from 'esbuild';

const packageDir = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);
const repoRoot = path.resolve(packageDir, '..', '..');
const distDir = path.join(packageDir, 'dist');
const isWindows = process.platform === 'win32';

function run(command, args, options = {}) {
  execFileSync(command, args, {
    cwd: repoRoot,
    stdio: 'inherit',
    ...options,
  });
}

function ensureBuilt(label, markerPath, workspace, script) {
  if (fs.existsSync(path.join(repoRoot, markerPath))) return;
  console.log(`Building ${label} (missing ${markerPath})...`);
  run('yarn', ['workspace', workspace, 'run', script]);
}

ensureBuilt(
  'plec-browser',
  'packages/plec-browser/dist/index.js',
  'plec-browser',
  'build',
);
console.log('Cleaning previous plec artifact...');
fs.rmSync(distDir, { recursive: true, force: true });
ensureBuilt(
  'plec-runtime WASM',
  'packages/plec/dist/runtime/runtime_bg.wasm',
  'plec',
  'build:wasm',
);
ensureBuilt(
  'plec-node-runtime',
  'packages/plec-node-runtime/dist/runtime.mjs',
  'plec-node-runtime',
  'build',
);

console.log('Compiling plec (tsc)...');
run('yarn', ['exec', 'tsc', '-p', 'packages/plec/tsconfig.json']);

const bundleEntries = [
  {
    entry: path.join(packageDir, 'src/server.ts'),
    outfile: path.join(distDir, 'server.js'),
    // Node builtins stay external; the server entry legitimately runs on Node.
    platform: 'node',
  },
  {
    entry: path.join(packageDir, 'src/browser.ts'),
    outfile: path.join(distDir, 'browser.js'),
    // The browser entry must never pull Node builtins into a client bundle;
    // platform=browser fails loudly if any source reaches for one.
    platform: 'browser',
  },
];
for (const { entry, outfile, platform } of bundleEntries) {
  console.log(`Bundling ${path.basename(outfile)} (${platform})...`);
  await esbuild.build({
    entryPoints: [entry],
    outfile,
    bundle: true,
    platform,
    format: 'esm',
    sourcemap: false,
    logLevel: 'warning',
  });
}

// The browser re-export declaration must be self-contained: its workspace
// dependency is a single-file declaration unit, so copy it verbatim.
console.log('Staging self-contained type declarations...');
fs.copyFileSync(
  path.join(repoRoot, 'packages/plec-browser/dist/index.d.ts'),
  path.join(distDir, 'browser.d.ts'),
);
fs.copyFileSync(
  path.join(repoRoot, 'packages/plec-node-runtime/dist/runtime.mjs'),
  path.join(distDir, 'node-runtime.mjs'),
);

console.log(
  'Building release CLI binary (PLEC_CLI_VERSION=release)...',
);
run('cargo', ['build', '--release', '-p', 'plec-cli'], {
  env: { ...process.env, PLEC_CLI_VERSION: 'release' },
});
const binaryName = isWindows ? 'plec.exe' : 'plec';
const binDir = path.join(distDir, 'bin');
fs.mkdirSync(binDir, { recursive: true });
fs.copyFileSync(
  path.join(repoRoot, 'target/release', binaryName),
  path.join(binDir, binaryName),
);
if (!isWindows) fs.chmodSync(path.join(binDir, binaryName), 0o755);

// ---------------------------------------------------------------------------
// Self-containment audit: the artifact must be inspectable as a folder alone.
// ---------------------------------------------------------------------------
const workspaceSpecifier = /(?:from|import)\s*['"]plec-browser['"]/;
const failures = [];
for (const entry of walk(distDir)) {
  // Test files ship in dist by existing convention; they are not runtime
  // surface and quote the audited patterns themselves.
  if (/\.test\.(js|mjs|cjs)$/.test(entry)) continue;
  if (!/\.(js|mjs|cjs)$/.test(entry)) continue;
  const source = fs.readFileSync(entry, 'utf8');
  if (workspaceSpecifier.test(source))
    failures.push(
      `${relative(entry)} references a workspace package specifier`,
    );
  if (/workspace:/.test(source))
    failures.push(
      `${relative(entry)} contains a workspace: protocol reference`,
    );
  if (
    entry.endsWith('browser.js') &&
    /(?:from|import)\s*['"]node:/.test(source)
  )
    failures.push(`${relative(entry)} imports Node builtins`);
}

for (const required of [
  'runtime/runtime_bg.wasm',
  'runtime/runtime_bg.wasm.br',
  'runtime/runtime.js',
  'runtime/runtime.js.br',
  'runtime/provenance.json',
  'node-runtime.mjs',
  `bin/${binaryName}`,
]) {
  if (!fs.existsSync(path.join(distDir, required)))
    failures.push(`missing artifact asset: dist/${required}`);
}

if (failures.length > 0) {
  console.error('Artifact self-containment audit failed:');
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}

console.log(`Artifact assembled: ${packageDir} (dist/, bin/)`);

function relative(filePath) {
  return path.relative(packageDir, filePath);
}

function* walk(directory) {
  for (const item of fs.readdirSync(directory, {
    withFileTypes: true,
  })) {
    const entryPath = path.join(directory, item.name);
    if (item.isDirectory()) yield* walk(entryPath);
    else yield entryPath;
  }
}
