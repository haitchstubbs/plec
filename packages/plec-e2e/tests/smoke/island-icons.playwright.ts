import { expect, test } from '@playwright/test';
import { waitForMount } from '../support/helpers';

test('host icons adopt through stable boundaries and never resize', async ({
  page,
}) => {
  // SSR owns the host boundary, not the provider-owned SVG descendants.
  const html = await (await page.request.get('/')).text();
  expect(html.match(/data-plec-host="lucide:[^"]+"/g)?.length ?? 0).toBeGreaterThan(0);

  await page.goto('/', { waitUntil: 'domcontentloaded' });
  expect(await page.locator('[data-plec-host^="lucide:"]').count()).toBeGreaterThan(0);
  await waitForMount(page);
  expect(await page.locator('svg').count()).toBeGreaterThan(0);
  const measure = () =>
    page.evaluate(() =>
      Array.from(document.querySelectorAll('svg')).map(
        (svg) => svg.getBoundingClientRect().width,
      ),
    );
  const firstPaint = await measure();
  expect(await measure()).toEqual(firstPaint);
  // Responsive variants render display:none, so zero widths are fine; at
  // least one icon must actually paint, at its final size.
  expect(firstPaint.some((width) => width > 0)).toBe(true);
});
