import { expect, test } from '@playwright/test';

test('stress terminal renders the keyed grid', async ({ page }) => {
  await page.goto('/stress', { waitUntil: 'domcontentloaded' });
  await expect(
    page.getByRole('heading', { name: 'Realtime market terminal' }),
  ).toBeVisible();
  await expect(
    page.locator('button[data-runtime-row-key]'),
  ).toHaveCount(1_000, {
    timeout: 30_000,
  });
});
