# Mutations

`useMutation` models an application-authored asynchronous operation:

```ts
const saveTodo = useMutation(async (todo: Todo) => {
  const response = await fetch('/api/todos', {
    method: 'POST',
    body: JSON.stringify(todo),
  });
  if (!response.ok) throw new Error('Could not save todo.');
  return (await response.json()) as Todo;
});

await saveTodo.run(todo);
saveTodo.pending;
saveTodo.error;
saveTodo.data;
```

Forms can use a mutation directly as their submit handler. Plec prevents the
native navigation and forwards a serializable submission event to the mutation
callback. It does not expose the DOM form object:

```tsx
const saveTodo = useMutation(async (submission) => {
  const title = submission.formData.title;
  const intent = submission.submitter?.value;
  return saveTodoRequest(String(title ?? ''), intent);
});

return (
  <form onSubmit={saveTodo}>
    <input name="title" />
    <button type="submit">Save</button>
  </form>
);
```

`formData` contains ordinary successful controls. Repeated names are represented
as arrays; a name with one value is a scalar. `submitter` contains the initiating
button's `name` and `value` when the browser provides one, and is otherwise
`null`. Browser validation, keyboard submission, and ordinary SSR form markup
remain native.

Every submit starts an invocation. The latest invocation owns published state;
Plec does not automatically suppress duplicate submissions. Disable the submit
button with `disabled={mutation.pending}` when only one request should be
allowed. Plec also does not reset the form after success. Authors reset values
explicitly when desired.

Mutation state belongs to component or graph region that creates mutation. When
that owner is disposed, later completion cannot publish mutation state.

Mutation callbacks use latest-started invocation wins publication. A completion
from older invocation is ignored when newer invocation already started. Starting
new invocation does not abort older invocation in V1.

Mutations reuse existing graph-owned fetch lifecycle. Fetches are aborted when
their graph is disposed, and graph-generation checks reject late completions.
Mutations do not add per-mutation abort controllers, mutation registries,
caches, retries, schedulers, or transport behavior.

Successful runs publish data, clear error, clear pending, and resolve with the
result. Failed runs publish error, clear pending, and reject with same failure.

When a route owns a loader, `Route.useReload()` returns its explicit
revalidation callable. It reruns only that route's loader with current params
and URL state; parent and child loaders are not recursively invalidated.
