import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import contextCases from '../../../testdata/node-host/request-context.json';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createPlecHandler } from './index.js';

const directories: string[] = [];

afterEach(async () => {
  await Promise.all(
    directories
      .splice(0)
      .map((dir) => rm(dir, { recursive: true, force: true })),
  );
});

describe('@plec/node API path', () => {
  it('dispatches /api, /api/, and /api/foo directly with canonical context', async () => {
    const dir = await fixture();
    const handler = await createPlecHandler({ dir, trustProxy: true });
    try {
      for (const pathname of ['/api', '/api/', '/api/foo']) {
        const response = await handler.fetch(
          new Request(
            `http://localhost${pathname}?tag=one&tag=two+words&empty=`,
            {
              headers: {
                cookie: 'session=hello%20world',
                'x-forwarded-proto': ' https, http',
              },
            },
          ),
        );
        expect(response.status).toBe(200);
        expect(await response.json()).toEqual({
          url: `https://localhost${pathname}?tag=one&tag=two+words&empty=`,
          pathname,
          query: { tag: ['one', 'two words'], empty: '' },
          cookies: { session: 'hello world' },
          body: null,
        });
      }
    } finally {
      await handler.close();
    }
  });

  it('matches the shared RequestContext parity fixtures', async () => {
    const dir = await fixture();
    for (const testCase of contextCases) {
      const handler = await createPlecHandler({
        dir,
        trustProxy: testCase.trustProxy,
      });
      try {
        const url = `${testCase.transportScheme}://${testCase.host}${testCase.target}`;
        const headers = new Headers({
          cookie: testCase.cookie!,
          'x-forwarded-proto': testCase.forwardedProto,
        });
        for (const entry of testCase.requestHeaders ?? [])
          headers.append(entry[0]!, entry[1]!);
        const response = await handler.fetch(
          new Request(url, {
            headers,
          }),
        );
        if ('invalid' in testCase) {
          expect(response.status).toBe(400);
          continue;
        }
        expect(response.status).toBe(200);
        const context = await response.json();
        expect(context.url).toBe(testCase.expectedUrl);
        expect(context.query).toEqual(testCase.expectedQuery);
        expect(context.cookies).toEqual(testCase.expectedCookies);
        if ('expectedHeaders' in testCase && testCase.expectedHeaders)
          expect(context.headers).toMatchObject(
            testCase.expectedHeaders,
          );
      } finally {
        await handler.close();
      }
    }
  });

  it('ignores GET and HEAD bodies and does not expose them to the application', async () => {
    const dir = await fixture();
    const handler = await createPlecHandler({ dir });
    try {
      for (const method of ['GET', 'HEAD']) {
        const response = await handler.fetch(
          new Request('http://localhost/api/body', {
            method,
          }),
        );
        expect(response.status).toBe(200);
        if (method === 'HEAD') expect(response.body).toBeNull();
        else
          expect(await response.json()).toMatchObject({ body: null });
      }
    } finally {
      await handler.close();
    }
  });

  it('rejects oversized streamed bodies before invoking application code', async () => {
    const dir = await fixture();
    const handler = await createPlecHandler({ dir });
    try {
      let calls = 0;
      (globalThis as Record<string, unknown>).plecApiTestCounter =
        () => {
          calls += 1;
        };
      const body = new ReadableStream<Uint8Array>({
        start(controller) {
          controller.enqueue(new Uint8Array(1024 * 1024));
          controller.enqueue(new Uint8Array([1]));
          controller.enqueue(new Uint8Array([2]));
        },
      });
      const response = await handler.fetch(
        new Request('http://localhost/api/body', {
          method: 'POST',
          body,
          duplex: 'half',
        } as RequestInit & { duplex: 'half' }),
      );
      expect(response.status).toBe(413);
      expect(await response.json()).toEqual({
        error: 'request body exceeds byte limit',
      });
      expect(calls).toBe(0);
    } finally {
      delete (globalThis as Record<string, unknown>).plecApiTestCounter;
      await handler.close();
    }
  });

  it('enforces the aggregate pre-read budget before dispatch and releases it after handler settlement', async () => {
    const dir = await fixture();
    const handler = await createPlecHandler({ dir });
    let release!: () => void;
    let dispatched = 0;
    (globalThis as Record<string, unknown>).plecApiTestGate =
      new Promise<void>((resolve) => {
        release = resolve;
      });
    (globalThis as Record<string, unknown>).plecApiTestCounter = () => {
      dispatched += 1;
    };
    try {
      const requests = Array.from({ length: 32 }, () =>
        handler.fetch(
          new Request('http://localhost/api/hold', {
            method: 'POST',
            body: new Uint8Array(1024 * 1024),
            duplex: 'half',
          } as RequestInit & { duplex: 'half' }),
        ),
      );
      for (
        let attempt = 0;
        attempt < 100 && dispatched < 32;
        attempt += 1
      ) {
        await new Promise<void>((resolve) => setImmediate(resolve));
      }
      expect(dispatched).toBe(32);
      const overloaded = await handler.fetch(
        new Request('http://localhost/api/overflow', {
          method: 'POST',
          body: new Uint8Array([1]),
          duplex: 'half',
        } as RequestInit & { duplex: 'half' }),
      );
      expect(overloaded.status).toBe(503);
      release();
      const responses = await Promise.all(requests);
      expect(
        responses.every((response) => response.status === 200),
      ).toBe(true);
      await Promise.all(
        responses.map((response) => response.body?.cancel()),
      );
      const afterRelease = await handler.fetch(
        new Request('http://localhost/api/ok'),
      );
      expect(afterRelease.status).toBe(200);
      await afterRelease.text();
    } finally {
      release();
      delete (globalThis as Record<string, unknown>).plecApiTestGate;
      delete (globalThis as Record<string, unknown>).plecApiTestCounter;
      await handler.close();
    }
  });

  it('detaches an aborted API wait but holds capacity until the handler settles', async () => {
    const dir = await fixture();
    const handler = await createPlecHandler({ dir });
    let release!: () => void;
    let started!: () => void;
    const entered = new Promise<void>((resolve) => {
      started = resolve;
    });
    (globalThis as Record<string, unknown>).plecApiTestGate =
      new Promise<void>((resolve) => {
        release = resolve;
      });
    let dispatched = 0;
    (globalThis as Record<string, unknown>).plecApiTestStarted = () => {
      dispatched += 1;
      if (dispatched === 32) started();
    };
    (globalThis as Record<string, unknown>).plecApiTestCounter =
      () => {};
    const controllers: AbortController[] = [];
    try {
      const requests = Array.from({ length: 32 }, () => {
        const controller = new AbortController();
        controllers.push(controller);
        return handler.fetch(
          new Request('http://localhost/api/hold', {
            method: 'POST',
            body: new Uint8Array(1024 * 1024),
            signal: controller.signal,
            duplex: 'half',
          } as RequestInit & { duplex: 'half' }),
        );
      });
      await entered;
      controllers[0]!.abort(
        new DOMException('client disconnected', 'AbortError'),
      );
      await expect(requests[0]).rejects.toThrow('client disconnected');
      const overloaded = await handler.fetch(
        new Request('http://localhost/api/overflow', {
          method: 'POST',
          body: new Uint8Array([1]),
          duplex: 'half',
        } as RequestInit & { duplex: 'half' }),
      );
      expect(overloaded.status).toBe(503);
      release();
      const settled = await Promise.all(requests.slice(1));
      await Promise.all(
        settled.map((response) => response.body?.cancel()),
      );
      const afterRelease = await handler.fetch(
        new Request('http://localhost/api/ok'),
      );
      expect(afterRelease.status).toBe(200);
      await afterRelease.text();
    } finally {
      release();
      for (const controller of controllers) controller.abort();
      delete (globalThis as Record<string, unknown>).plecApiTestGate;
      delete (globalThis as Record<string, unknown>).plecApiTestStarted;
      delete (globalThis as Record<string, unknown>).plecApiTestCounter;
      await handler.close();
    }
  });

  it('rejects API callback saturation before invoking another handler', async () => {
    const dir = await fixture();
    const handler = await createPlecHandler({ dir });
    let release!: () => void;
    let entered!: () => void;
    const allEntered = new Promise<void>((resolve) => {
      entered = resolve;
    });
    (globalThis as Record<string, unknown>).plecApiTestGate =
      new Promise<void>((resolve) => {
        release = resolve;
      });
    let dispatched = 0;
    (globalThis as Record<string, unknown>).plecApiTestCounter = () => {
      dispatched += 1;
    };
    (globalThis as Record<string, unknown>).plecApiTestStarted = () => {
      if (dispatched === 64) entered();
    };
    try {
      const pending = Array.from({ length: 64 }, () =>
        handler.fetch(new Request('http://localhost/api/hold')),
      );
      await allEntered;
      expect(dispatched).toBe(64);
      const overload = await handler.fetch(
        new Request('http://localhost/api/overload'),
      );
      expect(overload.status).toBe(503);
      expect(dispatched).toBe(64);
      release();
      const responses = await Promise.all(pending);
      await Promise.all(
        responses.map((response) => response.body?.cancel()),
      );
    } finally {
      release();
      delete (globalThis as Record<string, unknown>).plecApiTestGate;
      delete (globalThis as Record<string, unknown>).plecApiTestCounter;
      delete (globalThis as Record<string, unknown>).plecApiTestStarted;
      await handler.close();
    }
  });

  it('times out a stalled API pre-read and releases admission without dispatch', async () => {
    vi.useFakeTimers();
    const dir = await fixture();
    const handler = await createPlecHandler({ dir });
    let dispatched = 0;
    (globalThis as Record<string, unknown>).plecApiTestCounter = () => {
      dispatched += 1;
    };
    try {
      const stalled = new ReadableStream<Uint8Array>({ pull() {} });
      const pending = handler.fetch(
        new Request('http://localhost/api/timeout', {
          method: 'POST',
          body: stalled,
          duplex: 'half',
        } as RequestInit & { duplex: 'half' }),
      );
      await vi.advanceTimersByTimeAsync(30_000);
      const response = await pending;
      expect(response.status).toBe(408);
      expect(dispatched).toBe(0);
      const afterTimeout = await handler.fetch(
        new Request('http://localhost/api/ok'),
      );
      expect(afterTimeout.status).toBe(200);
      await afterTimeout.text();
    } finally {
      delete (globalThis as Record<string, unknown>).plecApiTestCounter;
      await handler.close();
      vi.useRealTimers();
    }
  });

  it('aborts an API pre-read without dispatch and releases its admission', async () => {
    const dir = await fixture();
    const handler = await createPlecHandler({ dir });
    const controller = new AbortController();
    let canceled = false;
    let dispatched = 0;
    (globalThis as Record<string, unknown>).plecApiTestCounter = () => {
      dispatched += 1;
    };
    try {
      const body = new ReadableStream<Uint8Array>({
        pull() {},
        cancel() {
          canceled = true;
        },
      });
      const pending = handler.fetch(
        new Request('http://localhost/api/abort', {
          method: 'POST',
          body,
          signal: controller.signal,
          duplex: 'half',
        } as RequestInit & { duplex: 'half' }),
      );
      controller.abort(
        new DOMException('client disconnected', 'AbortError'),
      );
      await expect(pending).rejects.toThrow('client disconnected');
      expect(canceled).toBe(true);
      expect(dispatched).toBe(0);
      const afterAbort = await handler.fetch(
        new Request('http://localhost/api/ok'),
      );
      expect(afterAbort.status).toBe(200);
      await afterAbort.text();
    } finally {
      delete (globalThis as Record<string, unknown>).plecApiTestCounter;
      await handler.close();
    }
  });

  it('returns the API fallback 404 and redacts handler failures', async () => {
    const dir = await fixture();
    const handler = await createPlecHandler({ dir });
    try {
      const missing = await handler.fetch(
        new Request('http://localhost/api/missing'),
      );
      expect(missing.status).toBe(404);
      expect(await missing.json()).toEqual({
        error: 'endpoint not found',
      });
      const failed = await handler.fetch(
        new Request('http://localhost/api/fail'),
      );
      expect(failed.status).toBe(500);
      expect(await failed.json()).toEqual({
        error: 'Internal Server Error',
      });
    } finally {
      await handler.close();
    }
  });
});

async function fixture(): Promise<string> {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'plec-node-api-'));
  directories.push(dir);
  await mkdir(path.join(dir, 'public'));
  await mkdir(path.join(dir, 'server'));
  await mkdir(path.join(dir, 'client/assets'), { recursive: true });
  await writeFile(
    path.join(dir, 'plec-server.json'),
    JSON.stringify({
      publicDir: 'public',
      clientDir: 'client',
      artifact: 'server/route-artifact.json',
      server: { entry: 'server/app.mjs' },
    }),
  );
  await writeFile(
    path.join(dir, 'client/host-providers.json'),
    JSON.stringify({ version: 2, revision: 'api-test', providers: [] }),
  );
  await writeFile(
    path.join(dir, 'server/route-artifact.json'),
    JSON.stringify({
      manifest: {
        revision: 'api-test',
        rootGraphId: 'root',
        routes: [],
      },
      graphs: [
        {
          graphId: 'root',
          graph: {
            rootComponent: 0,
            components: [
              {
                id: 'root',
                rootNode: 0,
                strings: ['div'],
                constants: [],
                nodes: [{ op: 'element', tag: 0, children: [] }],
                texts: [],
                bindings: [],
                propPrograms: [],
                stateSlots: [],
                parameters: [],
                expressions: [],
                loops: [],
                routeOutlets: [],
              },
            ],
          },
        },
      ],
    }),
  );
  await writeFile(
    path.join(dir, 'server/app.mjs'),
    `
    export function hasAction() { return false; }
    export async function invokeAction() { throw new Error('unknown action'); }
    export async function handleRequest(request, context) {
      globalThis.plecApiTestCounter?.();
      if (new URL(request.url).pathname.endsWith('/hold')) {
        globalThis.plecApiTestStarted?.();
        await globalThis.plecApiTestGate;
      }
      if (new URL(request.url).pathname.endsWith('/missing')) return null;
      if (new URL(request.url).pathname.endsWith('/fail')) throw new Error('private API diagnostic');
      return Response.json({
        url: context.url,
        pathname: context.pathname,
        headers: new URL(request.url).pathname.endsWith('/context') ? context.headers : undefined,
        query: context.query,
        cookies: context.cookies,
        body: request.method === 'GET' || request.method === 'HEAD' ? null : await request.text(),
      });
    }
  `,
  );
  return dir;
}
