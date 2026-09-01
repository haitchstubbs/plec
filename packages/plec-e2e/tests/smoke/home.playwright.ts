import { expect, test } from '@playwright/test';
import { waitForMount } from '../support/helpers';

test('home renders through SSR and mounts', async ({ page }) => {
  await page.goto('/', { waitUntil: 'domcontentloaded' });
  await waitForMount(page);
  await expect(
    page.getByRole('heading', {
      name: 'TSX enters as source. Plec owns the resulting DOM.',
    }),
  ).toBeVisible();
  await expect(
    page.getByRole('navigation', { name: 'Breadcrumb' }),
  ).toHaveCount(1);
});
