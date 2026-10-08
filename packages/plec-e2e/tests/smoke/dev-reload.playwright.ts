import { expect, test } from '@playwright/test';
import { spawn, type ChildProcess } from 'node:child_process';
import {
  cpSync,
  existsSync,
  mkdtempSync,
  mkdirSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import { createServer } from 'node:net';
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

function countOccurrences(value: string, needle: string): number {
  return value.split(needle).length - 1;
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
  const plecShim = path.join(repo, 'packages/plec/bin/plec.js');
  const sessionRoot = mkdtempSync(
    path.join(os.tmpdir(), 'plec-dev-browser-'),
  );
  const app = path.join(sessionRoot, 'app');
  const port = await freePort();
  let child: ChildProcess | undefined;
  let output = '';
  try {
    page.on('console', (message) => {
      output += `[browser console] ${message.text()}\n`;
    });
    page.on('pageerror', (error) => {
      output += `[browser error] ${error.message}\n`;
    });
    page.on('response', (response) => {
      if (response.status() >= 400)
        output += `[browser response ${response.status()}] ${response.url()}\n`;
      if (response.request().resourceType() === 'script') {
        void response
          .headerValue('content-type')
          .then((contentType) => {
            if (contentType?.includes('text/html'))
              output += `[script response ${response.status()} ${contentType}] ${response.url()}\n`;
          });
      }
    });
    mkdirSync(app, { recursive: true });
    cpSync(fixtureSource, path.join(app, 'src'), { recursive: true });
    writeFileSync(
      path.join(app, 'src/app.tsx'),
      "export { router } from './router';\n",
    );
    symlinkSync(
      path.join(repo, 'node_modules'),
      path.join(app, 'node_modules'),
      'dir',
    );
    writeFileSync(
      path.join(app, 'src/home.tsx'),
      "import logo from '../shared.svg'; export function Home() { return <div>Version A<img src={logo} /></div>; }\n",
    );
    writeFileSync(path.join(app, 'src/client.tsx'), 'export {};\n');
    writeFileSync(
      path.join(app, 'shared.svg'),
      '<svg><text>external A</text></svg>',
    );
    writeFileSync(
      path.join(app, 'src/styles.css'),
      'body { color: black; }\n',
    );
    mkdirSync(path.join(app, 'api'), { recursive: true });
    writeFileSync(
      path.join(app, 'api/version.ts'),
      "export function GET() { return new Response('API A'); }\n",
    );
    child = spawn(
      process.execPath,
      [plecShim, 'dev', '--port', String(port)],
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
    ).catch((error: Error) => {
      throw new Error(`${error.message}\nPlec dev output:\n${output}`);
    });

    await page.addInitScript(() => {
      const count = Number(
        sessionStorage.getItem('plec-dev-page-loads') ?? '0',
      );
      sessionStorage.setItem('plec-dev-page-loads', String(count + 1));
    });
    await page.goto(origin);
    await expect(page.getByText('Version A')).toBeVisible();
    expect(output).not.toContain('packages/plec-node-runtime');
    expect(output).toContain('Plec dev: Node host attached to Vite');
    expect(output).not.toContain('[PLEC] listening on');
    expect(
      await page.locator('script[src="/@vite/client"]').count(),
    ).toBe(1);
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
      (text) =>
        text.includes('[PLEC-PARSE-001]') &&
        text.includes('serving previous successful build'),
    );
    await expect(page.getByText('Version A')).toBeVisible();
    expect(
      await page.evaluate(() =>
        sessionStorage.getItem('plec-dev-page-loads'),
      ),
    ).toBe('1');

    writeFileSync(
      path.join(app, 'src/home.tsx'),
      "import logo from '../shared.svg'; export function Home() { return <div>Version B<img src={logo} /></div>; }\n",
    );
    const reloadsBeforeB = countOccurrences(
      output,
      'Node host reloaded',
    );
    await waitUntil(
      async () => output,
      (text) =>
        countOccurrences(text, 'Node host reloaded') > reloadsBeforeB,
    );
    await expect(page.getByText('Version B'))
      .toBeVisible({
        timeout: 60_000,
      })
      .catch((error: Error) => {
        throw new Error(
          `${error.message}\nPlec dev output:\n${output}`,
        );
      });
    expect(output).not.toContain('[script response');
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

    writeFileSync(
      path.join(app, 'src/home.tsx'),
      "import logo from '../shared.svg'; export function Home() { return <div>Version C intermediate<img src={logo} /></div>; }\n",
    );
    writeFileSync(
      path.join(app, 'src/home.tsx'),
      "import logo from '../shared.svg'; export function Home() { return <div>Version C final<img src={logo} /></div>; }\n",
    );
    await expect(page.getByText('Version C final'))
      .toBeVisible({ timeout: 60_000 })
      .catch(async (error: Error) => {
        throw new Error(
          `${error.message}\nPlec dev output:\n${output}\nPage:\n${await page.locator('#app').innerText()}`,
        );
      });
    await expect
      .poll(() =>
        page.evaluate(() =>
          sessionStorage.getItem('plec-dev-page-loads'),
        ),
      )
      .toBe('3');
    await page.waitForTimeout(500);
    expect(
      await page.evaluate(() =>
        sessionStorage.getItem('plec-dev-page-loads'),
      ),
    ).toBe('3');

    const oldAssetUrl = await page
      .locator('#app img')
      .getAttribute('src');
    const reloadsBeforeAsset = countOccurrences(
      output,
      'Node host reloaded',
    );
    writeFileSync(
      path.join(app, 'shared.svg'),
      '<svg><text>external B</text></svg>',
    );
    await expect
      .poll(async () => page.locator('#app img').getAttribute('src'), {
        timeout: 60_000,
      })
      .not.toBe(oldAssetUrl);
    await waitUntil(
      async () => output,
      (text) =>
        countOccurrences(text, 'Node host reloaded') >
        reloadsBeforeAsset,
    );
    await expect
      .poll(() =>
        page.evaluate(() =>
          sessionStorage.getItem('plec-dev-page-loads'),
        ),
      )
      .toBe('4');
    const reloadsBeforeApi = countOccurrences(
      output,
      'Node host reloaded',
    );
    writeFileSync(
      path.join(app, 'api/version.ts'),
      "export function GET() { return new Response('API B'); }\n",
    );
    await expect
      .poll(async () => (await fetch(`${origin}/api/version`)).text(), {
        timeout: 60_000,
      })
      .toBe('API B');
    await waitUntil(
      async () => output,
      (text) =>
        countOccurrences(text, 'Node host reloaded') > reloadsBeforeApi,
    );
    await expect
      .poll(() =>
        page.evaluate(() =>
          sessionStorage.getItem('plec-dev-page-loads'),
        ),
      )
      .toBe('5');

    expect(output).not.toMatch(
      /listening on http:\/\/127\.0\.0\.1:\d+/,
    );
    await stop(child);
    child = undefined;
    await expect
      .poll(() => canBind(port), { timeout: 10_000 })
      .toBe(true);
    await expect
      .poll(async () =>
        (await import('node:fs')).readdirSync(
          path.join(app, '.plec', 'vite-dev'),
        ),
      )
      .toEqual([]);
  } finally {
    if (child) await stop(child);
    rmSync(sessionRoot, { recursive: true, force: true });
  }
});

function canBind(port: number) {
  return new Promise<boolean>((resolve) => {
    const server = createServer();
    server.once('error', () => resolve(false));
    server.listen(port, '127.0.0.1', () => {
      server.close((error) => resolve(!error));
    });
  });
}
