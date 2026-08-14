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
const profile = process.env.PLEC_RUNTIME_PROFILE ?? 'full';
const profileFeatures = {
  full: undefined,
  core: [],
  router: ['router'],
  fetch: ['fetch'],
}[profile];
if (profileFeatures === undefined && profile !== 'full')
  throw new Error(`Unknown PLEC_RUNTIME_PROFILE: ${profile}`);
const selectedFeatures = features ?? profileFeatures;
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
    ...(selectedFeatures
      ? ['--', ...(profile === 'core' || profile === 'router' || profile === 'fetch' ? ['--no-default-features'] : []), ...(selectedFeatures.length ? ['--features', selectedFeatures.join(',')] : [])]
      : []),
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
