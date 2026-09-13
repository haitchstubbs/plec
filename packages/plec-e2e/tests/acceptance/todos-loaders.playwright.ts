import { expect, test } from '@playwright/test';
import {
  forwardTodoRequest,
  waitForMount,
  watch,
} from '../support/helpers';

const sleep = (ms: number) =>
  new Promise((resolve) => setTimeout(resolve, ms));

test('todos loader renders pending then success', async ({
  browser,
}) => {
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
    await page.goto('/todos', { waitUntil: 'domcontentloaded' });
    // SSR executes the loader server-side and transfers the outcome in the
    // snapshot, so adoption may land directly in the active Todos state. Only
    // when no active result transfers does the client loader run behind the
    // delayed interception and render the pending phase first.
    const pending = page.getByText('Loading todos…', { exact: true });
    const heading = page.getByRole('heading', { name: 'Todos' });
    await pending.or(heading).waitFor();
    // Mounted success looks identical on both paths: the active page is
    // interactive and no pending phase remains.
    await waitForMount(page);
    await expect(heading).toBeVisible();
    await expect(pending).toBeHidden();
    await done();
  } finally {
    await context.close();
  }
});

test('todos loader error renders and retries', async ({ browser }) => {
  const context = await browser.newContext();
  const page = await context.newPage();
  const done = watch(page);
  try {
    // SSR executes the route loader server-side, so the failure must be armed
    // on the server instead of intercepting a browser request that only fires
    // after SSR. The one-shot fixture rejects exactly the GET /api/todos the
    // SSR loader performs; the browser adopts the server-rendered error phase
    // from the rejected loader snapshot, and the retry refetches successfully.
    const armed = await page.request.post(
      '/api/acceptance/todo-loader-failure',
    );
    expect(armed.status(), 'acceptance fixture must be enabled').toBe(
      204,
    );
    await page.goto('/todos', { waitUntil: 'domcontentloaded' });
    await expect(
      page.getByText('Could not load todos.', { exact: true }),
    ).toBeVisible();
    // SSR markup is inert until runtime ownership transfers.
    await waitForMount(page);
    await page.getByRole('button', { name: 'Try again' }).click();
    await expect(
      page.getByRole('heading', { name: 'Todos' }),
    ).toBeVisible();
    await done();
  } finally {
    await context.close();
  }
});
