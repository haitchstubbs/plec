import { spawn } from 'node:child_process';
import {
  cp,
  mkdir,
  readFile,
  rename,
  rm,
  stat,
} from 'node:fs/promises';
import { rmSync } from 'node:fs';
import path from 'node:path';

const SERVER_OWNED = [
  'server/app.mjs',
  'server/runtime.mjs',
  'plec-server.json',
];

/** Adapts Vite's watcher/reload transport to Plec's Rust build and native host. */
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
            await startHost(candidate);
            activeOutput = candidate;
            console.log(`Plec dev: native host pid ${host.pid}`);
          } else if (
            (await classifyBuildImpact(activeOutput, candidate)) !==
            'client-only'
          ) {
            const oldHost = host;
            const oldOutput = activeOutput;
            await stopHost(oldHost);
            try {
              await startHost(candidate);
              activeOutput = candidate;
              await rm(oldOutput, { recursive: true, force: true });
              console.log(
                `Plec dev: native host restarted (pid ${host.pid})`,
              );
            } catch (error) {
              await startHost(oldOutput);
              throw error;
            }
          } else {
            await promoteClientOutput(candidate, activeOutput);
            await rm(candidate, { recursive: true, force: true });
            console.log(
              `Plec dev: client artifacts updated; native host pid ${host.pid}`,
            );
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

  async function startHost(directory) {
    const port = options.internalPort;
    const child = spawn(
      options.cli,
      [
        'serve',
        directory,
        '--development',
        '--host',
        '127.0.0.1',
        '--port',
        String(port),
      ],
      { cwd: options.root, stdio: 'inherit' },
    );
    try {
      await waitReady(port, child);
      host = child;
    } catch (error) {
      child.kill('SIGTERM');
      await waitExit(child);
      throw error;
    }
  }

  function cleanup() {
    if (!cleanupTask) {
      cleanupTask = (async () => {
        closing = true;
        await stopHost(host);
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
      vite.httpServer?.once('close', () => {
        void cleanup();
      });
      vite.middlewares.use(async (req, res, next) => {
        if (
          req.method !== 'GET' ||
          !req.headers.accept?.includes('text/html')
        )
          return next();
        try {
          const headers = Object.fromEntries(
            Object.entries(req.headers)
              .filter(
                ([name, value]) =>
                  value !== undefined &&
                  ![
                    'connection',
                    'content-length',
                    'transfer-encoding',
                  ].includes(name.toLowerCase()),
              )
              .map(([name, value]) => [
                name,
                Array.isArray(value) ? value.join(', ') : value,
              ]),
          );
          const response = await fetch(
            `http://127.0.0.1:${options.internalPort}${req.url}`,
            {
              headers,
            },
          );
          const html = await vite.transformIndexHtml(
            req.url ?? '/',
            await response.text(),
          );
          res.statusCode = response.status;
          for (const [name, value] of response.headers) {
            if (
              ![
                'connection',
                'content-length',
                'transfer-encoding',
              ].includes(name.toLowerCase())
            ) {
              res.setHeader(name, value);
            }
          }
          const setCookies = response.headers.getSetCookie?.();
          if (setCookies?.length)
            res.setHeader('set-cookie', setCookies);
          if (!res.hasHeader('content-type')) {
            res.setHeader('content-type', 'text/html; charset=utf-8');
          }
          res.end(html);
        } catch {
          next();
        }
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

async function waitReady(port, child) {
  const deadline = Date.now() + 15000;
  while (Date.now() < deadline) {
    if (child.exitCode !== null)
      throw new Error('native Plec host exited before becoming ready');
    try {
      const response = await fetch(
        `http://127.0.0.1:${port}/__plec/health`,
      );
      await response.body?.cancel();
      return;
    } catch {
      /* Still starting. */
    }
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error('timed out waiting for native Plec host');
}

async function stopHost(child) {
  if (!child || child.exitCode !== null) return;
  child.kill('SIGTERM');
  await Promise.race([
    waitExit(child),
    new Promise((resolve) => setTimeout(resolve, 5000)),
  ]);
  if (child.exitCode === null) child.kill('SIGKILL');
  await waitExit(child);
}

function waitExit(child) {
  return new Promise((resolve) => {
    if (child.exitCode !== null) resolve();
    else child.once('exit', resolve);
  });
}
