import { createRoute, useMutation, useState } from 'plec';
import { PageFrame, PageKicker } from '../components/page-primitives';
import { Route as rootRoute } from './index';

type Todo = { id: string; title: string; completed: boolean };
type TodoPatch = Pick<Todo, 'completed'> | Pick<Todo, 'title'>;
type TodoMutationInput = { id: string; body: string };

export const Route = createRoute({
  getParentRoute: () => rootRoute,
  path: 'todos',
  loader: async ({ signal }) => {
    const response = await fetch('/api/todos', { signal });
    if (!response.ok) throw new Error('Could not load todos');
    return (await response.json()) as Todo[];
  },
  pendingComponent: TodosPending,
  errorComponent: TodosError,
  component: TodosPage,
  meta: {
    title: 'Plec Todos',
    description: 'A Plec Todo API demonstration.',
  },
});

export function TodosPending() {
  return (
    <PageFrame>
      <p role="status" aria-busy="true">
        Loading todos…
      </p>
    </PageFrame>
  );
}

export function TodosError({ retry }: { retry?: () => void }) {
  return (
    <PageFrame>
      <p role="alert">Could not load todos.</p>
      <button type="button" onClick={retry}>
        Try again
      </button>
    </PageFrame>
  );
}

export function TodosPage() {
  const initialTodos = Route.useLoaderData();
  const [todos, setTodos] = useState(initialTodos);
  const [title, setTitle] = useState('');
  const [search, setSearch] = useState('');
  const [updatingId, setUpdatingId] = useState<string | undefined>(
    undefined,
  );
  const [removingId, setRemovingId] = useState<string | undefined>(
    undefined,
  );
  const [editingId, setEditingId] = useState<string | undefined>(
    undefined,
  );
  const [editingTitle, setEditingTitle] = useState('');
  const visibleTodos = todos.filter((todo) =>
    todo.title.toLowerCase().includes(search.trim().toLowerCase()),
  );
  const openCount = todos.filter((todo) => !todo.completed).length;

  const createTodo = useMutation(async (nextTitle: string) => {
    const response = await fetch('/api/todos', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ title: nextTitle }),
    });
    if (!response.ok)
      throw new Error('The Todo API rejected this change.');
    const todo = (await response.json()) as Todo;
    setTodos((items) => [...items, todo]);
    setTitle('');
    return todo;
  });

  const updateTodo = useMutation(async (input: TodoMutationInput) => {
    const response = await fetch(
      `/api/todos/${encodeURIComponent(input.id)}`,
      {
        method: 'PATCH',
        headers: { 'content-type': 'application/json' },
        body: input.body,
      },
    );
    if (!response.ok)
      throw new Error('The Todo API rejected this change.');
    const updated = (await response.json()) as Todo;
    return updated;
  });

  const removeTodo = useMutation(async (id: string) => {
    const response = await fetch(
      `/api/todos/${encodeURIComponent(id)}`,
      { method: 'DELETE' },
    );
    if (!response.ok)
      throw new Error('The Todo API rejected this change.');
    return id;
  });

  function submitTodo(event: Event) {
    event.preventDefault();
    const nextTitle = title.trim();
    if (!nextTitle) return;
    void createTodo.run(nextTitle);
  }

  async function updateTodoFor(todo: Todo, patch: TodoPatch) {
    setUpdatingId(todo.id);
    try {
      const updated = await updateTodo.run({
        id: todo.id,
        body: JSON.stringify(patch),
      });
      setTodos((items) =>
        items.map((item) => (item.id === updated.id ? updated : item)),
      );
    } catch (_) {
      // The mutation owns async lifecycle state; this handler only tracks row context.
    } finally {
      setUpdatingId((current) =>
        current === todo.id ? undefined : current,
      );
    }
  }

  async function removeTodoFor(todo: Todo) {
    setRemovingId(todo.id);
    try {
      const removedId = await removeTodo.run(todo.id);
      setTodos((items) =>
        items.filter((item) => item.id !== removedId),
      );
    } catch (_) {
      // The mutation owns async lifecycle state; this handler only tracks row context.
    } finally {
      setRemovingId((current) =>
        current === todo.id ? undefined : current,
      );
    }
  }

  const mutationError =
    createTodo.error || updateTodo.error || removeTodo.error;
  const createError =
    mutationError instanceof Error
      ? mutationError.message
      : mutationError
        ? 'Could not update todo.'
        : null;

  return (
    <PageFrame>
      <header className="grid gap-2">
        <PageKicker>Todo API</PageKicker>
        <h1 className="m-0 text-4xl font-bold tracking-tight">Todos</h1>
        <p className="m-0 text-muted-foreground">
          {openCount === 1
            ? '1 task remains'
            : `${openCount} tasks remain`}
        </p>
      </header>

      <form className="flex flex-wrap gap-2" onSubmit={submitTodo}>
        <input
          id="todo-new-title"
          className="min-w-0 flex-1 rounded-md border bg-background px-3 py-2"
          value={title}
          onInput={(event: Event) =>
            setTitle((event.currentTarget as HTMLInputElement).value)
          }
          placeholder="What needs doing?"
          aria-label="New todo"
          disabled={createTodo.pending}
        />
        <button
          type="submit"
          className="rounded-md bg-primary px-4 py-2 font-medium text-primary-foreground disabled:opacity-50"
          disabled={createTodo.pending || !title.trim()}
        >
          {createTodo.pending ? 'Adding…' : 'Add todo'}
        </button>
      </form>

      <input
        id="todo-search"
        className="w-full rounded-md border bg-background px-3 py-2"
        value={search}
        onInput={(event: Event) =>
          setSearch((event.currentTarget as HTMLInputElement).value)
        }
        placeholder="Search todos"
        aria-label="Search todos"
      />

      {createError && (
        <p role="alert" className="m-0 text-destructive">
          {createError}
        </p>
      )}

      {visibleTodos.length ? (
        <ul className="m-0 grid gap-2 p-0" aria-label="Todos">
          {visibleTodos.map((todo) => (
            <TodoRow
              key={todo.id}
              todo={todo}
              editing={editingId === todo.id}
              editingTitle={editingTitle}
              pending={
                (updateTodo.pending && updatingId === todo.id) ||
                (removeTodo.pending && removingId === todo.id)
              }
              onToggle={() =>
                updateTodoFor(todo, { completed: !todo.completed })
              }
              onStartEdit={() => {
                setEditingId(todo.id);
                setEditingTitle(todo.title);
              }}
              onEditTitle={setEditingTitle}
              onCancel={() => setEditingId(undefined)}
              onSave={() => {
                const nextTitle = editingTitle.trim();
                if (nextTitle && nextTitle !== todo.title)
                  updateTodoFor(todo, { title: nextTitle });
                setEditingId(undefined);
              }}
              onRemove={() => removeTodoFor(todo)}
            />
          ))}
        </ul>
      ) : (
        <p className="m-0 rounded-md border border-dashed p-5 text-muted-foreground">
          {todos.length
            ? 'No todos match your search.'
            : 'No todos yet. Add your first task above.'}
        </p>
      )}
    </PageFrame>
  );
}

function TodoRow({
  todo,
  editing,
  editingTitle,
  pending,
  onToggle,
  onStartEdit,
  onEditTitle,
  onSave,
  onCancel,
  onRemove,
}: {
  key?: string;
  todo: Todo;
  editing: boolean;
  editingTitle: string;
  pending: boolean;
  onToggle(): void;
  onStartEdit(): void;
  onEditTitle(title: string): void;
  onSave(): void;
  onCancel(): void;
  onRemove(): void;
}) {
  return (
    <li className="flex flex-wrap items-center gap-3 rounded-md border bg-card p-3">
      <input
        type="checkbox"
        checked={todo.completed}
        onChange={onToggle}
        disabled={pending}
        aria-label={`Mark ${todo.title} ${todo.completed ? 'open' : 'complete'}`}
      />
      {editing ? (
        <label className="contents">
          <span className="sr-only">{todo.title}</span>
          <input
            id={`todo-edit-${todo.id}`}
            className="min-w-0 flex-1 rounded border bg-background px-2 py-1"
            value={editingTitle}
            onInput={(event: Event) =>
              onEditTitle(
                (event.currentTarget as HTMLInputElement).value,
              )
            }
            onKeyDown={(event: KeyboardEvent) => {
              if (event.key === 'Enter') onSave();
              if (event.key === 'Escape') {
                onCancel();
              }
            }}
            onBlur={onSave}
            aria-label={`Rename ${todo.title}`}
            disabled={pending}
          />
        </label>
      ) : (
        <span
          className={`min-w-0 flex-1 ${todo.completed ? 'text-muted-foreground line-through' : ''}`}
        >
          {todo.title}
        </span>
      )}
      <button
        type="button"
        onClick={editing ? onCancel : onStartEdit}
        disabled={pending}
      >
        {editing ? 'Cancel' : 'Edit'}
      </button>
      <button type="button" onClick={onRemove} disabled={pending}>
        {pending ? 'Saving…' : 'Delete'}
      </button>
    </li>
  );
}
