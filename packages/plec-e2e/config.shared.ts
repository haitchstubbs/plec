import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import type { Config } from '@playwright/test';

const devPortsPath = fileURLToPath(
  new URL('../../.env.devports', import.meta.url),
);

// .env.devports is the canonical port registry for the repo. An environment
// variable with the same name overrides the file so parallel spikes can run
// without editing it.
export function devPort(name: string): number {
  const override = process.env[name];
  if (override) return Number(override);
  const line = readFileSync(devPortsPath, 'utf8')
    .split('\n')
    .find((candidate) => candidate.startsWith(`${name}=`));
  if (!line) {
    throw new Error(`${name} is not defined in .env.devports`);
  }
  return Number(line.slice(name.length + 1));
}

export const e2ePort = devPort('E2E_PORT');
export const baseURL = `http://127.0.0.1:${e2ePort}`;

// The native Axum host owns the public listener and spawns the Node sidecar
// for application-owned `/api/*` handlers.
const movedDist = process.env.PLEC_E2E_DIST;
const selectedHost = process.env.PLEC_E2E_HOST ?? 'axum';
if (selectedHost !== 'axum' && selectedHost !== 'node') {
  throw new Error('PLEC_E2E_HOST must be either "axum" or "node"');
}
const quoteShell = (value: string) =>
  `'${value.replaceAll("'", "'\\''")}'`;
const scriptsDir = path.dirname(fileURLToPath(import.meta.url));
const workspaceRoot = path.resolve(scriptsDir, '../..');
const dist =
  movedDist ?? path.join(workspaceRoot, 'apps/fullstack/dist');
const plecBin = path.join(workspaceRoot, 'packages/plec/bin/plec.js');
const startCommand =
  selectedHost === 'node'
    ? `yarn workspace @plec/node build && node ${quoteShell(plecBin)} serve ${quoteShell(dist)}`
    : process.env.PLEC_BIN
      ? `${quoteShell(path.resolve(process.env.PLEC_BIN))} serve ${quoteShell(dist)}`
      : `cargo run --locked --manifest-path crates/plec-cli/Cargo.toml -- serve ${quoteShell(dist)}`;

// Playwright owns the fullstack server: turbo builds it, webServer starts
// the native host, waits for HTTP readiness, and kills the process group
// afterwards. PLEC_ACCEPTANCE_CONTROL arms the one-shot loader-failure
// fixture used by the todos loader acceptance spec; it flows into the
// application bundle through the environment in either host.
export const webServer = {
  command: startCommand,
  url: baseURL,
  cwd: workspaceRoot,
  env: {
    PORT: String(e2ePort),
    PLEC_ACCEPTANCE_CONTROL: '1',
    ...(selectedHost === 'node' ? { PLEC_BIN: '' } : {}),
    ...(movedDist ? { PLEC_E2E_DIST: movedDist } : {}),
  },
  // Intentional: a stale server on the port must fail loudly instead of
  // being silently adopted (that is how orphaned servers went unnoticed).
  reuseExistingServer: false,
  timeout: selectedHost === 'node' ? 120_000 : 30_000,
} satisfies NonNullable<Config['webServer']>;
