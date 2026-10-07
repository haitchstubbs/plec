import {
  mkdtemp,
  mkdir,
  rm,
  symlink,
  writeFile,
} from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { serveStatic } from './static.js';

const directories: string[] = [];
afterEach(async () =>
  Promise.all(
    directories
      .splice(0)
      .map((directory) =>
        rm(directory, { recursive: true, force: true }),
      ),
  ),
);

describe('@plec/node static files', () => {
  it('serves contained files, HEAD, ranges, and precompressed sidecars', async () => {
    const dir = await mkdtemp(
      path.join(os.tmpdir(), 'plec-static-test-'),
    );
    directories.push(dir);
    const publicRoot = path.join(dir, 'public');
    const clientRoot = path.join(dir, 'client');
    await mkdir(publicRoot);
    await mkdir(clientRoot);
    await writeFile(path.join(publicRoot, 'style.css'), 'abcdef');
    await writeFile(
      path.join(publicRoot, 'style.css.br'),
      'compressed',
    );
    await writeFile(path.join(publicRoot, 'style.css.gz'), 'gzip');
    await writeFile(path.join(dir, 'secret.txt'), 'secret');
    await symlink(
      path.join(dir, 'secret.txt'),
      path.join(publicRoot, 'escape.txt'),
    );

    const ranged = await serveStatic(
      new Request('http://localhost/style.css', {
        headers: { range: 'bytes=1-3' },
      }),
      '/style.css',
      publicRoot,
      clientRoot,
    );
    expect(ranged.status).toBe(206);
    expect(await ranged.text()).toBe('bcd');
    const head = await serveStatic(
      new Request('http://localhost/style.css', { method: 'HEAD' }),
      '/style.css',
      publicRoot,
      clientRoot,
    );
    expect(head.status).toBe(200);
    expect(head.body).toBeNull();
    const compressed = await serveStatic(
      new Request('http://localhost/style.css', {
        headers: {
          'accept-encoding': 'br;q=0.4, gzip;q=0.8, identity;q=0.1',
        },
      }),
      '/style.css',
      publicRoot,
      clientRoot,
    );
    expect(compressed.headers.get('content-encoding')).toBe('gzip');
    expect(await compressed.text()).toBe('gzip');
    expect(
      (
        await serveStatic(
          new Request('http://localhost/escape.txt'),
          '/escape.txt',
          publicRoot,
          clientRoot,
        )
      ).status,
    ).toBe(400);
    expect(
      (
        await serveStatic(
          new Request('http://localhost/%2e%2e%2fsecret.txt'),
          '/%2e%2e%2fsecret.txt',
          publicRoot,
          clientRoot,
        )
      ).status,
    ).toBe(400);
    expect(
      (
        await serveStatic(
          new Request('http://localhost/missing.css'),
          '/missing.css',
          publicRoot,
          clientRoot,
        )
      ).status,
    ).toBe(404);
  });

  it('applies wildcard exclusions to identity unless explicitly overridden', async () => {
    const dir = await mkdtemp(
      path.join(os.tmpdir(), 'plec-static-encoding-'),
    );
    directories.push(dir);
    const publicRoot = path.join(dir, 'public');
    const clientRoot = path.join(dir, 'client');
    await mkdir(publicRoot);
    await mkdir(clientRoot);
    await writeFile(path.join(publicRoot, 'asset.txt'), 'identity');
    await writeFile(path.join(publicRoot, 'asset.txt.gz'), 'gzip');

    const respond = (acceptEncoding: string) =>
      serveStatic(
        new Request('http://localhost/asset.txt', {
          headers: { 'accept-encoding': acceptEncoding },
        }),
        '/asset.txt',
        publicRoot,
        clientRoot,
      );

    expect((await respond('*;q=0')).status).toBe(406);
    const identity = await respond('*;q=0, identity;q=1');
    expect(identity.status).toBe(200);
    expect(identity.headers.get('content-encoding')).toBeNull();
    const gzip = await respond('gzip;q=1, *;q=0');
    expect(gzip.status).toBe(200);
    expect(gzip.headers.get('content-encoding')).toBe('gzip');
    expect(await gzip.text()).toBe('gzip');
  });
});
