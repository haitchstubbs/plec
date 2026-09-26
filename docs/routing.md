# Route params and search values

`Route.useParams()` returns decoded path parameters owned by the matching route. Literal path definitions infer parameter names:

```ts
const Route = createRoute({
  path: 'projects/$projectId',
  component: Project,
});
const { projectId } = Route.useParams();
```

`Route.useSearch()` returns query values as strings. A key appearing once maps to `string`; repeated keys map to `string[]` in URL order. Query parsing uses form decoding, so `+` becomes a space. Invalid percent escapes throw, matching strict path-parameter decoding. The accessors are valid only while rendering their own route.
