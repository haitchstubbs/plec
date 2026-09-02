import { expect, test } from '@playwright/test';
import { waitForMount } from '../support/helpers';

test('island svg icons paint their class before adoption and never resize', async ({
  page,
}) => {
  // SSR must serialize the island component's className onto each icon so
  // the first paint already carries its final CSS size (issue r0l).
  const html = await (await page.request.get('/')).text();
  const svgs = html.match(/<svg\b[^>]*>/g) ?? [];
  expect(svgs.length).toBeGreaterThan(0);
  for (const svg of svgs) expect(svg, svg).toContain('class="');

  await page.goto('/', { waitUntil: 'domcontentloaded' });
  const measure = () =>
    page.evaluate(() =>
      Array.from(document.querySelectorAll('svg')).map(
        (svg) => svg.getBoundingClientRect().width,
      ),
    );
  const firstPaint = await measure();
  await waitForMount(page);
  expect(await measure()).toEqual(firstPaint);
  // Responsive variants render display:none, so zero widths are fine; at
  // least one icon must actually paint, at its final size.
  expect(firstPaint.some((width) => width > 0)).toBe(true);
});
