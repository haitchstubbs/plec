import { cp, mkdtemp, rm } from 'node:fs/promises';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';

const e2eDir = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);
const workspaceRoot = path.resolve(e2eDir, '../..');
const sourceDist = path.join(workspaceRoot, 'apps/fullstack/dist');
const temporaryRoot = await mkdtemp(
  path.join(tmpdir(), 'plec-moved-dist-'),
);
const movedDist = path.join(temporaryRoot, 'application');

try {
  execFileSync('yarn', ['workspace', 'fullstack', 'build'], {
    cwd: workspaceRoot,
    stdio: 'inherit',
  });
  await cp(sourceDist, movedDist, { recursive: true });

  const env = { ...process.env, PLEC_E2E_DIST: movedDist };
  const runner = path.join(workspaceRoot, 'scripts/run-playwright.mjs');
  execFileSync(
    process.execPath,
    [
      runner,
      '--grep',
      'home renders through SSR and mounts|host icons adopt through stable boundaries|todos renders through SSR and mounts|scoped API middleware|middleware rejects duplicate next calls',
    ],
    { cwd: e2eDir, env, stdio: 'inherit' },
  );
  execFileSync(
    process.execPath,
    [
      runner,
      '--config',
      'playwright.acceptance.config.ts',
      '--grep',
      'server action returns through the Rust and Node execution boundary',
    ],
    { cwd: e2eDir, env, stdio: 'inherit' },
  );
} finally {
  await rm(temporaryRoot, { recursive: true, force: true });
}
