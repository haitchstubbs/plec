import { once } from 'node:events';
import { mkdtemp, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { createPlecServer } from './index';

const servers: ReturnType<typeof createPlecServer>[] = [];
afterEach(async () => Promise.all(servers.splice(0).map((server) => new Promise<void>((resolve) => server.close(() => resolve())))));

it('renders a route artifact with document metadata and public request location', async () => {
  const publicDir = await mkdtemp(path.join(tmpdir(), 'plec-server-'));
  const component = (tag: string, text?: string) => ({ rootComponent: 0, components: [{ rootNode: 0, strings: [tag], constants: [], nodes: [{ op: 'element', tag: 0, children: text ? [1] : [] }, ...(text ? [{ op: 'text', text: 0 }] : [])], texts: text ? [{ value: text }] : [], bindings: [], propPrograms: [], stateSlots: [], parameters: [], expressions: [], loops: [], routeOutlets: tag === 'main' ? [{ id: 'main', node: 0 }] : [] }] });
  await writeFile(path.join(publicDir, 'route-artifact.json'), JSON.stringify({
    manifest: { revision: 'test-revision', rootGraphId: 'root', routes: [{ id: 'home', path: '', graphId: 'home', outletId: 'main', meta: { title: 'Home title', description: 'Home description' } }] },
    graphs: [{ graphId: 'root', graph: component('main') }, { graphId: 'home', graph: component('p', 'Server rendered') }],
  }));
  const server = createPlecServer({ publicDir, artifactPath: path.join(publicDir, 'route-artifact.json') });
  servers.push(server); server.listen(0); await once(server, 'listening');
  const address = server.address(); if (!address || typeof address === 'string') throw new Error('missing test address');
  const html = await (await fetch(`http://127.0.0.1:${address.port}/?source=test`)).text();
  expect(html).toContain('<title>Home title</title>');
  expect(html).toContain('name="description" content="Home description"');
  expect(html).toContain('Server rendered');
  expect(html).toContain('data-plec-node="root/node:0"');
  expect(html).toContain('"search":"?source=test"');
});
