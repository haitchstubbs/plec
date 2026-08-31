import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import path from 'node:path';
import { launchBrowser } from './browser.mjs';

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
    assert.equal(
      adoption.outcome,
      'adopted',
      `adoption outcome: ${JSON.stringify(adoption)}`,
    );
    assert.deepEqual(
      adoption.mismatchCodes,
      [],
      `mismatch codes: ${JSON.stringify(adoption.mismatchCodes)}`,
    );

    // SSR ownership survived: markers and server text are still in place.
    const dom = await page.evaluate(() => {
      const countComments = (node) => {
        let count = node.nodeType === 8 ? 1 : 0;
        for (const child of node.childNodes)
          count += countComments(child);
        return count;
      };
      return {
        requestText:
          document.querySelector('#ssr-request')?.textContent ?? '',
        markers: document.querySelectorAll('[data-plec-node]').length,
        comments: countComments(document.querySelector('#app')),
      };
    });
    assert.ok(
      dom.markers > 0,
      'SSR data-plec-node markers must survive adoption',
    );
    assert.ok(
      dom.comments > 0,
      'SSR ownership comments must survive adoption',
    );
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

// A $param route must adopt from the server-published chain instead of
// falling back: the retired gate could only compare path strings.
async function adoptedParamRouteUsesServerChain(browser) {
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
    await page.goto(`${origin}/projects/p42`, {
      waitUntil: 'networkidle',
    });
    await page.waitForFunction(
      () => (window.__adoptions ?? []).length > 0,
      undefined,
      { timeout: 15000 },
    );
    const adoption = await page.evaluate(
      () => window.__adoptions[window.__adoptions.length - 1],
    );
    assert.equal(
      adoption.outcome,
      'adopted',
      `adoption outcome: ${JSON.stringify(adoption)}`,
    );
    assert.deepEqual(
      adoption.mismatchCodes,
      [],
      `mismatch codes: ${JSON.stringify(adoption.mismatchCodes)}`,
    );
    assert.equal(
      adoption.snapshotImported,
      true,
      'param adoption must import the snapshot',
    );

    // The bootstrap publishes the matched route and its decoded params.
    const bootstrap = await page.evaluate(() =>
      JSON.parse(document.querySelector('#plec-bootstrap').textContent),
    );
    const [instance] = bootstrap.snapshot.routes;
    assert.equal(instance.phase, 'active');
    assert.deepEqual(instance.params, { id: 'p42' });
    assert.ok(
      instance.routeId.endsWith('project.route.tsx#Route'),
      `unexpected chain route: ${instance.routeId}`,
    );

    // Param-dependent text renders the server value and survives adoption.
    const text = await page.locator('#ssr-param').textContent();
    assert.ok(
      text.includes('/projects/p42'),
      `param text must render the request path: "${text}"`,
    );
    const markers = await page.evaluate(
      () => document.querySelectorAll('[data-plec-node]').length,
    );
    assert.ok(
      markers > 0,
      'SSR data-plec-node markers must survive adoption',
    );
    assert.deepEqual(errors, [], errors.join('\n'));
  } finally {
    await context.close();
  }
}

// A loader route must adopt with the server-executed outcome: no
// unsupported:ssr-route-loader gate, and zero client refetch of the loader
// URL while the transferred data renders.
async function adoptedLoaderRouteTransfersServerData(browser) {
  const context = await browser.newContext({
    viewport: { width: 1280, height: 720 },
  });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', (error) => errors.push(error.message));
  let loaderRequests = 0;
  page.on('request', (request) => {
    if (
      new URL(request.url()).pathname === '/api/notes' &&
      request.method() === 'GET'
    )
      loaderRequests += 1;
  });
  await page.addInitScript(() => {
    window.__adoptions = [];
    window.addEventListener('plec:adoption', (event) => {
      window.__adoptions.push(event.detail);
    });
  });
  try {
    await page.goto(`${origin}/notes`, { waitUntil: 'networkidle' });
    await page.waitForFunction(
      () => (window.__adoptions ?? []).length > 0,
      undefined,
      { timeout: 15000 },
    );
    const adoption = await page.evaluate(
      () => window.__adoptions[window.__adoptions.length - 1],
    );
    assert.equal(
      adoption.outcome,
      'adopted',
      `adoption outcome: ${JSON.stringify(adoption)}`,
    );
    assert.deepEqual(
      adoption.mismatchCodes,
      [],
      `mismatch codes: ${JSON.stringify(adoption.mismatchCodes)}`,
    );
    assert.equal(adoption.snapshotImported, true);

    // The request-count contract: the loader ran on the server, so adoption
    // must not refetch the loader URL from the client.
    assert.equal(
      loaderRequests,
      0,
      'the SSR loader outcome must transfer without a client refetch',
    );

    // The transferred outcome is both in the snapshot and on screen.
    const bootstrap = await page.evaluate(() =>
      JSON.parse(document.querySelector('#plec-bootstrap').textContent),
    );
    const [loader] = bootstrap.snapshot.loaders;
    assert.equal(loader.state.kind, 'resolved');
    assert.ok(
      loader.state.value.headline.includes('server'),
      `unexpected loader payload: ${JSON.stringify(loader.state.value)}`,
    );
    const headline = await page.getByRole('heading').textContent();
    assert.equal(headline, loader.state.value.headline);
    const markers = await page.evaluate(
      () => document.querySelectorAll('[data-plec-node]').length,
    );
    assert.ok(
      markers > 0,
      'SSR data-plec-node markers must survive adoption',
    );
    assert.deepEqual(errors, [], errors.join('\n'));
  } finally {
    await context.close();
  }
}

// Conditional structural ownership: the server renders one branch between
// `plec:conditional` markers and records the instantiated side in the
// snapshot. Adoption must keep the server branch DOM and later flips must
// replace exactly that region through the normal reconcile path.
async function adoptedConditionalBranchFlipsInPlace(browser) {
  const context = await browser.newContext({
    viewport: { width: 1280, height: 720 },
  });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', (error) => errors.push(error.message));
  page.addInitScript(() => {
    window.__adoptions = [];
    window.addEventListener('plec:adoption', (event) => {
      window.__adoptions.push(event.detail);
    });
  });
  try {
    await page.goto(`${origin}/projects/p42`, {
      waitUntil: 'networkidle',
    });
    await page.waitForFunction(
      () => (window.__adoptions ?? []).length > 0,
      undefined,
      { timeout: 15000 },
    );
    const adoption = await page.evaluate(
      () => window.__adoptions[window.__adoptions.length - 1],
    );
    assert.equal(
      adoption.outcome,
      'adopted',
      `adoption outcome: ${JSON.stringify(adoption)}`,
    );
    assert.equal(adoption.snapshotImported, true, 'snapshot must be imported');

    // The server rendered the collapsed branch (initial state false) between
    // the conditional markers, and adoption kept that exact DOM in place.
    const closed = await page.evaluate(() => {
      const element = document.querySelector('#ssr-branch-closed');
      return {
        text: element?.textContent ?? '',
        marker: element?.getAttribute('data-plec-node') ?? null,
        openPresent: document.querySelector('#ssr-branch-open') !== null,
      };
    });
    assert.ok(
      closed.text.includes('collapsed'),
      `server-rendered collapsed branch must survive adoption: "${closed.text}"`,
    );
    assert.ok(
      closed.marker,
      'the adopted branch node must keep its server ownership marker',
    );
    assert.equal(closed.openPresent, false);

    // The snapshot records the instantiated branch for the route instance.
    const branchRecord = await page.evaluate(() => {
      const bootstrap = JSON.parse(
        document.querySelector('#plec-bootstrap').textContent,
      );
      const graphs = bootstrap.snapshot.structure.graphs;
      const routeInstance = Object.keys(graphs).find(
        (key) => key !== 'root/outlet:main',
      );
      return { routeInstance, branches: graphs[routeInstance].branches };
    });
    assert.equal(
      branchRecord.routeInstance,
      'root%2Foutlet:main/outlet:main',
      `route instance key must mirror the runtime grammar: ${branchRecord.routeInstance}`,
    );
    assert.ok(
      branchRecord.branches.some((branch) => branch.selected === 'alternate'),
      `branch record must name the server-rendered side: ${JSON.stringify(branchRecord.branches)}`,
    );

    // Toggling flips the adopted region in place: the server branch is
    // replaced by the instantiated other branch, then restored on a second
    // toggle (branch-flip reconciliation over adopted ownership).
    await page.getByRole('button', { name: 'Toggle project detail' }).click();
    await page.waitForTimeout(200);
    assert.equal(
      await page.locator('#ssr-branch-open').count(),
      1,
      'toggle must expand the adopted conditional region',
    );
    assert.equal(
      await page.locator('#ssr-branch-closed').count(),
      0,
      'the collapsed branch must be removed on flip',
    );
    await page.getByRole('button', { name: 'Toggle project detail' }).click();
    await page.waitForTimeout(200);
    assert.equal(
      await page.locator('#ssr-branch-closed').count(),
      1,
      'toggling again must restore the collapsed branch',
    );
    assert.equal(
      await page.locator('#ssr-branch-open').count(),
      0,
      'the expanded branch must be removed on flip back',
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
  const browser = await launchBrowser({ headless: true });
  try {
    await adoptedHomeKeepsDomAndActions(browser);
    await adoptedParamRouteUsesServerChain(browser);
    await adoptedConditionalBranchFlipsInPlace(browser);
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
