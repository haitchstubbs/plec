import { mkdir, mkdtemp, writeFile } from 'node:fs/promises';
import { connect } from 'node:net';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { PassThrough } from 'node:stream';
import { afterEach, describe, expect, it } from 'vitest';
import {
  start,
  toWebRequest,
  buildContext,
  parseCookies,
  parseQuery,
  type StartOptions,
} from './runtime';

// The sidecar always authenticates with this header before any dispatch.
const TOKEN = 'test-token-0123456789abcdef';
const CONTEXT = {
  url: 'https://example.test/action',
  pathname: '/action',
  method: 'POST',
  headers: { cookie: 'session=one' },
  cookies: { session: 'one' },
  params: {},
  query: {},
};

const servers: Array<Awaited<ReturnType<typeof start>>> = [];
afterEach(async () => {
  await Promise.all(
    servers.splice(0).map(
      (server) =>
        new Promise<void>((resolve) => {
          server.closeAllConnections?.();
          server.close(() => resolve());
        }),
    ),
  );
});

/** Spawns the real runtime script against an ephemeral loopback port. */
async function startRuntime(
  applicationModule: string,
): Promise<string> {
  const dir = await mkdtemp(path.join(tmpdir(), 'plec-node-runtime-'));
  const bundle = path.join(dir, 'app.mjs');
  await writeFile(bundle, applicationModule);
  const options: StartOptions = {
    socket: 'tcp:127.0.0.1:0',
    bundle,
    token: TOKEN,
  };
  const server = await start(options);
  servers.push(server);
  const address = server.address();
  if (address === null || typeof address === 'string') {
    throw new Error('runtime tcp address missing');
  }
  return `http://127.0.0.1:${address.port}`;
}

interface RequestOptions {
  headers?: Record<string, string>;
  method?: string;
  body?: string;
}

function request(
  origin: string,
  target: string,
  options: RequestOptions = {},
): Promise<Response> {
  const { headers, ...rest } = options;
  return fetch(`${origin}${target}`, {
    ...rest,
    headers: { 'x-plec-internal-token': TOKEN, ...(headers ?? {}) },
  });
}

describe('sidecar supervision', () => {
  it('lets Node reject conflicting HTTP framing before application dispatch', async () => {
    const key = `plecFramingDispatches${Date.now()}`;
    (globalThis as Record<string, unknown>)[key] = 0;
    const origin = await startRuntime(`
      export function handleRequest() {
        globalThis[${JSON.stringify(key)}] += 1;
        return new Response('unexpected');
      }
    `);
    const address = new URL(origin);
    const response = await new Promise<string>((resolve, reject) => {
      const socket = connect(Number(address.port), address.hostname);
      let bytes = '';
      socket.setTimeout(3000, () => {
        socket.destroy(
          new Error('Node did not reject the malformed request'),
        );
      });
      socket.on('connect', () => {
        socket.write(
          'POST /api HTTP/1.1\r\n' +
            `Host: ${address.host}\r\n` +
            `x-plec-internal-token: ${TOKEN}\r\n` +
            'Content-Length: 3\r\n' +
            'Transfer-Encoding: chunked\r\n' +
            '\r\n' +
            '0\r\n\r\n',
        );
      });
      socket.on('data', (chunk: Buffer) => {
        bytes += chunk.toString('latin1');
      });
      socket.on('error', reject);
      socket.on('close', () => resolve(bytes));
    });

    expect(response).toMatch(/^HTTP\/1\.1 400\b/);
    expect((globalThis as Record<string, unknown>)[key]).toBe(0);
    delete (globalThis as Record<string, unknown>)[key];
  });

  it('rejects conflicting duplicate Content-Length values before dispatch', async () => {
    const key = `plecDuplicateLengthDispatches${Date.now()}`;
    (globalThis as Record<string, unknown>)[key] = 0;
    const origin = await startRuntime(`
      export function handleRequest() {
        globalThis[${JSON.stringify(key)}] += 1;
        return new Response('unexpected');
      }
    `);
    const address = new URL(origin);
    const response = await new Promise<string>((resolve, reject) => {
      const socket = connect(Number(address.port), address.hostname);
      let bytes = '';
      socket.setTimeout(3000, () => {
        socket.destroy(
          new Error('Node did not reject duplicate Content-Length'),
        );
      });
      socket.on('connect', () => {
        socket.write(
          'POST /api HTTP/1.1\r\n' +
            `Host: ${address.host}\r\n` +
            `x-plec-internal-token: ${TOKEN}\r\n` +
            'Content-Length: 3\r\n' +
            'Content-Length: 4\r\n' +
            '\r\n' +
            'data',
        );
      });
      socket.on('data', (chunk: Buffer) => {
        bytes += chunk.toString('latin1');
      });
      socket.on('error', reject);
      socket.on('close', () => resolve(bytes));
    });

    expect(response).toMatch(/^HTTP\/1\.1 400\b/);
    expect((globalThis as Record<string, unknown>)[key]).toBe(0);
    delete (globalThis as Record<string, unknown>)[key];
  });

  it('rejects SSR provider modules outside the emitted client asset root', async () => {
    const dir = await mkdtemp(
      path.join(tmpdir(), 'plec-provider-path-'),
    );
    const bundle = path.join(dir, 'app.mjs');
    const manifest = path.join(dir, 'client/host-providers.json');
    await mkdir(path.dirname(manifest), { recursive: true });
    await writeFile(bundle, 'export function handleRequest() {}');
    await writeFile(
      manifest,
      JSON.stringify({
        version: 2,
        revision: 'test',
        providers: [
          {
            id: 'icons',
            module: '/_plec/assets/../../server/private.mjs',
            components: ['Icon'],
            ssr: true,
          },
        ],
      }),
    );
    await expect(
      start({
        socket: 'tcp:127.0.0.1:0',
        bundle,
        token: TOKEN,
        providerManifest: manifest,
      }),
    ).rejects.toThrow('invalid host provider module for icons');
  });

  it('invokes only registered generated server actions over the authenticated protocol', async () => {
    const origin = await startRuntime(`
      export async function handleRequest() {}
      const actions = new Map([['sa_echo', async (value) => ({ echoed: value })]]);
      export function hasAction(id) { return actions.has(id); }
      export async function invokeAction(id, args) {
        const action = actions.get(id);
        if (typeof action !== 'function') throw new Error('unknown action');
        return action(...args);
      }
    `);
    const accepted = await request(origin, '/_plec-runtime/action', {
      method: 'POST',
      body: JSON.stringify({
        id: 'sa_echo',
        arguments: ['value'],
        context: CONTEXT,
      }),
      headers: { 'content-type': 'application/json' },
    });
    expect(accepted.status).toBe(200);
    expect(await accepted.json()).toEqual({
      ok: true,
      value: { echoed: 'value' },
    });
    const unknown = await request(origin, '/_plec-runtime/action', {
      method: 'POST',
      body: JSON.stringify({
        id: 'sa_missing',
        arguments: [],
        context: CONTEXT,
      }),
      headers: { 'content-type': 'application/json' },
    });
    expect(unknown.status).toBe(404);
  });

  it('keeps concurrent action request contexts isolated through invocation', async () => {
    const origin = await startRuntime(`
      import { AsyncLocalStorage } from 'node:async_hooks';
      export async function handleRequest() {}
      const contexts = new AsyncLocalStorage();
      export function hasAction(id) { return id === 'sa_context'; }
      export async function invokeAction(id, args, context) {
        return contexts.run(context, async () => {
          await new Promise((resolve) => setTimeout(resolve, args[0]));
          return { sessionObserved: contexts.getStore().cookies.session === args[1] };
        });
      }
    `);
    const invoke = (delay: number, session: string) =>
      request(origin, '/_plec-runtime/action', {
        method: 'POST',
        body: JSON.stringify({
          id: 'sa_context',
          arguments: [delay, session],
          context: { ...CONTEXT, cookies: { session } },
        }),
        headers: { 'content-type': 'application/json' },
      });
    const [first, second] = await Promise.all([
      invoke(15, 'first'),
      invoke(0, 'second'),
    ]);
    expect(await first.json()).toMatchObject({
      value: { sessionObserved: true },
    });
    expect(await second.json()).toMatchObject({
      value: { sessionObserved: true },
    });
  });

  it('redacts server-action implementation exceptions', async () => {
    const origin = await startRuntime(`
      export async function handleRequest() {}
      export function hasAction(id) { return id === 'sa_boom'; }
      export async function invokeAction() { throw new Error('PRIVATE_ACTION_SECRET'); }
    `);
    const response = await request(origin, '/_plec-runtime/action', {
      method: 'POST',
      body: JSON.stringify({
        id: 'sa_boom',
        arguments: [],
        context: CONTEXT,
      }),
      headers: { 'content-type': 'application/json' },
    });
    expect(response.status).toBe(500);
    const body = await response.text();
    expect(body).toContain('Internal Server Error');
    expect(body).not.toContain('PRIVATE_ACTION_SECRET');
  });

  it('refuses requests without the internal token before any dispatch', async () => {
    const origin = await startRuntime(`
      export async function handleRequest() {
        return new Response('should never run');
      }
    `);
    const rejected = await fetch(`${origin}/api/todos`);
    expect(rejected.status).toBe(403);
    expect(await rejected.text()).toBe('');

    // The authenticated request reaches the same handler the rejection
    // never touched.
    const accepted = await request(origin, '/api/todos');
    expect(accepted.status).toBe(200);
    expect(await accepted.text()).toBe('should never run');
  });

  it('marks an undefined handler result with the unhandled sentinel', async () => {
    const origin = await startRuntime(`
      export async function handleRequest() {}
    `);
    const response = await request(origin, '/api/none');
    expect(response.status).toBe(404);
    expect(response.headers.get('x-plec-runtime-result')).toBe(
      'unhandled',
    );
    expect(await response.text()).toBe('');
  });

  it('passes application responses through verbatim, including their own 404', async () => {
    const origin = await startRuntime(`
      export async function handleRequest(request) {
        if (new URL(request.url).pathname === '/api/nope') {
          return new Response('nope', { status: 404, headers: { 'content-type': 'text/plain' } });
        }
        return Response.json({ ok: true });
      }
    `);
    const notFound = await request(origin, '/api/nope');
    expect(notFound.status).toBe(404);
    expect(notFound.headers.get('x-plec-runtime-result')).toBeNull();
    expect(await notFound.text()).toBe('nope');

    const found = await request(origin, '/api/ok');
    expect(await found.json()).toEqual({ ok: true });
  });

  it('never lets application headers forge the unhandled sentinel', async () => {
    const origin = await startRuntime(`
      export async function handleRequest() {
        return new Response('app 404', {
          status: 404,
          headers: { 'x-plec-runtime-result': 'unhandled' },
        });
      }
    `);
    const response = await request(origin, '/api/anything');
    expect(response.status).toBe(404);
    expect(response.headers.get('x-plec-runtime-result')).toBeNull();
    expect(await response.text()).toBe('app 404');
  });

  it('answers the health path and forwards bodies and request state', async () => {
    const origin = await startRuntime(`
      export async function handleRequest(request, context) {
        const body = await request.json();
        return Response.json({
          body,
          method: context.method,
          query: context.query,
          cookies: context.cookies,
          params: context.params,
          pathname: context.pathname,
        });
      }
    `);
    const health = await request(origin, '/_plec-runtime/health');
    expect(health.status).toBe(204);

    const response = await request(
      origin,
      '/api/todos?active=true&tag=a&tag=b',
      {
        method: 'POST',
        headers: {
          'content-type': 'application/json',
          cookie: 'session=s%20id; other=x',
        },
        body: JSON.stringify({ title: 'Ship Plec' }),
      },
    );
    const payload = (await response.json()) as Record<string, any>;
    expect(payload.body).toEqual({ title: 'Ship Plec' });
    expect(payload.method).toBe('POST');
    expect(payload.query).toEqual({ active: 'true', tag: ['a', 'b'] });
    expect(payload.cookies).toEqual({ session: 's id', other: 'x' });
    expect(payload.params).toEqual({});
    expect(payload.pathname).toBe('/api/todos');
  });

  it('maps handler throws to a redacted structured 500', async () => {
    const origin = await startRuntime(`
      export async function handleRequest() {
        throw new Error('/srv/private/app/database.rs secret-token-123');
      }
    `);
    const response = await request(origin, '/api/todos');
    expect(response.status).toBe(500);
    expect(await response.json()).toEqual({
      error: 'Internal Server Error',
    });
  });

  it('fails loudly when the bundle does not export handleRequest', async () => {
    await expect(
      startRuntime(`export const notTheRightExport = () => {};`),
    ).rejects.toThrow(/handleRequest/);
  });
});

describe('request translation helpers', () => {
  it('decodes cookies strictly and query state tolerantly', () => {
    expect(parseCookies('a=1; b=s%20p; c')).toEqual({
      a: '1',
      b: 's p',
    });
    expect(() => parseCookies('bad=%zz')).toThrow(/percent/);

    const query = parseQuery('http://x/p?a=1&a=2&b=hello+world&c');
    expect(query).toEqual({ a: ['1', '2'], b: 'hello world', c: '' });
  });

  it('builds a web request with a buffered body and merged headers', async () => {
    const stream = new PassThrough();
    stream.end(Buffer.from('{"a":1}'));
    const incoming = {
      method: 'POST',
      url: '/x?y=1',
      headers: {
        host: 'example.test',
        'content-type': 'application/json',
      },
      rawHeaders: [
        'Host',
        'example.test',
        'Content-Type',
        'application/json',
      ],
      on: stream.on.bind(stream),
    } as any;
    const resolved = await toWebRequest(incoming);
    expect(resolved.url).toBe('http://example.test/x?y=1');
    expect(await resolved.json()).toEqual({ a: 1 });
    expect(resolved.headers.get('content-type')).toBe(
      'application/json',
    );
  });

  it('builds the context the application handlers observe', () => {
    const incoming = {
      method: 'PATCH',
      url: '/api/todos/7?done=true',
      headers: { host: 'example.test', cookie: 'a=b' },
    } as any;
    const context = buildContext(incoming);
    expect(context).toMatchObject({
      url: 'http://example.test/api/todos/7?done=true',
      pathname: '/api/todos/7',
      method: 'PATCH',
      cookies: { a: 'b' },
      params: {},
      query: { done: 'true' },
    });
  });
});
