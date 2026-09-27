import { expect, test } from '@playwright/test';
import { waitForMount, watch } from '../support/helpers';

// Loader terminal outcomes: redirects and not-found. SSR and client
// navigation must agree: a loader redirect never renders the origin route,
// a not-found outcome renders the boundary owner's notFoundComponent, and
// the boundary owner truncates everything below it.

test('SSR loader redirect answers 307 and the destination document adopts', async ({
  browser,
}) => {
  const context = await browser.newContext();
  const page = await context.newPage();
  const done = watch(page, { requireGraph: false });
  try {
    // The server resolves the loader redirect as one 307 hop to /notes
    // (it follows hops internally only to bound loops).
    const response = await page.request.get('/admin', {
      maxRedirects: 0,
    });
    expect(response.status()).toBe(307);
    expect(response.headers()['location']).toBe('/notes');
  } finally {
    await context.close();
  }

  // A real browser navigation lands on the notes document with the URL
  // updated and the admin route never rendered.
  const context2 = await browser.newContext();
  const page2 = await context2.newPage();
  const done2 = watch(page2, { requireGraph: false });
  try {
    await page2.goto('/admin', { waitUntil: 'networkidle' });
    await waitForMount(page2);
    await expect(page2).toHaveURL(/\/notes$/);
    await expect(
      page2.getByRole('heading', {
        name: /notes transferred from the server/i,
      }),
    ).toBeVisible();
    await expect(
      page2.getByRole('heading', { name: 'Admin' }),
    ).toHaveCount(0);
    await done2();
  } finally {
    await context2.close();
  }
  await done();
});

test('client navigation redirect swaps routes without committing the origin', async ({
  browser,
}) => {
  const context = await browser.newContext();
  const page = await context.newPage();
  const done = watch(page, { requireGraph: false });
  let notesResponses = 0;
  page.on('response', (response) => {
    if (
      new URL(response.url()).pathname === '/api/notes' &&
      response.ok()
    ) {
      notesResponses += 1;
    }
  });
  try {
    await page.goto('/', { waitUntil: 'networkidle' });
    await waitForMount(page);
    await page.evaluate(() => {
      (window as unknown as { __navProbe?: number }).__navProbe = 42;
    });

    await page.getByRole('link', { name: 'Admin' }).click();
    await expect(page).toHaveURL(/\/notes$/, { timeout: 10_000 });
    await expect(
      page.getByRole('heading', {
        name: /notes transferred from the server/i,
      }),
    ).toBeVisible({
      timeout: 10_000,
    });
    // The redirect performed a client-side navigation (no reload) and the
    // destination loader ran.
    expect(
      await page.evaluate(
        () => (window as unknown as { __navProbe?: number }).__navProbe,
      ),
    ).toBe(42);
    expect(notesResponses).toBeGreaterThan(0);
    await expect(
      page.getByText('Admin page', { exact: true }),
    ).toHaveCount(0);
    await done();
  } finally {
    await context.close();
  }
});

test('SSR not-found renders the boundary owner with a 404 status', async ({
  browser,
}) => {
  const context = await browser.newContext();
  const page = await context.newPage();
  const done = watch(page, { requireGraph: false });
  try {
    const response = await page.goto('/projects/missing-project', {
      waitUntil: 'domcontentloaded',
    });
    expect(response!.status()).toBe(404);
    await expect(page.getByTestId('project-not-found')).toBeVisible();
    // The origin route's normal page never rendered.
    await expect(
      page.getByText('A parameterized Plec route.'),
    ).toHaveCount(0);
  } finally {
    await context.close();
  }
  await done();
});

test('a known project renders normally while a missing one resolves not found', async ({
  browser,
}) => {
  const context = await browser.newContext();
  const page = await context.newPage();
  const done = watch(page);
  try {
    await page.goto('/projects/plec', { waitUntil: 'networkidle' });
    await waitForMount(page);
    await expect(page.getByTestId('project-not-found')).toHaveCount(0);

    // Client navigation into the missing project swaps to the boundary
    // graph in place, without a reload and without committing the origin.
    await page.evaluate(() => {
      (window as unknown as { __navProbe?: number }).__navProbe = 42;
    });
    await page.getByRole('link', { name: /missing project/i }).click();
    await expect(page).toHaveURL(/\/projects\/ghost$/, {
      timeout: 10_000,
    });
    await expect(page.getByTestId('project-not-found')).toBeVisible({
      timeout: 10_000,
    });
    expect(
      await page.evaluate(
        () => (window as unknown as { __navProbe?: number }).__navProbe,
      ),
      'the not-found boundary must resolve inside the SPA',
    ).toBe(42);
  } finally {
    await context.close();
  }
  await done();
});
