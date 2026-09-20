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
