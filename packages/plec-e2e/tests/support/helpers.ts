import { expect, type Page, type Route } from '@playwright/test';

export interface PlecPerformanceSnapshot {
  marks: Record<string, number>;
  navigation: { ttfb?: number; htmlReceived?: number };
  fcp?: number;
  lcp?: number;
  durations: {
    clientBootstrap?: number;
    irReceived?: number;
    wasmCompiledInstantiated?: number;
    runtimeMount?: number;
  };
  missing: string[];
}

export interface AdoptionDetail {
  outcome: string;
  mismatchCodes: string[];
  snapshotImported?: boolean;
  [key: string]: unknown;
}

declare global {
  interface Window {
    __plecPerformance?: { snapshot(): PlecPerformanceSnapshot };
    __adoptions?: AdoptionDetail[];
    __plecStressStableRow?: Element;
  }
}

// Proxies an intercepted API request to the real server so route
// interception can observe or delay traffic without stubbing responses.
export async function forwardTodoRequest(route: Route): Promise<void> {
  const request = route.request();
  const contentType = request.headers()['content-type'];
  const response = await fetch(request.url(), {
    method: request.method(),
    ...(contentType
      ? { headers: { 'content-type': contentType } }
      : {}),
    ...(request.postData() ? { body: request.postData() } : {}),
  });
  await route.fulfill({
    status: response.status,
    ...(response.headers.get('content-type')
      ? { contentType: response.headers.get('content-type') }
      : {}),
    body: await response.text(),
  });
}

// SSR markup is inert until runtime ownership transfers; the mount-end mark
// fires after listener installation on both adopt and mount paths.
export async function waitForMount(page: Page): Promise<void> {
  await page.waitForFunction(
    () =>
      window.__plecPerformance?.snapshot().marks['plec:mount-end'] !==
      undefined,
  );
}

// Collects page errors, failed same-origin requests, and fetched graph
// artifacts. The returned assertion must be awaited before the test ends.
export function watch(page: Page): () => Promise<void> {
  const errors: string[] = [];
  const graphs = new Set<string>();
  page.on('pageerror', (error) => errors.push(error.message));
  page.on('requestfailed', (request) => {
    if (request.url().startsWith('http://127.0.0.1')) {
      errors.push(`request failed: ${request.url()}`);
    }
  });
  page.on('response', (response) => {
    const match = /\/graphs\/([^/?]+)\.json/.exec(response.url());
    if (match) graphs.add(match[1]);
  });
  return async () => {
    expect(
      graphs.size,
      'the browser did not fetch graph artifacts',
    ).toBeGreaterThan(0);
    expect(errors, errors.join('\n')).toEqual([]);
  };
}

// Installs in the page before any app code so adoption events are recorded.
export const trackAdoptions = () => {
  window.__adoptions = [];
  window.addEventListener('plec:adoption', (event) => {
    window.__adoptions!.push((event as CustomEvent).detail);
  });
};

export async function lastAdoption(
  page: Page,
): Promise<AdoptionDetail> {
  await page.waitForFunction(
    () => (window.__adoptions ?? []).length > 0,
    undefined,
    { timeout: 15_000 },
  );
  return page.evaluate(
    () => window.__adoptions![window.__adoptions!.length - 1],
  );
}
