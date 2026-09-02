import { expect, test } from '@playwright/test';
import { forwardTodoRequest } from '../support/helpers';

// Rename requires the full native-event chain: a real keydown on the edit
// input, a PATCH emitted by Plec, a 2xx response, and the row text updating.
test('rename emits native keydown, PATCH, and updates the row', async ({
  page,
}) => {
  page.setDefaultTimeout(3000);
  page.setDefaultNavigationTimeout(15000);

  let nativeKeydown = false;
  let patchRequest: { url: string; postData: string | null } | null =
    null;
  let patchResponse: { status: number } | null = null;

  page.on('console', (msg) => {
    if (msg.text().includes('[native keydown]')) nativeKeydown = true;
  });
  page.on('request', (request) => {
    if (
      request.method() === 'PATCH' &&
      new URL(request.url()).pathname.startsWith('/api/todos/')
    ) {
      patchRequest = {
        url: request.url(),
        postData: request.postData(),
      };
    }
  });
  page.on('response', (response) => {
    if (
      response.request().method() === 'PATCH' &&
      new URL(response.url()).pathname.startsWith('/api/todos/')
    ) {
      patchResponse = { status: response.status() };
    }
  });

  await page.route(
    (url) => /^\/api\/todos(?:\/[^/]+)?$/.test(url.pathname),
    forwardTodoRequest,
  );

  await page.goto('/todos', { waitUntil: 'domcontentloaded' });
  await expect(
    page.getByRole('heading', { name: 'Todos' }),
  ).toBeVisible();

  const row = page.locator('li[data-runtime-row-key]').first();
  await row.waitFor();
  const rowKey = await row.getAttribute('data-runtime-row-key');
  expect(rowKey, 'No todo row found').toBeTruthy();

  await row.locator('button', { hasText: 'Edit' }).click();
  const rename = row.locator('input:not([type="checkbox"])');
  await rename.waitFor();

  await rename.evaluate((el) => {
    el.addEventListener('keydown', (event) => {
      const key = event as KeyboardEvent;
      console.log(
        '[native keydown]',
        key.key,
        key.code,
        (el as HTMLInputElement).value,
      );
    });
  });

  const renamed = `Renamed ${Date.now()}`;
  await rename.fill(renamed);
  await expect(rename).toHaveValue(renamed);

  await rename.press('Enter');
  await page.waitForTimeout(100);

  expect(nativeKeydown, 'browser never emitted native keydown').toBe(
    true,
  );
  // Read through an explicitly typed local: the assignments happen in event
  // callbacks, which TypeScript's flow analysis cannot see.
  const emitted = patchRequest as {
    url: string;
    postData: string | null;
  } | null;
  expect(
    emitted,
    'native Enter fired but Plec emitted no PATCH request',
  ).not.toBeNull();

  let response = patchResponse as { status: number } | null;
  for (let i = 0; i < 20 && !response; i += 1) {
    await page.waitForTimeout(50);
    response = patchResponse as { status: number } | null;
  }
  expect(
    response,
    'PATCH was emitted but no response returned',
  ).not.toBeNull();
  expect(response!.status).toBeGreaterThanOrEqual(200);
  expect(response!.status).toBeLessThan(300);

  await expect(page.getByText(renamed, { exact: true })).toBeVisible();
});
