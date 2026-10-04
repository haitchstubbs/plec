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
  expect(ssrLogoUrl).toMatch(/^\/assets\/compiled\/[a-f0-9]{24}\.png$/);
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
  const duck = page.locator('#plec-duck-head');
  await expect(duck.locator('img')).toHaveCount(9);
  await expect(
    duck.locator('img[data-duck-yaw="0"][data-duck-pitch="0"]'),
  ).toHaveCSS('opacity', '1');

  const primaryNavigation = page.getByRole('navigation', {
    name: 'Primary navigation',
  });
  await primaryNavigation.getByRole('link', { name: 'About' }).click();
  await expect(duck).toHaveCount(0);
  await primaryNavigation.getByRole('link', { name: 'Home' }).click();
  await expect(page.locator('#plec-duck-head img')).toHaveCount(9);
});
