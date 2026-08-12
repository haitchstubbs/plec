import {
  createServer,
  type IncomingMessage,
  type Server,
  type ServerResponse,
} from 'node:http';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import v8 from 'node:v8';

export interface Todo {
  id: string;
  title: string;
  completed: boolean;
}

export function createTodoApi(
  initial: Todo[] = [
    { id: 'welcome', title: 'Try the Plec Todo API', completed: false },
  ],
) {
  const todos = new Map(initial.map((todo) => [todo.id, { ...todo }]));
  const list = () => [...todos.values()];
  return {
    list,
    create(title: unknown) {
      if (typeof title !== 'string' || !title.trim()) return undefined;
      const todo = {
        id: crypto.randomUUID(),
        title: title.trim(),
        completed: false,
      };
      todos.set(todo.id, todo);
      return todo;
    },
    update(id: string, patch: unknown) {
      const todo = todos.get(id);
      if (!todo || !patch || typeof patch !== 'object')
        return undefined;
      const candidate = patch as {
        title?: unknown;
        completed?: unknown;
      };
      if (
        candidate.title !== undefined &&
        (typeof candidate.title !== 'string' || !candidate.title.trim())
      )
        return null;
      if (
        candidate.completed !== undefined &&
        typeof candidate.completed !== 'boolean'
      )
        return null;
      if (candidate.title !== undefined)
        todo.title = candidate.title.trim();
      if (candidate.completed !== undefined)
        todo.completed = candidate.completed;
      return todo;
    },
    remove(id: string) {
      return todos.delete(id);
    },
  };
}

function sendJson(
  response: ServerResponse,
  status: number,
  value: unknown,
) {
  response.writeHead(status, {
    'content-type': 'application/json; charset=utf-8',
    'cache-control': 'no-store',
  });
  response.end(JSON.stringify(value));
}

/** A deliberately small, local-development diagnostic surface. The browser
 * cannot inspect a Node process, especially in Firefox and Safari, so expose
 * the process measurements explicitly instead of relying on non-standard tab
 * memory APIs. */
function developmentMemorySnapshot() {
  const memory = process.memoryUsage();
  return {
    timestamp: new Date().toISOString(),
    pid: process.pid,
    uptimeSeconds: Math.round(process.uptime()),
    rssBytes: memory.rss,
    heapTotalBytes: memory.heapTotal,
    heapUsedBytes: memory.heapUsed,
    externalBytes: memory.external,
    arrayBuffersBytes: memory.arrayBuffers,
    heapLimitBytes: v8.getHeapStatistics().heap_size_limit,
    activeResources: process.getActiveResourcesInfo(),
  };
}

async function readJson(request: IncomingMessage): Promise<unknown> {
  const chunks: Buffer[] = [];
  for await (const chunk of request)
    chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk));
  try {
    return JSON.parse(Buffer.concat(chunks).toString('utf8'));
  } catch {
    return undefined;
  }
}

const contentTypes: Record<string, string> = {
  '.css': 'text/css; charset=utf-8',
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.wasm': 'application/wasm',
  '.woff2': 'font/woff2',
};

export function createAppServer(
  publicDir: string,
  api = createTodoApi(),
): Server {
  return createServer(async (request, response) => {
    const url = new URL(request.url ?? '/', 'http://localhost');
    const todoMatch = /^\/api\/todos\/([^/]+)$/.exec(url.pathname);
    if (url.pathname === '/api/dev/memory' && request.method === 'GET')
      return sendJson(response, 200, developmentMemorySnapshot());
    if (url.pathname === '/api/todos' && request.method === 'GET')
      return sendJson(response, 200, api.list());
    if (url.pathname === '/api/todos' && request.method === 'POST') {
      const body = (await readJson(request)) as
        { title?: unknown } | undefined;
      const todo = api.create(body?.title);
      return todo
        ? sendJson(response, 201, todo)
        : sendJson(response, 400, {
            error: 'title must be a non-empty string',
          });
    }
    if (todoMatch && request.method === 'PATCH') {
      const updated = api.update(
        decodeURIComponent(todoMatch[1]!),
        await readJson(request),
      );
      return updated === null
        ? sendJson(response, 400, {
            error:
              'title must be non-empty and completed must be boolean',
          })
        : updated
          ? sendJson(response, 200, updated)
          : sendJson(response, 404, { error: 'todo not found' });
    }
    if (todoMatch && request.method === 'DELETE')
      return api.remove(decodeURIComponent(todoMatch[1]!))
        ? sendJson(response, 204, undefined)
        : sendJson(response, 404, { error: 'todo not found' });
    if (url.pathname.startsWith('/api/'))
      return sendJson(response, 404, { error: 'endpoint not found' });

    const requested =
      url.pathname === '/' || !path.extname(url.pathname)
        ? 'index.html'
        : url.pathname.replace(/^\/+/, '');
    const filePath = path.resolve(publicDir, requested);
    if (
      !filePath.startsWith(`${path.resolve(publicDir)}${path.sep}`) &&
      filePath !== path.resolve(publicDir, 'index.html')
    )
      return sendJson(response, 400, { error: 'invalid asset path' });
    try {
      const encoding = selectEncoding(
        request.headers['accept-encoding'],
      );
      const compressedPath = encoding && url.pathname.startsWith('/runtime/')
        ? `${filePath}.${encoding === 'gzip' ? 'gz' : 'br'}`
        : undefined;
      // wasm-pack emits the uncompressed runtime by default. Prefer a
      // precompressed sibling when one exists, but never turn an ordinary
      // module request into a 404 merely because the browser accepts br/gzip.
      const [body, servedCompressed] = compressedPath
        ? await readFile(compressedPath)
            .then((value) => [value, true] as const)
            .catch(() => readFile(filePath).then((value) => [value, false] as const))
        : [await readFile(filePath), false] as const;
      const ext = path.extname(filePath);
      // Build artifacts deliberately use stable URLs during this experiment.
      // They must therefore revalidate: marking a fixed `client.js` or IR URL
      // immutable leaves an already-open development browser on an old graph.
      response.writeHead(200, {
        'content-type': contentTypes[ext] ?? 'application/octet-stream',
        'cache-control': 'no-cache',
        ...(encoding && servedCompressed
          ? { 'content-encoding': encoding, vary: 'Accept-Encoding' }
          : {}),
      });
      response.end(body);
    } catch {
      sendJson(response, 404, { error: 'asset not found' });
    }
  });
}

function selectEncoding(
  header: string | string[] | undefined,
): 'br' | 'gzip' | undefined {
  const value = Array.isArray(header)
    ? header.join(',')
    : (header ?? '');
  if (/\bbr\b/.test(value)) return 'br';
  if (/\bgzip\b/.test(value)) return 'gzip';
  return undefined;
}

if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  const port = Number(process.env.PORT ?? 3100);
  createAppServer(path.resolve('dist/public')).listen(port, () =>
    console.log(`Plec fullstack playground on http://localhost:${port}`),
  );
}
