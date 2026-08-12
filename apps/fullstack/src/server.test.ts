import { once } from 'node:events';
import { mkdtemp, mkdir, writeFile } from 'node:fs/promises';
import { gzipSync } from 'node:zlib';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { createAppServer } from './server';

const servers: Array<ReturnType<typeof createAppServer>> = [];
afterEach(async () => {
  await Promise.all(
    servers
      .splice(0)
      .map(
        (server) =>
          new Promise<void>((resolve) => server.close(() => resolve())),
      ),
  );
});

async function testServer() {
  const publicDir = await mkdtemp(path.join(tmpdir(), 'plec-fullstack-'));
  await writeFile(
    path.join(publicDir, 'index.html'),
    '<div id=app></div>',
  );
  const server = createAppServer(publicDir);
  servers.push(server);
  server.listen(0);
  await once(server, 'listening');
  const address = server.address();
  if (!address || typeof address === 'string')
    throw new Error('missing test port');
  return `http://127.0.0.1:${address.port}`;
}

describe('Plec application server', () => {
  it('serves the SPA shell and CRUD Todo API', async () => {
    const origin = await testServer();
    expect(await (await fetch(`${origin}/about`)).text()).toContain(
      'id=app',
    );
    const initial = await (await fetch(`${origin}/api/todos`)).json();
    expect(initial).toHaveLength(1);
    const created = await (
      await fetch(`${origin}/api/todos`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ title: 'Ship Plec' }),
      })
    ).json();
    expect(created.title).toBe('Ship Plec');
    const updated = await (
      await fetch(`${origin}/api/todos/${created.id}`, {
        method: 'PATCH',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ completed: true }),
      })
    ).json();
    expect(updated.completed).toBe(true);
    expect(
      (
        await fetch(`${origin}/api/todos/${created.id}`, {
          method: 'DELETE',
        })
      ).status,
    ).toBe(204);
  });

  it('rejects invalid mutations and unknown API routes', async () => {
    const origin = await testServer();
    expect(
      (
        await fetch(`${origin}/api/todos`, {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: '{}',
        })
      ).status,
    ).toBe(400);
    expect(
      (
        await fetch(`${origin}/api/todos/nope`, {
          method: 'PATCH',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify({ completed: 'yes' }),
        })
      ).status,
    ).toBe(404);
    expect((await fetch(`${origin}/api/nope`)).status).toBe(404);
  });

  it('reports Node memory diagnostics for the cross-browser development HUD', async () => {
    const origin = await testServer();
    const response = await fetch(`${origin}/api/dev/memory`);
    expect(response.status).toBe(200);
    expect(await response.json()).toMatchObject({
      pid: expect.any(Number),
      rssBytes: expect.any(Number),
      heapUsedBytes: expect.any(Number),
      heapLimitBytes: expect.any(Number),
      activeResources: expect.any(Array),
    });
  });

  it('serves precompressed runtime assets with the negotiated encoding', async () => {
    const publicDir = await mkdtemp(
      path.join(tmpdir(), 'plec-fullstack-'),
    );
    await mkdir(path.join(publicDir, 'runtime'), { recursive: true });
    await writeFile(
      path.join(publicDir, 'runtime', 'runtime.js'),
      'export default {};',
    );
    await writeFile(
      path.join(publicDir, 'runtime', 'runtime.js.gz'),
      gzipSync('export default {};'),
    );
    const server = createAppServer(publicDir);
    servers.push(server);
    server.listen(0);
    await once(server, 'listening');
    const address = server.address();
    if (!address || typeof address === 'string')
      throw new Error('missing test port');
    const response = await fetch(
      `http://127.0.0.1:${address.port}/runtime/runtime.js`,
      { headers: { 'accept-encoding': 'gzip' } },
    );
    expect(response.headers.get('content-encoding')).toBe('gzip');
    expect(response.headers.get('vary')).toContain('Accept-Encoding');
  });

  it('falls back to an uncompressed runtime asset when no negotiated sibling exists', async () => {
    const publicDir = await mkdtemp(
      path.join(tmpdir(), 'plec-fullstack-'),
    );
    await mkdir(path.join(publicDir, 'runtime'), { recursive: true });
    await writeFile(
      path.join(publicDir, 'runtime', 'runtime.js'),
      'export default {};',
    );
    const server = createAppServer(publicDir);
    servers.push(server);
    server.listen(0);
    await once(server, 'listening');
    const address = server.address();
    if (!address || typeof address === 'string')
      throw new Error('missing test port');
    const response = await fetch(
      `http://127.0.0.1:${address.port}/runtime/runtime.js`,
      { headers: { 'accept-encoding': 'br, gzip' } },
    );
    expect(response.status).toBe(200);
    expect(response.headers.get('content-encoding')).toBeNull();
    expect(await response.text()).toBe('export default {};');
  });

  it('serves published font files with the font MIME type', async () => {
    const publicDir = await mkdtemp(
      path.join(tmpdir(), 'plec-fullstack-'),
    );
    await mkdir(path.join(publicDir, 'assets', 'files'), {
      recursive: true,
    });
    await writeFile(
      path.join(
        publicDir,
        'assets',
        'files',
        'outfit-latin-wght-normal.woff2',
      ),
      'font',
    );
    const server = createAppServer(publicDir);
    servers.push(server);
    server.listen(0);
    await once(server, 'listening');
    const address = server.address();
    if (!address || typeof address === 'string')
      throw new Error('missing test port');
    const response = await fetch(
      `http://127.0.0.1:${address.port}/assets/files/outfit-latin-wght-normal.woff2`,
    );
    expect(response.status).toBe(200);
    expect(response.headers.get('content-type')).toBe('font/woff2');
  });
});
