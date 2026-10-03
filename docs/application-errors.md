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
implementation message is not application data. The Node application runtime
redacts uncaught handler exceptions to the generic JSON body
`{"error":"Internal Server Error"}` and logs the original exception on the
server process. The Rust HTTP host likewise uses a generic 500 body for
uncaught host/SSR errors. Explicit application `Response` objects are not
rewritten.

Server-action implementation failures follow the same redaction boundary:
Node logs the original exception, while the Rust-owned reserved endpoint
returns only a generic action failure. Unknown/stale action IDs are a distinct 404. The action ID is not an authorization mechanism; every action must perform
application-specific authentication and authorization itself.

No stack, cause, prototype, arbitrary custom field, filesystem path, or
sidecar detail is part of the production 500 payload. Development SSR fallback
diagnostics are available only on the server response header and do not alter
the browser bootstrap payload.

## Development diagnostics

Development-facing boundary diagnostics use a stable code, owning phase, and
concise message; source locations and detail are included only when the owning
boundary already knows them and it is safe to expose them. Compiler errors
continue to use their compiler diagnostic codes and source spans. Native host
startup/request failures identify the server or sidecar phase in the CLI
output. Sidecar stderr and captured startup detail are included only when the
host is started in development mode (`NODE_ENV` is not `production`).

The browser adapter publishes actionable boundary failures to the optional
`onDiagnostic` callback and as a `plec:diagnostic` `CustomEvent`. Existing
adoption outcome reporting remains available through `onAdoptionDiagnostic`
and `plec:adoption`; a fallback also produces a concise common diagnostic.
Set `development: true` in `PlecRouterMountOptions` to include safe local
failure detail. Production diagnostics omit that detail. Browser diagnostic
codes identify the failing boundary; `phase` tells whether to inspect artifact
loading, runtime validation, SSR adoption, provider resolution, or protocol
compatibility. The CLI/server console is the place to inspect server-side
details; they are never copied into public HTTP bodies or browser bootstrap
state.

## Compatibility

- `redirect()` and `notFound()` remain distinct framework routing outcomes.
- Loader failures do not become loader data or commit an interrupted route.
- Mutation publication remains latest-started-wins; the initiating operation
  still rejects on failure.
- File API and middleware `Response` status and body semantics remain HTTP
  semantics and are not converted into successful JSON payloads.

## Issue #45 acceptance map

| Acceptance criterion                                    | Implementation boundary                                                                                           | Regression coverage                                                                                                                                                                                                                                                                                                                  |
| ------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Major execution boundaries are documented               | This document's execution-boundary table                                                                          | `application-errors.md` — Execution boundaries                                                                                                                                                                                                                                                                                       |
| Explicit server serialization and redaction             | `crates/plec-server/src/loader.rs`, Node sidecar HTTP dispatch, Rust HTTP host                                    | `renders_the_error_phase_and_records_the_rejection_when_the_loader_fails`; `application_responses_pass_through_verbatim`; `uncaught_api_handler_failure_is_redacted_http_500_and_runtime_survives`; smoke middleware failure redaction test                                                                                          |
| SSR/client route-loader parity                          | `plec-schema` public record conversions; server loader; client route navigation; snapshot adoption                | `ssr_adoption_and_fresh_navigation_expose_equivalent_http_route_errors`; `loader_network_failure_uses_the_browser_public_failure_kind`; `loader_decode_failure_uses_the_browser_public_failure_kind`; `typed_fetch_routes_http_decode_network_and_abort_failures`; `malformed_public_loader_failure_adopts_as_generic_route_failure` |
| Mutation caller rejection and `mutation.error`          | Compiled action machine and mutation state publisher                                                              | `todo create, complete, rename, and delete stay targeted` in `packages/plec-e2e/tests/acceptance/todos-actions.playwright.ts` asserts visible rejection, retained draft, pending clear, later-success clear, and stale-failure suppression; VM concurrency suite covers settlement ordering                                          |
| Standard API/middleware HTTP failures                   | Node app request runtime and server HTTP response boundary                                                        | `application_responses_pass_through_verbatim`; `uncaught_api_handler_failure_is_redacted_http_500_and_runtime_survives`; smoke `middleware rejects duplicate next calls through the application error path`                                                                                                                          |
| Internal runtime failures excluded from public records  | Shared public normalizer plus explicit host redaction                                                             | `malformed_runtime_failure_uses_generic_public_record`; `malformed_public_loader_failure_normalizes_without_rejecting_snapshot`; server generic-500 assertions                                                                                                                                                                       |
| Richer development diagnostics without production drift | `crates/plec-server/src/http.rs` development fallback header and error logging; production HTTP response boundary | `falls_back_to_the_public_shell_when_the_artifact_is_unreadable`; `production_internal_errors_are_generic_and_independent_of_diagnostic_detail`; `route_loaders_without_a_valid_program_fail_the_document_render`; `uncaught_api_handler_failure_is_redacted_http_500_and_runtime_survives`                                          |

Malformed public failure normalization is scoped to `Rejected.failure` in
snapshot deserialization. `malformed_unrelated_snapshot_structure_still_fails_closed`
and runtime adoption mismatch tests cover the strict behavior for unrelated
snapshot structure.
