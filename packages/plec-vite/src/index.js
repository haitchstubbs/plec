import { spawn } from 'node:child_process';
import {
  cp,
  mkdir,
  readFile,
  rename,
  rm,
  stat,
} from 'node:fs/promises';
import { rmSync, statSync } from 'node:fs';
import path from 'node:path';
import { canonicalRequestTarget } from '@plec/node/request-target';

const nodeHostEntry = import.meta.resolve('@plec/node');
const nodeHttpEntry = import.meta.resolve('@plec/node/internal/http');
const SERVER_OWNED = [
  'server/app.mjs',
  'server/route-artifact.json',
  'plec-server.json',
];

/** Adds Plec's Node host to Vite's existing development HTTP server. */
export function plec(options) {
  let host;
  let activeOutput;
  let building = false;
  let dirty = false;
  let closing = false;
  let cleanupTask;
  let watcher;
  const assetDependencies = new Set();
  const outputRoot = path.resolve(options.outDir);
  process.once('exit', () => {
    if (activeOutput) {
      try {
        rmSync(activeOutput, { recursive: true, force: true });
      } catch {
        // Shutdown cleanup must not obscure the process's exit result.
      }
    }
  });

  function command(args, { cwd = options.root, capture = false } = {}) {
    return new Promise((resolve, reject) => {
      const child = spawn(options.cli, args, {
        cwd,
        stdio: capture ? ['ignore', 'pipe', 'inherit'] : 'inherit',
      });
      let stdout = '';
      child.stdout?.on('data', (chunk) => {
        stdout += chunk;
      });
      child.once('error', reject);
      child.once('exit', (code, signal) => {
        if (code === 0) resolve(stdout);
        else
          reject(
            new Error(
              `plec ${args[0]} ${signal ? `terminated by ${signal}` : `exited with ${code}`}`,
            ),
          );
      });
    });
  }

  async function build(vite, initial = false) {
    if (building) {
      dirty = true;
      return;
    }
    building = true;
    try {
      let replacementReady = false;
      do {
        dirty = false;
        const candidate = path.join(
          options.root,
          '.plec',
          'vite-dev',
          `${process.pid}-${Date.now()}`,
        );
        try {
          await command([
            'build',
            options.source,
            '--client-entry',
            options.clientEntry,
            '--server-entry',
            options.serverEntry,
            '--out-dir',
            candidate,
            '--no-optimize',
          ]);
          await registerAssetDependencies(
            vite,
            candidate,
            options.root,
          );

          if (!activeOutput) {
            host = await startHost(candidate, vite);
            activeOutput = candidate;
            console.log('Plec dev: Node host attached to Vite');
          } else if (
            (await classifyBuildImpact(activeOutput, candidate)) !==
            'client-only'
          ) {
            const oldHost = host;
            const oldOutput = activeOutput;
            host = await startHost(candidate, vite);
            activeOutput = candidate;
            await closeHost(oldHost);
            await rm(oldOutput, { recursive: true, force: true });
            console.log('Plec dev: Node host reloaded');
          } else {
            await promoteClientOutput(candidate, activeOutput);
            await rm(candidate, { recursive: true, force: true });
            console.log('Plec dev: client artifacts updated');
          }
          replacementReady = true;
        } catch (error) {
          await rm(candidate, { recursive: true, force: true });
          console.error(`Plec dev: ${error.message}`);
          console.error('Plec dev: serving previous successful build');
        }
      } while (dirty && !closing);
      if (!initial && replacementReady && !closing) {
        vite.ws.send({ type: 'full-reload', path: '*' });
      }
    } finally {
      building = false;
    }
  }

  async function startHost(directory, vite) {
    const [nodeHost, nodeHttp] = await Promise.all([
      import(nodeHostEntry),
      import(nodeHttpEntry),
    ]);
    if (typeof nodeHttp.createPlecHttpDispatcher !== 'function')
      throw new Error(
        `@plec/node HTTP dispatcher missing from ${nodeHttpEntry} (exports: ${Object.keys(nodeHttp).join(', ')})`,
      );
    const { createPlecHandler } = nodeHost;
    const { createPlecHttpDispatcher } = nodeHttp;
    const handler = await createPlecHandler({
      dir: directory,
      development: true,
    });
    const dispatcher = createPlecHttpDispatcher(
      handler,
      undefined,
      async (request, response) => {
        if (
          request.method !== 'GET' ||
          !response.headers.get('content-type')?.startsWith('text/html')
        )
          return response;
        const html = await vite.transformIndexHtml(
          request.url ?? '/',
          await response.text(),
        );
        const headers = new Headers(response.headers);
        headers.delete('content-length');
        return new Response(html, {
          status: response.status,
          statusText: response.statusText,
          headers,
        });
      },
    );
    return { handler, dispatcher };
  }

  async function closeHost(current) {
    if (!current) return;
    current.dispatcher.stopAdmission();
    current.dispatcher.abortActiveRequests();
    await current.dispatcher.waitForActiveRequests();
    await current.handler.close();
  }

  function cleanup() {
    if (!cleanupTask) {
      cleanupTask = (async () => {
        closing = true;
        await closeHost(host);
        host = undefined;
        if (activeOutput) {
          await rm(activeOutput, { recursive: true, force: true });
          activeOutput = undefined;
        }
      })();
    }
    return cleanupTask;
  }

  return {
    name: 'plec:vite-dev',
    close: cleanup,
    transformIndexHtml: {
      order: 'pre',
      handler(html) {
        const withoutPreloads = html.replace(
          /<link\s+rel="preload"\s+as="font"[^>]*>/gi,
          '',
        );
        const styled =
          /<link\s+rel="stylesheet"\s+href="[^"]*"\s*>/i.test(
            withoutPreloads,
          )
            ? withoutPreloads.replace(
                /<link\s+rel="stylesheet"\s+href="[^"]*"\s*>/i,
                '<link rel="stylesheet" href="/src/styles.css">',
              )
            : withoutPreloads.replace(
                '</head>',
                '<link rel="stylesheet" href="/src/styles.css"></head>',
              );
        return styled.replace(
          /<script\s+type="module"\s+src="([^\"]*\/_plec\/assets\/client-[^\"]+\.js[^\"]*)"><\/script>/i,
          '<template data-plec-client-src="$1"></template>',
        );
      },
    },
    configureServer(vite) {
      watcher = vite.watcher;
      if (vite.httpServer) {
        vite.httpServer.headersTimeout = 10_000;
        vite.httpServer.requestTimeout = 30_000;
      }
      vite.httpServer?.once('close', () => {
        void cleanup();
      });
      vite.middlewares.use((req, res, next) => {
        let pathname;
        try {
          pathname = canonicalRequestTarget(req.url ?? '').path;
        } catch {
          host?.dispatcher.handleRequest(req, res);
          return;
        }
        if (shouldViteServeStatic(pathname)) return next();
        if (!host) {
          res.statusCode = 503;
          res.end('Plec dev host is starting');
          return;
        }
        host.dispatcher.handleRequest(req, res);
      });
      vite.watcher.on('all', (event, file) => {
        const absolute = path.resolve(file);
        if (!['add', 'change', 'unlink'].includes(event)) return;
        if (absolute.includes(`${path.sep}node_modules${path.sep}`))
          return;
        if (
          absolute === outputRoot ||
          absolute.startsWith(`${outputRoot}${path.sep}`)
        )
          return;
        if (
          !assetDependencies.has(absolute) &&
          ![
            '.ts',
            '.tsx',
            '.js',
            '.jsx',
            '.mjs',
            '.mts',
            '.json',
            '.toml',
          ].includes(path.extname(absolute).toLowerCase())
        )
          return;
        void build(vite);
      });
      void build(vite, true);
    },
  };

  async function registerAssetDependencies(vite, directory, root) {
    let assets;
    try {
      assets = JSON.parse(
        await readFile(
          path.join(directory, 'plec-assets.json'),
          'utf8',
        ),
      );
    } catch {
      return;
    }
    for (const asset of assets) {
      if (typeof asset.source !== 'string') continue;
      const source = path.resolve(root, asset.source);
      try {
        if ((await stat(source)).isFile()) {
          assetDependencies.add(source);
          watcher.add(source);
        }
      } catch {
        /* Missing dependencies are compiler diagnostics on the next build. */
      }
    }
  }

  async function classifyBuildImpact(previous, candidate) {
    for (const relative of SERVER_OWNED) {
      const [left, right] = await Promise.all([
        readFile(path.join(previous, relative)).catch(() => null),
        readFile(path.join(candidate, relative)).catch(() => null),
      ]);
      if (left === null && right === null) continue;
      if (!left || !right || !left.equals(right))
        return 'server-runtime';
    }
    return 'client-only';
  }

  async function promoteClientOutput(candidate, active) {
    for (const relative of ['client', 'public']) {
      const source = path.join(candidate, relative);
      const destination = path.join(active, relative);
      const next = `${destination}.plec-next-${process.pid}`;
      const previous = `${destination}.plec-previous-${process.pid}`;
      await rm(next, { recursive: true, force: true });
      await rm(previous, { recursive: true, force: true });
      await cp(source, next, { recursive: true });
      try {
        await rename(destination, previous);
        await rename(next, destination);
        await rm(previous, { recursive: true, force: true });
      } catch (error) {
        await rm(destination, { recursive: true, force: true });
        await rename(previous, destination).catch(() => {});
        throw error;
      }
    }
    for (const relative of [
      'plec-assets.json',
      'server/route-artifact.json',
    ]) {
      const source = path.join(candidate, relative);
      const destination = path.join(active, relative);
      const temporary = `${destination}.plec-next-${process.pid}`;
      await mkdir(path.dirname(destination), { recursive: true });
      await cp(source, temporary);
      await rename(temporary, destination);
    }
  }

  function shouldViteServeStatic(pathname) {
    if (/^\/(?:@vite|@id|@fs|node_modules|src)(?:\/|$)/u.test(pathname))
      return true;
    try {
      const segments = pathname
        .slice(1)
        .split('/')
        .map((segment) => decodeURIComponent(segment));
      if (
        segments.some(
          (segment) =>
            !segment ||
            segment === '.' ||
            segment === '..' ||
            /[/\\]/u.test(segment),
        )
      )
        return false;
      const publicRoot = path.resolve(options.root, 'public');
      const candidate = path.resolve(publicRoot, ...segments);
      const relative = path.relative(publicRoot, candidate);
      if (
        relative.startsWith(`..${path.sep}`) ||
        relative === '..' ||
        path.isAbsolute(relative)
      )
        return false;
      return statSync(candidate).isFile();
    } catch {
      return false;
    }
  }
}

/** Runs after Vite's HTML transform so Plec's prebuilt browser bundle stays opaque. */
export function plecHtmlFinalizer() {
  return {
    name: 'plec:vite-html-finalizer',
    transformIndexHtml: {
      order: 'post',
      handler(html) {
        const restored = html.replace(
          /<template\s+data-plec-client-src="([^\"]+)"><\/template>/i,
          '<script type="module" src="$1"></script>',
        );
        return restored;
      },
    },
  };
}
