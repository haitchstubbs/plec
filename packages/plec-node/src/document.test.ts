import { mkdtemp, mkdir, rm, symlink, unlink, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { createPlecHandler } from './index.js';

const temporaryDirectories: string[] = [];

afterEach(async () => {
  await Promise.all(temporaryDirectories.splice(0).map((directory) => rm(directory, { recursive: true, force: true })));
});

describe('@plec/node document path', () => {
  it('serves static files with containment, HEAD, ranges, and precompressed sidecars', async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), 'plec-node-static-'));
    temporaryDirectories.push(dir);
    await mkdir(path.join(dir, 'public'));
    await mkdir(path.join(dir, 'server'));
    await mkdir(path.join(dir, 'client/assets/foo'), { recursive: true });
    await mkdir(path.join(dir, 'public/nested'), { recursive: true });
    await writeFile(path.join(dir, 'plec-server.json'), JSON.stringify({ publicDir: 'public', artifact: 'server/artifact.json', server: { entry: 'server/app.mjs' } }));
    await writeFile(path.join(dir, 'client/host-providers.json'), JSON.stringify({ version: 2, revision: 'static', providers: [] }));
    await writeFile(path.join(dir, 'server/app.mjs'), generatedApp());
    await writeFile(path.join(dir, 'server/artifact.json'), JSON.stringify({ manifest: { revision: 'static', rootGraphId: 'root', routes: [] }, graphs: [{ graphId: 'root', graph: graph('root', 'div', '', false) }] }));
    await writeFile(path.join(dir, 'public/style.css'), 'abcdef');
    await writeFile(path.join(dir, 'public/nested/style.css'), 'nested-css');
    await writeFile(path.join(dir, 'client/assets/foo/bar.js'), 'nested-client-js');
    await writeFile(path.join(dir, 'public/style.css.br'), 'compressed');
    await writeFile(path.join(dir, 'public/style.css.gz'), 'gzip');
    await writeFile(path.join(dir, 'public/file.unknownplectype'), 'bytes');
    await writeFile(path.join(dir, 'public/.visible'), 'dotfile');
    const outside = path.join(dir, 'secret.txt');
    await writeFile(outside, 'secret');
    await symlink(outside, path.join(dir, 'public/escape.txt'));
    const handler = await createPlecHandler({ dir });
    try {
      const ranged = await handler.fetch(new Request('http://localhost/style.css', { headers: { range: 'bytes=1-3' } }));
      expect(ranged.status).toBe(206);
      expect(ranged.headers.get('content-range')).toBe('bytes 1-3/6');
      expect(await ranged.text()).toBe('bcd');
      expect(await (await handler.fetch(new Request('http://localhost/nested/style.css'))).text()).toBe('nested-css');
      expect(await (await handler.fetch(new Request('http://localhost/_plec/assets/foo/bar.js'))).text()).toBe('nested-client-js');
      const suffix = await handler.fetch(new Request('http://localhost/style.css', { headers: { range: 'bytes=-2' } }));
      expect(suffix.status).toBe(206);
      expect(await suffix.text()).toBe('ef');
      const unsatisfiable = await handler.fetch(new Request('http://localhost/style.css', { headers: { range: 'bytes=99-' } }));
      expect(unsatisfiable.status).toBe(416);
      expect(unsatisfiable.headers.get('content-range')).toBe('bytes */6');
      const multipleRanges = await handler.fetch(new Request('http://localhost/style.css', { headers: { range: 'bytes=0-1,4-5' } }));
      expect(multipleRanges.status).toBe(200);
      expect(await multipleRanges.text()).toBe('abcdef');
      const head = await handler.fetch(new Request('http://localhost/style.css', { method: 'HEAD' }));
      expect(head.status).toBe(200);
      expect(head.body).toBeNull();
      expect(head.headers.get('cache-control')).toBe('no-cache');
      const compressed = await handler.fetch(new Request('http://localhost/style.css', { headers: { 'accept-encoding': 'br;q=0.4, gzip;q=0.8, identity;q=0.1' } }));
      expect(compressed.headers.get('content-encoding')).toBe('gzip');
      expect(await compressed.text()).toBe('gzip');
      const brotliTie = await handler.fetch(new Request('http://localhost/style.css', { headers: { 'accept-encoding': '*;q=0.5, identity;q=0' } }));
      expect(brotliTie.headers.get('content-encoding')).toBe('br');
      expect(brotliTie.headers.get('vary')).toBe('Accept-Encoding');
      expect(brotliTie.headers.get('content-type')).toBe('text/css; charset=utf-8');
      expect(await brotliTie.text()).toBe('compressed');
      const identityForbidden = await handler.fetch(new Request('http://localhost/style.css', { headers: { 'accept-encoding': 'br;q=0, gzip;q=0, identity;q=0' } }));
      expect(identityForbidden.status).toBe(406);
      const unknownMime = await handler.fetch(new Request('http://localhost/file.unknownplectype'));
      expect(unknownMime.headers.get('content-type')).toBe('application/octet-stream');
      expect(await (await handler.fetch(new Request('http://localhost/.visible'))).text()).toBe('dotfile');
      expect((await handler.fetch(new Request('http://localhost/escape.txt'))).status).toBe(400);
      expect((await handler.fetch(new Request('http://localhost/%2e%2e%2fsecret.txt'))).status).toBe(400);
      expect((await handler.fetch(new Request('http://localhost/encoded%5cseparator.txt'))).status).toBe(400);
      expect((await handler.fetch(new Request('http://localhost/missing.css'))).status).toBe(404);
    } finally { await handler.close(); }
  });

  it('renders a compiled document through the native engine and closes explicitly', async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), 'plec-node-document-'));
    temporaryDirectories.push(dir);
    await mkdir(path.join(dir, 'public'));
    await mkdir(path.join(dir, 'server'));
    await mkdir(path.join(dir, 'client/assets'), { recursive: true });
    await writeFile(path.join(dir, 'client/host-providers.json'), JSON.stringify({
      version: 2,
      revision: 'node-document-test',
      providers: [],
    }));
    await writeFile(path.join(dir, 'plec-server.json'), JSON.stringify({
      publicDir: 'public',
      artifact: 'server/route-artifact.json',
      server: { entry: 'server/app.mjs' },
    }));
    await writeFile(path.join(dir, 'server/app.mjs'), generatedApp());
    await writeFile(path.join(dir, 'server/route-artifact.json'), JSON.stringify({
      manifest: {
        revision: 'node-document-test',
        rootGraphId: 'root',
        routes: [{ id: 'home', path: '', graphId: 'home', outletId: 'main' }],
      },
      graphs: [
        { graphId: 'root', graph: graph('root', 'div', '', true) },
        { graphId: 'home', graph: graph('home', 'p', 'from rust', false) },
      ],
    }));

    const handler = await createPlecHandler({ dir });
    await unlink(path.join(dir, 'server/route-artifact.json'));
    const response = await handler.fetch(new Request('http://localhost/'));
    expect(response.status).toBe(200);
    expect(response.headers.get('content-type')).toBe('text/html; charset=utf-8');
    const html = await response.text();
    expect(html).toContain('<p data-plec-node="root/outlet:main/node:0">');
    expect(html).toContain('from rust');

    await Promise.all([handler.close(), handler.close()]);
    await expect(handler.fetch(new Request('http://localhost/'))).rejects.toThrow('PLEC_APPLICATION_CLOSED');
    await handler.close();
  });

  it('renders an allowlisted host provider through the native Promise callback', async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), 'plec-node-provider-'));
    temporaryDirectories.push(dir);
    await mkdir(path.join(dir, 'public'));
    await mkdir(path.join(dir, 'server'));
    await mkdir(path.join(dir, 'client/assets'), { recursive: true });
    const pendingKey = `plecPendingProvider${Date.now()}`;
    const startedKey = `${pendingKey}Started`;
    (globalThis as Record<string, unknown>)[pendingKey] = false;
    (globalThis as Record<string, unknown>)[startedKey] = false;
    await writeFile(path.join(dir, 'public/index.html'), 'fallback');
    await writeFile(path.join(dir, 'plec-server.json'), JSON.stringify({
      publicDir: 'public',
      clientDir: 'client',
      artifact: 'server/route-artifact.json',
      server: { entry: 'server/app.mjs' },
    }));
    await writeFile(path.join(dir, 'server/app.mjs'), generatedApp());
    await writeFile(path.join(dir, 'client/host-providers.json'), JSON.stringify({
      version: 2,
      revision: 'provider-test',
      providers: [{ id: 'demo', module: '/_plec/assets/provider.mjs', components: ['Greeting', 'Failure', 'Large', 'Pending'], ssr: true }],
    }));
    await writeFile(path.join(dir, 'client/assets/provider.mjs'), `
      export default () => ({
        Greeting: { async render(props) { return '<strong>' + props.name + '</strong>'; } },
        Failure: { async render() { throw new Error('provider secret'); } },
        Large: { async render() { return 'x'.repeat(1024 * 1024 + 1); } },
        Pending: { render() {
          if (globalThis[${JSON.stringify(pendingKey)}]) {
            globalThis[${JSON.stringify(startedKey)}] = true;
            return new Promise(() => {});
          }
          return '';
        } },
      });
    `);
    await writeFile(path.join(dir, 'server/route-artifact.json'), JSON.stringify({
      manifest: {
        revision: 'node-provider-test',
        rootGraphId: 'root',
        routes: [{ id: 'home', path: '', graphId: 'home', outletId: 'main' }],
      },
      graphs: [
        { graphId: 'root', graph: {
          rootComponent: 0,
          components: [{
            id: 'root', rootNode: 0, strings: ['div', 'name'], constants: ['Plec'],
            nodes: [
              { op: 'element', tag: 0, children: [1, 2, 3, 4] },
              { op: 'hostComponent', provider: 'demo', component: 'Greeting', props: [{ kind: 'value', name: 1, expression: 0 }] },
              { op: 'hostComponent', provider: 'demo', component: 'Failure', props: [] },
              { op: 'hostComponent', provider: 'demo', component: 'Large', props: [] },
              { op: 'hostComponent', provider: 'demo', component: 'Pending', props: [] },
            ],
            texts: [], bindings: [], propPrograms: [], stateSlots: [], parameters: [],
            expressions: [{ instructions: [{ op: 'constant', constant: 0 }, { op: 'return' }] }],
            loops: [], routeOutlets: [{ id: 'main', node: 0 }],
          }],
        } },
        { graphId: 'home', graph: graph('home', 'p', 'route', false) },
      ],
    }));

    const handler = await createPlecHandler({ dir, development: true });
    try {
      const response = await handler.fetch(new Request('http://localhost/'));
      expect(response.status).toBe(200);
      expect(response.headers.get('x-plec-ssr-fallback')).toBeNull();
      const html = await response.text();
      expect(html).toContain('<strong>Plec</strong>');
      expect(html).toContain('data-plec-host="demo:Failure"');
      expect(html).toContain('data-plec-host="demo:Large"');
      expect(html).not.toContain('provider secret');
      expect(html).not.toContain('x'.repeat(1024));

      (globalThis as Record<string, unknown>)[pendingKey] = true;
      const pending = handler.fetch(new Request('http://localhost/'));
      for (let attempt = 0; attempt < 100 && !(globalThis as Record<string, unknown>)[startedKey]; attempt += 1) {
        await new Promise<void>((resolve) => setImmediate(resolve));
      }
      expect((globalThis as Record<string, unknown>)[startedKey]).toBe(true);
      const pendingRejection = expect(pending).rejects.toThrow('PLEC_APPLICATION_CLOSED');
      await handler.close();
      await pendingRejection;
    } finally {
      await handler.close();
      delete (globalThis as Record<string, unknown>)[pendingKey];
      delete (globalThis as Record<string, unknown>)[startedKey];
    }
  });
});

function graph(id: string, tag: string, text: string, withOutlet: boolean) {
  return {
    rootComponent: 0,
    components: [{
      id,
      rootNode: 0,
      strings: [tag],
      constants: [],
      nodes: [
        { op: 'element', tag: 0, children: [1] },
        { op: 'text', text: 0 },
      ],
      texts: [{ value: text }],
      bindings: [],
      propPrograms: [],
      stateSlots: [],
      parameters: [],
      expressions: [],
      loops: [],
      routeOutlets: withOutlet ? [{ id: 'main', node: 0 }] : [],
    }],
  };
}

function generatedApp(): string {
  return 'export async function handleRequest() { return null; } export function hasAction() { return false; } export async function invokeAction() { throw new Error("unknown action"); }';
}
