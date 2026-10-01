# Server Actions V1

Server actions expose explicitly declared server-side JavaScript functions to
compiled Plec actions:

```ts
import { action } from '@plec/core';

export const echo = action(async (value: string) => ({
  echoed: value,
}));
```

Only an exported `const` initialized directly with
`action(async (...) => ...)` is supported. The implementation runs as
ordinary JavaScript in the generated Node server bundle; its body and imports
are not compiled into Plec IR or included in public graphs. Compiled client
graphs contain only an opaque action reference. Plec actions invoke it with
`await echo(value)`. `useMutation` wraps that asynchronous operation and
continues to own its existing pending, error, data, and latest-invocation
semantics.

Arguments and results are limited to Plec's bounded serializable runtime value
space: null, booleans, finite numbers, strings, arrays, and records. Functions,
class instances, and other arbitrary JavaScript objects are unsupported.

The Rust host exposes actions as same-origin `POST /_plec/actions/<id>`
requests. The implementation's exception details are logged server-side and
redacted from the public response. Action IDs are opaque routing identifiers,
not authorization: applications must enforce their own authentication and
authorization inside each action. The V1 endpoint rejects cross-origin POSTs.

Server actions are public mutation endpoints; do not treat an opaque ID or the
same-origin check as a substitute for authorization. V1 does not implement
directives, forms, FormData, redirects, cache invalidation, streaming, or
batching.
