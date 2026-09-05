import { spawnSync } from 'node:child_process';
import { playwrightBrowsersPath, repoRoot } from './toolchain.mjs';

const verify = spawnSync(
  process.execPath,
  ['scripts/verify-toolchain.mjs', '--browser'],
  {
    cwd: repoRoot,
    stdio: 'inherit',
  },
);

if (verify.status !== 0) process.exit(verify.status ?? 1);

const yarn = process.platform === 'win32' ? 'yarn.cmd' : 'yarn';
const result = spawnSync(
  yarn,
  [
    'workspace',
    'plec-e2e',
    'exec',
    'playwright',
    'test',
    ...process.argv.slice(2),
  ],
  {
    cwd: repoRoot,
    stdio: 'inherit',
    env: {
      ...process.env,
      PLAYWRIGHT_BROWSERS_PATH: playwrightBrowsersPath,
    },
  },
);

if (result.error) {
  console.error(`Failed to run Playwright: ${result.error.message}`);
  process.exitCode = 1;
} else {
  process.exitCode = result.status ?? 1;
}
