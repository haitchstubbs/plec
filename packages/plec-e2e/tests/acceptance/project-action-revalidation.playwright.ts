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

  const [actionResponse] = await Promise.all([action, loaderRefresh]);
  expect(actionResponse.status()).toBe(200);
  expect(await actionResponse.json()).toMatchObject({
    kind: 'success',
    invalidations: ['project:plec'],
  });
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
    history.pushState({}, '', '/projects/p42');
    window.dispatchEvent(new PopStateEvent('popstate'));
  });
  await initialLoader;
  await actionResponse;
  await expect(page.locator('#project-name')).toHaveText('Project 42');
  expect(projectLoaderResponses).toBe(1);
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
