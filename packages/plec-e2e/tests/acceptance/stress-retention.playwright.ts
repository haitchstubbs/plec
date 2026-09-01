import { expect, test } from '@playwright/test';
import { watch } from '../support/helpers';

test('untouched keyed rows retain their DOM node', async ({
  browser,
}) => {
  const context = await browser.newContext({
    viewport: { width: 1440, height: 900 },
  });
  const page = await context.newPage();
  const done = watch(page);
  try {
    await page.goto('/stress', { waitUntil: 'domcontentloaded' });
    await expect(
      page.getByRole('heading', { name: 'Realtime market terminal' }),
    ).toBeVisible();
    await expect(
      page.locator('button[data-runtime-row-key]'),
    ).toHaveCount(1_000);
    const stableRow = page.locator(
      'button[data-runtime-row-key="PX0001"]',
    );
    await stableRow.evaluate((node) => {
      window.__plecStressStableRow = node;
    });
    await page.waitForTimeout(350);
    expect(
      await stableRow.evaluate(
        (node) => window.__plecStressStableRow === node,
      ),
      'an untouched keyed row should retain its DOM node',
    ).toBe(true);
    await stableRow.click();
    await expect(
      page.getByText('Selected instrument', { exact: true }),
    ).toBeVisible();
    await expect(
      page.getByText('PX0001', { exact: true }).last(),
    ).toBeVisible();
    const telemetry = await page
      .locator('aside')
      .filter({ hasText: 'Runtime telemetry' })
      .innerText();
    expect(telemetry).toMatch(
      /DOM operations\s+[1-9]/,
      'the developer panel should report targeted runtime work',
    );
    await done();
  } finally {
    await context.close();
  }
});
