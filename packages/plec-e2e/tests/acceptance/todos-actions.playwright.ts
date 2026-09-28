import { expect, test } from '@playwright/test';
import {
  forwardTodoRequest,
  waitForMount,
  watch,
} from '../support/helpers';

const sleep = (ms: number) =>
  new Promise((resolve) => setTimeout(resolve, ms));

test('todo create, complete, rename, and delete stay targeted', async ({
  browser,
}) => {
  const context = await browser.newContext();
  const page = await context.newPage();
  const done = watch(page);

  try {
    await page.route(
      (url) => /^\/api\/todos(?:\/[^/]+)?$/.test(url.pathname),
      forwardTodoRequest,
    );
    await page.goto('/todos', { waitUntil: 'domcontentloaded' });
    await waitForMount(page);
    await expect(
      page.getByRole('heading', { name: 'Todos' }),
    ).toBeVisible();

    const search = page.locator('#todo-search');
    await search.fill('missing');
    await expect(page.getByText('Try the Plec Todo API')).toBeHidden();
    await page.reload({ waitUntil: 'domcontentloaded' });
    await waitForMount(page);
    await expect(
      page.getByRole('heading', { name: 'Todos' }),
    ).toBeVisible();

    let post = 'delay';
    await page.route(
      (url) => url.pathname === '/api/todos',
      async (route) => {
        if (route.request().method() === 'POST' && post === 'delay') {
          post = 'done';
          const contentType = route.request().headers()['content-type'];
          const response = await fetch(route.request().url(), {
            method: route.request().method(),
            ...(contentType
              ? { headers: { 'content-type': contentType } }
              : {}),
            body: route.request().postData(),
          });
          await sleep(250);
          await route.fulfill({
            status: response.status,
            contentType:
              response.headers.get('content-type') ?? undefined,
            body: await response.text(),
          });
        } else if (
          route.request().method() === 'POST' &&
          post === 'reject'
        ) {
          post = 'done';
          await route.fulfill({
            status: 500,
            contentType: 'application/json',
            body: '{}',
          });
        } else {
          await forwardTodoRequest(route);
        }
      },
    );
    const title = page.locator('#todo-new-title');
    await title.fill('Acceptance todo');
    await page.getByRole('button', { name: 'Add todo' }).click();
    const adding = page.getByRole('button', { name: 'Adding…' });
    await adding.waitFor();
    expect(await adding.isDisabled(), 'create should be pending').toBe(
      true,
    );
    await expect(page.getByText('Acceptance todo')).toBeVisible();

    post = 'reject';
    await title.fill('Rejected todo');
    await page.getByRole('button', { name: 'Add todo' }).click();
    await expect(
      page.getByText('The Todo API rejected this change.', {
        exact: true,
      }),
    ).toBeVisible();
    expect(
      await page.getByRole('button', { name: 'Add todo' }).isDisabled(),
      'finally should clear pending',
    ).toBe(false);
    await expect(title).toHaveValue('Rejected todo');

    // A later current success clears the public mutation.error value. The
    // `run()` caller is the submit action above: its handled rejection leaves
    // the draft in place while mutation.error publishes the same message.
    post = 'done';
    await page.getByRole('button', { name: 'Add todo' }).click();
    await expect(page.getByText('Rejected todo')).toBeVisible();
    await expect(
      page.getByText('The Todo API rejected this change.', {
        exact: true,
      }),
    ).toHaveCount(0);

    let releaseStaleFailure: (() => void) | undefined;
    let markFirstStarted: (() => void) | undefined;
    const firstStarted = new Promise<void>((resolve) => {
      markFirstStarted = resolve;
    });
    let posts = 0;
    await page.route('**/api/todos', async (route) => {
      if (route.request().method() !== 'POST') {
        await forwardTodoRequest(route);
        return;
      }
      posts += 1;
      if (posts === 1) {
        markFirstStarted?.();
        await new Promise<void>((resolve) => {
          releaseStaleFailure = resolve;
        });
        await route.fulfill({
          status: 500,
          contentType: 'application/json',
          body: '{"private":"stale"}',
        });
      } else {
        await route.fulfill({
          status: 201,
          contentType: 'application/json',
          body: JSON.stringify({
            id: 'current-winner',
            title: 'Current winner',
            completed: false,
          }),
        });
      }
    });
    await title.fill('Stale failure');
    // Dispatch the compiled form event directly to overlap invocations while
    // pending disables the visual submit control.
    await page
      .locator('form')
      .evaluate((form) =>
        form.dispatchEvent(
          new Event('submit', { bubbles: true, cancelable: true }),
        ),
      );
    await firstStarted;
    await title.evaluate((input) => {
      (input as HTMLInputElement).value = 'Current winner';
      input.dispatchEvent(new Event('input', { bubbles: true }));
    });
    await page
      .locator('form')
      .evaluate((form) =>
        form.dispatchEvent(
          new Event('submit', { bubbles: true, cancelable: true }),
        ),
      );
    await expect(
      page.getByText('Current winner', { exact: true }),
    ).toBeVisible();
    releaseStaleFailure?.();
    await expect(
      page.getByText('The Todo API rejected this change.', {
        exact: true,
      }),
    ).toHaveCount(0);
    await expect(
      page.getByRole('button', { name: 'Adding…' }),
    ).toHaveCount(0);

    const rowKey = await page
      .locator('li')
      .filter({ hasText: 'Acceptance todo' })
      .getAttribute('data-runtime-row-key');
    let row = page.locator(`[data-runtime-row-key="${rowKey}"]`);
    await row.locator('input').click();
    // The row renders `Mark {title} open` once completed.
    await expect(
      page.getByRole('checkbox', { name: 'Mark Acceptance todo open' }),
    ).toBeVisible();
    await row.locator('button', { hasText: 'Edit' }).click();
    const rename = row.locator('input:not([type="checkbox"])');
    await rename.fill('Renamed acceptance todo');
    await rename.press('Enter');
    await expect(
      page.getByText('Renamed acceptance todo'),
    ).toBeVisible();

    let patch = 'reject';
    await page.route(
      (url) => /\/api\/todos\/[^/]+$/.test(url.pathname),
      async (route) => {
        if (
          route.request().method() === 'PATCH' &&
          patch === 'reject'
        ) {
          patch = 'done';
          await route.fulfill({
            status: 500,
            contentType: 'application/json',
            body: '{}',
          });
        } else {
          await forwardTodoRequest(route);
        }
      },
    );
    row = page
      .locator('li')
      .filter({ hasText: 'Renamed acceptance todo' });
    await row.locator('button', { hasText: 'Edit' }).click();
    const rejected = row.locator('input:not([type="checkbox"])');
    await rejected.fill('Should not stick');
    await rejected.press('Enter');
    await expect(
      page.getByText('The Todo API rejected this change.', {
        exact: true,
      }),
    ).toBeVisible();
    await expect(
      page.getByText('Renamed acceptance todo'),
    ).toBeVisible();

    row = page
      .locator('li')
      .filter({ hasText: 'Renamed acceptance todo' });
    await row.locator('button', { hasText: 'Delete' }).click();
    await expect(row).toBeHidden();
    await done();
  } finally {
    await context.close();
  }
});
