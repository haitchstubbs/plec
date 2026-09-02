/**
 * The intentionally small Node host for compiled Plec applications.  It owns
 * HTTP mechanics only; application API handlers are an explicit temporary
 * escape hatch and are not part of Plec's semantic server model.
 */
import {
  createServer,
  type IncomingMessage,
  type Server,
  type ServerResponse,
} from 'node:http';
import { readFile } from 'node:fs/promises';
import path from 'node:path';

export type RequestContext = {
  url: string;
  pathname: string;
  method: string;
  headers: Record<string, string>;
  cookies: Record<string, string>;
  params: Record<string, string>;
  query: Record<string, string | string[]>;
};

export type AppRequestHandler = (
  request: Request,
  context: RequestContext,
) => Response | undefined | Promise<Response | undefined>;

export type PlecServerOptions = {
  publicDir: string;
  /** Rust compiler output written by the application build. */
  artifactPath: string;
  clientScript?: string;
  stylesHref?: string;
  document?: { title: string; description: string };
  handleAppRequest?: AppRequestHandler;
  development?: boolean;
};

type ArtifactBundle = {
  manifest: Manifest;
  graphs: Array<{ graphId: string; graph: App }>;
};
type Manifest = {
  revision: string;
  rootGraphId: string;
  routes: Route[];
};
type Route = {
  id: string;
  path: string;
  graphId: string;
  outletId: string;
  parentId?: string;
  pendingGraphId?: string;
  errorGraphId?: string;
  loaderAction?: number;
  meta?: DocumentMetadata;
};
type DocumentMetadata = { title?: string; description?: string };
type App = { rootComponent: number; components: Component[] };
type Component = {
  rootNode: number;
  strings: string[];
  constants: unknown[];
  nodes: Node[];
  texts: Array<{ value?: string; binding?: number }>;
  bindings: Array<{
    target: number;
    sink: string;
    name?: number;
    expression: number;
  }>;
  propPrograms: Array<{
    target: number;
    writes: Array<{
      name?: number;
      kind: string;
      constant?: number;
      expression?: number;
      spread?: boolean;
    }>;
  }>;
  hostSlots?: Array<{ kind: string; name?: number }>;
  stateSlots: Array<{ initialExpression: number }>;
  parameters: Array<{ name: number }>;
  expressions: Array<{ instructions: Instruction[] }>;
  loops: Array<{
    sourceExpression: number;
    keyExpression: number;
    itemSlot: number;
    rowTemplate: number;
  }>;
  routeOutlets?: Array<{ id: string; node: number }>;
  /** Loader-action subset only: the server executes route loaders, never
   * general actions (see `executeRouteLoader`). */
  actions?: Array<{
    routeLoader?: boolean;
    loaderResultState?: number | null;
    instructions: Array<Record<string, unknown> & { op: string }>;
  }>;
};
type Node = {
  op: string;
  tag?: number;
  namespace?: string;
  children?: number[];
  text?: number;
  test?: number;
  consequent?: number;
  alternate?: number;
  component?: number;
  prop?: number;
  props?: Array<{
    kind: string;
    name: number;
    expression?: number;
    component?: number;
  }>;
  loop?: number;
};
type Instruction = Record<string, unknown> & { op: string };

/** Which side of a conditional the server instantiated. The grammar mirrors
 * `SsrSelectedBranch` (crates/plec-ir): the branch record is the adoption
 * ownership cause, and the markers around the rendered branch are its proof. */
type BranchSide = 'consequent' | 'alternate' | 'none';
type SsrBranchRecord = { node: number; selected: BranchSide };
type SsrLoopRecord = { node: number; keys: string[] };
/** Graph instance id -> selected conditional branches (node-handle keyed). */
type SsrBranchGraph = Map<string, Map<number, BranchSide>>;
/** Graph instance id -> keyed loop rows (node-handle keyed). */
type SsrLoopGraph = Map<string, Map<number, string[]>>;

/** One instance-id segment (`graph_instance_id` escapes `/` as `%2F` and
 * `%` as `%25`), so nested snapshot keys match runtime instance ids. */
function escapeInstanceSegment(value: string): string {
  return value.replace(/%/g, '%25').replace(/\//g, '%2F');
}

/** Server-only boundary enforcement during one document render. Request
 * cookies can never satisfy `validate_public_export` (not explicitly public,
 * server-owned private state), so the render evaluates them as absent and
 * records each gate for the development-only diagnostic header. */
type SsrRenderGate = { development: boolean; gated: Set<string> };

const contentTypes: Record<string, string> = {
  '.css': 'text/css; charset=utf-8',
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.wasm': 'application/wasm',
  '.woff2': 'font/woff2',
};

export function createPlecServer(options: PlecServerOptions): Server {
  return createServer(async (incoming, outgoing) => {
    const request = await toRequest(incoming);
    const context = requestContext(request);
    if (context.pathname.startsWith('/api/')) {
      const response = await options.handleAppRequest?.(
        request,
        context,
      );
      return response
        ? sendFetchResponse(outgoing, response)
        : sendJson(outgoing, 404, { error: 'endpoint not found' });
    }
    if (isDocumentRequest(context.pathname)) {
      try {
        const artifact = JSON.parse(
          await readFile(options.artifactPath, 'utf8'),
        ) as ArtifactBundle;
        const match = matchRoute(artifact.manifest, context.pathname);
        // SSR markup and app handlers must observe the same matched params the
        // browser snapshot carries, so the request context stops dropping them.
        if (match) context.params = match.params;
        const document = match?.route.meta ?? options.document ?? {};
        const gate: SsrRenderGate = {
          development: options.development === true,
          gated: new Set(),
        };
        const loader = match
          ? await executeRouteLoader(artifact, match.route, context)
          : undefined;
        const rendered = renderApplication(
          artifact,
          match?.route,
          context,
          gate,
          loader,
        );
        const bootstrap = JSON.stringify(
          bootstrapPayload(
            artifact,
            match,
            context,
            loader,
            rendered.branches,
            rendered.loops,
            rendered.childGraph,
          ),
        ).replace(/</g, '\\u003c');
        return sendHtml(
          outgoing,
          document,
          rendered.body,
          bootstrap,
          options,
          gate.development ? [...gate.gated] : [],
        );
      } catch (error) {
        // A fixture without compiler artifacts remains useful for HTTP-host
        // tests. Real Plec builds always supply the artifact and therefore
        // take the SSR path above.
        try {
          const shell = await readFile(
            path.join(options.publicDir, 'index.html'),
          );
          outgoing.writeHead(200, {
            'content-type': 'text/html; charset=utf-8',
            'cache-control': 'no-cache',
            ...(options.development
              ? { 'x-plec-ssr-fallback': String(error) }
              : {}),
          });
          return outgoing.end(shell);
        } catch {
          return sendJson(outgoing, 500, {
            error: `Plec SSR failed: ${String(error)}`,
          });
        }
      }
    }
    return serveAsset(
      outgoing,
      incoming,
      context.pathname,
      options.publicDir,
    );
  });
}

export function serve(
  server: Server,
  port = Number(process.env.PORT ?? 3000),
): Server {
  server.listen(port);
  return server;
}

async function toRequest(incoming: IncomingMessage): Promise<Request> {
  const origin = `http://${incoming.headers.host ?? 'localhost'}`;
  const body = ['GET', 'HEAD'].includes(incoming.method ?? 'GET')
    ? undefined
    : await readBody(incoming);
  return new Request(new URL(incoming.url ?? '/', origin), {
    method: incoming.method,
    headers: incoming.headers as HeadersInit,
    body: body && new Uint8Array(body),
  });
}

async function readBody(request: IncomingMessage): Promise<Buffer> {
  const chunks: Buffer[] = [];
  for await (const chunk of request)
    chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk));
  return Buffer.concat(chunks);
}

function requestContext(request: Request): RequestContext {
  const url = new URL(request.url);
  const headers = Object.fromEntries(request.headers.entries());
  const query: RequestContext['query'] = {};
  for (const key of new Set(url.searchParams.keys())) {
    const values = url.searchParams.getAll(key);
    query[key] = values.length === 1 ? values[0]! : values;
  }
  return {
    url: url.href,
    pathname: url.pathname,
    method: request.method,
    headers,
    cookies: parseCookies(headers.cookie),
    params: {},
    query,
  };
}

function parseCookies(header = ''): Record<string, string> {
  return Object.fromEntries(
    header.split(';').flatMap((part) => {
      const index = part.indexOf('=');
      return index < 0
        ? []
        : [
            [
              part.slice(0, index).trim(),
              decodeURIComponent(part.slice(index + 1).trim()),
            ],
          ];
    }),
  );
}

function isDocumentRequest(pathname: string) {
  return pathname === '/' || !path.extname(pathname);
}

async function serveAsset(
  response: ServerResponse,
  request: IncomingMessage,
  pathname: string,
  publicDir: string,
) {
  const requested = pathname.replace(/^\/+/, '');
  const filePath = path.resolve(publicDir, requested);
  const root = path.resolve(publicDir);
  if (!filePath.startsWith(`${root}${path.sep}`))
    return sendJson(response, 400, { error: 'invalid asset path' });
  const encoding = selectEncoding(request.headers['accept-encoding']);
  const compressed =
    encoding && pathname.startsWith('/runtime/')
      ? `${filePath}.${encoding === 'gzip' ? 'gz' : 'br'}`
      : undefined;
  try {
    const [body, compressedServed] = compressed
      ? await readFile(compressed)
          .then((value) => [value, true] as const)
          .catch(() =>
            readFile(filePath).then((value) => [value, false] as const),
          )
      : ([await readFile(filePath), false] as const);
    response.writeHead(200, {
      'content-type':
        contentTypes[path.extname(filePath)] ??
        'application/octet-stream',
      'cache-control': 'no-cache',
      ...(encoding && compressedServed
        ? { 'content-encoding': encoding, vary: 'Accept-Encoding' }
        : {}),
    });
    response.end(body);
  } catch {
    sendJson(response, 404, { error: 'asset not found' });
  }
}

function selectEncoding(
  header: string | string[] | undefined,
): 'br' | 'gzip' | undefined {
  const value = Array.isArray(header)
    ? header.join(',')
    : (header ?? '');
  return /\bbr\b/.test(value)
    ? 'br'
    : /\bgzip\b/.test(value)
      ? 'gzip'
      : undefined;
}

function sendFetchResponse(response: ServerResponse, value: Response) {
  const headers = Object.fromEntries(value.headers.entries());
  response.writeHead(value.status, headers);
  void value
    .arrayBuffer()
    .then((body) => response.end(Buffer.from(body)));
}
function sendJson(
  response: ServerResponse,
  status: number,
  value: unknown,
) {
  response.writeHead(status, {
    'content-type': 'application/json; charset=utf-8',
    'cache-control': 'no-store',
  });
  response.end(JSON.stringify(value));
}
function sendHtml(
  response: ServerResponse,
  metadata: DocumentMetadata,
  body: string,
  bootstrap: string,
  options: PlecServerOptions,
  gating: string[] = [],
) {
  const title = escapeHtml(metadata.title ?? 'Plec application');
  const description = escapeHtml(metadata.description ?? '');
  const styles = options.stylesHref
    ? `<link rel="stylesheet" href="${escapeAttribute(options.stylesHref)}">`
    : '';
  const script = options.clientScript
    ? `<script type="module" src="${escapeAttribute(options.clientScript)}"></script>`
    : '';
  // x-plec-ssr-fallback-style observability: development hosts learn which
  // server-only host loads were gated out of the public artifacts.
  response.writeHead(200, {
    'content-type': 'text/html; charset=utf-8',
    'cache-control': 'no-cache',
    ...(gating.length ? { 'x-plec-ssr-gating': gating.join(',') } : {}),
  });
  response.end(
    `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>${title}</title>${description ? `<meta name="description" content="${escapeAttribute(description)}">` : ''}${styles}</head><body><div id="app">${body}</div><script id="plec-bootstrap" type="application/json">${bootstrap}</script>${script}</body></html>`,
  );
}

/** One executed route loader. The shape mirrors `SsrLoaderOutcome` in
 * crates/plec-ir (the strict WASM consumer); values are always
 * JSON-transportable because they come from `Response.json()`. */
type LoaderOutcome = {
  graphId: string;
  action: number;
  state:
    | { kind: 'resolved'; value: unknown }
    | { kind: 'rejected'; message: string };
};

/** Executes the narrow server loader subset: the compiler lowers a route
 * loader to one `routeLoader` action whose only capability is a static-URL
 * GET fetch with JSON decoding, so running exactly that program gives SSR the
 * same outcome the browser loader would observe. */
async function executeRouteLoader(
  bundle: ArtifactBundle,
  route: Route,
  context: RequestContext,
): Promise<LoaderOutcome | undefined> {
  const action = route.loaderAction;
  if (action === undefined) return undefined;
  const graph = bundle.graphs.find(
    (entry) => entry.graphId === route.graphId,
  )?.graph;
  const component = graph?.components[graph.rootComponent];
  const program = component?.actions?.[action];
  const fetchRequest = program?.routeLoader
    ? (program.instructions.find(
        (instruction) =>
          instruction.op === 'capabilityRequest' &&
          instruction.capability === 'fetch',
      )?.request as
        | {
            url?: number;
            method?: string;
            decode?: string;
            requireOk?: boolean;
          }
        | undefined)
    : undefined;
  if (!component || !fetchRequest)
    throw new Error(`route loader action is invalid for ${route.id}`);
  const rawUrl = evaluate(
    component,
    typeof fetchRequest.url === 'number' ? fetchRequest.url : -1,
    { request: context, path: 'root', states: [] },
  );
  const url = new URL(String(rawUrl), context.url);
  const rejected = (message: string): LoaderOutcome => ({
    graphId: route.graphId,
    action,
    state: { kind: 'rejected', message },
  });
  try {
    const response = await fetch(url, {
      method: fetchRequest.method ?? 'GET',
    });
    if ((fetchRequest.requireOk ?? false) && !response.ok) {
      return rejected(
        `fetch ${url.pathname} failed with status ${response.status}`,
      );
    }
    if (fetchRequest.decode !== 'responseJson') {
      throw new Error(
        `unsupported route loader decode ${String(fetchRequest.decode)}`,
      );
    }
    return {
      graphId: route.graphId,
      action,
      state: { kind: 'resolved', value: await response.json() },
    };
  } catch (error) {
    return rejected(
      `fetch ${url.pathname} failed: ${error instanceof Error ? error.message : String(error)}`,
    );
  }
}

/** The v2 bootstrap carries the typed SSR execution snapshot (see the
 * `PlecSsrSnapshot` contract in crates/plec-ir). Without a matched route there
 * is nothing to resume, so the legacy v1 shape is emitted and the browser
 * treats the page as non-snapshot SSR. */
function bootstrapPayload(
  bundle: ArtifactBundle,
  match: ReturnType<typeof matchRoute> | undefined,
  context: RequestContext,
  loader: LoaderOutcome | undefined,
  branches: SsrBranchGraph,
  loops: SsrLoopGraph,
  childGraph: { instance: string; graphId: string } | undefined,
) {
  if (!match?.route) {
    return {
      version: 1,
      revision: bundle.manifest.revision,
      routeId: null,
      public: {
        location: {
          pathname: context.pathname,
          search: new URL(context.url).search,
        },
      },
    };
  }
  // Branch records are the structural ownership cause for conditional
  // adoption: `branches[nodeHandle] = which side the server instantiated`,
  // ordered by node handle exactly as snapshot validation requires.
  const branchRecords = (instance: string): SsrBranchRecord[] =>
    [...(branches.get(instance)?.entries() ?? [])]
      .sort(([left], [right]) => left - right)
      .map(([node, selected]) => ({ node, selected }));
  const loopRecords = (instance: string): SsrLoopRecord[] =>
    [...(loops.get(instance)?.entries() ?? [])]
      .sort(([left], [right]) => left - right)
      .map(([node, keys]) => ({ node, keys }));
  const structure = (instance: string, graphId: string) => ({
    graphId,
    branches: branchRecords(instance),
    ...(loopRecords(instance).length
      ? { loops: loopRecords(instance) }
      : {}),
  });
  return {
    version: 2,
    snapshot: {
      version: 1,
      revision: bundle.manifest.revision,
      // A rejected loader rendered the error phase; the browser resumes that
      // phase from the snapshot instead of refetching on first paint.
      routes: [
        {
          routeId: match.route.id,
          params: match.params,
          phase: loader?.state.kind === 'rejected' ? 'error' : 'active',
        },
      ],
      // Public request state: the location is public by contract and cookies
      // are gated at the render boundary. `exports` holds explicitly public
      // values only (PublicExport/validate_public_export in crates/plec-ir);
      // loader outcomes transfer through `loaders` instead.
      public: {
        location: `${context.pathname}${new URL(context.url).search}`,
        exports: {},
      },
      loaders: loader ? [loader] : [],
      structure: {
        graphs: {
          'root/outlet:main': structure(
            'root/outlet:main',
            bundle.manifest.rootGraphId,
          ),
          ...(childGraph
            ? {
                [childGraph.instance]: structure(
                  childGraph.instance,
                  childGraph.graphId,
                ),
              }
            : {}),
        },
      },
    },
  };
}

function matchRoute(
  manifest: Manifest,
  pathname: string,
): { route: Route; params: Record<string, string> } | undefined {
  const parts = pathname
    .replace(/^\/+|\/+$/g, '')
    .split('/')
    .filter(Boolean);
  const candidates = manifest.routes
    .filter(
      (route) =>
        route.path === '' ||
        route.path === '*' ||
        route.path.split('/').length === parts.length,
    )
    // A catch-all is a fallback, never a competing match for the index route.
    .sort(
      (left, right) =>
        Number(left.path === '*') - Number(right.path === '*'),
    );
  for (const route of candidates) {
    if (route.path === '') {
      if (!parts.length) return { route, params: {} };
      continue;
    }
    if (route.path === '*') return { route, params: {} };
    const params: Record<string, string> = {};
    let ok = true;
    route.path.split('/').forEach((segment, index) => {
      const part = parts[index]!;
      if (segment.startsWith('$'))
        params[segment.slice(1)] = decodeURIComponent(part);
      else if (segment !== part) ok = false;
    });
    if (ok) return { route, params };
  }
  return undefined;
}

/** SSR consumes exactly the executable component graph. It deliberately has no JSX/VDOM path. */
function renderApplication(
  bundle: ArtifactBundle,
  route: Route | undefined,
  request: RequestContext,
  gate: SsrRenderGate,
  loader?: LoaderOutcome,
): {
  body: string;
  branches: SsrBranchGraph;
  loops: SsrLoopGraph;
  childGraph?: { instance: string; graphId: string };
} {
  const graph = new Map(
    bundle.graphs.map((entry) => [entry.graphId, entry.graph]),
  );
  const root = graph.get(bundle.manifest.rootGraphId);
  if (!root) throw new Error('root graph missing');
  // A rejected loader rendered the route's error phase, so the outlet child
  // is the error graph the browser will resume into.
  const routeGraphId =
    loader?.state.kind === 'rejected' && route?.errorGraphId
      ? route.errorGraphId
      : route?.graphId;
  const child =
    route && routeGraphId ? graph.get(routeGraphId) : undefined;
  // Instance ids mirror graph_instance_id (crates/plec-runtime): the root
  // graph always mounts at `root/outlet:main`; a route child composes its
  // instance from the escaped parent instance and the outlet id. Branch
  // records are keyed by these ids so the adopter finds its ownership cause
  // verbatim.
  const branches: SsrBranchGraph = new Map([
    ['root/outlet:main', new Map()],
  ]);
  const loops: SsrLoopGraph = new Map([
    ['root/outlet:main', new Map()],
  ]);
  const childGraph =
    child && route
      ? {
          instance: `${escapeInstanceSegment('root/outlet:main')}/outlet:${escapeInstanceSegment(route.outletId)}`,
          graphId: routeGraphId!,
        }
      : undefined;
  if (childGraph) branches.set(childGraph.instance, new Map());
  if (childGraph) loops.set(childGraph.instance, new Map());
  const body = renderApp(root, {
    request,
    outlet: child,
    path: 'root',
    gate,
    loaderData:
      loader?.state.kind === 'resolved'
        ? loader.state.value
        : undefined,
    instance: 'root/outlet:main',
    rootComponent: root.rootComponent,
    branches,
    loops,
  });
  return { body, branches, loops, childGraph };
}
type Scope = {
  request: RequestContext;
  outlet?: App;
  path: string;
  props?: unknown[];
  componentProps?: Record<number, { app: App; component: number }>;
  states?: unknown[];
  row?: Record<string, unknown>;
  rowIndex?: number;
  rowKey?: string;
  rowRoot?: boolean;
  slot?: { app: App; component: number; nodes: number[]; scope: Scope };
  gate?: SsrRenderGate;
  loaderData?: unknown;
  instance?: string;
  rootComponent?: number;
  branches?: SsrBranchGraph;
  loops?: SsrLoopGraph;
};
function renderApp(app: App, scope: Scope): string {
  return renderComponent(app, app.rootComponent, scope);
}
function renderComponent(
  app: App,
  componentIndex: number,
  scope: Scope,
): string {
  const component = app.components[componentIndex]!;
  const states = component.stateSlots.map((slot) =>
    evaluate(component, slot.initialExpression, {
      ...scope,
      states: [],
    }),
  );
  return renderNode(app, componentIndex, component.rootNode, {
    ...scope,
    states,
  });
}
function renderNode(
  app: App,
  componentIndex: number,
  index: number,
  scope: Scope,
): string {
  const component = app.components[componentIndex]!;
  const node = component.nodes[index];
  if (!node) throw new Error(`missing node ${componentIndex}:${index}`);
  if (node.op === 'text') {
    const text = component.texts[node.text!]!;
    const binding =
      text.binding === undefined
        ? (text.value ?? '')
        : String(
            evaluate(
              component,
              component.bindings[text.binding]!.expression,
              scope,
            ) ?? '',
          );
    return `<!--plec:text:${scope.path}:${index}-->${escapeHtml(binding)}`;
  }
  if (node.op === 'conditional') {
    const truthy = truthyValue(evaluate(component, node.test!, scope));
    const selected: BranchSide = truthy
      ? 'consequent'
      : node.alternate == null
        ? 'none'
        : 'alternate';
    // Only root-component conditionals are recordable: snapshot branch
    // records address nodes within the graph's own component table.
    if (
      scope.branches &&
      scope.instance &&
      componentIndex === scope.rootComponent &&
      !scope.row
    )
      scope.branches.get(scope.instance)!.set(index, selected);
    const inner = truthy
      ? renderNode(app, componentIndex, node.consequent!, scope)
      : node.alternate == null
        ? ''
        : renderNode(app, componentIndex, node.alternate, scope);
    // The boundary grammar matches the runtime's marker-index adoption
    // contract (`plec:conditional:{path}:{index}` / `-end`); the region
    // between the markers is the branch DOM the adopter will claim.
    return `<!--plec:conditional:${scope.path}:${index}-->${inner}<!--plec:conditional-end:${scope.path}:${index}-->`;
  }
  if (node.op === 'loop') {
    const loop = component.loops[node.loop!];
    if (!loop)
      throw new Error(`missing loop ${componentIndex}:${node.loop}`);
    const values = evaluate(component, loop.sourceExpression, scope);
    if (!Array.isArray(values))
      throw new Error('LOOP_SOURCE_NOT_ARRAY');
    const keys: string[] = [];
    const rows = values
      .map((value, rowIndex) => {
        if (
          value === null ||
          typeof value !== 'object' ||
          Array.isArray(value)
        )
          throw new Error('LOOP_ROW_NOT_OBJECT');
        const row = value as Record<string, unknown>;
        const key = canonicalKey(
          evaluate(component, loop.keyExpression, {
            ...scope,
            row,
            rowIndex,
          }),
        );
        if (keys.includes(key))
          throw new Error(`DUPLICATE_LOOP_KEY:${key}`);
        keys.push(key);
        const rowPath = `${scope.path}/loop:${index}/key:${escapeInstanceSegment(key)}`;
        return `<!--plec:loop:${rowPath}-->${renderNode(app, componentIndex, loop.rowTemplate, { ...scope, path: rowPath, row, rowIndex, rowKey: key, rowRoot: true })}<!--plec:loop-end:${rowPath}-->`;
      })
      .join('');
    if (
      scope.loops &&
      scope.instance &&
      componentIndex === scope.rootComponent
    )
      scope.loops.get(scope.instance)!.set(index, keys);
    return rows;
  }
  if (node.op === 'slot')
    return `<!--plec:slot:${scope.path}:${index}-->${(scope.slot?.nodes ?? []).map((child) => renderNode(scope.slot!.app, scope.slot!.component, child, scope.slot!.scope)).join('')}<!--plec:slot-end:${scope.path}:${index}-->`;
  if (node.op === 'component' || node.op === 'dynamicComponent') {
    const dynamic = node.op === 'dynamicComponent';
    const target = dynamic
      ? scope.componentProps?.[node.prop!]
      : { app, component: node.component! };
    if (!target)
      throw new Error(
        `SSR dynamic component is unavailable at ${scope.path}:${index}`,
      );
    const child = target.app.components[target.component]!;
    const props: unknown[] = [];
    // Dynamic targets keep their props as named values (`className`), while a
    // direct `(props)` parameter reads the whole named record — the SSR
    // mirror of `component_runtime_props` (crates/plec-runtime).
    const namedProps: Record<string, unknown> = {};
    const componentProps: Record<
      number,
      { app: App; component: number }
    > = {};
    for (const prop of node.props ?? []) {
      const name = component.strings[prop.name]!;
      if (prop.kind === 'component') {
        if (prop.component !== undefined) {
          const parameter = child.parameters.findIndex(
            (candidate) => child.strings[candidate.name] === name,
          );
          if (parameter >= 0)
            componentProps[parameter] = { app, component: prop.component };
        }
        continue;
      }
      if (prop.kind !== 'value') continue;
      const value = evaluate(component, prop.expression!, scope);
      namedProps[name] = value;
      const parameter = child.parameters.findIndex(
        (candidate) => child.strings[candidate.name] === name,
      );
      if (parameter >= 0) props[parameter] = value;
    }
    const directParameter = child.parameters.findIndex(
      (candidate) => child.strings[candidate.name] === '__plec_props',
    );
    if (
      directParameter >= 0 &&
      props[directParameter] === undefined &&
      !node.props?.some(
        (prop) => component.strings[prop.name] === '__plec_props',
      )
    )
      props[directParameter] = namedProps;
    const componentPath = `${scope.path}/component:${index}`;
    return `<!--plec:component:${scope.path}:${index}-->${renderComponent(target.app, target.component, { ...scope, props, componentProps, path: componentPath, slot: { app, component: componentIndex, nodes: node.children ?? [], scope } })}<!--plec:component-end:${scope.path}:${index}-->`;
  }
  if (node.op !== 'element') return '';
  const tag = component.strings[node.tag!]!;
  // Writes keep program order: a spread bag is written where it occurs, and
  // later writes overwrite earlier attribute names (same-key overwrites keep
  // their original position), mirroring the runtime's sequential application.
  const attributes = new Map<string, string | null>();
  const writeAttribute = (name: string, value: unknown) => {
    if (name.startsWith('on')) return;
    if (value === false || value === null || value === undefined) return;
    const attr = name === 'className' ? 'class' : name;
    attributes.set(attr, value === true ? null : String(value));
  };
  component.propPrograms
    .filter((program) => program.target === index)
    .flatMap((program) => program.writes)
    .forEach((write) => {
      // Component props reach an element through a `{...props}` spread
      // (e.g. generated icons); serialize the record so the first paint
      // already carries final attributes like `class`. This mirrors
      // `typed_apply_spread` (crates/plec-runtime dom/bindings).
      if (write.spread) {
        const bag = evaluate(
          component,
          write.expression!,
          scope,
        ) as Record<string, unknown> | null | undefined;
        if (bag !== null && bag !== undefined && typeof bag === 'object')
          for (const [name, value] of Object.entries(bag))
            writeAttribute(name, value);
        return;
      }
      const name = component.strings[write.name ?? -1];
      if (!name) return;
      const value =
        write.expression === undefined
          ? component.constants[write.constant!]
          : evaluate(component, write.expression, scope);
      writeAttribute(name, value);
    });
  if (scope.rowRoot && scope.rowKey !== undefined)
    attributes.set(
      'data-runtime-row-key',
      escapeAttribute(scope.rowKey),
    );
  attributes.set(
    'data-plec-node',
    escapeAttribute(`${scope.path}/node:${index}`),
  );
  const attributeText = [...attributes]
    .map(([attr, value]) =>
      value === null ? attr : `${attr}="${escapeAttribute(value)}"`,
    )
    .join(' ');
  const children = (node.children ?? [])
    .map((child) =>
      renderNode(app, componentIndex, child, {
        ...scope,
        rowRoot: false,
      }),
    )
    .join('');
  const outlet = component.routeOutlets?.find(
    (entry) => entry.node === index,
  );
  const outletHtml =
    outlet && scope.outlet
      ? renderApp(scope.outlet, {
          ...scope,
          outlet: undefined,
          path: `${scope.path}/outlet:${outlet.id}`,
          instance: `${escapeInstanceSegment(scope.instance!)}/outlet:${escapeInstanceSegment(outlet.id)}`,
          rootComponent: scope.outlet.rootComponent,
        })
      : '';
  return `<${tag}${attributeText ? ` ${attributeText}` : ''}>${children}${outletHtml}</${tag}>`;
}
function evaluate(
  component: Component,
  expression: number,
  scope: Scope,
): unknown {
  const stack: unknown[] = [];
  const instructions =
    component.expressions[expression]?.instructions ?? [];
  for (let pc = 0; pc < instructions.length; pc += 1) {
    const instruction = instructions[pc]!;
    switch (instruction.op) {
      case 'constant':
        stack.push(component.constants[instruction.constant as number]);
        break;
      case 'loadState':
        stack.push(scope.states?.[instruction.state as number]);
        break;
      case 'loadProp':
        stack.push(scope.props?.[instruction.prop as number]);
        break;
      case 'loadHost': {
        const slot = component.hostSlots?.[instruction.host as number];
        const cookieName =
          slot?.name === undefined
            ? undefined
            : component.strings[slot.name];
        if (slot?.kind === 'cookie') {
          if (scope.gate?.development)
            scope.gate.gated.add(`cookie:${cookieName ?? '<unnamed>'}`);
          stack.push(undefined);
          break;
        }
        if (slot?.kind === 'loaderData') {
          stack.push(scope.loaderData);
          break;
        }
        stack.push(
          slot?.kind === 'location'
            ? {
                pathname: scope.request.pathname,
                search: new URL(scope.request.url).search,
              }
            : undefined,
        );
        break;
      }
      case 'loadRowField':
        stack.push(
          scope.row?.[component.strings[instruction.field as number]!],
        );
        break;
      case 'loadRowRecord':
        stack.push(scope.row);
        break;
      case 'field': {
        const value = stack.pop() as
          Record<string, unknown> | undefined;
        stack.push(
          value?.[component.strings[instruction.field as number]!],
        );
        break;
      }
      case 'unary': {
        const value = stack.pop();
        stack.push(
          instruction.kind === 'not'
            ? !value
            : instruction.kind === 'minus'
              ? -Number(value)
              : value,
        );
        break;
      }
      case 'binary': {
        const right = stack.pop();
        const left = stack.pop();
        stack.push(binary(String(instruction.kind), left, right));
        break;
      }
      case 'makeRecord': {
        const fields = instruction.fields as number[];
        const value: Record<string, unknown> = {};
        for (let i = fields.length - 1; i >= 0; i -= 1)
          value[component.strings[fields[i]!]!] = stack.pop();
        stack.push(value);
        break;
      }
      case 'makeArray': {
        const count = instruction.count as number;
        stack.push(
          stack.splice(Math.max(0, stack.length - count), count),
        );
        break;
      }
      case 'jump':
        pc = Number(instruction.target) - 1;
        break;
      case 'jumpIfFalse':
        if (!stack.pop()) pc = Number(instruction.target) - 1;
        break;
      case 'return':
        return stack.pop();
    }
  }
  return stack.pop();
}
function binary(kind: string, left: unknown, right: unknown) {
  switch (kind) {
    case 'equal':
      return left === right;
    case 'notEqual':
      return left !== right;
    case 'and':
      return left && right;
    case 'or':
      return left || right;
    case 'add':
      return typeof left === 'string' || typeof right === 'string'
        ? `${left ?? ''}${right ?? ''}`
        : Number(left) + Number(right);
    case 'greaterEqual':
      return Number(left) >= Number(right);
    default:
      return undefined;
  }
}
function canonicalKey(value: unknown): string {
  if (value === null || value === undefined) return '';
  if (typeof value === 'string') return value;
  if (typeof value === 'boolean' || typeof value === 'number')
    return String(value);
  return JSON.stringify(value) ?? '';
}
/** Mirrors `typed_truthy` (crates/plec-runtime eval/typed_vm) so the branch
 * the server instantiates is exactly the branch the runtime reconciles to. */
function truthyValue(value: unknown): boolean {
  if (value === null || value === undefined) return false;
  if (typeof value === 'boolean') return value;
  if (typeof value === 'number') return value !== 0;
  if (typeof value === 'string') return value.length > 0;
  if (Array.isArray(value)) return value.length > 0;
  return true;
}
function escapeHtml(value: string) {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');
}
function escapeAttribute(value: string) {
  return escapeHtml(value).replace(/"/g, '&quot;');
}
