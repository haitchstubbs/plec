import { mkdtemp, writeFile } from 'node:fs/promises';
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

  it('maps handler throws to a structured 500', async () => {
    const origin = await startRuntime(`
      export async function handleRequest() {
        throw new Error('database exploded');
      }
    `);
    const response = await request(origin, '/api/todos');
    expect(response.status).toBe(500);
    expect(await response.json()).toEqual({
      error: 'database exploded',
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
