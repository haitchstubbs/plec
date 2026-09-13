import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import esbuild from 'esbuild';

export const packageDir = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);
export const repoRoot = path.resolve(packageDir, '..', '..');
export const distDir = path.join(packageDir, 'dist');

export function run(command, args, options = {}) {
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

export async function buildWorkspaceSurface() {
  ensureBuilt(
    'plec-browser',
    'packages/plec-browser/dist/index.js',
    'plec-browser',
    'build',
  );
  console.log('Cleaning previous plec package build...');
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
      platform: 'node',
    },
    {
      entry: path.join(packageDir, 'src/browser.ts'),
      outfile: path.join(distDir, 'browser.js'),
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

  console.log('Staging self-contained type declarations...');
  fs.copyFileSync(
    path.join(repoRoot, 'packages/plec-browser/dist/index.d.ts'),
    path.join(distDir, 'browser.d.ts'),
  );
  fs.copyFileSync(
    path.join(repoRoot, 'packages/plec-node-runtime/dist/runtime.mjs'),
    path.join(distDir, 'node-runtime.mjs'),
  );
}
