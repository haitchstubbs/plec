# Application errors

Plec currently has no general application-failure type or error-throw syntax
for route loaders. Compiled loaders support `redirect()` and `notFound()` as
framework-owned terminal outcomes; other loader source forms are rejected by
the compiler. Browser actions use the action machine's rejection path, and
`useMutation` publishes that value as mutation state while rejecting the
initiating operation.

## Execution boundaries

| Boundary              | Application-visible behavior                                                                                                                                                                                                            | HTTP behavior                                                                           | Serialization                                                                                                                        |
| --------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| Route loader          | Successful value becomes loader data. Failed fetch/action selects the route error graph; it is not loader data. `redirect()` and `notFound()` remain terminal routing outcomes.                                                         | SSR renders the selected error graph with the normal document status; not-found is 404. | SSR and CSR use `PublicRouteLoaderFailure` from `plec-ir`; snapshot validation bounds and checks it before adoption.                 |
| Mutation              | A failed current invocation clears `pending`, publishes its rejection to `mutation.error`, and rejects its caller. A later successful current invocation clears the published error. Stale invocations do not publish over newer state. | None inherently.                                                                        | Local to the browser action runtime.                                                                                                 |
| File API              | A returned `Response` is passed through as HTTP. An uncaught handler exception becomes a 500 response.                                                                                                                                  | Explicit `Response` status is authoritative; thrown failures produce 500.               | Standard HTTP response. Thrown details are redacted to `{"error":"Internal Server Error"}`.                                          |
| Filesystem middleware | A returned `Response` remains authoritative. Uncaught failures use the host's HTTP 500 path.                                                                                                                                            | HTTP-owned.                                                                             | Standard HTTP response.                                                                                                              |
| SSR internal failure  | Not application loader data. The server returns its failure response (or the configured development shell fallback).                                                                                                                    | Server-owned 500/fallback behavior; not-found is 404.                                   | Generic 500 response in production-facing body. Development fallback diagnostics may be emitted in the `x-plec-ssr-fallback` header. |
| Client navigation     | Failed loaders select the route error graph; interrupted route data is not committed.                                                                                                                                                   | None.                                                                                   | Already local to the browser. The route error value is a `RuntimeValue` record.                                                      |
| Browser action        | A failed action remains a rejected operation.                                                                                                                                                                                           | None.                                                                                   | Normally local to the browser. Internal action-machine errors are host errors, not generated application records.                    |
| Server actions        | Not implemented.                                                                                                                                                                                                                        | —                                                                                       | —                                                                                                                                    |

## Route error values

The authoritative public route-loader failure shape is `{ kind, message,
status?, statusText?, body?, url? }`. `kind` is `http`, `network`, `abort`,
`decode`, or `runtime`. HTTP response status, status text, body, and URL are
preserved where available. SSR snapshots carry this same typed representation;
adoption restores its fields without changing the failure kind. Invalid or
over-budget serialized failures are rejected by snapshot validation and cannot
be adopted as application data; the client falls back to its normal remount
path. (Generic public-failure normalization for malformed snapshot entries is
still pending.)

The compiler currently rejects unsupported loader forms rather than allowing
arbitrary throws. There is no mapping for thrown `Error`, strings, numbers,
`null`, or objects; this issue does not add throw syntax. An object's `status`
field is not authoritative HTTP status.
The HTTP API/middleware status comes only from the returned `Response` or the
host's failure handling.

## Internal failures and redaction

Compiler, runtime, artifact, protocol, SSR, and host failures are internal
diagnostics. They may select an error UI or produce an HTTP 500, but their
implementation message is not application data. The Node application runtime
redacts uncaught handler exceptions to the generic JSON body
`{"error":"Internal Server Error"}` and logs the original exception on the
server process. The Rust HTTP host likewise uses a generic 500 body for
uncaught host/SSR errors. Explicit application `Response` objects are not
rewritten.

No stack, cause, prototype, arbitrary custom field, filesystem path, or
sidecar detail is part of the production 500 payload. Development SSR fallback
diagnostics are available only on the server response header and do not alter
the browser bootstrap payload.

## Compatibility

- `redirect()` and `notFound()` remain distinct framework routing outcomes.
- Loader failures do not become loader data or commit an interrupted route.
- Mutation publication remains latest-started-wins; the initiating operation
  still rejects on failure.
- File API and middleware `Response` status and body semantics remain HTTP
  semantics and are not converted into successful JSON payloads.
