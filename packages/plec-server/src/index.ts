/**
 * The intentionally small Node host for compiled Plec applications.  It owns
 * HTTP mechanics only; application API handlers are an explicit temporary
 * escape hatch and are not part of Plec's semantic server model.
 */
import { createServer, type IncomingMessage, type Server, type ServerResponse } from 'node:http';
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

type ArtifactBundle = { manifest: Manifest; graphs: Array<{ graphId: string; graph: App }> };
type Manifest = { revision: string; rootGraphId: string; routes: Route[] };
type Route = { id: string; path: string; graphId: string; outletId: string; meta?: DocumentMetadata };
type DocumentMetadata = { title?: string; description?: string };
type App = { rootComponent: number; components: Component[] };
type Component = {
  rootNode: number; strings: string[]; constants: unknown[]; nodes: Node[]; texts: Array<{ value?: string; binding?: number }>;
  bindings: Array<{ target: number; sink: string; name?: number; expression: number }>;
  propPrograms: Array<{ target: number; writes: Array<{ name?: number; kind: string; constant?: number; expression?: number }> }>;
  hostSlots?: Array<{ kind: string; name?: number }>;
  stateSlots: Array<{ initialExpression: number }>; parameters: Array<{ name: number }>;
  expressions: Array<{ instructions: Instruction[] }>; loops: Array<{ sourceExpression: number; keyExpression: number; itemSlot: number; rowTemplate: number }>;
  routeOutlets?: Array<{ id: string; node: number }>;
};
type Node = { op: string; tag?: number; namespace?: string; children?: number[]; text?: number; test?: number; consequent?: number; alternate?: number; component?: number; prop?: number; props?: Array<{ kind: string; name: number; expression?: number; component?: number }>; loop?: number };
type Instruction = Record<string, unknown> & { op: string };

const contentTypes: Record<string, string> = {
  '.css': 'text/css; charset=utf-8', '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8',
  '.json': 'application/json; charset=utf-8', '.wasm': 'application/wasm', '.woff2': 'font/woff2',
};

export function createPlecServer(options: PlecServerOptions): Server {
  return createServer(async (incoming, outgoing) => {
    const request = await toRequest(incoming);
    const context = requestContext(request);
    if (context.pathname.startsWith('/api/')) {
      const response = await options.handleAppRequest?.(request, context);
      return response ? sendFetchResponse(outgoing, response) : sendJson(outgoing, 404, { error: 'endpoint not found' });
    }
    if (isDocumentRequest(context.pathname)) {
      try {
        const artifact = JSON.parse(await readFile(options.artifactPath, 'utf8')) as ArtifactBundle;
        const match = matchRoute(artifact.manifest, context.pathname);
        const document = match?.route.meta ?? options.document ?? {};
        const body = renderApplication(artifact, match?.route, context);
        const bootstrap = JSON.stringify(bootstrapPayload(artifact, match, context)).replace(/</g, '\\u003c');
        return sendHtml(outgoing, document, body, bootstrap, options);
      } catch (error) {
        // A fixture without compiler artifacts remains useful for HTTP-host
        // tests. Real Plec builds always supply the artifact and therefore
        // take the SSR path above.
        try {
          const shell = await readFile(path.join(options.publicDir, 'index.html'));
          outgoing.writeHead(200, { 'content-type': 'text/html; charset=utf-8', 'cache-control': 'no-cache', ...(options.development ? { 'x-plec-ssr-fallback': String(error) } : {}) });
          return outgoing.end(shell);
        } catch { return sendJson(outgoing, 500, { error: `Plec SSR failed: ${String(error)}` }); }
      }
    }
    return serveAsset(outgoing, incoming, context.pathname, options.publicDir);
  });
}

export function serve(server: Server, port = Number(process.env.PORT ?? 3000)): Server {
  server.listen(port);
  return server;
}

async function toRequest(incoming: IncomingMessage): Promise<Request> {
  const origin = `http://${incoming.headers.host ?? 'localhost'}`;
  const body = ['GET', 'HEAD'].includes(incoming.method ?? 'GET') ? undefined : await readBody(incoming);
  return new Request(new URL(incoming.url ?? '/', origin), { method: incoming.method, headers: incoming.headers as HeadersInit, body: body && new Uint8Array(body) });
}

async function readBody(request: IncomingMessage): Promise<Buffer> {
  const chunks: Buffer[] = [];
  for await (const chunk of request) chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk));
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
  return { url: url.href, pathname: url.pathname, method: request.method, headers, cookies: parseCookies(headers.cookie), params: {}, query };
}

function parseCookies(header = ''): Record<string, string> {
  return Object.fromEntries(header.split(';').flatMap((part) => {
    const index = part.indexOf('=');
    return index < 0 ? [] : [[part.slice(0, index).trim(), decodeURIComponent(part.slice(index + 1).trim())]];
  }));
}

function isDocumentRequest(pathname: string) { return pathname === '/' || !path.extname(pathname); }

async function serveAsset(response: ServerResponse, request: IncomingMessage, pathname: string, publicDir: string) {
  const requested = pathname.replace(/^\/+/, '');
  const filePath = path.resolve(publicDir, requested);
  const root = path.resolve(publicDir);
  if (!filePath.startsWith(`${root}${path.sep}`)) return sendJson(response, 400, { error: 'invalid asset path' });
  const encoding = selectEncoding(request.headers['accept-encoding']);
  const compressed = encoding && pathname.startsWith('/runtime/') ? `${filePath}.${encoding === 'gzip' ? 'gz' : 'br'}` : undefined;
  try {
    const [body, compressedServed] = compressed
      ? await readFile(compressed).then((value) => [value, true] as const).catch(() => readFile(filePath).then((value) => [value, false] as const))
      : [await readFile(filePath), false] as const;
    response.writeHead(200, { 'content-type': contentTypes[path.extname(filePath)] ?? 'application/octet-stream', 'cache-control': 'no-cache', ...(encoding && compressedServed ? { 'content-encoding': encoding, vary: 'Accept-Encoding' } : {}) });
    response.end(body);
  } catch { sendJson(response, 404, { error: 'asset not found' }); }
}

function selectEncoding(header: string | string[] | undefined): 'br' | 'gzip' | undefined {
  const value = Array.isArray(header) ? header.join(',') : header ?? '';
  return /\bbr\b/.test(value) ? 'br' : /\bgzip\b/.test(value) ? 'gzip' : undefined;
}

function sendFetchResponse(response: ServerResponse, value: Response) {
  const headers = Object.fromEntries(value.headers.entries());
  response.writeHead(value.status, headers);
  void value.arrayBuffer().then((body) => response.end(Buffer.from(body)));
}
function sendJson(response: ServerResponse, status: number, value: unknown) { response.writeHead(status, { 'content-type': 'application/json; charset=utf-8', 'cache-control': 'no-store' }); response.end(JSON.stringify(value)); }
function sendHtml(response: ServerResponse, metadata: DocumentMetadata, body: string, bootstrap: string, options: PlecServerOptions) {
  const title = escapeHtml(metadata.title ?? 'Plec application');
  const description = escapeHtml(metadata.description ?? '');
  const styles = options.stylesHref ? `<link rel="stylesheet" href="${escapeAttribute(options.stylesHref)}">` : '';
  const script = options.clientScript ? `<script type="module" src="${escapeAttribute(options.clientScript)}"></script>` : '';
  response.writeHead(200, { 'content-type': 'text/html; charset=utf-8', 'cache-control': 'no-cache' });
  response.end(`<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>${title}</title>${description ? `<meta name="description" content="${escapeAttribute(description)}">` : ''}${styles}</head><body><div id="app">${body}</div><script id="plec-bootstrap" type="application/json">${bootstrap}</script>${script}</body></html>`);
}

/** The v2 bootstrap carries the typed SSR execution snapshot (see the
 * `PlecSsrSnapshot` contract in crates/plec-ir). Without a matched route there
 * is nothing to resume, so the legacy v1 shape is emitted and the browser
 * treats the page as non-snapshot SSR. */
function bootstrapPayload(bundle: ArtifactBundle, match: ReturnType<typeof matchRoute> | undefined, context: RequestContext) {
  if (!match?.route) {
    return {
      version: 1, revision: bundle.manifest.revision, routeId: null,
      public: { location: { pathname: context.pathname, search: new URL(context.url).search } },
    };
  }
  return {
    version: 2,
    snapshot: {
      version: 1,
      revision: bundle.manifest.revision,
      routes: [{ routeId: match.route.id, params: match.params, phase: 'active' }],
      public: { location: `${context.pathname}${new URL(context.url).search}` },
      loaders: [],
      structure: {
        graphs: {
          'root/outlet:main': { graphId: bundle.manifest.rootGraphId },
        },
      },
    },
  };
}

function matchRoute(manifest: Manifest, pathname: string): { route: Route; params: Record<string, string> } | undefined {
  const parts = pathname.replace(/^\/+|\/+$/g, '').split('/').filter(Boolean);
  const candidates = manifest.routes
    .filter((route) => route.path === '' || route.path === '*' || route.path.split('/').length === parts.length)
    // A catch-all is a fallback, never a competing match for the index route.
    .sort((left, right) => Number(left.path === '*') - Number(right.path === '*'));
  for (const route of candidates) {
    if (route.path === '') { if (!parts.length) return { route, params: {} }; continue; }
    if (route.path === '*') return { route, params: {} };
    const params: Record<string, string> = {}; let ok = true;
    route.path.split('/').forEach((segment, index) => { const part = parts[index]!; if (segment.startsWith('$')) params[segment.slice(1)] = decodeURIComponent(part); else if (segment !== part) ok = false; });
    if (ok) return { route, params };
  }
  return undefined;
}

/** SSR consumes exactly the executable component graph. It deliberately has no JSX/VDOM path. */
function renderApplication(bundle: ArtifactBundle, route: Route | undefined, request: RequestContext): string {
  const graph = new Map(bundle.graphs.map((entry) => [entry.graphId, entry.graph]));
  const root = graph.get(bundle.manifest.rootGraphId);
  if (!root) throw new Error('root graph missing');
  const child = route ? graph.get(route.graphId) : undefined;
  return renderApp(root, { request, outlet: child, path: 'root' });
}
type Scope = { request: RequestContext; outlet?: App; path: string; props?: unknown[]; componentProps?: Record<number, { app: App; component: number }>; states?: unknown[]; row?: Record<string, unknown>; slot?: { app: App; component: number; nodes: number[]; scope: Scope } };
function renderApp(app: App, scope: Scope): string { return renderComponent(app, app.rootComponent, scope); }
function renderComponent(app: App, componentIndex: number, scope: Scope): string {
  const component = app.components[componentIndex]!;
  const states = component.stateSlots.map((slot) => evaluate(component, slot.initialExpression, { ...scope, states: [] }));
  return renderNode(app, componentIndex, component.rootNode, { ...scope, states });
}
function renderNode(app: App, componentIndex: number, index: number, scope: Scope): string {
  const component = app.components[componentIndex]!; const node = component.nodes[index];
  if (!node) throw new Error(`missing node ${componentIndex}:${index}`);
  if (node.op === 'text') { const text = component.texts[node.text!]!; const binding = text.binding === undefined ? text.value ?? '' : String(evaluate(component, component.bindings[text.binding]!.expression, scope) ?? ''); return `<!--plec:text:${scope.path}:${index}-->${escapeHtml(binding)}`; }
  if (node.op === 'conditional') return evaluate(component, node.test!, scope) ? renderNode(app, componentIndex, node.consequent!, scope) : node.alternate == null ? '' : renderNode(app, componentIndex, node.alternate, scope);
  if (node.op === 'loop') return '';
  if (node.op === 'slot') return `<!--plec:slot:${scope.path}:${index}-->${(scope.slot?.nodes ?? []).map((child) => renderNode(scope.slot!.app, scope.slot!.component, child, scope.slot!.scope)).join('')}<!--plec:slot-end:${scope.path}:${index}-->`;
  if (node.op === 'component' || node.op === 'dynamicComponent') {
    const dynamic = node.op === 'dynamicComponent';
    const target = dynamic ? scope.componentProps?.[node.prop!] : { app, component: node.component! };
    if (!target) throw new Error(`SSR dynamic component is unavailable at ${scope.path}:${index}`);
    const child = target.app.components[target.component]!;
    const props: unknown[] = [];
    const componentProps: Record<number, { app: App; component: number }> = {};
    for (const prop of node.props ?? []) {
      const name = component.strings[prop.name]!;
      const parameter = child.parameters.findIndex((candidate) => child.strings[candidate.name] === name);
      if (parameter < 0) continue;
      if (prop.kind === 'value') props[parameter] = evaluate(component, prop.expression!, scope);
      if (prop.kind === 'component' && prop.component !== undefined) componentProps[parameter] = { app, component: prop.component };
    }
    return `<!--plec:component:${scope.path}:${index}-->${renderComponent(target.app, target.component, { ...scope, props, componentProps, path: `${scope.path}/component:${index}`, slot: { app, component: componentIndex, nodes: node.children ?? [], scope } })}<!--plec:component-end:${scope.path}:${index}-->`;
  }
  if (node.op !== 'element') return '';
  const tag = component.strings[node.tag!]!; const props = new Map<number, { name?: number; kind: string; constant?: number; expression?: number }>();
  component.propPrograms.filter((program) => program.target === index).flatMap((program) => program.writes).forEach((write) => props.set(write.name ?? -1, write));
  const attributes = [...props.values()].flatMap((write) => {
    const name = component.strings[write.name ?? -1]; if (!name || name.startsWith('on')) return [];
    const value = write.expression === undefined ? component.constants[write.constant!] : evaluate(component, write.expression, scope);
    if (value === false || value === null || value === undefined) return [];
    const attr = name === 'className' ? 'class' : name;
    return value === true ? [attr] : [`${attr}="${escapeAttribute(String(value))}"`];
  });
  attributes.push(`data-plec-node="${escapeAttribute(`${scope.path}/node:${index}`)}"`);
  const children = (node.children ?? []).map((child) => renderNode(app, componentIndex, child, scope)).join('');
  const outlet = component.routeOutlets?.find((entry) => entry.node === index);
  const outletHtml = outlet && scope.outlet ? renderApp(scope.outlet, { ...scope, outlet: undefined, path: `${scope.path}/outlet:${outlet.id}` }) : '';
  return `<${tag}${attributes.length ? ` ${attributes.join(' ')}` : ''}>${children}${outletHtml}</${tag}>`;
}
function evaluate(component: Component, expression: number, scope: Scope): unknown {
  const stack: unknown[] = []; const instructions = component.expressions[expression]?.instructions ?? [];
  for (let pc = 0; pc < instructions.length; pc += 1) { const instruction = instructions[pc]!; switch (instruction.op) {
    case 'constant': stack.push(component.constants[instruction.constant as number]); break;
    case 'loadState': stack.push(scope.states?.[instruction.state as number]); break;
    case 'loadProp': stack.push(scope.props?.[instruction.prop as number]); break;
    case 'loadHost': { const slot = component.hostSlots?.[instruction.host as number]; const cookieName = slot?.name === undefined ? undefined : component.strings[slot.name]; stack.push(slot?.kind === 'location' ? { pathname: scope.request.pathname, search: new URL(scope.request.url).search } : slot?.kind === 'cookie' && cookieName ? scope.request.cookies[cookieName] : undefined); break; }
    case 'loadRowField': stack.push(scope.row?.[component.strings[instruction.field as number]!]); break;
    case 'field': { const value = stack.pop() as Record<string, unknown> | undefined; stack.push(value?.[component.strings[instruction.field as number]!]); break; }
    case 'unary': { const value = stack.pop(); stack.push(instruction.kind === 'not' ? !value : instruction.kind === 'minus' ? -Number(value) : value); break; }
    case 'binary': { const right = stack.pop(); const left = stack.pop(); stack.push(binary(String(instruction.kind), left, right)); break; }
    case 'makeRecord': { const fields = instruction.fields as number[]; const value: Record<string, unknown> = {}; for (let i = fields.length - 1; i >= 0; i -= 1) value[component.strings[fields[i]!]!] = stack.pop(); stack.push(value); break; }
    case 'makeArray': { const count = instruction.count as number; stack.push(stack.splice(Math.max(0, stack.length - count), count)); break; }
    case 'jump': pc = Number(instruction.target) - 1; break;
    case 'jumpIfFalse': if (!stack.pop()) pc = Number(instruction.target) - 1; break;
    case 'return': return stack.pop();
  }} return stack.pop();
}
function binary(kind: string, left: unknown, right: unknown) { switch (kind) { case 'equal': return left === right; case 'notEqual': return left !== right; case 'and': return left && right; case 'or': return left || right; case 'add': return typeof left === 'string' || typeof right === 'string' ? `${left ?? ''}${right ?? ''}` : Number(left) + Number(right); case 'greaterEqual': return Number(left) >= Number(right); default: return undefined; } }
function escapeHtml(value: string) { return value.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;'); }
function escapeAttribute(value: string) { return escapeHtml(value).replace(/"/g, '&quot;'); }
