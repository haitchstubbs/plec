import v8 from 'node:v8';

export interface Todo { id: string; title: string; completed: boolean; }

export function createTodoApi(initial: Todo[] = [{ id: 'welcome', title: 'Try the Plec Todo API', completed: false }]) {
  const todos = new Map(initial.map((todo) => [todo.id, { ...todo }]));
  return {
    list: () => [...todos.values()],
    create(title: unknown) {
      if (typeof title !== 'string' || !title.trim()) return undefined;
      const todo = { id: crypto.randomUUID(), title: title.trim(), completed: false };
      todos.set(todo.id, todo); return todo;
    },
    update(id: string, patch: unknown) {
      const todo = todos.get(id);
      if (!todo || !patch || typeof patch !== 'object') return undefined;
      const candidate = patch as { title?: unknown; completed?: unknown };
      if (candidate.title !== undefined && (typeof candidate.title !== 'string' || !candidate.title.trim())) return null;
      if (candidate.completed !== undefined && typeof candidate.completed !== 'boolean') return null;
      if (candidate.title !== undefined) todo.title = candidate.title.trim();
      if (candidate.completed !== undefined) todo.completed = candidate.completed;
      return todo;
    },
    remove(id: string) { return todos.delete(id); },
  };
}

/** Temporary host/business-logic escape hatch, not a Plec server capability. */
export function createTodoApiHandler(api = createTodoApi()) {
  return async (request: Request): Promise<Response | undefined> => {
    const url = new URL(request.url);
    const todoMatch = /^\/api\/todos\/([^/]+)$/.exec(url.pathname);
    const json = (value: unknown, status = 200) => new Response(status === 204 ? undefined : JSON.stringify(value), { status, headers: status === 204 ? {} : { 'content-type': 'application/json; charset=utf-8', 'cache-control': 'no-store' } });
    if (url.pathname === '/api/dev/memory' && request.method === 'GET') {
      const memory = process.memoryUsage();
      return json({ timestamp: new Date().toISOString(), pid: process.pid, uptimeSeconds: Math.round(process.uptime()), rssBytes: memory.rss, heapTotalBytes: memory.heapTotal, heapUsedBytes: memory.heapUsed, externalBytes: memory.external, arrayBuffersBytes: memory.arrayBuffers, heapLimitBytes: v8.getHeapStatistics().heap_size_limit, activeResources: process.getActiveResourcesInfo() });
    }
    if (url.pathname === '/api/todos' && request.method === 'GET') return json(api.list());
    if (url.pathname === '/api/notes' && request.method === 'GET') {
      return json({
        headline: 'Notes transferred from the server',
        detail: 'This loader ran during SSR; the browser resumed the outcome instead of refetching.',
      });
    }
    if (url.pathname === '/api/todos' && request.method === 'POST') {
      const body = await request.json().catch(() => undefined) as { title?: unknown } | undefined;
      const todo = api.create(body?.title);
      return todo ? json(todo, 201) : json({ error: 'title must be a non-empty string' }, 400);
    }
    if (todoMatch && request.method === 'PATCH') {
      const updated = api.update(decodeURIComponent(todoMatch[1]!), await request.json().catch(() => undefined));
      return updated === null ? json({ error: 'title must be non-empty and completed must be boolean' }, 400) : updated ? json(updated) : json({ error: 'todo not found' }, 404);
    }
    if (todoMatch && request.method === 'DELETE') return api.remove(decodeURIComponent(todoMatch[1]!)) ? new Response(null, { status: 204 }) : json({ error: 'todo not found' }, 404);
    return undefined;
  };
}
