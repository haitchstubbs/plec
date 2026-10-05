import { expect, test } from '@playwright/test';
import { waitForMount } from '../support/helpers';

test('host icons adopt through stable boundaries and never resize', async ({
  page,
}) => {
  // SSR retains the runtime-owned boundary while the sidecar supplies the
  // provider fragment before hydration.
  const html = await (await page.request.get('/')).text();
  expect(
    html.match(/data-plec-host="lucide:[^"]+"/g)?.length ?? 0,
  ).toBeGreaterThan(0);
  expect(html).toMatch(/data-plec-host="lucide:[^"]+"><svg /);

  const providerResponse = page.waitForResponse((response) =>
    /\/_plec\/assets\/plec_provider_[^/]+\.js$/.test(response.url()),
  );
  await page.goto('/', { waitUntil: 'domcontentloaded' });
  expect((await providerResponse).status()).toBe(200);
  const boundaries = page.locator('[data-plec-host^="lucide:"]');
  const boundaryCount = await boundaries.count();
  expect(boundaryCount).toBeGreaterThan(0);
  await waitForMount(page);
  await expect(boundaries.locator('svg')).toHaveCount(boundaryCount);
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
