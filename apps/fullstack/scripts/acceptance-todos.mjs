import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { chromium } from 'playwright';

const appDir = path.resolve(import.meta.dirname, '..');
const port = Number(process.env.PLEC_ACCEPTANCE_PORT ?? 3201);
const origin = `http://127.0.0.1:${port}`;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
let server;

async function waitForServer() {
  for (let attempt = 0; attempt < 100; attempt += 1) {
    try {
      if ((await fetch(origin)).ok) return;
    } catch {}
    await sleep(100);
  }
  throw new Error('Fullstack acceptance server did not become ready');
}

async function assertArtifacts() {
  const publicDir = path.join(appDir, 'dist', 'public');
  const manifest = JSON.parse(
    await readFile(path.join(publicDir, 'route-manifest.json'), 'utf8'),
  );
  const todos = manifest.routes.find((route) => route.path === 'todos');
  assert.ok(Number.isInteger(todos?.loaderAction), 'todos needs a typed loader action');
  const graphIds = new Set([
    manifest.rootGraphId,
    ...manifest.routes.flatMap((route) =>
      [route.graphId, route.pendingGraphId, route.errorGraphId].filter(Boolean),
    ),
  ]);
  for (const id of graphIds) {
    const graph = JSON.parse(
      await readFile(path.join(publicDir, 'graphs', `${id}.json`), 'utf8'),
    );
    assert.equal(graph.version, '0.9', `${id} is not a typed graph`);
  }
  return graphIds;
}

function watch(page) {
  const errors = [];
  const graphs = new Set();
  page.on('pageerror', (error) => errors.push(error.message));
  page.on('requestfailed', (request) => {
    if (request.url().startsWith(origin)) {
      errors.push(`request failed: ${request.url()}`);
    }
  });
  page.on('response', (response) => {
    const match = /\/graphs\/([^/?]+)\.json/.exec(response.url());
    if (match) graphs.add(match[1]);
  });
  return () => {
    assert.deepEqual(errors, [], errors.join('\n'));
    assert.ok(graphs.size > 0, 'the browser did not fetch graph artifacts');
  };
}

async function forwardTodoRequest(route) {
  const request = route.request();
  const contentType = request.headers()['content-type'];
  const response = await fetch(request.url(), {
    method: request.method(),
    ...(contentType ? { headers: { 'content-type': contentType } } : {}),
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

async function waitForMount(page) {
  await page.waitForFunction(
    () => window.__plecPerformance?.snapshot().marks['plec:mount-end'] !== undefined,
  );
}

async function loaderPendingThenSuccess(browser) {
  const context = await browser.newContext();
  const page = await context.newPage();
  const done = watch(page);
  let delayed = false;
  await page.route('**/api/todos', async (route) => {
    if (route.request().method() === 'GET' && !delayed) {
      delayed = true;
      await sleep(250);
    }
    await forwardTodoRequest(route);
  });
  try {
    await page.goto(`${origin}/todos`, { waitUntil: 'domcontentloaded' });
    await page.getByText('Loading todos…', { exact: true }).waitFor();
    await page.getByRole('heading', { name: 'Todos' }).waitFor();
    await waitForMount(page);
    done();
  } finally {
    await context.close();
  }
}

async function loaderErrorThenRetry(browser) {
  const context = await browser.newContext();
  const page = await context.newPage();
  const done = watch(page);
  let failed = false;
  await page.route('**/api/todos', async (route) => {
    if (route.request().method() === 'GET' && !failed) {
      failed = true;
      await route.fulfill({ status: 500, body: '{}' });
    } else await forwardTodoRequest(route);
  });
  try {
    await page.goto(`${origin}/todos`, { waitUntil: 'domcontentloaded' });
    await page.getByText('Could not load todos.', { exact: true }).waitFor();
    await page.getByRole('button', { name: 'Try again' }).click();
    await page.getByRole('heading', { name: 'Todos' }).waitFor();
    done();
  } finally {
    await context.close();
  }
}

async function todoActions(browser) {
  const context = await browser.newContext();
  const page = await context.newPage();
  const done = watch(page);
  try {
    await page.route('**/api/todos', forwardTodoRequest);
    await page.goto(`${origin}/todos`, { waitUntil: 'domcontentloaded' });
    await page.getByRole('heading', { name: 'Todos' }).waitFor();

    const search = page.locator('#todo-search');
    await search.fill('missing');
    await page.getByText('Try the Plec Todo API').waitFor({ state: 'detached' });
    await page.reload({ waitUntil: 'domcontentloaded' });
    await page.getByRole('heading', { name: 'Todos' }).waitFor();

    let post = 'delay';
    await page.route('**/api/todos', async (route) => {
      if (route.request().method() === 'POST' && post === 'delay') {
        post = 'done';
        const response = await fetch(route.request().url(), {
          method: route.request().method(),
          headers: { 'content-type': route.request().headers()['content-type'] },
          body: route.request().postData(),
        });
        await sleep(250);
        await route.fulfill({
          status: response.status,
          contentType: response.headers.get('content-type') ?? undefined,
          body: await response.text(),
        });
      } else if (route.request().method() === 'POST' && post === 'reject') {
        post = 'done';
        await route.fulfill({
          status: 500,
          contentType: 'application/json',
          body: '{}',
        });
      } else await forwardTodoRequest(route);
    });
    const title = page.locator('#todo-new-title');
    await title.fill('Acceptance todo');
    await page.getByRole('button', { name: 'Add todo' }).click();
    const adding = page.getByRole('button', { name: 'Adding…' });
    await adding.waitFor();
    assert.equal(await adding.isDisabled(), true, 'create should be pending');
    await page.getByText('Acceptance todo').waitFor();

    post = 'reject';
    await title.fill('Rejected todo');
    await page.getByRole('button', { name: 'Add todo' }).click();
    await page.getByText('The Todo API rejected this change.', { exact: true }).waitFor();
    assert.equal(await page.getByRole('button', { name: 'Add todo' }).isDisabled(), false, 'finally should clear pending');
    assert.equal(await title.inputValue(), 'Rejected todo');
    let row = page.locator('li').filter({ hasText: 'Acceptance todo' });
    await row.locator('input').check();
    await row.locator('input').waitFor();
    await row.locator('button', { hasText: 'Edit' }).click();
    const rename = row.locator('input');
    await rename.fill('Renamed acceptance todo');
    await rename.press('Enter');
    await page.getByText('Renamed acceptance todo').waitFor();
    row = page.locator('li').filter({ hasText: 'Renamed acceptance todo' });
    await row.locator('button', { hasText: 'Delete' }).click();
    await row.waitFor({ state: 'detached' });
    done();
  } finally {
    await context.close();
  }
}

async function main() {
  const graphIds = await assertArtifacts();
  server = spawn(process.execPath, ['dist/server.mjs'], {
    cwd: appDir,
    env: { ...process.env, PORT: String(port) },
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
    await loaderPendingThenSuccess(browser);
    await loaderErrorThenRetry(browser);
    await todoActions(browser);
    console.log(`[plec-acceptance] passed with ${graphIds.size} typed graphs`);
  } finally {
    await browser.close();
  }
}

try {
  await main();
} finally {
  if (server?.exitCode === null) server.kill();
}
