import { expect, test } from '@playwright/test';
import { waitForMount } from '../support/helpers';

test('successful action invalidation refreshes the matching mounted route loader', async ({ page }) => {
  await page.goto('/projects/plec', { waitUntil: 'domcontentloaded' });
  await waitForMount(page);
  await expect(page.locator('#project-name')).toHaveText('Plec compiler and runtime');

  const loaderRefresh = page.waitForResponse((response) =>
    new URL(response.url()).pathname === '/api/projects/plec',
  );
  const action = page.waitForResponse((response) =>
    new URL(response.url()).pathname.startsWith('/_plec/actions/'),
  );
  await page.locator('#rename-project').click();

  const actionResponse = await action;
  expect(actionResponse.status()).toBe(200);
  expect(await actionResponse.json()).toMatchObject({
    kind: 'success',
    invalidations: ['project:plec'],
  });
  await loaderRefresh;
  await expect(page.locator('#project-name')).toHaveText('Project renamed by action');
});

test('invalidation is router-owned when the action initiator is disposed', async ({ page }) => {
  await page.goto('/', { waitUntil: 'domcontentloaded' });
  await waitForMount(page);
  let projectLoaderResponses = 0;
  page.on('response', (response) => {
    if (new URL(response.url()).pathname === '/api/projects/plec') {
      projectLoaderResponses += 1;
    }
  });
  const actionResponse = page.waitForResponse((response) =>
    new URL(response.url()).pathname.startsWith('/_plec/actions/'),
  );
  await page.locator('#rename-project-from-home').click();
  await page.evaluate(() => {
    history.pushState({}, '', '/projects/plec');
    window.dispatchEvent(new PopStateEvent('popstate'));
  });
  const response = await actionResponse;
  expect(await response.json()).toMatchObject({
    kind: 'success',
    invalidations: ['project:plec'],
  });
  await expect(page.locator('#project-name')).toHaveText('Project renamed from disposed page');
  await expect.poll(() => projectLoaderResponses).toBeGreaterThanOrEqual(2);
});

test('parameterized identities do not refresh a different project instance', async ({ page }) => {
  await page.goto('/', { waitUntil: 'domcontentloaded' });
  await waitForMount(page);
  let projectLoaderResponses = 0;
  page.on('response', (response) => {
    if (new URL(response.url()).pathname === '/api/projects/p42') {
      projectLoaderResponses += 1;
    }
  });
  const actionResponse = page.waitForResponse((response) =>
    new URL(response.url()).pathname.startsWith('/_plec/actions/'),
  );
  await page.locator('#rename-project-from-home').click();
  const initialLoader = page.waitForResponse((response) =>
    new URL(response.url()).pathname === '/api/projects/p42',
  );
  await page.evaluate(() => {
    history.pushState({}, '', '/projects/p42/details');
    window.dispatchEvent(new PopStateEvent('popstate'));
  });
  await initialLoader;
  await expect.poll(() => projectLoaderResponses).toBeGreaterThanOrEqual(2);
  await actionResponse;
  await expect(page.locator('#project-name')).toHaveText('Project 42');
  await expect(page.locator('#project-details-name')).toHaveText('Project 42');
  await new Promise((resolve) => setTimeout(resolve, 250));
  expect(projectLoaderResponses).toBe(2);
});

test('overlapping successful mutations each deliver their invalidation batch', async ({ page }) => {
  await page.goto('/', { waitUntil: 'domcontentloaded' });
  await waitForMount(page);
  let projectLoaderResponses = 0;
  const actionResponses: import('@playwright/test').Response[] = [];
  page.on('response', (response) => {
    if (new URL(response.url()).pathname === '/api/projects/plec') {
      projectLoaderResponses += 1;
    }
    if (new URL(response.url()).pathname.startsWith('/_plec/actions/')) {
      actionResponses.push(response);
    }
  });
  await page.locator('#rename-project-from-home').click();
  await page.locator('#rename-project-from-home').click();
  const initialLoader = page.waitForResponse((response) =>
    new URL(response.url()).pathname === '/api/projects/plec',
  );
  await page.evaluate(() => {
    history.pushState({}, '', '/projects/plec');
    window.dispatchEvent(new PopStateEvent('popstate'));
  });
  await initialLoader;
  await expect.poll(() => actionResponses.length).toBe(2);
  for (const outcome of await Promise.all(actionResponses.map((response) => response.json()))) {
    expect(outcome).toMatchObject({ kind: 'success', invalidations: ['project:plec'] });
  }
  await expect(page.locator('#project-name')).toHaveText('Project renamed from disposed page');
  await expect.poll(() => projectLoaderResponses).toBeGreaterThanOrEqual(3);
});

test('all active loaders sharing an identity reload once in one invalidation batch', async ({ page }) => {
  let projectLoaderResponses = 0;
  page.on('response', (response) => {
    if (new URL(response.url()).pathname === '/api/projects/plec') {
      projectLoaderResponses += 1;
    }
  });
  await page.goto('/projects/plec/details', { waitUntil: 'domcontentloaded' });
  await waitForMount(page);
  await expect(page.locator('#project-name')).toHaveText(/.+/);
  await expect(page.locator('#project-details-name')).toHaveText(/.+/);
  const action = page.waitForResponse((response) =>
    new URL(response.url()).pathname.startsWith('/_plec/actions/'),
  );
  await page.locator('#rename-project').click();
  await action;
  await expect(page.locator('#project-name')).toHaveText('Project renamed by action');
  await expect(page.locator('#project-details-name')).toHaveText('Project renamed by action');
  await expect.poll(() => projectLoaderResponses).toBe(2);
});

test('a slower older loader response cannot overwrite a newer invalidation result', async ({ page }) => {
  await page.goto('/projects/plec', { waitUntil: 'domcontentloaded' });
  await waitForMount(page);
  let loaderRequests = 0;
  let resolveFirstRequest!: () => void;
  const firstRequestStarted = new Promise<void>((resolve) => {
    resolveFirstRequest = resolve;
  });
  await page.route('**/api/projects/plec', async (route) => {
    loaderRequests += 1;
    const requestNumber = loaderRequests;
    if (requestNumber === 1) {
      resolveFirstRequest();
      await new Promise((resolve) => setTimeout(resolve, 400));
    } else {
      await new Promise((resolve) => setTimeout(resolve, 20));
    }
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        missing: false,
        name: `loader-response-${requestNumber}`,
      }),
    });
  });
  const actionResponses: import('@playwright/test').Response[] = [];
  page.on('response', (response) => {
    if (new URL(response.url()).pathname.startsWith('/_plec/actions/')) {
      actionResponses.push(response);
    }
  });

  await page.locator('#rename-project').click();
  await firstRequestStarted;
  await page.locator('#rename-project').click();
  await expect.poll(() => loaderRequests).toBe(2);
  await expect.poll(() => actionResponses.length).toBe(2);
  await expect(page.locator('#project-name')).toHaveText('loader-response-2');
  await new Promise((resolve) => setTimeout(resolve, 450));
  await expect(page.locator('#project-name')).toHaveText('loader-response-2');
});

test('a pending route reload runs overlapping loaders on the preserved loader graph', async ({ page }) => {
  await page.goto('/', { waitUntil: 'domcontentloaded' });
  await waitForMount(page);
  const firstProjectLoad = page.waitForResponse((response) =>
    new URL(response.url()).pathname === '/api/projects/plec',
  );
  await page.evaluate(() => {
    history.pushState({}, '', '/projects/plec');
    window.dispatchEvent(new PopStateEvent('popstate'));
  });
  await firstProjectLoad;
  await expect(page.locator('#project-name')).toHaveText(/.+/);
  await page.evaluate(() => {
    history.pushState({}, '', '/');
    window.dispatchEvent(new PopStateEvent('popstate'));
  });
  await expect(page.locator('#rename-project-from-home')).toBeVisible();

  let loaderRequests = 0;
  let releaseFirstLoader!: () => void;
  let firstLoaderStarted!: () => void;
  const firstLoaderGate = new Promise<void>((resolve) => {
    releaseFirstLoader = resolve;
  });
  const firstLoaderSignal = new Promise<void>((resolve) => {
    firstLoaderStarted = resolve;
  });
  await page.route('**/api/projects/plec', async (route) => {
    loaderRequests += 1;
    const requestNumber = loaderRequests;
    if (requestNumber === 1) {
      firstLoaderStarted();
      await firstLoaderGate;
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ missing: false, name: 'older pending result' }),
      });
      return;
    }
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ missing: false, name: `loader result ${requestNumber}` }),
    });
  });
  const actionResponses: import('@playwright/test').Response[] = [];
  page.on('response', (response) => {
    if (new URL(response.url()).pathname.startsWith('/_plec/actions/')) {
      actionResponses.push(response);
    }
  });

  try {
    await page.locator('#rename-project-from-home').click();
    await page.locator('#rename-project-from-home').click();
    await page.evaluate(() => {
      history.pushState({}, '', '/projects/plec');
      window.dispatchEvent(new PopStateEvent('popstate'));
    });
    await firstLoaderSignal;
    await expect(page.locator('#project-pending')).toBeVisible();
    await expect.poll(() => loaderRequests).toBe(3);
    await expect.poll(() => actionResponses.length).toBe(2);
    await expect(page.locator('#project-name')).toHaveText('loader result 3');
  } finally {
    releaseFirstLoader();
  }
  await new Promise((resolve) => setTimeout(resolve, 50));
  await expect(page.locator('#project-name')).toHaveText('loader result 3');
});

test('invalidation during an interrupted navigation does not reload the departing route', async ({ page }) => {
  await page.goto('/projects/plec', { waitUntil: 'domcontentloaded' });
  await waitForMount(page);
  let departingRouteReloads = 0;
  let releaseSlowLoader!: () => void;
  let resolveSlowLoaderStarted!: () => void;
  const slowLoaderGate = new Promise<void>((resolve) => {
    releaseSlowLoader = resolve;
  });
  const slowLoaderStarted = new Promise<void>((resolve) => {
    resolveSlowLoaderStarted = resolve;
  });
  await page.route('**/api/projects/p42', async (route) => {
    resolveSlowLoaderStarted();
    await slowLoaderGate;
    try {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ missing: false, name: 'Slow project' }),
      });
    } catch {
      // A newer navigation may abort the pending loader request.
    }
  });
  page.on('request', (request) => {
    if (new URL(request.url()).pathname === '/api/projects/plec') {
      departingRouteReloads += 1;
    }
  });
  const actionResponse = page.waitForResponse((response) =>
    new URL(response.url()).pathname.startsWith('/_plec/actions/'),
  );
  await page.locator('#rename-project').click();
  await page.evaluate(() => {
    history.pushState({}, '', '/projects/p42');
    window.dispatchEvent(new PopStateEvent('popstate'));
  });
  await slowLoaderStarted;
  await page.evaluate(() => {
    history.pushState({}, '', '/about');
    window.dispatchEvent(new PopStateEvent('popstate'));
  });
  const aboutHeading = page.getByRole('heading', {
    name: 'Plec is a renderer, not a React compatibility layer.',
  });
  await expect(aboutHeading).toBeVisible();
  const response = await actionResponse;
  expect(await response.json()).toMatchObject({
    kind: 'success',
    invalidations: ['project:plec'],
  });
  releaseSlowLoader();
  await expect.poll(() => departingRouteReloads).toBe(0);
  await expect(aboutHeading).toBeVisible();
});
