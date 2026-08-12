import { execFileSync } from 'node:child_process';
import { mkdir } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const packageRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);
const repoRoot = path.resolve(packageRoot, '..', '..');
const temporaryDirectory = path.join(repoRoot, '.tmp', 'wasm-pack');
const features = process.env.PLEC_RUNTIME_FEATURES
  ?.split(',')
  .map((feature) => feature.trim())
  .filter(Boolean);
await mkdir(temporaryDirectory, { recursive: true });
execFileSync(
  'wasm-pack',
  [
    'build',
    'crates/runtime',
    '--target',
    'web',
    '--out-dir',
    'dist/runtime',
    '--out-name',
    'runtime',
    ...(features?.length ? ['--', '--features', features.join(',')] : []),
  ],
  {
    cwd: packageRoot,
    stdio: 'inherit',
    env: {
      ...process.env,
      TMP: temporaryDirectory,
      TEMP: temporaryDirectory,
    },
  },
);
