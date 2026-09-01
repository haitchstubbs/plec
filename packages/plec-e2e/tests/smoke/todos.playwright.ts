import { expect, test } from '@playwright/test';
import { waitForMount } from '../support/helpers';

test('todos renders through SSR and mounts', async ({ page }) => {
  await page.goto('/todos', { waitUntil: 'domcontentloaded' });
  // SSR executes the loader server-side, so the page may adopt directly in
  // the active state or briefly show the client-side pending phase.
  const pending = page.getByText('Loading todos…', { exact: true });
  const heading = page.getByRole('heading', { name: 'Todos' });
  await pending.or(heading).waitFor();
  await waitForMount(page);
  await expect(heading).toBeVisible();
  await expect(pending).toBeHidden();
});
