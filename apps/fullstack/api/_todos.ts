import type { Todo } from 'plec/server';

let failure = false;
export const api = (() => {
  const todos = new Map<string, Todo>([
    [
      'welcome',
      {
        id: 'welcome',
        title: 'Try the Plec Todo API',
        completed: false,
      },
    ],
  ]);
  return {
    list: () => [...todos.values()],
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
    remove: (id: string) => todos.delete(id),
    failNext: () => {
      failure = true;
    },
    consumeFailure: () => {
      const current = failure;
      failure = false;
      return current;
    },
  };
})();

export const json = (value: unknown, status = 200) =>
  new Response(status === 204 ? undefined : JSON.stringify(value), {
    status,
    headers:
      status === 204
        ? {}
        : {
            'content-type': 'application/json; charset=utf-8',
            'cache-control': 'no-store',
          },
  });
