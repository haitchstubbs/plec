import { mkdtemp, mkdir, rm, unlink, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { createPlecHandler } from './index.js';

const temporaryDirectories: string[] = [];

afterEach(async () => {
  await Promise.all(temporaryDirectories.splice(0).map((directory) => rm(directory, { recursive: true, force: true })));
});

describe('@plec/node document path', () => {
  it('renders a compiled document through the native engine and closes explicitly', async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), 'plec-node-document-'));
    temporaryDirectories.push(dir);
    await mkdir(path.join(dir, 'public'));
    await mkdir(path.join(dir, 'server'));
    await writeFile(path.join(dir, 'plec-server.json'), JSON.stringify({
      publicDir: 'public',
      artifact: 'server/route-artifact.json',
    }));
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
