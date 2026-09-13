import { expect, test } from '@playwright/test';
import {
  lastAdoption,
  trackAdoptions,
  waitForMount,
  watch,
} from '../support/helpers';

test('desktop sidebar collapses and restores', async ({ browser }) => {
  const context = await browser.newContext({
    viewport: { width: 1280, height: 720 },
  });
  const page = await context.newPage();
  const done = watch(page);
  try {
    await page.goto('/', { waitUntil: 'domcontentloaded' });
    // SSR markup is interactive only once runtime ownership is live.
    await waitForMount(page);
    await expect(
      page.getByRole('heading', {
        name: 'TSX enters as source. Plec owns the resulting DOM.',
      }),
    ).toBeVisible();
    expect(
      await page
        .getByRole('navigation', { name: 'Breadcrumb' })
        .count(),
      'the persistent layout should render one breadcrumb',
    ).toBe(1);
    expect(
      await page
        .getByRole('navigation', { name: 'Primary navigation' })
        .getByRole('link', { name: 'Runtime stress' })
        .locator('svg')
        .getAttribute('class'),
      'a dynamic icon must receive its className props',
    ).toBe('size-4 shrink-0');

    await page.getByRole('button', { name: 'Toggle sidebar' }).click();
    await expect(
      page.locator('[data-collapsed]').first(),
    ).toHaveAttribute('data-collapsed', 'true');
    await page.getByRole('button', { name: 'Toggle sidebar' }).click();
    await expect(
      page.locator('[data-collapsed]').first(),
    ).toHaveAttribute('data-collapsed', 'false');
    await done();
  } finally {
    await context.close();
  }
});

test('sidebar links stay client-side across overlapping graph swaps', async ({
  browser,
}) => {
  const context = await browser.newContext({
    viewport: { width: 1280, height: 720 },
  });
  const page = await context.newPage();
  const done = watch(page);
  try {
    await page.addInitScript(trackAdoptions);
    await page.goto('/', { waitUntil: 'domcontentloaded' });
    await waitForMount(page);
    expect((await lastAdoption(page)).outcome).toBe('adopted');

    let releaseFirstGraph!: () => void;
    const firstGraphReleased = new Promise<void>((resolve) => {
      releaseFirstGraph = resolve;
    });
    let firstGraphRequested!: () => void;
    const firstGraphRequest = new Promise<void>((resolve) => {
      firstGraphRequested = resolve;
    });
    let firstGraphContinued!: () => void;
    const firstGraphFinished = new Promise<void>((resolve) => {
      firstGraphContinued = resolve;
    });
    let delayNextGraph = true;
    await page.route('**/graphs/*.json*', async (route) => {
      if (delayNextGraph) {
        delayNextGraph = false;
        firstGraphRequested();
        await firstGraphReleased;
        await route.continue();
        firstGraphContinued();
        return;
      }
      await route.continue();
    });

    const documentRequests: string[] = [];
    page.on('request', (request) => {
      if (
        request.isNavigationRequest() &&
        request.frame() === page.mainFrame()
      )
        documentRequests.push(request.url());
    });
    await page.evaluate(() => {
      (
        window as Window & { __plecLinkDefaults?: boolean[] }
      ).__plecLinkDefaults = [];
      window.addEventListener('click', (event) => {
        const target = event.target;
        if (target instanceof Element && target.closest('a[href]'))
          (
            window as Window & { __plecLinkDefaults?: boolean[] }
          ).__plecLinkDefaults!.push(event.defaultPrevented);
      });
    });

    const navigation = page.getByRole('navigation', {
      name: 'Primary navigation',
    });
    const sidebarInset = await page
      .locator('.plec-sidebar-inset')
      .elementHandle();
    expect(sidebarInset).not.toBeNull();

    await navigation.getByRole('link', { name: 'About' }).click();
    await firstGraphRequest;
    await navigation.getByRole('link', { name: 'Todos' }).click();
    await expect(page).toHaveURL(/\/todos$/);
    await expect(
      page.getByRole('heading', { name: 'Todos', exact: true }),
    ).toBeVisible();

    releaseFirstGraph();
    await firstGraphFinished;
    await expect(
      page.getByRole('heading', { name: 'Todos', exact: true }),
    ).toBeVisible();
    expect(documentRequests).toEqual([]);
    expect(
      await page.evaluate(
        () =>
          (window as Window & { __plecLinkDefaults?: boolean[] })
            .__plecLinkDefaults,
      ),
    ).toEqual([true, true]);
    expect(
      await sidebarInset!.evaluate((element) => element.isConnected),
    ).toBe(true);
    await done();
  } finally {
    await context.close();
  }
});

test('mobile navigation drawer opens', async ({ browser }) => {
  const context = await browser.newContext({
    viewport: { width: 640, height: 720 },
  });
  const page = await context.newPage();
  const done = watch(page);
  try {
    await page.goto('/', { waitUntil: 'domcontentloaded' });
    await waitForMount(page);
    await page
      .getByRole('button', { name: 'Toggle navigation' })
      .click();
    await expect(
      page.locator('[data-mobile-open]').first(),
    ).toHaveAttribute('data-mobile-open', 'true');
    await done();
  } finally {
    await context.close();
  }
});
