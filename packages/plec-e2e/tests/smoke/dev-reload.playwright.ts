import { expect, test } from '@playwright/test';
import { spawn, type ChildProcess } from 'node:child_process';
import {
  cpSync,
  existsSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import { createServer } from 'node:net';
import { execFileSync } from 'node:child_process';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repo = fileURLToPath(new URL('../../../../', import.meta.url));
const fixtureSource = path.join(
  repo,
  'crates/plec-cli/tests/fixtures/mini-repo/apps/mini-app/src',
);

async function freePort() {
  const listener = createServer();
  await new Promise<void>((resolve) =>
    listener.listen(0, '127.0.0.1', resolve),
  );
  const address = listener.address();
  if (!address || typeof address === 'string')
    throw new Error('no test port');
  const port = address.port;
  await new Promise<void>((resolve, reject) =>
    listener.close((error) => (error ? reject(error) : resolve())),
  );
  return port;
}

async function waitUntil<T>(
  read: () => Promise<T>,
  ready: (value: T) => boolean,
  timeoutMs = 30_000,
) {
  const deadline = Date.now() + timeoutMs;
  let last: T;
  do {
    last = await read();
    if (ready(last)) return last;
    await new Promise((resolve) => setTimeout(resolve, 50));
  } while (Date.now() < deadline);
  throw new Error(
    `timed out waiting for dev process state: ${String(last)}`,
  );
}

function stop(child: ChildProcess) {
  return new Promise<void>((resolve) => {
    if (child.exitCode !== null || child.killed) return resolve();
    const timer = setTimeout(() => {
      child.kill('SIGKILL');
    }, 8_000);
    child.once('exit', () => {
      clearTimeout(timer);
      resolve();
    });
    child.kill('SIGTERM');
  });
}

test('plec dev keeps failed builds live and reloads the browser once after recovery', async ({
  page,
}) => {
  test.setTimeout(120_000);
  const plecBinary = process.env.PLEC_BIN
    ? path.resolve(process.env.PLEC_BIN)
    : path.join(repo, 'target/debug/plec');
  if (!existsSync(plecBinary)) {
    execFileSync('cargo', ['build', '-p', 'plec-cli'], {
      cwd: repo,
      stdio: 'ignore',
    });
  }
  const sessionRoot = mkdtempSync(
    path.join(os.tmpdir(), 'plec-dev-browser-'),
  );
  const app = path.join(sessionRoot, 'app');
  const port = await freePort();
  let child: ChildProcess | undefined;
  let output = '';
  try {
    mkdirSync(app, { recursive: true });
    cpSync(fixtureSource, path.join(app, 'src'), { recursive: true });
    symlinkSync(
      path.join(repo, 'node_modules'),
      path.join(app, 'node_modules'),
      'dir',
    );
    writeFileSync(
      path.join(app, 'src/home.tsx'),
      'export function Home() { return <div>Version A</div>; }\n',
    );
    writeFileSync(path.join(app, 'src/client.tsx'), 'export {};\n');
    child = spawn(
      plecBinary,
      ['dev', 'src/router.tsx', '--port', String(port)],
      {
        cwd: app,
        env: { ...process.env, NODE_ENV: 'development' },
        stdio: ['ignore', 'pipe', 'pipe'],
      },
    );
    child.stdout?.on(
      'data',
      (chunk: Buffer) => (output += chunk.toString()),
    );
    child.stderr?.on(
      'data',
      (chunk: Buffer) => (output += chunk.toString()),
    );
    const origin = `http://127.0.0.1:${port}`;
    await waitUntil(
      async () => {
        try {
          return await (await fetch(origin)).text();
        } catch {
          return '';
        }
      },
      (html) => html.includes('Version A'),
    );

    await page.addInitScript(() => {
      const count = Number(
        sessionStorage.getItem('plec-dev-page-loads') ?? '0',
      );
      sessionStorage.setItem('plec-dev-page-loads', String(count + 1));
    });
    await page.goto(origin);
    await expect(page.getByText('Version A')).toBeVisible();
    expect(
      await page.evaluate(() =>
        sessionStorage.getItem('plec-dev-page-loads'),
      ),
    ).toBe('1');

    writeFileSync(
      path.join(app, 'src/home.tsx'),
      'export function Home( {\n',
    );
    await waitUntil(
      async () => output,
      (text) => text.includes('build failed'),
    );
    await expect(page.getByText('Version A')).toBeVisible();
    expect(
      await page.evaluate(() =>
        sessionStorage.getItem('plec-dev-page-loads'),
      ),
    ).toBe('1');

    writeFileSync(
      path.join(app, 'src/home.tsx'),
      'export function Home() { return <div>Version B</div>; }\n',
    );
    await expect(page.getByText('Version B')).toBeVisible({
      timeout: 60_000,
    });
    await expect
      .poll(() =>
        page.evaluate(() =>
          sessionStorage.getItem('plec-dev-page-loads'),
        ),
      )
      .toBe('2');
    await page.waitForTimeout(1_000);
    expect(
      await page.evaluate(() =>
        sessionStorage.getItem('plec-dev-page-loads'),
      ),
    ).toBe('2');

    const marker = path.join(app, `.plec-dev-state-${child.pid}.json`);
    await stop(child);
    child = undefined;
    expect(() => readFileSync(marker)).toThrow();
    const probe = createServer();
    await new Promise<void>((resolve, reject) =>
      probe.listen(port, '127.0.0.1', (error?: Error) =>
        error ? reject(error) : resolve(),
      ),
    );
    await new Promise<void>((resolve) => probe.close(() => resolve()));
    expect(
      (await import('node:fs'))
        .readdirSync(app)
        .some((name) => name.startsWith('.plec-dev-')),
    ).toBe(false);
  } finally {
    if (child) await stop(child);
    rmSync(sessionRoot, { recursive: true, force: true });
  }
});
