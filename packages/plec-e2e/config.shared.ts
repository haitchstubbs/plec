import { readFileSync } from 'node:fs';
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
const startCommand = 'yarn workspace fullstack start';

// Playwright owns the fullstack server: turbo builds it, webServer starts
// the native host, waits for HTTP readiness, and kills the process group
// afterwards. PLEC_ACCEPTANCE_CONTROL arms the one-shot loader-failure
// fixture used by the todos loader acceptance spec; it flows into the
// application bundle through the environment in either host.
export const webServer = {
  command: startCommand,
  url: baseURL,
  env: {
    PORT: String(e2ePort),
    PLEC_ACCEPTANCE_CONTROL: '1',
  },
  // Intentional: a stale server on the port must fail loudly instead of
  // being silently adopted (that is how orphaned servers went unnoticed).
  reuseExistingServer: false,
  timeout: 30_000,
} satisfies NonNullable<Config['webServer']>;
