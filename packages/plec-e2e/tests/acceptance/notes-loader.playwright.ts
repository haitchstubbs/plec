import { expect, test } from '@playwright/test';
import { waitForMount, watch } from '../support/helpers';

// Client-side navigation to a loader route must seed the fetched outcome
// into the freshly mounted graph. The hard-load path transfers the loader
// outcome through the SSR snapshot (covered by the adoption suite); this
// test pins the fresh-navigation path where the client loader runs against
// the live normal graph with no pending phase in between.
test('client navigation to a loader route renders the loader data', async ({
  browser,
}) => {
  const context = await browser.newContext();
  const page = await context.newPage();
  const done = watch(page);
  let loaderResponses = 0;
  page.on('response', (response) => {
    if (
      new URL(response.url()).pathname === '/api/notes' &&
      response.request().method() === 'GET' &&
      response.ok()
    ) {
      loaderResponses += 1;
    }
  });
  try {
    await page.goto('/', { waitUntil: 'networkidle' });
    await waitForMount(page);
    await page.evaluate(() => {
      (window as unknown as { __navProbe?: number }).__navProbe = 42;
    });

    await page.getByRole('link', { name: 'Notes' }).click();
    const headline = page.getByRole('heading', { name: /server/i });
    await headline.waitFor({ timeout: 10_000 });

    expect(
      await page.evaluate(
        () => (window as unknown as { __navProbe?: number }).__navProbe,
      ),
      'the click must stay inside the SPA: a reload means the router never intercepted it',
    ).toBe(42);

    expect(
      loaderResponses,
      'the client loader must fetch /api/notes on fresh navigation',
    ).toBeGreaterThan(0);
    await expect(headline).toBeVisible();
    await expect(
      page.getByText('Could not load notes.', { exact: true }),
    ).toBeHidden();
    await done();
  } finally {
    await context.close();
  }
});
