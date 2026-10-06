#!/usr/bin/env node
import { createServer, searchForWorkspaceRoot } from 'vite';
import { plec, plecHtmlFinalizer } from '../src/index.js';
import path from 'node:path';
import { readdir, rm, statSync } from 'node:fs';
import net from 'node:net';
import { fileURLToPath } from 'node:url';

const [
  source = 'src/router.tsx',
  outDir = 'dist',
  host = '127.0.0.1',
  port = '3000',
  clientEntry = 'src/client.tsx',
  serverEntry = 'src/server.ts',
] = process.argv.slice(2);
const root = process.cwd();
const workspaceRoot = searchForWorkspaceRoot(root);
const internalPort = await freePort(Number(port));
const viteEntry = fileURLToPath(import.meta.resolve('vite'));
const vitePackageRoot = path.resolve(path.dirname(viteEntry), '../..');
const plecPlugin = plec({
  root,
  source,
  clientEntry,
  serverEntry,
  outDir: path.resolve(root, outDir),
  internalPort,
  cli: process.env.PLEC_CLI_BINARY ?? process.env.PLEC_BIN ?? 'plec',
});
const server = await createServer({
  root,
  plugins: [plecPlugin, plecHtmlFinalizer()],
  server: {
    host,
    port: Number(port),
    strictPort: true,
    watch: {
      ignored: ['**/.plec/**'],
      awaitWriteFinish: { stabilityThreshold: 100, pollInterval: 10 },
    },
    fs: { allow: [workspaceRoot, vitePackageRoot] },
    proxy: {
      '^/(?!@vite|@id|@fs|node_modules|src/)': {
        target: `http://127.0.0.1:${internalPort}`,
        changeOrigin: false,
        bypass(request) {
          const pathname = decodeURIComponent(
            new URL(request.url, 'http://plec').pathname,
          );
          try {
            if (statSync(path.join(root, 'public', pathname)).isFile())
              return request.url;
          } catch {
            /* Not a Vite public asset; proxy it to the Plec host. */
          }
          return undefined;
        },
      },
    },
  },
});
await server.listen();
server.printUrls();
let closing = false;
let closeTask;
const close = () => {
  if (closeTask) return closeTask;
  closing = true;
  process.stdin.pause();
  closeTask = (async () => {
    await server.close();
    await plecPlugin.close();
    const sessionDir = path.join(root, '.plec', 'vite-dev');
    const prefix = `${process.pid}-`;
    const entries = await readdir(sessionDir).catch(() => []);
    await Promise.all(
      entries
        .filter((entry) => entry.startsWith(prefix))
        .map((entry) =>
          rm(path.join(sessionDir, entry), {
            recursive: true,
            force: true,
          }),
        ),
    );
  })();
  return closeTask;
};
for (const signal of ['SIGINT', 'SIGTERM']) process.once(signal, close);
process.stdin.on('end', close);
process.stdin.resume();

async function freePort(exclude) {
  for (;;) {
    const listener = net.createServer();
    await new Promise((resolve, reject) =>
      listener.listen(0, '127.0.0.1', resolve).once('error', reject),
    );
    const port = listener.address().port;
    await new Promise((resolve, reject) =>
      listener.close((error) => (error ? reject(error) : resolve())),
    );
    if (port !== exclude) return port;
  }
}
