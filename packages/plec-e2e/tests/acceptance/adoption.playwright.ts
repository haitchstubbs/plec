import {
  expect,
  test,
  type Browser,
  type Page,
} from '@playwright/test';
import {
  lastAdoption,
  trackAdoptions,
  type AdoptionDetail,
} from '../support/helpers';

// Every scenario opens a fresh context and waits for the `plec:adoption`
// event: the SSR bootstrap must end in `adopted` — the server DOM is kept,
// listeners attach to it, and no full client mount ever replaces the markup.
async function adoptedPage(
  browser: Browser,
  path: string,
): Promise<{
  context: Awaited<ReturnType<Browser['newContext']>>;
  page: Page;
  adoption: AdoptionDetail;
}> {
  const context = await browser.newContext({
    viewport: { width: 1280, height: 720 },
  });
  const page = await context.newPage();
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.addInitScript(trackAdoptions);
  await page.goto(path, { waitUntil: 'networkidle' });
  const adoption = await lastAdoption(page);
  expect(adoption.outcome, JSON.stringify(adoption)).toBe('adopted');
  expect(
    adoption.mismatchCodes,
    `mismatch codes: ${JSON.stringify(adoption.mismatchCodes)}`,
  ).toEqual([]);
  return { context, page, adoption };
}

function readBootstrap(page: Page) {
  return page.evaluate(() =>
    JSON.parse(
      (document.querySelector('#plec-bootstrap') as HTMLScriptElement)
        .textContent!,
    ),
  );
}

test('home adoption keeps DOM and actions', async ({ browser }) => {
  const { context, page } = await adoptedPage(browser, '/');
  try {
    // SSR ownership survived: markers and server text are still in place.
    const dom = await page.evaluate(() => {
      const countComments = (node: Node): number => {
        let count = node.nodeType === 8 ? 1 : 0;
        for (const child of node.childNodes)
          count += countComments(child);
        return count;
      };
      return {
        requestText:
          document.querySelector('#ssr-request')?.textContent ?? '',
        markers: document.querySelectorAll('[data-plec-node]').length,
        comments: countComments(
          document.querySelector('#app') ?? document.body,
        ),
      };
    });
    expect(
      dom.markers,
      'SSR data-plec-node markers must survive adoption',
    ).toBeGreaterThan(0);
    expect(
      dom.comments,
      'SSR ownership comments must survive adoption',
    ).toBeGreaterThan(0);
    expect(
      dom.requestText.startsWith('Requested'),
      `server request text must survive: "${dom.requestText}"`,
    ).toBe(true);

    // The reported symptom: the sidebar action must update the adopted DOM.
    const sidebar = page.locator('[data-collapsed]').first();
    await page.getByRole('button', { name: 'Toggle sidebar' }).click();
    await page.waitForTimeout(200);
    expect(
      await sidebar.getAttribute('data-collapsed'),
      'toggling must collapse the adopted sidebar',
    ).toBe('true');
    await page.getByRole('button', { name: 'Toggle sidebar' }).click();
    await page.waitForTimeout(200);
    expect(
      await sidebar.getAttribute('data-collapsed'),
      'toggling again must restore the adopted sidebar',
    ).toBe('false');

    // Route-graph ownership: the outlet counter must react too.
    const counter = page.locator('#ssr-counter');
    const before = await counter.textContent();
    await counter.click();
    await page.waitForTimeout(200);
    expect(
      await counter.textContent(),
      `adopted route counter must update (${before})`,
    ).toBe('SSR counter:1');
  } finally {
    await context.close();
  }
});

test('param route adopts from the server-published chain', async ({
  browser,
}) => {
  const { context, page, adoption } = await adoptedPage(
    browser,
    '/projects/p42',
  );
  try {
    expect(
      adoption.snapshotImported,
      'param adoption must import the snapshot',
    ).toBe(true);

    // The bootstrap publishes the matched route and its decoded params.
    const bootstrap = await readBootstrap(page);
    const [instance] = bootstrap.snapshot.routes;
    expect(instance.phase).toBe('active');
    expect(instance.params).toEqual({ id: 'p42' });
    expect(
      instance.routeId,
      `unexpected chain route: ${instance.routeId}`,
    ).toMatch(/project\.route\.tsx#Route$/);

    // Param-dependent text renders the server value and survives adoption.
    const text = await page.locator('#ssr-param').textContent();
    expect(
      text?.includes('/projects/p42'),
      `param text must render the request path: "${text}"`,
    ).toBe(true);
    const markers = await page.evaluate(
      () => document.querySelectorAll('[data-plec-node]').length,
    );
    expect(
      markers,
      'SSR data-plec-node markers must survive adoption',
    ).toBeGreaterThan(0);
  } finally {
    await context.close();
  }
});

test('conditional branch flips in place after adoption', async ({
  browser,
}) => {
  const { context, page, adoption } = await adoptedPage(
    browser,
    '/projects/p42',
  );
  try {
    expect(adoption.snapshotImported, 'snapshot must be imported').toBe(
      true,
    );

    // The server rendered the collapsed branch (initial state false) between
    // the conditional markers, and adoption kept that exact DOM in place.
    const closed = await page.evaluate(() => {
      const element = document.querySelector('#ssr-branch-closed');
      return {
        text: element?.textContent ?? '',
        marker: element?.getAttribute('data-plec-node') ?? null,
        openPresent:
          document.querySelector('#ssr-branch-open') !== null,
      };
    });
    expect(
      closed.text.includes('collapsed'),
      `server-rendered collapsed branch must survive adoption: "${closed.text}"`,
    ).toBe(true);
    expect(
      closed.marker,
      'the adopted branch node must keep its server ownership marker',
    ).toBeTruthy();
    expect(closed.openPresent).toBe(false);

    // The snapshot records the instantiated branch for the route instance.
    const bootstrap = await readBootstrap(page);
    const graphs = bootstrap.snapshot.structure.graphs;
    const routeInstance = Object.keys(graphs).find(
      (key) => key !== 'root/outlet:main',
    );
    expect(
      routeInstance,
      `route instance key must mirror the runtime grammar: ${routeInstance}`,
    ).toBe('root%2Foutlet:main/outlet:main');
    expect(
      graphs[routeInstance].branches.some(
        (branch: { selected: string }) =>
          branch.selected === 'alternate',
      ),
      `branch record must name the server-rendered side: ${JSON.stringify(graphs[routeInstance].branches)}`,
    ).toBe(true);

    // Toggling flips the adopted region in place: the server branch is
    // replaced by the instantiated other branch, then restored on a second
    // toggle (branch-flip reconciliation over adopted ownership).
    await page
      .getByRole('button', { name: 'Toggle project detail' })
      .click();
    await page.waitForTimeout(200);
    expect(
      await page.locator('#ssr-branch-open').count(),
      'toggle must expand the adopted conditional region',
    ).toBe(1);
    expect(
      await page.locator('#ssr-branch-closed').count(),
      'the collapsed branch must be removed on flip',
    ).toBe(0);
    await page
      .getByRole('button', { name: 'Toggle project detail' })
      .click();
    await page.waitForTimeout(200);
    expect(
      await page.locator('#ssr-branch-closed').count(),
      'toggling again must restore the collapsed branch',
    ).toBe(1);
    expect(
      await page.locator('#ssr-branch-open').count(),
      'the expanded branch must be removed on flip back',
    ).toBe(0);
  } finally {
    await context.close();
  }
});

test('todo loop rows survive adoption with keys and markers', async ({
  browser,
}) => {
  const { context, page, adoption } = await adoptedPage(
    browser,
    '/todos',
  );
  try {
    expect(adoption.snapshotImported, 'snapshot must be imported').toBe(
      true,
    );

    // The snapshot records ordered row keys (identity only, no values).
    const loopRecord = await page.evaluate(() => {
      const bootstrap = JSON.parse(
        (document.querySelector('#plec-bootstrap') as HTMLScriptElement)
          .textContent!,
      );
      const graphs = bootstrap.snapshot.structure.graphs;
      for (const [instance, structure] of Object.entries(graphs)) {
        if ((structure as { loops?: unknown[] }).loops?.length) {
          return {
            instance,
            loops: (structure as { loops: unknown[] }).loops,
          };
        }
      }
      return null;
    });
    expect(
      loopRecord,
      'snapshot must record loop row keys',
    ).toBeTruthy();
    expect(
      (loopRecord!.loops as Array<{ keys: unknown }>).every(
        (loopEntry) => Array.isArray(loopEntry.keys),
      ),
      `loop records must carry key lists: ${JSON.stringify(loopRecord)}`,
    ).toBe(true);
    expect(
      (loopRecord!.loops as Array<{ keys: unknown[] }>).some(
        (loopEntry) => loopEntry.keys.length > 0,
      ),
      `the todos loop must record its server-rendered keys: ${JSON.stringify(loopRecord)}`,
    ).toBe(true);

    // Server rows survived adoption: row roots keep the runtime row key and
    // their server ownership markers (a remount would drop both).
    const stamped = await page.evaluate(() => {
      const rows = [
        ...document.querySelectorAll('li[data-runtime-row-key]'),
      ] as HTMLLIElement[];
      rows.forEach((row, index) => {
        row.setAttribute('data-acceptance-probe', String(index));
      });
      return rows.map((row) => ({
        key: row.getAttribute('data-runtime-row-key'),
        marker: row.getAttribute('data-plec-node'),
      }));
    });
    expect(
      stamped.length,
      'SSR todo rows must exist after adoption',
    ).toBeGreaterThan(0);
    expect(
      stamped.every(
        (row) =>
          row.marker &&
          row.marker.includes('/loop:') &&
          row.marker.includes('/key:'),
      ),
      `adopted rows must keep server loop ownership markers: ${JSON.stringify(stamped)}`,
    ).toBe(true);
  } finally {
    await context.close();
  }
});

// Blocked by wasm-runtime-hv3: on adopted pages the runtime re-evaluates
// loop-row bindings with a lost row scope, so the toggle never rebinds the
// checkbox/aria-label and the conditional renders the editing branch.
// Un-skip once adopted loop-row re-evaluation is fixed.
test.fixme('todo loop rows stay targeted across toggle and insert', async ({
  browser,
}) => {
  const { context, page } = await adoptedPage(browser, '/todos');
  try {
    const stamped = await page.evaluate(() => {
      const rows = [
        ...document.querySelectorAll('li[data-runtime-row-key]'),
      ] as HTMLLIElement[];
      rows.forEach((row, index) => {
        row.setAttribute('data-acceptance-probe', String(index));
      });
      return rows.map((row) => ({
        key: row.getAttribute('data-runtime-row-key'),
        marker: row.getAttribute('data-plec-node'),
      }));
    });
    expect(stamped.length).toBeGreaterThan(0);

    // A one-row toggle is targeted: the toggled row updates in place while
    // every other claimed row keeps its element identity.
    const firstTitle = stamped[0].key;
    await page.getByRole('checkbox').first().click();
    await page.waitForFunction(
      (key) =>
        document
          .querySelector(
            `li[data-runtime-row-key="${key}"] input[type="checkbox"]`,
          )
          ?.getAttribute('aria-label')
          ?.includes('open'),
      firstTitle,
      { timeout: 5_000 },
    );
    const probesAfterToggle = await page.evaluate(() =>
      [...document.querySelectorAll('li[data-acceptance-probe]')].map(
        (row) => row.getAttribute('data-runtime-row-key'),
      ),
    );
    expect(
      probesAfterToggle,
      'toggling one row must keep every claimed row element in place',
    ).toEqual(stamped.map((row) => row.key));

    // Delta insert through the adopted loop: a new row appears with a
    // runtime row key while the claimed rows stay untouched.
    await page.fill('#todo-new-title', 'Adoption second todo');
    await page.getByRole('button', { name: 'Add todo' }).click();
    await expect(page.getByText('Adoption second todo')).toBeVisible({
      timeout: 5_000,
    });
    const afterAdd = await page.evaluate(() => ({
      rows: document.querySelectorAll('li[data-runtime-row-key]')
        .length,
      probes: document.querySelectorAll('li[data-acceptance-probe]')
        .length,
    }));
    expect(
      afterAdd.rows,
      'the inserted row must appear in the adopted list',
    ).toBe(stamped.length + 1);
    expect(
      afterAdd.probes,
      'the claimed rows must keep their identity across the insert',
    ).toBe(stamped.length);
  } finally {
    await context.close();
  }
});

test('loader route transfers the server outcome without refetch', async ({
  browser,
}) => {
  const context = await browser.newContext({
    viewport: { width: 1280, height: 720 },
  });
  const page = await context.newPage();
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  let loaderRequests = 0;
  page.on('request', (request) => {
    if (
      new URL(request.url()).pathname === '/api/notes' &&
      request.method() === 'GET'
    ) {
      loaderRequests += 1;
    }
  });
  await page.addInitScript(trackAdoptions);
  try {
    await page.goto('/notes', { waitUntil: 'networkidle' });
    const adoption = await lastAdoption(page);
    expect(adoption.outcome, JSON.stringify(adoption)).toBe('adopted');
    expect(
      adoption.mismatchCodes,
      `mismatch codes: ${JSON.stringify(adoption.mismatchCodes)}`,
    ).toEqual([]);
    expect(adoption.snapshotImported).toBe(true);

    // The request-count contract: the loader ran on the server, so adoption
    // must not refetch the loader URL from the client.
    expect(
      loaderRequests,
      'the SSR loader outcome must transfer without a client refetch',
    ).toBe(0);

    // The transferred outcome is both in the snapshot and on screen.
    const bootstrap = await readBootstrap(page);
    const [loader] = bootstrap.snapshot.loaders;
    expect(loader.state.kind).toBe('resolved');
    expect(
      loader.state.value.headline.includes('server'),
      `unexpected loader payload: ${JSON.stringify(loader.state.value)}`,
    ).toBe(true);
    const headline = await page.getByRole('heading').textContent();
    expect(headline).toBe(loader.state.value.headline);
    const markers = await page.evaluate(
      () => document.querySelectorAll('[data-plec-node]').length,
    );
    expect(
      markers,
      'SSR data-plec-node markers must survive adoption',
    ).toBeGreaterThan(0);
    expect(errors, errors.join('\n')).toEqual([]);
  } finally {
    await context.close();
  }
});
