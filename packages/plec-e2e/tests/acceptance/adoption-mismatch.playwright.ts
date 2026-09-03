import {
  expect,
  test,
  type Browser,
  type Page,
  type Route,
} from '@playwright/test';
import {
  lastAdoption,
  trackAdoptions,
  type AdoptionDetail,
} from '../support/helpers';

// The fail-closed half of the adoption contract: every structural mismatch
// between the served HTML/bootstrap and the runtime's expectation must end
// in a fallback remount with the exact diagnostic code, never a silent
// success. Mismatches are induced by rewriting the served document before
// the browser ever executes it, so the server itself stays untouched.

const BOOTSTRAP_OPEN =
  '<script id="plec-bootstrap" type="application/json">';

async function interceptedPage(
  browser: Browser,
  path: string,
  transform: (html: string) => string,
): Promise<{
  context: Awaited<ReturnType<Browser['newContext']>>;
  page: Page;
  adoption: AdoptionDetail;
}> {
  const context = await browser.newContext({
    viewport: { width: 1280, height: 720 },
  });
  const page = await context.newPage();
  await page.route('**/*', async (route: Route) => {
    if (route.request().resourceType() !== 'document')
      return route.fallback();
    const response = await route.fetch();
    await route.fulfill({
      response,
      body: transform(await response.text()),
      contentType:
        response.headers()['content-type'] ??
        'text/html; charset=utf-8',
    });
  });
  await page.addInitScript(trackAdoptions);
  await page.goto(path, { waitUntil: 'networkidle' });
  const adoption = await lastAdoption(page);
  return { context, page, adoption };
}

/** Rewrites the `#plec-bootstrap` JSON payload inside the served HTML. */
function rewriteBootstrap(
  html: string,
  mutate: (payload: { version: number; snapshot: any }) => void,
): string {
  const start = html.indexOf(BOOTSTRAP_OPEN);
  expect(
    start,
    'the served document must carry a bootstrap script',
  ).toBeGreaterThan(-1);
  const openEnd = start + BOOTSTRAP_OPEN.length;
  const end = html.indexOf('</script>', openEnd);
  const payload = JSON.parse(html.slice(openEnd, end));
  mutate(payload);
  return (
    html.slice(0, openEnd) + JSON.stringify(payload) + html.slice(end)
  );
}

/** Replaces the bootstrap payload body wholesale (used to make it unparseable). */
function replaceBootstrapBody(html: string, body: string): string {
  const start = html.indexOf(BOOTSTRAP_OPEN);
  const openEnd = start + BOOTSTRAP_OPEN.length;
  const end = html.indexOf('</script>', openEnd);
  return html.slice(0, openEnd) + body + html.slice(end);
}

async function fallbackRemounted(page: Page): Promise<void> {
  // Fail-closed means the destructive fallback actually ran. With one
  // canonical address grammar (docs/dom-address-protocol.md) both SSR and
  // CSR DOM carry `data-plec-node`, so provenance is instead visible in the
  // text-marker contract: only server rendering emits `plec:text` comment
  // markers, and the fallback remount wipes them. Their absence plus live
  // canonical addresses proves the client mount owns the DOM.
  await page.waitForFunction(
    () => {
      const app = document.querySelector('#app')!;
      return (
        app.querySelectorAll('[data-plec-node]').length > 0 &&
        !app.innerHTML.includes('<!--plec:text:')
      );
    },
    undefined,
    { timeout: 15_000 },
  );
}

test('stale snapshot revision fails closed with stale-revision', async ({
  browser,
}) => {
  const { context, page, adoption } = await interceptedPage(
    browser,
    '/',
    (html) =>
      rewriteBootstrap(html, (payload) => {
        payload.snapshot.revision = 'stale-revision-probe';
      }),
  );
  try {
    expect(adoption.outcome, JSON.stringify(adoption)).toBe('fallback');
    expect(adoption.mismatchCodes).toEqual(['stale-revision']);
    expect(adoption.observedRevision).toBe('stale-revision-probe');
    await fallbackRemounted(page);
  } finally {
    await context.close();
  }
});

test('route-chain disagreement fails closed with the chain detail', async ({
  browser,
}) => {
  const { context, page, adoption } = await interceptedPage(
    browser,
    '/',
    (html) =>
      rewriteBootstrap(html, (payload) => {
        payload.snapshot.routes[0].routeId = 'ghost#Route';
      }),
  );
  try {
    expect(adoption.outcome, JSON.stringify(adoption)).toBe('fallback');
    expect(adoption.mismatchCodes).toEqual([
      'mismatch:ssr-route-chain:unknown-route:ghost#Route',
    ]);
    await fallbackRemounted(page);
  } finally {
    await context.close();
  }
});

test('invalid snapshot version fails closed in the WASM gate', async ({
  browser,
}) => {
  // The bootstrap stays a v2 payload (so the browser glue hands the
  // snapshot to WASM), but the snapshot itself claims an unknown version.
  const { context, page, adoption } = await interceptedPage(
    browser,
    '/',
    (html) =>
      rewriteBootstrap(html, (payload) => {
        payload.snapshot.version = 999;
      }),
  );
  try {
    expect(adoption.outcome, JSON.stringify(adoption)).toBe('fallback');
    expect(adoption.mismatchCodes).toEqual([
      'unsupported:ssr-snapshot-version',
    ]);
    await fallbackRemounted(page);
  } finally {
    await context.close();
  }
});

test('missing ownership marker fails closed with missing:ssr-node', async ({
  browser,
}) => {
  const { context, page, adoption } = await interceptedPage(
    browser,
    '/',
    // Strip the first element's ownership attribute: the claim walk must
    // refuse to attach runtime ownership to an unmarked node.
    (html) => html.replace(/ data-plec-node="[^"]+"/, ''),
  );
  try {
    expect(adoption.outcome, JSON.stringify(adoption)).toBe('fallback');
    expect(adoption.mismatchCodes).toHaveLength(1);
    expect(adoption.mismatchCodes[0]).toMatch(/^missing:ssr-node:/);
    await fallbackRemounted(page);
  } finally {
    await context.close();
  }
});

test('duplicated text marker fails closed instead of hijacking a claim', async ({
  browser,
}) => {
  const { context, page, adoption } = await interceptedPage(
    browser,
    '/',
    (html) => html.replace(/(<!--plec:text:[^>]+-->)/, '$1$1'),
  );
  try {
    expect(adoption.outcome, JSON.stringify(adoption)).toBe('fallback');
    expect(adoption.mismatchCodes).toHaveLength(1);
    expect(adoption.mismatchCodes[0]).toMatch(
      /^duplicate:ssr-marker:plec:text:/,
    );
    await fallbackRemounted(page);
  } finally {
    await context.close();
  }
});

test('element injected between a text marker and its value fails closed with adjacency:ssr-text', async ({
  browser,
}) => {
  // Text-marker adjacency contract (docs/dom-address-protocol.md): the
  // marker claims the node that immediately follows it. An injected element
  // between the two must fail the claim closed — synthesizing around it
  // would leave the served text stale next to a rewritten sink. The
  // row-level variant of this claim (`adopt_row_node`, diagnostic
  // `adjacency:ssr-row-text`) is covered by the Rust wasm suite in
  // crates/plec-runtime/tests/typed_events.rs: the demo app renders every
  // SSR row through a nested component or fills collections client-side,
  // so no served document reaches that claim path.
  const { context, page, adoption } = await interceptedPage(
    browser,
    '/',
    (html) =>
      html.replace(/(<!--plec:text:[^>]+-->)([^<])/, '$1<i></i>$2'),
  );
  try {
    expect(adoption.outcome, JSON.stringify(adoption)).toBe('fallback');
    expect(adoption.mismatchCodes).toHaveLength(1);
    expect(adoption.mismatchCodes[0]).toMatch(/^adjacency:ssr-text:/);
    await fallbackRemounted(page);
  } finally {
    await context.close();
  }
});

test('unparseable bootstrap fails closed with invalid:ssr-bootstrap', async ({
  browser,
}) => {
  const { context, page, adoption } = await interceptedPage(
    browser,
    '/',
    (html) => replaceBootstrapBody(html, '{not json'),
  );
  try {
    expect(adoption.outcome, JSON.stringify(adoption)).toBe('fallback');
    expect(adoption.mismatchCodes).toEqual(['invalid:ssr-bootstrap']);
    await fallbackRemounted(page);
  } finally {
    await context.close();
  }
});

test('binding divergence is allowed, recomputed, and reported', async ({
  browser,
}) => {
  const { context, page, adoption } = await interceptedPage(
    browser,
    '/',
    (html) => {
      // Tamper the server-rendered value of the location binding inside
      // #ssr-request. The paragraph's children are marker+text pairs: the
      // static "Requested" text (never re-evaluated), then the pathname
      // binding, then the search binding. The pathname binding is the
      // second marker; tampering its value must make the snapshot-backed
      // recompute diverge and be reported.
      const idAt = html.indexOf('id="ssr-request"');
      if (idAt < 0) return html;
      const firstMarker = html.indexOf('<!--plec:text:', idAt);
      if (firstMarker < 0) return html;
      const markerAt = html.indexOf('<!--plec:text:', firstMarker + 1);
      if (markerAt < 0) return html;
      const valueStart =
        html.indexOf('-->', markerAt + '<!--plec:text:'.length) + 3;
      const valueEnd = html.indexOf('<', valueStart);
      if (valueEnd < 0) return html;
      return (
        html.slice(0, valueStart) +
        '/tampered-probe' +
        html.slice(valueEnd)
      );
    },
  );
  try {
    expect(adoption.outcome, JSON.stringify(adoption)).toBe('adopted');
    expect(adoption.mismatchCodes).toEqual([]);
    expect(
      adoption.snapshotImported,
      'divergence reporting requires the imported snapshot',
    ).toBe(true);
    expect(
      adoption.textDivergences,
      `text divergences: ${JSON.stringify(adoption)}`,
    ).toBeGreaterThanOrEqual(1);
    // Recompute-consequences win: the tampered server value is replaced by
    // the client-evaluated one in place.
    expect(await page.locator('#ssr-request').textContent()).toBe(
      'Requested/',
    );
  } finally {
    await context.close();
  }
});

test('whitespace injected between a text marker and its value merges into the claimed node and self-heals', async ({
  browser,
}) => {
  // Bare injected whitespace is contiguous with the served text, so the
  // HTML parser merges it into one text node and the claim still lands on
  // the right node. The adjacency contract covers this explicitly: the
  // snapshot-backed recompute rewrites the merged data, so the visible
  // value is exact even though the served bytes were mangled. Only a
  // non-text node between marker and value breaks the claim (fail-closed).
  const { context, page, adoption } = await interceptedPage(
    browser,
    '/',
    (html) => {
      const idAt = html.indexOf('id="ssr-request"');
      if (idAt < 0) return html;
      const firstMarker = html.indexOf('<!--plec:text:', idAt);
      if (firstMarker < 0) return html;
      const markerAt = html.indexOf('<!--plec:text:', firstMarker + 1);
      if (markerAt < 0) return html;
      const markerEnd = html.indexOf('-->', markerAt) + 3;
      return html.slice(0, markerEnd) + '\n  ' + html.slice(markerEnd);
    },
  );
  try {
    expect(adoption.outcome, JSON.stringify(adoption)).toBe('adopted');
    expect(
      adoption.textDivergences,
      `text divergences: ${JSON.stringify(adoption)}`,
    ).toBeGreaterThanOrEqual(1);
    expect(await page.locator('#ssr-request').textContent()).toBe(
      'Requested/',
    );
  } finally {
    await context.close();
  }
});

test('an empty server text value adopts through marker synthesis', async ({
  browser,
}) => {
  // The search binding renders '' at '/': the server serializes the marker
  // followed by the empty-comment sentinel, and adoption synthesizes the
  // text node between the two so the binding owns a sink from the start.
  const { context, page, adoption } = await interceptedPage(
    browser,
    '/',
    (html) => html,
  );
  try {
    expect(adoption.outcome, JSON.stringify(adoption)).toBe('adopted');
    expect(adoption.mismatchCodes).toEqual([]);
    const afterMarker = await page.evaluate(() => {
      const paragraph = document.querySelector('#ssr-request')!;
      const children = Array.from(paragraph.childNodes);
      let lastMarker = -1;
      children.forEach((node, index) => {
        if (
          node.nodeType === Node.COMMENT_NODE &&
          (node.nodeValue ?? '').startsWith('plec:text:')
        ) {
          lastMarker = index;
        }
      });
      return children.slice(lastMarker + 1).map((node) => ({
        type: node.nodeType,
        data: node.nodeValue ?? '',
      }));
    });
    expect(
      afterMarker,
      'marker must be followed by the synthesized empty text node and the sentinel',
    ).toEqual([
      { type: 3, data: '' },
      { type: 8, data: '' },
    ]);
  } finally {
    await context.close();
  }
});

test('binding writes target the text node synthesized for an empty server value', async ({
  browser,
}) => {
  // Stripping the served counter value leaves its marker with no text node
  // at all (the value was not empty, so no sentinel was emitted): adoption
  // synthesizes the node, the snapshot recompute seeds the evaluated value
  // into it, and the later click writes land in that same synthesized node.
  // The surgery targets the binding's marker (the last one in the button),
  // not the static "SSR counter: " text's own marker.
  const { context, page, adoption } = await interceptedPage(
    browser,
    '/',
    (html) => {
      const idAt = html.indexOf('id="ssr-counter"');
      if (idAt < 0) return html;
      const buttonEnd = html.indexOf('</button>', idAt);
      if (buttonEnd < 0) return html;
      const markerAt = html.lastIndexOf('<!--plec:text:', buttonEnd);
      if (markerAt < 0) return html;
      const valueStart = html.indexOf('-->', markerAt) + 3;
      const valueEnd = html.indexOf('<', valueStart);
      if (valueEnd < 0 || valueEnd > buttonEnd) return html;
      return html.slice(0, valueStart) + html.slice(valueEnd);
    },
  );
  try {
    expect(adoption.outcome, JSON.stringify(adoption)).toBe('adopted');
    expect(adoption.snapshotImported, JSON.stringify(adoption)).toBe(
      true,
    );
    // The recompute seeded the synthesized node with the evaluated value,
    // and the click's binding write landed in that same synthesized node —
    // the runtime holds only the direct Text reference, so this proves the
    // write targeted the synthesis, not a marker re-resolution. (The
    // compiler elides the trailing space before the expression, so the
    // static prefix is 'SSR counter:'.)
    expect(await page.locator('#ssr-counter').textContent()).toMatch(
      /^SSR counter:\s?0$/,
    );
    await page.locator('#ssr-counter').click();
    expect(await page.locator('#ssr-counter').textContent()).toMatch(
      /^SSR counter:\s?1$/,
    );
  } finally {
    await context.close();
  }
});

test('cookie host slots stay server-gated and never leak into markup', async ({
  browser,
}) => {
  const context = await browser.newContext();
  const page = await context.newPage();
  let gatingHeader: string | null = null;
  let documentBody = '';
  page.on('response', async (response) => {
    if (response.request().resourceType() !== 'document') return;
    gatingHeader = response.headers()['x-plec-ssr-gating'] ?? null;
    documentBody = await response.text();
  });
  await page.addInitScript(trackAdoptions);
  await page.goto('/', { waitUntil: 'networkidle' });
  const adoption = await lastAdoption(page);
  try {
    // The layout reads `cookie.getSync('sidebar_state')`, so the dev-only
    // header must name the gated load...
    expect(
      gatingHeader,
      'the SSR gating header must record the cookie load',
    ).toContain('cookie:sidebar_state');
    // ...and the cookie name must appear nowhere in the served document:
    // neither in markup nor in the bootstrap JSON.
    expect(
      documentBody,
      'cookie names must not leak into the document',
    ).not.toContain('sidebar_state');
    // Adoption itself is unaffected by the gating.
    expect(adoption.outcome, JSON.stringify(adoption)).toBe('adopted');
    expect(adoption.mismatchCodes).toEqual([]);
  } finally {
    await context.close();
  }
});
