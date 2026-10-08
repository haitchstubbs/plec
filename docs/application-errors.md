# Application errors

Plec currently has no general application-failure type or error-throw syntax
for route loaders. Compiled loaders support `redirect()` and `notFound()` as
framework-owned terminal outcomes; other loader source forms are rejected by
the compiler. Browser actions use the action machine's rejection path, and
`useMutation` publishes that value as mutation state while rejecting the
initiating operation.

## Execution boundaries

| Boundary              | Application-visible behavior                                                                                                                                                                                                            | HTTP behavior                                                                                             | Serialization                                                                                                                        |
| --------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| Route loader          | Successful value becomes loader data. Failed fetch/action selects the route error graph; it is not loader data. `redirect()` and `notFound()` remain terminal routing outcomes.                                                         | SSR renders the selected error graph with the normal document status; not-found is 404.                   | `plec-schema` owns the shared RuntimeValue conversion; snapshots carry `PublicRouteLoaderFailure`.                                   |
| Mutation              | A failed current invocation clears `pending`, publishes its rejection to `mutation.error`, and rejects its caller. A later successful current invocation clears the published error. Stale invocations do not publish over newer state. | None inherently.                                                                                          | Local to the browser action runtime.                                                                                                 |
| File API              | A returned `Response` is passed through as HTTP. An uncaught handler exception becomes a 500 response.                                                                                                                                  | Explicit `Response` status is authoritative; thrown failures produce 500.                                 | Standard HTTP response. Thrown details are redacted to `{"error":"Internal Server Error"}`.                                          |
| Filesystem middleware | A returned `Response` remains authoritative. Uncaught failures use the host's HTTP 500 path.                                                                                                                                            | HTTP-owned.                                                                                               | Standard HTTP response.                                                                                                              |
| SSR internal failure  | Not application loader data. The server returns its failure response (or the configured development shell fallback).                                                                                                                    | Server-owned 500/fallback behavior; not-found is 404.                                                     | Generic 500 response in production-facing body. Development fallback diagnostics may be emitted in the `x-plec-ssr-fallback` header. |
| Client navigation     | Failed loaders select the route error graph; interrupted route data is not committed.                                                                                                                                                   | None.                                                                                                     | Already local to the browser. The route error value is a `RuntimeValue` record.                                                      |
| Browser action        | A failed action remains a rejected operation.                                                                                                                                                                                           | None.                                                                                                     | Normally local to the browser. Internal action-machine errors are host errors, not generated application records.                    |
| Server actions        | Successful serializable result resolves the compiled action; invocation failures reject it and flow into `useMutation.error` when wrapped by a mutation.                                                                                | Reserved same-origin POST endpoint. Unknown IDs return 404; implementation failures return a generic 500. | Plec RuntimeValue arguments/results are bounded; uncaught Node exceptions are redacted.                                              |

## Route error values

The authoritative public route-loader failure shape is `{ kind, message,
status?, statusText?, body?, url? }`. `kind` is `http`, `network`, `abort`,
`decode`, or `runtime`. HTTP response status, canonical status text, body, and
URL are represented where available. RuntimeValue failures with a string
`statusText` and valid HTTP status are normalized to the canonical phrase;
omitted text stays omitted. A phrase without status is malformed. SSR snapshots
carry this same typed representation; adoption restores its fields without
changing the failure kind. Supplementary
failure bodies are capped at `PublicRouteLoaderFailure::MAX_BODY_BYTES` (512
KiB), safely below the 4 MiB snapshot ceiling; unreadable or over-budget HTTP
error bodies normalize to `null` while retaining the HTTP failure. Browser and
SSR HTTP failure decoding only parses JSON media types, and malformed public
failure records normalize to the generic public failure without relaxing
validation of other snapshot structure.

Field applicability is enforced by the shared record validator: `http` may
carry status, statusText, body, and URL; `network`, `abort`, and `decode` may
carry only URL in addition to message; `runtime` must be exactly the generic
`route loader failed` record with no supplemental fields. Status is limited to
100–599. If present, `statusText` must exactly match the canonical phrase for
that numeric status from the shared Rust mapping (unknown/extension codes use
the empty string). Serialized snapshots with a missing status or noncanonical
phrase normalize to the generic failure. The phrase never comes from browser
or upstream reason-phrase text. Public network failures use
`network request failed`, and aborts use `request aborted`; host-native Fetch
error text remains diagnostic-only.

The compiler currently rejects unsupported loader forms rather than allowing
arbitrary throws. There is no mapping for thrown `Error`, strings, numbers,
`null`, or objects; this issue does not add throw syntax. An object's `status`
field is not authoritative HTTP status.
The HTTP API/middleware status comes only from the returned `Response` or the
host's failure handling.

## Internal failures and redaction

Compiler, runtime, artifact, protocol, SSR, and host failures are internal
diagnostics. They may select an error UI or produce an HTTP 500, but their
implementation message is not application data. The Node host redacts
uncaught API handler exceptions to the generic JSON body
`{"error":"Internal Server Error"}` and logs the original exception on the
server process. Document failures follow the configured development-shell or
generic production response policy. Explicit application `Response` objects
are not rewritten.

Server-action implementation failures follow the same redaction boundary:
Node logs the original exception, while the Node host maps the Rust engine's
action outcome to a generic public failure. Unknown/stale action IDs are a
distinct 404. The action ID is not an authorization mechanism; every action
must perform application-specific authentication and authorization itself.

No stack, cause, prototype, arbitrary custom field, filesystem path, or
internal host detail is part of the production 500 payload. Development SSR fallback
diagnostics are available only on the server response header and do not alter
the browser bootstrap payload.

## Development diagnostics

Development-facing boundary diagnostics use a stable `PLEC-*` code, owning
phase, concise message, and optional source/location, detail, and suggestion.
The adapter preserves authoritative domain errors; it does not turn them into
application state. Compiler parse failures print `[PLEC-PARSE-001] parse:`
with the module and line/column retained from the parser's source map. Other
compiler failures use `[PLEC-COMPILE-001] compile:`. Build failures map their
existing build stage into a `[PLEC-BUILD-*]` or
`[PLEC-DISCOVERY-ROUTES] discovery:` diagnostic. Configuration failures
identify the config/build phase and suggest checking `plec.toml`.

The CLI defaults to concise output. Set `PLEC_DIAGNOSTICS=verbose` to include
the original compiler diagnostic text or safe build error cause where
available. Compiler parse/compile codes use numeric subcodes; boundary codes
use stable domain/class names under the same `PLEC-*` prefix. Native server/operator logs
identify request, SSR, or action boundaries and retain operational error
context; the public HTTP body remains bounded/redacted. Ordinary application
`console.log` / `console.error` output from the Node host is forwarded in both
development and production. Native host startup and request errors use the
Node host's controlled diagnostics; public HTTP/action output never receives
internal detail. A thrown application action is classified as
`PLEC-SERVER-ACTION`, not as a protocol failure. Host-provider rendering uses
`PLEC-PROVIDER-RENDER`; server manifest loading uses
`PLEC-SERVER-MANIFEST`.

The browser adapter publishes actionable boundary failures to the optional
`onDiagnostic` callback and as a `plec:diagnostic` `CustomEvent`. Existing
adoption outcome reporting remains available through `onAdoptionDiagnostic`
and `plec:adoption`; a fallback also produces a concise common diagnostic.
Set `development: true` in `PlecRouterMountOptions` to include safe local
failure detail. Browser codes include `PLEC-ARTIFACT-LOAD`,
`PLEC-SSR-ADOPTION`, `PLEC-PROVIDER-RESOLUTION`,
`PLEC-PROTOCOL-COMPATIBILITY`, and `PLEC-BROWSER-RUNTIME`. Production browser
diagnostic objects omit local detail. The `phase` identifies whether to
inspect artifact loading, WASM runtime validation, SSR adoption, provider
resolution, or protocol compatibility. These diagnostics are developer
output; loader, action, and mutation public failure contracts remain their
existing typed or redacted contracts. Server-only source/stack detail is not
copied into public HTTP bodies or browser bootstrap state.

These are four separate output surfaces: application-visible failures follow
the existing public contracts; development diagnostics add Plec codes and
phases; server/operator logs retain process-level application logs; browser
diagnostics are delivered only through the callback/events described above.
Production redaction applies at the HTTP/browser boundary and to diagnostic
enrichment, not to ordinary operator logging.

## Compatibility

- `redirect()` and `notFound()` remain distinct framework routing outcomes.
- Loader failures do not become loader data or commit an interrupted route.
- Mutation publication remains latest-started-wins; the initiating operation
  still rejects on failure.
- File API and middleware `Response` status and body semantics remain HTTP
  semantics and are not converted into successful JSON payloads.

## Issue #45 acceptance map

| Acceptance criterion                                    | Implementation boundary                                                                                          | Regression coverage                                                                                                                                                                                                                                                                         |
| ------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Major execution boundaries are documented               | This document's execution-boundary table                                                                         | `application-errors.md` — Execution boundaries                                                                                                                                                                                                                                              |
| Explicit server serialization and redaction             | `crates/plec-server-engine/src/loader.rs`, `packages/plec-node/src` HTTP dispatch and redaction                  | `returns the API fallback 404 and redacts handler failures`; `preserves action method, origin, ID, JSON, unknown-ID, and callback failure outcomes`; smoke middleware failure redaction test                                                                                                |
| SSR/client route-loader parity                          | `plec-schema` public record conversions; `plec-server-engine` loader; client route navigation; snapshot adoption | `adoption.playwright.ts` loader transfer; `loader-outcomes.playwright.ts` redirect and not-found coverage; `todos-loaders.playwright.ts` pending, success, error, and retry                                                                                                                 |
| Mutation caller rejection and `mutation.error`          | Compiled action machine and mutation state publisher                                                             | `todo create, complete, rename, and delete stay targeted` in `packages/plec-e2e/tests/acceptance/todos-actions.playwright.ts` asserts visible rejection, retained draft, pending clear, later-success clear, and stale-failure suppression; VM concurrency suite covers settlement ordering |
| Standard API/middleware HTTP failures                   | Node app request runtime and server HTTP response boundary                                                       | `returns the API fallback 404 and redacts handler failures`; smoke `middleware rejects duplicate next calls through the application error path`                                                                                                                                             |
| Internal runtime failures excluded from public records  | Shared public normalizer plus Node host redaction                                                                | `plec-schema` public failure validation; `api.test.ts` handler-failure redaction; `actions.test.ts` action-failure outcomes                                                                                                                                                                 |
| Richer development diagnostics without production drift | `packages/plec-node/src/index.ts` fallback header and logging; production HTTP response boundary                 | `document.test.ts` compiled-document path; `plec dev` failed-build recovery in `dev-reload.playwright.ts`                                                                                                                                                                                   |

Malformed public failure normalization is scoped to `Rejected.failure` in
snapshot deserialization. `malformed_unrelated_snapshot_structure_still_fails_closed`
and runtime adoption mismatch tests cover the strict behavior for unrelated
snapshot structure.
