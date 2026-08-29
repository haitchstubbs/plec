import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import path from 'node:path';
import { chromium } from 'playwright';

const appDir = path.resolve(import.meta.dirname, '..');
const port = Number(process.env.PLEC_ADOPTION_PORT ?? 3202);
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
  throw new Error('adoption acceptance server did not become ready');
}

// The SSR bootstrap must end in `adopted`: the server DOM is kept, listeners
// attach to it, and no full client mount ever replaces the markup.
async function adoptedHomeKeepsDomAndActions(browser) {
  const context = await browser.newContext({
    viewport: { width: 1280, height: 720 },
  });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.addInitScript(() => {
    window.__adoptions = [];
    window.addEventListener('plec:adoption', (event) => {
      window.__adoptions.push(event.detail);
    });
  });
  try {
    await page.goto(origin, { waitUntil: 'networkidle' });
    await page.waitForFunction(
      () => (window.__adoptions ?? []).length > 0,
      undefined,
      { timeout: 15000 },
    );
    const adoption = await page.evaluate(
      () => window.__adoptions[window.__adoptions.length - 1],
    );
    assert.equal(adoption.outcome, 'adopted', `adoption outcome: ${JSON.stringify(adoption)}`);
    assert.deepEqual(adoption.mismatchCodes, [], `mismatch codes: ${JSON.stringify(adoption.mismatchCodes)}`);

    // SSR ownership survived: markers and server text are still in place.
    const dom = await page.evaluate(() => {
      const countComments = (node) => {
        let count = node.nodeType === 8 ? 1 : 0;
        for (const child of node.childNodes) count += countComments(child);
        return count;
      };
      return {
        requestText: document.querySelector('#ssr-request')?.textContent ?? '',
        markers: document.querySelectorAll('[data-plec-node]').length,
        comments: countComments(document.querySelector('#app')),
      };
    });
    assert.ok(dom.markers > 0, 'SSR data-plec-node markers must survive adoption');
    assert.ok(dom.comments > 0, 'SSR ownership comments must survive adoption');
    assert.ok(
      dom.requestText.startsWith('Requested'),
      `server request text must survive: "${dom.requestText}"`,
    );

    // The reported symptom: the sidebar action must update the adopted DOM.
    const sidebar = page.locator('[data-collapsed]').first();
    await page.getByRole('button', { name: 'Toggle sidebar' }).click();
    await page.waitForTimeout(200);
    assert.equal(
      await sidebar.getAttribute('data-collapsed'),
      'true',
      'toggling must collapse the adopted sidebar',
    );
    await page.getByRole('button', { name: 'Toggle sidebar' }).click();
    await page.waitForTimeout(200);
    assert.equal(
      await sidebar.getAttribute('data-collapsed'),
      'false',
      'toggling again must restore the adopted sidebar',
    );

    // Route-graph ownership: the outlet counter must react too.
    const counter = page.locator('#ssr-counter');
    const before = await counter.textContent();
    await counter.click();
    await page.waitForTimeout(200);
    assert.equal(
      await counter.textContent(),
      'SSR counter:1',
      `adopted route counter must update (${before})`,
    );
    assert.deepEqual(errors, [], errors.join('\n'));
  } finally {
    await context.close();
  }
}

async function main() {
  server = spawn(process.execPath, ['dist/server.mjs'], {
    cwd: appDir,
    env: { ...process.env, PORT: String(port) },
    stdio: 'inherit',
    windowsHide: true,
  });
  await waitForServer();
  await sleep(100);
  const browser = await chromium.launch({
    headless: true,
    channel: process.env.PLEC_CHROME_EXECUTABLE ? undefined : 'chrome',
    executablePath: process.env.PLEC_CHROME_EXECUTABLE,
  });
  try {
    await adoptedHomeKeepsDomAndActions(browser);
    console.log('[plec-adoption-acceptance] passed');
  } finally {
    await browser.close();
  }
}

try {
  await main();
} finally {
  if (server?.exitCode === null) server.kill();
}
