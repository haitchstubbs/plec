import { expect, test } from '@playwright/test';
import { waitForMount, watch } from '../support/helpers';

test('desktop sidebar collapses and restores', async ({ browser }) => {
  const context = await browser.newContext({
    viewport: { width: 1280, height: 720 },
  });
  const page = await context.newPage();
  const done = watch(page);
  try {
    await page.goto('/', { waitUntil: 'domcontentloaded' });
    // SSR markup is interactive only once runtime ownership is live.
    await waitForMount(page);
    await expect(
      page.getByRole('heading', {
        name: 'TSX enters as source. Plec owns the resulting DOM.',
      }),
    ).toBeVisible();
    expect(
      await page
        .getByRole('navigation', { name: 'Breadcrumb' })
        .count(),
      'the persistent layout should render one breadcrumb',
    ).toBe(1);
    expect(
      await page
        .getByRole('navigation', { name: 'Primary navigation' })
        .getByRole('link', { name: 'Runtime stress' })
        .locator('svg')
        .getAttribute('class'),
      'a dynamic icon must receive its className props',
    ).toBe('size-4 shrink-0');

    await page.getByRole('button', { name: 'Toggle sidebar' }).click();
    await expect(
      page.locator('[data-collapsed]').first(),
    ).toHaveAttribute('data-collapsed', 'true');
    await page.getByRole('button', { name: 'Toggle sidebar' }).click();
    await expect(
      page.locator('[data-collapsed]').first(),
    ).toHaveAttribute('data-collapsed', 'false');
    await done();
  } finally {
    await context.close();
  }
});

test('mobile navigation drawer opens', async ({ browser }) => {
  const context = await browser.newContext({
    viewport: { width: 640, height: 720 },
  });
  const page = await context.newPage();
  const done = watch(page);
  try {
    await page.goto('/', { waitUntil: 'domcontentloaded' });
    await waitForMount(page);
    await page
      .getByRole('button', { name: 'Toggle navigation' })
      .click();
    await expect(
      page.locator('[data-mobile-open]').first(),
    ).toHaveAttribute('data-mobile-open', 'true');
    await done();
  } finally {
    await context.close();
  }
});
