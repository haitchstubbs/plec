import { expect, test } from '@playwright/test';
import { waitForMount } from '../support/helpers';

test('home renders through SSR and mounts', async ({ page }) => {
  const compiledAssetResponse = page.waitForResponse((response) =>
    response.url().includes('/assets/compiled/'),
  );
  await page.goto('/', { waitUntil: 'domcontentloaded' });
  expect((await compiledAssetResponse).status()).toBe(200);
  const logo = page.locator('#compiled-asset-logo');
  const ssrLogoUrl = await logo.getAttribute('src');
  expect(ssrLogoUrl).toMatch(/^\/assets\/compiled\/[a-f0-9]{24}\.svg$/);
  await waitForMount(page);
  await expect(logo).toHaveAttribute('src', ssrLogoUrl!);
  await expect(logo).toHaveJSProperty('complete', true);
  await expect(
    page.getByRole('heading', {
      name: 'TSX enters as source. Plec owns the resulting DOM.',
    }),
  ).toBeVisible();
  await expect(
    page.getByRole('navigation', { name: 'Breadcrumb' }),
  ).toHaveCount(1);
});
