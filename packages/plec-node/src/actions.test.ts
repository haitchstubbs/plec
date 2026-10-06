import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { createPlecHandler } from './index.js';

const temporaryDirectories: string[] = [];

afterEach(async () => {
  await Promise.all(temporaryDirectories.splice(0).map((directory) => rm(directory, { recursive: true, force: true })));
});

describe('@plec/node action path', () => {
  it('streams a multi-chunk action body through Rust and invokes the generated action callback', async () => {
    const dir = await fixture();
    const handler = await createPlecHandler({ dir });
    try {
      const chunks = new ReadableStream<Uint8Array>({
        start(controller) {
          controller.enqueue(new TextEncoder().encode('["multi-'));
          controller.enqueue(new TextEncoder().encode('chunk"]'));
          controller.close();
        },
      });
      const response = await handler.fetch(actionRequest('echo', chunks));
      expect(response.status).toBe(200);
      expect(response.headers.get('cache-control')).toBe('no-store');
      expect(await response.json()).toEqual({ value: 'multi-chunk', pathname: '/_plec/actions/echo', requestCase: null });
    } finally {
      await handler.close();
    }
  });

  it('keeps concurrent action request contexts isolated across async callbacks', async () => {
    const dir = await fixture();
    const handler = await createPlecHandler({ dir });
    try {
      const [first, second] = await Promise.all([
        handler.fetch(actionRequest('echo', jsonBody('["first", 20]'), 'http://localhost', 'first')),
        handler.fetch(actionRequest('echo', jsonBody('["second", 0]'), 'http://localhost', 'second')),
      ]);
      expect(await first.json()).toMatchObject({ value: 'first', requestCase: 'first' });
      expect(await second.json()).toMatchObject({ value: 'second', requestCase: 'second' });
    } finally {
      await handler.close();
    }
  });

  it('preserves action method, origin, ID, JSON, unknown-ID, and callback failure outcomes', async () => {
    const dir = await fixture();
    const handler = await createPlecHandler({ dir });
    try {
      expect((await handler.fetch(new Request('http://localhost/_plec/actions/echo'))).status).toBe(405);
      expect((await handler.fetch(actionRequest('echo', jsonBody('[]'), 'https://elsewhere.test'))).status).toBe(403);
      expect((await handler.fetch(actionRequest('bad%2Fid', jsonBody('[]')))).status).toBe(404);
      expect((await handler.fetch(actionRequest('echo', jsonBody('{')))).status).toBe(400);
      expect((await handler.fetch(actionRequest('missing', jsonBody('[]')))).status).toBe(404);
      const failed = await handler.fetch(actionRequest('fail', jsonBody('[]')));
      expect(failed.status).toBe(500);
      expect(await failed.json()).toEqual({ error: 'server action failed' });
    } finally {
      await handler.close();
    }
  });

  it('rejects streamed overflow before reading the remaining source chunks', async () => {
    const dir = await fixture();
    const handler = await createPlecHandler({ dir });
    let pulls = 0;
    let cancelled = false;
    let pullsAtCancel = -1;
    try {
      const body = new ReadableStream<Uint8Array>({
        pull(controller) {
          pulls += 1;
          if (pulls === 1) controller.enqueue(new Uint8Array(1024 * 1024));
          else if (pulls === 2) controller.enqueue(new Uint8Array([1]));
          else controller.enqueue(new Uint8Array([2]));
        },
        cancel() { cancelled = true; pullsAtCancel = pulls; },
      });
      const response = await handler.fetch(actionRequest('echo', body));
      expect(response.status).toBe(413);
      expect(await response.json()).toEqual({ error: 'server action request exceeds limit' });
      expect(cancelled).toBe(true);
      await new Promise<void>((resolve) => setImmediate(resolve));
      expect(pulls).toBe(pullsAtCancel);
    } finally {
      await handler.close();
    }
  });

  it('close cancels the native wait for a pending action callback', async () => {
    const startedKey = `plecActionPendingStarted${Date.now()}`;
    const dir = await fixture(startedKey);
    const handler = await createPlecHandler({ dir });
    try {
      const pending = handler.fetch(actionRequest('pending', jsonBody('[]')));
      for (let attempt = 0; attempt < 100 && !(globalThis as Record<string, unknown>)[startedKey]; attempt++) {
        await new Promise<void>((resolve) => setImmediate(resolve));
      }
      expect((globalThis as Record<string, unknown>)[startedKey]).toBe(1);
      const rejected = expect(pending).rejects.toThrow('PLEC_APPLICATION_CLOSED');
      let timeout: ReturnType<typeof setTimeout> | undefined;
      try {
        await Promise.race([
          handler.close(),
          new Promise((_, reject) => { timeout = setTimeout(() => reject(new Error('close waited for unresolved JS action')), 1000); }),
        ]);
      } finally {
        if (timeout) clearTimeout(timeout);
      }
      await rejected;
    } finally {
      await handler.close();
      delete (globalThis as Record<string, unknown>)[startedKey];
    }
  });

});

async function fixture(pendingKey?: string): Promise<string> {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'plec-node-action-'));
  temporaryDirectories.push(dir);
  await mkdir(path.join(dir, 'public'));
  await mkdir(path.join(dir, 'server'));
  await mkdir(path.join(dir, 'client/assets'), { recursive: true });
  await writeFile(path.join(dir, 'plec-server.json'), JSON.stringify({
    publicDir: 'public', clientDir: 'client', artifact: 'server/route-artifact.json',
    server: { entry: 'server/app.mjs' },
  }));
  await writeFile(path.join(dir, 'client/host-providers.json'), JSON.stringify({
    version: 2, revision: 'action-test', providers: [],
  }));
  await writeFile(path.join(dir, 'server/route-artifact.json'), JSON.stringify({
    manifest: { revision: 'action-test', rootGraphId: 'root', routes: [] },
    graphs: [{ graphId: 'root', graph: {
      rootComponent: 0,
      components: [{
        id: 'root', rootNode: 0, strings: ['div'], constants: [],
        nodes: [{ op: 'element', tag: 0, children: [] }], texts: [], bindings: [],
        propPrograms: [], stateSlots: [], parameters: [], expressions: [], loops: [], routeOutlets: [],
      }],
    } }],
  }));
  await writeFile(path.join(dir, 'server/app.mjs'), `
    import { AsyncLocalStorage } from 'node:async_hooks';
    const actionContext = new AsyncLocalStorage();
    export function hasAction(id) { return id === 'echo' || id === 'fail' || id === 'pending'; }
    export async function invokeAction(id, args, context) {
      if (id === 'fail') throw new Error('private action diagnostic');
      if (id === 'pending') { globalThis[${JSON.stringify(pendingKey)}] = (globalThis[${JSON.stringify(pendingKey)}] ?? 0) + 1; return new Promise(() => {}); }
      return actionContext.run(context, async () => {
        await new Promise(resolve => setTimeout(resolve, args[1] ?? 0));
        const current = actionContext.getStore();
        return { value: args[0], pathname: current.pathname, requestCase: current.headers['x-case'] ?? null };
      });
    }
  `);
  return dir;
}

function actionRequest(
  id: string,
  body: ReadableStream<Uint8Array>,
  origin = 'http://localhost',
  requestCase?: string,
): Request {
  const headers = new Headers({ origin, 'content-type': 'application/json' });
  if (requestCase) headers.set('x-case', requestCase);
  return new Request(`http://localhost/_plec/actions/${id}`, {
    method: 'POST', headers, body, duplex: 'half',
  } as RequestInit & { duplex: 'half' });
}

function jsonBody(value: string): ReadableStream<Uint8Array> {
  return new ReadableStream({
    start(controller) { controller.enqueue(new TextEncoder().encode(value)); controller.close(); },
  });
}
