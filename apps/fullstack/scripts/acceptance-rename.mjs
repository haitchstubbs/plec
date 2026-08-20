import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import path from 'node:path';
import { chromium } from 'playwright';

const appDir = path.resolve(import.meta.dirname, '..');
const port = Number(process.env.PLEC_ACCEPTANCE_PORT ?? 3201);
const origin = `http://127.0.0.1:${port}`;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

let server;

async function waitForServer() {
  for (let attempt = 0; attempt < 50; attempt += 1) {
    try {
      if ((await fetch(origin)).ok) return;
    } catch {}

    await sleep(100);
  }

  throw new Error('Server did not become ready');
}

async function forwardTodoRequest(route) {
  const request = route.request();
  const contentType = request.headers()['content-type'];

  const response = await fetch(request.url(), {
    method: request.method(),
    ...(contentType
      ? { headers: { 'content-type': contentType } }
      : {}),
    ...(request.postData() ? { body: request.postData() } : {}),
  });

  await route.fulfill({
    status: response.status,
    ...(response.headers.get('content-type')
      ? { contentType: response.headers.get('content-type') }
      : {}),
    body: await response.text(),
  });
}

async function renameTest(browser) {
  const context = await browser.newContext();
  const page = await context.newPage();

  page.setDefaultTimeout(3000);
  page.setDefaultNavigationTimeout(15000);

  let nativeKeydown = false;
  let patchRequest = null;
  let patchResponse = null;

  page.on('pageerror', (error) => {
    console.error('[pageerror]', error.stack ?? error.message);
  });

  page.on('console', (msg) => {
    console.log('[browser]', msg.text());

    if (msg.text().includes('[native keydown]')) {
      nativeKeydown = true;
    }
  });

  page.on('request', (request) => {
    if (
      request.method() === 'PATCH' &&
      new URL(request.url()).pathname.startsWith('/api/todos/')
    ) {
      patchRequest = request;

      console.log('[PATCH request]', request.url(), request.postData());
    }
  });

  page.on('response', (response) => {
    if (
      response.request().method() === 'PATCH' &&
      new URL(response.url()).pathname.startsWith('/api/todos/')
    ) {
      patchResponse = response;

      console.log(
        '[PATCH response]',
        response.status(),
        response.url(),
      );
    }
  });

  await page.route(
    (url) => /^\/api\/todos(?:\/[^/]+)?$/.test(url.pathname),
    forwardTodoRequest,
  );

  try {
    await page.goto(`${origin}/todos`, {
      waitUntil: 'domcontentloaded',
    });

    await page.getByRole('heading', { name: 'Todos' }).waitFor();

    const row = page.locator('li[data-runtime-row-key]').first();
    await row.waitFor();

    const rowKey = await row.getAttribute('data-runtime-row-key');
    assert.ok(rowKey, 'No todo row found');

    console.log('[row]', rowKey);

    await row.locator('button', { hasText: 'Edit' }).click();

    const rename = row.locator('input:not([type="checkbox"])');

    await rename.waitFor();

    await rename.evaluate((el) => {
      el.addEventListener('keydown', (event) => {
        console.log(
          '[native keydown]',
          event.key,
          event.code,
          el.value,
        );
      });
    });

    const renamed = `Renamed ${Date.now()}`;

    await rename.fill(renamed);
    assert.equal(
      await rename.inputValue(),
      renamed,
      'Rename input did not receive new value',
    );

    console.log('[press] Enter');

    await rename.press('Enter');

    await sleep(100);

    assert.equal(
      nativeKeydown,
      true,
      'FAIL: browser never emitted native keydown',
    );

    assert.ok(
      patchRequest,
      'FAIL: native Enter fired but Plec emitted no PATCH request',
    );

    await page.waitForFunction(() => true, null, { timeout: 100 });

    for (let i = 0; i < 20 && !patchResponse; i += 1) {
      await sleep(50);
    }

    assert.ok(
      patchResponse,
      'FAIL: PATCH was emitted but no response returned',
    );

    assert.ok(
      patchResponse.status() >= 200 && patchResponse.status() < 300,
      `FAIL: PATCH returned ${patchResponse.status()}`,
    );

    await page
      .getByText(renamed, { exact: true })
      .waitFor({ timeout: 2000 });

    console.log('[PASS] rename completed');
  } finally {
    await context.close();
  }
}

async function main() {
  server = spawn(process.execPath, ['dist/server.mjs'], {
    cwd: appDir,
    env: {
      ...process.env,
      PORT: String(port),
    },
    stdio: 'inherit',
    windowsHide: true,
  });

  await waitForServer();

  const browser = await chromium.launch({
    headless: true,
    channel: process.env.PLEC_CHROME_EXECUTABLE ? undefined : 'chrome',
    executablePath: process.env.PLEC_CHROME_EXECUTABLE,
  });

  try {
    await renameTest(browser);
  } finally {
    await browser.close();
  }
}

try {
  await main();
} finally {
  if (server?.exitCode === null) {
    server.kill();
  }
}
