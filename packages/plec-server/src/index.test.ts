import { once } from 'node:events';
import { mkdtemp, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { createPlecServer } from './index';

const servers: ReturnType<typeof createPlecServer>[] = [];
afterEach(async () =>
  Promise.all(
    servers
      .splice(0)
      .map(
        (server) =>
          new Promise<void>((resolve) => server.close(() => resolve())),
      ),
  ),
);

it('renders a route artifact with document metadata and public request location', async () => {
  const publicDir = await mkdtemp(path.join(tmpdir(), 'plec-server-'));
  const component = (tag: string, text?: string) => ({
    rootComponent: 0,
    components: [
      {
        rootNode: 0,
        strings: [tag],
        constants: [],
        nodes: [
          { op: 'element', tag: 0, children: text ? [1] : [] },
          ...(text ? [{ op: 'text', text: 0 }] : []),
        ],
        texts: text ? [{ value: text }] : [],
        bindings: [],
        propPrograms: [],
        stateSlots: [],
        parameters: [],
        expressions: [],
        loops: [],
        routeOutlets: tag === 'main' ? [{ id: 'main', node: 0 }] : [],
      },
    ],
  });
  await writeFile(
    path.join(publicDir, 'route-artifact.json'),
    JSON.stringify({
      manifest: {
        revision: 'test-revision',
        rootGraphId: 'root',
        routes: [
          {
            id: 'home',
            path: '',
            graphId: 'home',
            outletId: 'main',
            meta: {
              title: 'Home title',
              description: 'Home description',
            },
          },
        ],
      },
      graphs: [
        { graphId: 'root', graph: component('main') },
        { graphId: 'home', graph: component('p', 'Server rendered') },
      ],
    }),
  );
  const server = createPlecServer({
    publicDir,
    artifactPath: path.join(publicDir, 'route-artifact.json'),
  });
  servers.push(server);
  server.listen(0);
  await once(server, 'listening');
  const address = server.address();
  if (!address || typeof address === 'string')
    throw new Error('missing test address');
  const html = await (
    await fetch(`http://127.0.0.1:${address.port}/?source=test`)
  ).text();
  expect(html).toContain('<title>Home title</title>');
  expect(html).toContain(
    'name="description" content="Home description"',
  );
  expect(html).toContain('Server rendered');
  expect(html).toContain('data-plec-node="root/node:0"');
  // The v2 bootstrap carries the typed SSR execution snapshot: route chain
  // identity plus the public request location (path + search).
  expect(html).toContain('"version":2');
  expect(html).toContain('"revision":"test-revision"');
  expect(html).toContain('"routeId":"home"');
  expect(html).toContain('"location":"/?source=test"');
});

it('emits font preload links before the stylesheet', async () => {
  const publicDir = await mkdtemp(path.join(tmpdir(), 'plec-server-'));
  const graph = {
    rootComponent: 0,
    components: [
      {
        rootNode: 0,
        strings: ['main'],
        constants: [],
        nodes: [{ op: 'element', tag: 0, children: [] }],
        texts: [],
        bindings: [],
        propPrograms: [],
        stateSlots: [],
        parameters: [],
        expressions: [],
        loops: [],
        routeOutlets: [{ id: 'main', node: 0 }],
      },
    ],
  };
  await writeFile(
    path.join(publicDir, 'route-artifact.json'),
    JSON.stringify({
      manifest: {
        revision: 'test-revision',
        rootGraphId: 'root',
        routes: [{ id: 'home', path: '', graphId: 'home', outletId: 'main' }],
      },
      graphs: [
        { graphId: 'root', graph },
        { graphId: 'home', graph },
      ],
    }),
  );
  const server = createPlecServer({
    publicDir,
    artifactPath: path.join(publicDir, 'route-artifact.json'),
    stylesHref: '/assets/styles.css',
    preloads: ['/assets/files/outfit-latin-wght-normal.woff2'],
  });
  servers.push(server);
  server.listen(0);
  await once(server, 'listening');
  const address = server.address();
  if (!address || typeof address === 'string')
    throw new Error('missing test address');
  const html = await (
    await fetch(`http://127.0.0.1:${address.port}/`)
  ).text();
  expect(html).toContain(
    '<link rel="preload" as="font" type="font/woff2" crossorigin href="/assets/files/outfit-latin-wght-normal.woff2">',
  );
  expect(html.indexOf('rel="preload"')).toBeLessThan(
    html.indexOf('rel="stylesheet"'),
  );
});

it('publishes matched $param routes with their params in the snapshot chain', async () => {
  const publicDir = await mkdtemp(path.join(tmpdir(), 'plec-server-'));
  const component = (tag: string, text?: string) => ({
    rootComponent: 0,
    components: [
      {
        rootNode: 0,
        strings: [tag],
        constants: [],
        nodes: [
          { op: 'element', tag: 0, children: text ? [1] : [] },
          ...(text ? [{ op: 'text', text: 0 }] : []),
        ],
        texts: text ? [{ value: text }] : [],
        bindings: [],
        propPrograms: [],
        stateSlots: [],
        parameters: [],
        expressions: [],
        loops: [],
        routeOutlets: tag === 'main' ? [{ id: 'main', node: 0 }] : [],
      },
    ],
  });
  await writeFile(
    path.join(publicDir, 'route-artifact.json'),
    JSON.stringify({
      manifest: {
        revision: 'test-revision',
        rootGraphId: 'root',
        routes: [
          { id: 'home', path: '', graphId: 'home', outletId: 'main' },
          {
            id: 'project',
            path: 'projects/$id',
            graphId: 'project',
            outletId: 'main',
            meta: { title: 'Project' },
          },
          {
            id: 'missing',
            path: '*',
            graphId: 'missing',
            outletId: 'main',
          },
        ],
      },
      graphs: [
        { graphId: 'root', graph: component('main') },
        { graphId: 'home', graph: component('p', 'Server rendered') },
        {
          graphId: 'project',
          graph: component('p', 'Server rendered'),
        },
        {
          graphId: 'missing',
          graph: component('p', 'Server rendered'),
        },
      ],
    }),
  );
  const server = createPlecServer({
    publicDir,
    artifactPath: path.join(publicDir, 'route-artifact.json'),
  });
  servers.push(server);
  server.listen(0);
  await once(server, 'listening');
  const address = server.address();
  if (!address || typeof address === 'string')
    throw new Error('missing test address');
  const origin = `http://127.0.0.1:${address.port}`;

  // The parameterized route matches, and its decoded params are published in
  // the snapshot chain instead of being discarded.
  const project = await (
    await fetch(`${origin}/projects/a%20b`)
  ).text();
  expect(project).toContain('<title>Project</title>');
  expect(project).toContain('"routeId":"project"');
  expect(project).toContain('"params":{"id":"a b"}');
  expect(project).toContain('"phase":"active"');

  // Static and catch-all routes keep empty param records.
  const home = await (await fetch(`${origin}/`)).text();
  expect(home).toContain('"routeId":"home"');
  expect(home).toContain('"params":{}');
  const missing = await (await fetch(`${origin}/nowhere`)).text();
  expect(missing).toContain('"routeId":"missing"');
  expect(missing).toContain('"params":{}');
});

it('gates server-only cookie host loads out of markup and the bootstrap', async () => {
  const publicDir = await mkdtemp(path.join(tmpdir(), 'plec-server-'));
  const cookiePage = {
    rootComponent: 0,
    components: [
      {
        rootNode: 0,
        strings: ['span', 'data-token', 'sidebar_state'],
        constants: [],
        nodes: [
          { op: 'element', tag: 0, children: [1] },
          { op: 'text', text: 0 },
        ],
        texts: [{ binding: 0 }],
        bindings: [{ target: 0, sink: 'text', expression: 0 }],
        propPrograms: [
          {
            target: 0,
            writes: [{ name: 1, kind: 'attribute', expression: 1 }],
          },
        ],
        hostSlots: [{ kind: 'cookie', name: 2 }],
        stateSlots: [],
        parameters: [],
        loops: [],
        routeOutlets: [],
        expressions: [
          { instructions: [{ op: 'loadHost', host: 0 }] },
          { instructions: [{ op: 'loadHost', host: 0 }] },
        ],
      },
    ],
  };
  await writeFile(
    path.join(publicDir, 'route-artifact.json'),
    JSON.stringify({
      manifest: {
        revision: 'test-revision',
        rootGraphId: 'root',
        routes: [
          { id: 'home', path: '', graphId: 'home', outletId: 'main' },
        ],
      },
      graphs: [
        {
          graphId: 'root',
          graph: {
            rootComponent: 0,
            components: [
              {
                rootNode: 0,
                strings: ['main'],
                constants: [],
                nodes: [{ op: 'element', tag: 0, children: [] }],
                texts: [],
                bindings: [],
                propPrograms: [],
                stateSlots: [],
                parameters: [],
                expressions: [],
                loops: [],
                routeOutlets: [{ id: 'main', node: 0 }],
              },
            ],
          },
        },
        { graphId: 'home', graph: cookiePage },
      ],
    }),
  );
  const renderPage = async (development: boolean) => {
    const server = createPlecServer({
      publicDir,
      artifactPath: path.join(publicDir, 'route-artifact.json'),
      development,
    });
    servers.push(server);
    server.listen(0);
    await once(server, 'listening');
    const address = server.address();
    if (!address || typeof address === 'string')
      throw new Error('missing test address');
    return fetch(`http://127.0.0.1:${address.port}/`, {
      headers: { cookie: 'sidebar_state=secret-token-value' },
    }).then((response) =>
      response.text().then((html) => ({ response, html })),
    );
  };

  // A cookie-bound text node and attribute must not embed the request cookie:
  // server-only values never cross the SSR boundary (validate_public_export).
  const development = await renderPage(true);
  expect(development.html).not.toContain('secret-token-value');
  const bootstrap = JSON.parse(
    development.html.match(
      /<script id="plec-bootstrap" type="application\/json">([^<]+)<\/script>/,
    )![1]!,
  ) as { snapshot: { public: unknown } };
  expect(JSON.stringify(bootstrap)).not.toContain('secret-token-value');
  // v1 designates no exports: the location is carried as public state and
  // cookies are refused by construction.
  expect(bootstrap.snapshot.public).toEqual({
    location: '/',
    exports: {},
  });
  // Development observability identifies the gated server-only host load.
  expect(
    development.response.headers.get('x-plec-ssr-gating'),
  ).toContain('cookie:sidebar_state');

  // Gating diagnostics are development-only; the boundary itself is not.
  const production = await renderPage(false);
  expect(production.html).not.toContain('secret-token-value');
  expect(
    production.response.headers.get('x-plec-ssr-gating'),
  ).toBeNull();
});

it('marks conditional boundaries and records the instantiated branch in the snapshot', async () => {
  const publicDir = await mkdtemp(path.join(tmpdir(), 'plec-server-'));
  // The route page renders one conditional: p (consequent) vs span
  // (alternate) decided by a constant test expression.
  const conditionalPage = (
    testValue: boolean,
    withAlternate: boolean,
  ) => ({
    rootComponent: 0,
    components: [
      {
        rootNode: 0,
        strings: ['section', 'p', 'span', 'branch detail'],
        constants: [testValue],
        nodes: [
          { op: 'element', tag: 0, children: [1] },
          {
            op: 'conditional',
            test: 0,
            parent: 0,
            consequent: 2,
            alternate: withAlternate ? 3 : null,
          },
          { op: 'element', tag: 1, parent: 0, children: [4] },
          { op: 'element', tag: 2, parent: 0, children: [] },
          { op: 'text', text: 0, parent: 2 },
        ],
        texts: [{ value: 'branch detail' }],
        bindings: [],
        propPrograms: [],
        stateSlots: [{ initialExpression: 0, frameSlot: 0 }],
        parameters: [],
        loops: [],
        routeOutlets: [],
        expressions: [
          {
            instructions: [
              { op: 'constant', constant: 0 },
              { op: 'return' },
            ],
          },
        ],
      },
    ],
  });
  await writeFile(
    path.join(publicDir, 'route-artifact.json'),
    JSON.stringify({
      manifest: {
        revision: 'test-revision',
        rootGraphId: 'root',
        routes: [
          { id: 'home', path: '', graphId: 'home', outletId: 'main' },
        ],
      },
      graphs: [
        {
          graphId: 'root',
          graph: {
            rootComponent: 0,
            components: [
              {
                rootNode: 0,
                strings: ['main'],
                constants: [],
                nodes: [{ op: 'element', tag: 0, children: [] }],
                texts: [],
                bindings: [],
                propPrograms: [],
                stateSlots: [],
                parameters: [],
                expressions: [],
                loops: [],
                routeOutlets: [{ id: 'main', node: 0 }],
              },
            ],
          },
        },
        { graphId: 'home', graph: conditionalPage(true, true) },
      ],
    }),
  );
  const server = createPlecServer({
    publicDir,
    artifactPath: path.join(publicDir, 'route-artifact.json'),
  });
  servers.push(server);
  server.listen(0);
  await once(server, 'listening');
  const address = server.address();
  if (!address || typeof address === 'string')
    throw new Error('missing test address');
  const origin = `http://127.0.0.1:${address.port}`;
  const bootstrapOf = (html: string) =>
    JSON.parse(
      html.match(
        /<script id="plec-bootstrap" type="application\/json">([^<]+)<\/script>/,
      )![1]!,
    ) as {
      snapshot: {
        structure: {
          graphs: Record<
            string,
            {
              graphId: string;
              branches: Array<{ node: number; selected: string }>;
            }
          >;
        };
      };
    };

  // The selected branch renders between the runtime's boundary-marker
  // grammar, and the snapshot records the instantiated side per instance.
  const consequent = await (await fetch(origin)).text();
  expect(consequent).toContain(
    '<!--plec:conditional:root/outlet:main:1--><p data-plec-node="root/outlet:main/node:2"><!--plec:text:root/outlet:main:4-->branch detail</p><!--plec:conditional-end:root/outlet:main:1-->',
  );
  const consequentBootstrap = bootstrapOf(consequent);
  expect(
    consequentBootstrap.snapshot.structure.graphs['root/outlet:main'],
  ).toEqual({ graphId: 'root', branches: [] });
  expect(
    consequentBootstrap.snapshot.structure.graphs[
      'root%2Foutlet:main/outlet:main'
    ],
  ).toEqual({
    graphId: 'home',
    branches: [{ node: 1, selected: 'consequent' }],
  });

  // An unselected branch with no alternate records `none` and renders an
  // empty region: the markers still delimit the (empty) ownership span.
  await writeFile(
    path.join(publicDir, 'route-artifact.json'),
    JSON.stringify({
      manifest: {
        revision: 'test-revision',
        rootGraphId: 'root',
        routes: [
          { id: 'home', path: '', graphId: 'home', outletId: 'main' },
        ],
      },
      graphs: [
        {
          graphId: 'root',
          graph: {
            rootComponent: 0,
            components: [
              {
                rootNode: 0,
                strings: ['main'],
                constants: [],
                nodes: [{ op: 'element', tag: 0, children: [] }],
                texts: [],
                bindings: [],
                propPrograms: [],
                stateSlots: [],
                parameters: [],
                expressions: [],
                loops: [],
                routeOutlets: [{ id: 'main', node: 0 }],
              },
            ],
          },
        },
        { graphId: 'home', graph: conditionalPage(false, false) },
      ],
    }),
  );
  const none = await (await fetch(origin)).text();
  expect(none).toContain(
    '<!--plec:conditional:root/outlet:main:1--><!--plec:conditional-end:root/outlet:main:1-->',
  );
  const noneBootstrap = bootstrapOf(none);
  expect(
    noneBootstrap.snapshot.structure.graphs[
      'root%2Foutlet:main/outlet:main'
    ]!.branches,
  ).toEqual([{ node: 1, selected: 'none' }]);
});

/** A route page whose only content is the `loaderData` host slot, paired with
 * the route-loader action the compiler emits for a static-URL fetch. */
function loaderPage(url: string) {
  return {
    rootComponent: 0,
    components: [
      {
        rootNode: 0,
        strings: ['p', 'section'],
        constants: [null, url],
        nodes: [
          { op: 'element', tag: 1, children: [1] },
          { op: 'text', text: 0 },
        ],
        texts: [{ binding: 0 }],
        bindings: [{ target: 1, sink: 'text', expression: 0 }],
        propPrograms: [],
        hostSlots: [{ kind: 'loaderData' }],
        stateSlots: [],
        parameters: [],
        loops: [],
        routeOutlets: [],
        expressions: [
          {
            instructions: [
              { op: 'loadHost', host: 0 },
              { op: 'return' },
            ],
          },
          {
            instructions: [
              { op: 'constant', constant: 1 },
              { op: 'return' },
            ],
          },
        ],
        actions: [
          {
            routeLoader: true,
            loaderResultState: null,
            instructions: [
              {
                op: 'capabilityRequest',
                capability: 'fetch',
                request: {
                  url: 1,
                  method: 'GET',
                  decode: 'responseJson',
                  requireOk: true,
                },
                successPc: 1,
                failurePc: 2,
                resultSlot: 0,
                errorSlot: 1,
              },
              { op: 'return' },
              { op: 'return', outcome: 'failure' },
            ],
          },
        ],
      },
    ],
  };
}

async function startLoaderServer(
  handler: (request: Request) => Response | undefined,
  errorPage = false,
) {
  const publicDir = await mkdtemp(path.join(tmpdir(), 'plec-server-'));
  const layout = {
    rootComponent: 0,
    components: [
      {
        rootNode: 0,
        strings: ['main'],
        constants: [],
        nodes: [{ op: 'element', tag: 0, children: [] }],
        texts: [],
        bindings: [],
        propPrograms: [],
        stateSlots: [],
        parameters: [],
        expressions: [],
        loops: [],
        routeOutlets: [{ id: 'main', node: 0 }],
      },
    ],
  };
  const errorComponent = {
    rootComponent: 0,
    components: [
      {
        rootNode: 0,
        strings: ['p'],
        constants: [],
        nodes: [
          { op: 'element', tag: 0, children: [1] },
          { op: 'text', text: 0 },
        ],
        texts: [{ value: 'Error phase rendered' }],
        bindings: [],
        propPrograms: [],
        stateSlots: [],
        parameters: [],
        expressions: [],
        loops: [],
        routeOutlets: [],
      },
    ],
  };
  await writeFile(
    path.join(publicDir, 'route-artifact.json'),
    JSON.stringify({
      manifest: {
        revision: 'test-revision',
        rootGraphId: 'root',
        routes: [
          {
            id: 'notes',
            path: 'notes',
            graphId: 'page',
            outletId: 'main',
            loaderAction: 0,
            ...(errorPage ? { errorGraphId: 'error-page' } : {}),
          },
        ],
      },
      graphs: [
        { graphId: 'root', graph: layout },
        { graphId: 'page', graph: loaderPage('/api/endpoint') },
        { graphId: 'error-page', graph: errorComponent },
      ],
    }),
  );
  const server = createPlecServer({
    publicDir,
    artifactPath: path.join(publicDir, 'route-artifact.json'),
    handleAppRequest: handler,
  });
  servers.push(server);
  server.listen(0);
  await once(server, 'listening');
  const address = server.address();
  if (!address || typeof address === 'string')
    throw new Error('missing test address');
  return `http://127.0.0.1:${address.port}`;
}

function bootstrapOf(html: string) {
  return JSON.parse(
    html.match(
      /<script id="plec-bootstrap" type="application\/json">([^<]+)<\/script>/,
    )![1]!,
  ) as {
    snapshot: {
      routes: Array<{ phase: string }>;
      loaders: Array<{
        graphId: string;
        action: number;
        state: { kind: string; value?: unknown; message?: string };
      }>;
    };
  };
}

it('executes a route loader server-side and transfers the outcome in the snapshot', async () => {
  const origin = await startLoaderServer((request) =>
    new URL(request.url).pathname === '/api/endpoint'
      ? new Response(JSON.stringify({ headline: 'loader payload' }), {
          status: 200,
          headers: { 'content-type': 'application/json' },
        })
      : undefined,
  );
  const html = await (await fetch(`${origin}/notes`)).text();
  // The resolved value rendered through the loaderData host slot and shipped
  // in the snapshot so the browser resumes without refetching.
  expect(html).toContain('loader payload');
  const bootstrap = bootstrapOf(html);
  expect(bootstrap.snapshot.routes[0]!.phase).toBe('active');
  expect(bootstrap.snapshot.loaders).toEqual([
    {
      graphId: 'page',
      action: 0,
      state: {
        kind: 'resolved',
        value: { headline: 'loader payload' },
      },
    },
  ]);
});

it('renders the error phase and records the rejection when the loader fetch fails', async () => {
  const origin = await startLoaderServer(
    () =>
      new Response(JSON.stringify({ error: 'boom' }), { status: 500 }),
    true,
  );
  const html = await (await fetch(`${origin}/notes`)).text();
  // The error graph is what the server rendered; the normal page text is gone.
  expect(html).toContain('Error phase rendered');
  expect(html).not.toContain('loader payload');
  const bootstrap = bootstrapOf(html);
  expect(bootstrap.snapshot.routes[0]!.phase).toBe('error');
  expect(bootstrap.snapshot.loaders).toHaveLength(1);
  expect(bootstrap.snapshot.loaders[0]!.state.kind).toBe('rejected');
  expect(bootstrap.snapshot.loaders[0]!.state.message).toContain(
    'status 500',
  );
});

it('renders keyed loop rows with row-scoped component props and records their keys', async () => {
  const publicDir = await mkdtemp(path.join(tmpdir(), 'plec-server-'));
  // The route page renders one keyed loop whose row template is a component
  // call. The row prop transfers the whole row record (`loadRowRecord`), so
  // the child renders the row title from its prop.
  const loopPage = {
    rootComponent: 0,
    components: [
      {
        rootNode: 0,
        strings: ['section', 'ul', 'li', 'title', 'id'],
        constants: ['one', 'One', 'two', 'Two'],
        nodes: [
          { op: 'element', tag: 0, children: [1] },
          { op: 'element', tag: 1, parent: 0, children: [2] },
          { op: 'loop', loop: 0, parent: 1 },
          {
            op: 'component',
            component: 1,
            parent: null,
            props: [{ kind: 'value', name: 3, expression: 0 }],
          },
        ],
        texts: [],
        bindings: [],
        propPrograms: [],
        stateSlots: [],
        parameters: [],
        expressions: [
          {
            instructions: [{ op: 'loadRowRecord' }, { op: 'return' }],
          },
          {
            instructions: [
              { op: 'loadRowField', field: 4 },
              { op: 'return' },
            ],
          },
          {
            instructions: [
              { op: 'constant', constant: 0 },
              { op: 'constant', constant: 1 },
              { op: 'makeRecord', fields: [4, 3] },
              { op: 'constant', constant: 2 },
              { op: 'constant', constant: 3 },
              { op: 'makeRecord', fields: [4, 3] },
              { op: 'makeArray', count: 2 },
              { op: 'return' },
            ],
          },
        ],
        actions: [],
        loops: [
          {
            sourceExpression: 2,
            keyExpression: 1,
            itemSlot: 0,
            rowTemplate: 3,
          },
        ],
        routeOutlets: [],
      },
      {
        rootNode: 0,
        strings: ['li', 'span', 'title'],
        constants: [],
        nodes: [
          { op: 'element', tag: 0, children: [1] },
          { op: 'element', tag: 1, parent: 0, children: [2] },
          { op: 'text', text: 0, parent: 1 },
        ],
        texts: [{ binding: 0 }],
        bindings: [{ target: 2, sink: 'text', expression: 0 }],
        propPrograms: [],
        stateSlots: [],
        parameters: [{ name: 2, callable: false }],
        expressions: [
          {
            instructions: [
              { op: 'loadProp', prop: 0 },
              { op: 'field', field: 2 },
              { op: 'return' },
            ],
          },
        ],
        loops: [],
        routeOutlets: [],
      },
    ],
  };
  await writeFile(
    path.join(publicDir, 'route-artifact.json'),
    JSON.stringify({
      manifest: {
        revision: 'test-revision',
        rootGraphId: 'root',
        routes: [
          { id: 'home', path: '', graphId: 'home', outletId: 'main' },
        ],
      },
      graphs: [
        {
          graphId: 'root',
          graph: {
            rootComponent: 0,
            components: [
              {
                rootNode: 0,
                strings: ['main'],
                constants: [],
                nodes: [{ op: 'element', tag: 0, children: [] }],
                texts: [],
                bindings: [],
                propPrograms: [],
                stateSlots: [],
                parameters: [],
                expressions: [],
                loops: [],
                routeOutlets: [{ id: 'main', node: 0 }],
              },
            ],
          },
        },
        { graphId: 'home', graph: loopPage },
      ],
    }),
  );
  const server = createPlecServer({
    publicDir,
    artifactPath: path.join(publicDir, 'route-artifact.json'),
  });
  servers.push(server);
  server.listen(0);
  await once(server, 'listening');
  const address = server.address();
  if (!address || typeof address === 'string')
    throw new Error('missing test address');
  const html = await (
    await fetch(`http://127.0.0.1:${address.port}/`)
  ).text();

  // Each row renders between the runtime's loop marker grammar with the
  // row key stamped on its root, and the child component received the row
  // record: the title text comes from the transferred prop.
  expect(html).toContain(
    '<!--plec:loop:root/outlet:main/loop:2/key:one-->',
  );
  expect(html).toContain(
    '<!--plec:loop-end:root/outlet:main/loop:2/key:one-->',
  );
  expect(html).toContain('data-runtime-row-key="one"');
  expect(html).toContain('data-runtime-row-key="two"');
  expect(html).toContain('>One</span>');
  expect(html).toContain('>Two</span>');
  const bootstrap = JSON.parse(
    html.match(
      /<script id="plec-bootstrap" type="application\/json">([^<]+)<\/script>/,
    )![1]!,
  ) as {
    snapshot: {
      structure: {
        graphs: Record<
          string,
          { loops?: Array<{ node: number; keys: string[] }> }
        >;
      };
    };
  };
  const graphs = Object.values(bootstrap.snapshot.structure.graphs);
  const instance = graphs.find(
    (entry) => (entry.loops ?? []).length > 0,
  );
  expect(instance!.loops).toEqual([{ node: 2, keys: ['one', 'two'] }]);
});

it('fails the document render when a loop produces duplicate keys', async () => {
  const publicDir = await mkdtemp(path.join(tmpdir(), 'plec-server-'));
  const duplicatePage = {
    rootComponent: 0,
    components: [
      {
        rootNode: 0,
        strings: ['section', 'ul', 'li', 'id'],
        constants: ['one', 'Same', 'one', 'Again'],
        nodes: [
          { op: 'element', tag: 0, children: [1] },
          { op: 'element', tag: 1, parent: 0, children: [2] },
          { op: 'loop', loop: 0, parent: 1 },
          { op: 'element', tag: 2, parent: null, children: [] },
        ],
        texts: [],
        bindings: [],
        propPrograms: [],
        stateSlots: [],
        parameters: [],
        expressions: [
          {
            instructions: [
              { op: 'constant', constant: 0 },
              { op: 'makeRecord', fields: [3] },
              { op: 'constant', constant: 0 },
              { op: 'makeRecord', fields: [3] },
              { op: 'makeArray', count: 2 },
              { op: 'return' },
            ],
          },
          {
            instructions: [
              { op: 'loadRowField', field: 3 },
              { op: 'return' },
            ],
          },
        ],
        actions: [],
        loops: [
          {
            sourceExpression: 0,
            keyExpression: 1,
            itemSlot: 0,
            rowTemplate: 3,
          },
        ],
        routeOutlets: [],
      },
    ],
  };
  await writeFile(
    path.join(publicDir, 'route-artifact.json'),
    JSON.stringify({
      manifest: {
        revision: 'test-revision',
        rootGraphId: 'root',
        routes: [
          { id: 'home', path: '', graphId: 'home', outletId: 'main' },
        ],
      },
      graphs: [
        {
          graphId: 'root',
          graph: {
            rootComponent: 0,
            components: [
              {
                rootNode: 0,
                strings: ['main'],
                constants: [],
                nodes: [{ op: 'element', tag: 0, children: [] }],
                texts: [],
                bindings: [],
                propPrograms: [],
                stateSlots: [],
                parameters: [],
                expressions: [],
                loops: [],
                routeOutlets: [{ id: 'main', node: 0 }],
              },
            ],
          },
        },
        { graphId: 'home', graph: duplicatePage },
      ],
    }),
  );
  const server = createPlecServer({
    publicDir,
    artifactPath: path.join(publicDir, 'route-artifact.json'),
    development: true,
  });
  servers.push(server);
  server.listen(0);
  await once(server, 'listening');
  const address = server.address();
  if (!address || typeof address === 'string')
    throw new Error('missing test address');
  const response = await fetch(`http://127.0.0.1:${address.port}/`);
  // A duplicate key is a render failure, never a silently degraded page.
  expect(response.status).toBe(500);
  expect(await response.text()).toContain('DUPLICATE_LOOP_KEY:one');
});

it('serializes `{...props}` spreads so island icons paint with their class attribute', async () => {
  const publicDir = await mkdtemp(path.join(tmpdir(), 'plec-server-'));
  // Mirrors a generated lucide icon: the svg element carries explicit
  // attributes followed by `{...props}`, and the island call passes a
  // className through the direct props bag (`__plec_props`). SSR must
  // serialize the spread so the first paint already has `class="..."`.
  const iconPage = {
    rootComponent: 0,
    components: [
      {
        rootNode: 0,
        strings: ['section', '__plec_props', 'className'],
        constants: ['size-4 shrink-0'],
        nodes: [
          { op: 'element', tag: 0, children: [1] },
          {
            op: 'component',
            component: 1,
            parent: 0,
            props: [{ kind: 'value', name: 1, expression: 0 }],
          },
        ],
        texts: [],
        bindings: [],
        propPrograms: [],
        stateSlots: [],
        parameters: [],
        expressions: [
          {
            instructions: [
              { op: 'constant', constant: 0 },
              { op: 'makeRecord', fields: [2] },
              { op: 'return' },
            ],
          },
        ],
        loops: [],
        routeOutlets: [],
      },
      {
        rootNode: 0,
        strings: ['svg', '__plec_props', 'xmlns', 'width', 'height'],
        constants: ['http://www.w3.org/2000/svg', '24', '24'],
        nodes: [{ op: 'element', tag: 0, children: [] }],
        texts: [],
        bindings: [],
        propPrograms: [
          {
            target: 0,
            writes: [
              { name: 2, kind: 'attribute', constant: 0 },
              { name: 3, kind: 'attribute', constant: 1 },
              { name: 4, kind: 'attribute', constant: 2 },
              { kind: 'attribute', expression: 0, spread: true },
            ],
          },
        ],
        stateSlots: [],
        parameters: [{ name: 1, callable: false }],
        expressions: [
          {
            instructions: [{ op: 'loadProp', prop: 0 }, { op: 'return' }],
          },
        ],
        loops: [],
        routeOutlets: [],
      },
    ],
  };
  await writeFile(
    path.join(publicDir, 'route-artifact.json'),
    JSON.stringify({
      manifest: {
        revision: 'test-revision',
        rootGraphId: 'root',
        routes: [
          { id: 'home', path: '', graphId: 'home', outletId: 'main' },
        ],
      },
      graphs: [
        {
          graphId: 'root',
          graph: {
            rootComponent: 0,
            components: [
              {
                rootNode: 0,
                strings: ['main'],
                constants: [],
                nodes: [{ op: 'element', tag: 0, children: [] }],
                texts: [],
                bindings: [],
                propPrograms: [],
                stateSlots: [],
                parameters: [],
                expressions: [],
                loops: [],
                routeOutlets: [{ id: 'main', node: 0 }],
              },
            ],
          },
        },
        { graphId: 'home', graph: iconPage },
      ],
    }),
  );
  const server = createPlecServer({
    publicDir,
    artifactPath: path.join(publicDir, 'route-artifact.json'),
  });
  servers.push(server);
  server.listen(0);
  await once(server, 'listening');
  const address = server.address();
  if (!address || typeof address === 'string')
    throw new Error('missing test address');
  const html = await (
    await fetch(`http://127.0.0.1:${address.port}/`)
  ).text();
  // The spread lands after the explicit attributes, exactly where the icon
  // writes it, so the painted svg already carries its final CSS size class.
  expect(html).toContain(
    '<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" class="size-4 shrink-0"',
  );
});

it('passes named props to a dynamic island component and serializes its spread', async () => {
  const publicDir = await mkdtemp(path.join(tmpdir(), 'plec-server-'));
  // NavLink-style call: `<Icon className="size-4 shrink-0" />` where `Icon`
  // arrives as a component prop. The dynamic call keeps its props named, and
  // the icon's direct `(props)` parameter reads the whole named record.
  const dynamicIconPage = {
    rootComponent: 0,
    components: [
      {
        rootNode: 0,
        strings: ['section', 'Icon'],
        constants: [],
        nodes: [
          { op: 'element', tag: 0, children: [1] },
          {
            op: 'component',
            component: 1,
            parent: 0,
            props: [{ kind: 'component', name: 1, component: 2 }],
          },
        ],
        texts: [],
        bindings: [],
        propPrograms: [],
        stateSlots: [],
        parameters: [],
        expressions: [],
        loops: [],
        routeOutlets: [],
      },
      {
        rootNode: 0,
        strings: ['Icon', 'className'],
        constants: ['size-4 shrink-0'],
        nodes: [
          {
            op: 'dynamicComponent',
            prop: 0,
            parent: null,
            props: [{ kind: 'value', name: 1, expression: 0 }],
          },
        ],
        texts: [],
        bindings: [],
        propPrograms: [],
        stateSlots: [],
        parameters: [{ name: 0, component: true }],
        expressions: [
          {
            instructions: [{ op: 'constant', constant: 0 }, { op: 'return' }],
          },
        ],
        loops: [],
        routeOutlets: [],
      },
      {
        rootNode: 0,
        strings: ['svg', '__plec_props', 'width'],
        constants: ['24'],
        nodes: [{ op: 'element', tag: 0, children: [] }],
        texts: [],
        bindings: [],
        propPrograms: [
          {
            target: 0,
            writes: [
              { name: 2, kind: 'attribute', constant: 0 },
              { kind: 'attribute', expression: 0, spread: true },
            ],
          },
        ],
        stateSlots: [],
        parameters: [{ name: 1, callable: false }],
        expressions: [
          {
            instructions: [{ op: 'loadProp', prop: 0 }, { op: 'return' }],
          },
        ],
        loops: [],
        routeOutlets: [],
      },
    ],
  };
  await writeFile(
    path.join(publicDir, 'route-artifact.json'),
    JSON.stringify({
      manifest: {
        revision: 'test-revision',
        rootGraphId: 'root',
        routes: [
          { id: 'home', path: '', graphId: 'home', outletId: 'main' },
        ],
      },
      graphs: [
        {
          graphId: 'root',
          graph: {
            rootComponent: 0,
            components: [
              {
                rootNode: 0,
                strings: ['main'],
                constants: [],
                nodes: [{ op: 'element', tag: 0, children: [] }],
                texts: [],
                bindings: [],
                propPrograms: [],
                stateSlots: [],
                parameters: [],
                expressions: [],
                loops: [],
                routeOutlets: [{ id: 'main', node: 0 }],
              },
            ],
          },
        },
        { graphId: 'home', graph: dynamicIconPage },
      ],
    }),
  );
  const server = createPlecServer({
    publicDir,
    artifactPath: path.join(publicDir, 'route-artifact.json'),
  });
  servers.push(server);
  server.listen(0);
  await once(server, 'listening');
  const address = server.address();
  if (!address || typeof address === 'string')
    throw new Error('missing test address');
  const html = await (
    await fetch(`http://127.0.0.1:${address.port}/`)
  ).text();
  expect(html).toContain('<svg width="24" class="size-4 shrink-0"');
});
