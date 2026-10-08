# Plec Node Host Target Implementation Architecture

> **Final architecture:** This document began as a migration plan. Its migration-era Axum/sidecar steps below are historical, not current architecture. `@plec/node` is the sole first-party HTTP host, `plec-server-engine` is the sole Rust server-semantics authority, and `plec-node-bindings` is their in-process N-API boundary. Slice 8 retires the Axum host, Rust `serve`, `packages/plec-node-runtime`, private Rust↔Node transport, and generated `runtime.mjs` artifacts. In development, Vite attaches the Node request dispatcher to its listener; no internal host port is opened.

## Preface

Plec's current production server architecture is primarily Rust-owned. This keeps Plec-native semantics—routing, loaders, SSR, snapshots, action validation—inside the Rust implementation, but arbitrary application JavaScript still needs a JavaScript runtime.

Today this is solved through a supervised Node sidecar:

```text id="7pyvqt"
public HTTP
    ↓
Rust / Axum
    │
    ├── Plec semantics
    │
    └── private UDS / loopback HTTP
               ↓
          Node sidecar
               ↓
          application JS
```

This works, but it imposes:

- two processes;
- private HTTP serialization/deserialization;
- sidecar authentication;
- readiness and health protocols;
- sidecar process supervision;
- duplicated HTTP/request normalization;
- buffered request/response boundaries in the current Node sidecar;
- an operational model that is unusual for a Node-deployed framework.

Before Plec 0.1, introduce a first-class Node production host where:

> **Node owns the public process, socket, HTTP transport, arbitrary application JavaScript, and static file transport. Rust remains authoritative for Plec semantics.**

The target architecture is:

```text id="kwazvc"
                    Node
                      │
                @plec/node
                      │
                      ▼
              PlecApplication
               napi-rs class
                      │
         ┌────────────┴────────────┐
         │                         │
      streams                 callbacks
         │                         │
  napi ReadableStream       PlecCallbackManager
 pull/cancel semantics       │             │
         │              invokeAction   renderHost
         │                   │             │
         ▼                   └──────┬──────┘
   plec execution                   ▼
      engine                  Promise-aware TSFN
         │                         │
         └─────────────────────────┘
```

Lifecycle:

```text id="i2vgd7"
load
 ↓
accept requests
 ↓
close()
 ↓
stop admission
 ↓
cancel/drain native operations
 ↓
release all scoped TSFNs
 ↓
drop Rust application
```

This is an architectural migration, not a redesign of Plec's application-facing APIs.

---

# 1. Decisions locked before implementation

The implementation should treat the following as architectural decisions rather than questions to rediscover.

## 1.1 Node owns the public server

The production request path becomes:

```text id="2o8gye"
Browser
   ↓
node:http
   ↓
@plec/node
```

Rust must not bind the public production listener in the new host.

The existing Axum host remains operational during migration and parity testing, but does not define the target production architecture.

## 1.2 Rust continues to own Plec semantics

Rust remains authoritative for:

```text id="h1sy6p"
document route matching
compiled route loaders
redirect/not-found semantics
SSR
snapshot generation
compiled artifact interpretation
server-action transport validation
RuntimeValue validation and limits
host-provider SSR boundary ownership
```

Do not reimplement any of these semantics in TypeScript.

## 1.3 `/api` and `/api/*` remain JavaScript-owned

The generated application bundle already owns:

```text id="3bls88"
API route matching
method matching
middleware composition
route params
application fallback dispatch
```

The new production path is therefore:

```text id="m1s0tu"
Node HTTP
   ↓
generated server/app.mjs
```

not:

```text id="78er5h"
Node
 ↓
Rust
 ↓
Node
```

for ordinary `/api/*` traffic.

The API path predicate must include the exact `/api` path as well as its
descendants:

```ts
function isApiPath(pathname: string): boolean {
  return pathname === '/api' || pathname.startsWith('/api/');
}
```

This matches the generated matcher in
`crates/plec-build/src/modules/server.rs::matchRoute`, which recognizes both
`/api` and `/api/...`. It intentionally corrects the current Rust dispatcher's
`/api/`-only check. Add explicit coverage for `/api`, `/api/`, and `/api/foo`.

This permits `ApplicationRuntime::dispatch()` to become unnecessary in the Node-host architecture.

## 1.4 Server actions remain Rust-mediated

Server actions cross Rust because their transport and value semantics are Plec-owned:

```text id="sy06lc"
browser
   ↓
Node
   ↓
Rust action semantic boundary
   ↓
JS invokeAction()
   ↓
Rust result validation
   ↓
Node response
```

## 1.5 Host-provider SSR remains Rust-mediated

```text id="nwrsgg"
Rust SSR
   ↓
renderHost callback
   ↓
Node provider registry
   ↓
HTML fragment
   ↓
Rust resumes SSR
```

## 1.6 Static files are Node-owned in the target architecture

Do not send large static files through N-API solely to reuse `tower_http::ServeDir`.

The Node host should eventually reproduce the current static-file contract:

```text id="ck8wd1"
containment/traversal protection
content type
HEAD
range requests
.br/.gz negotiation
cache-control behavior
404 / invalid-path behavior
```

The current `ServeDir` implementation remains the parity reference.

## 1.7 The N-API boundary supports streaming; bounded API bodies are an exception

Do not introduce a permanent buffer-all bridge. Native request/response bodies
and static assets use streams. The existing API body ceiling is deliberately
different: API handlers must not run until the complete body has passed the
current 1 MiB limit, so Node will read API bodies into a bounded buffer before
constructing the Web `Request`.

Native action request:

```text id="eq075z"
IncomingMessage
 ↓
Readable.toWeb()
 ↓
ReadableStream<Uint8Array>
 ↓
napi-rs
 ↓
Rust pull-based stream
  ↓
incremental limit check, then bounded buffer for JSON decoding
```

This is the streaming request path used to prove the native bridge. Document
requests do not need an inbound body stream. API bodies are fully pre-read up
to the limit before application dispatch, preserving the current
reject-before-handler security contract.

Response:

```text id="ckh8ci"
Rust Stream<Result<Bytes>>
 ↓
napi-rs ReadableStream
 ↓
Readable.fromWeb()
 ↓
Node ServerResponse
```

## 1.8 napi-rs owns the actual FFI machinery

Use napi-rs primitives directly for:

```text id="9ht2u0"
async exports
generated .d.ts
native classes
object DTOs
ThreadsafeFunction
Promise-returning callbacks
ReadableStream
platform package generation
```

Do not build another private protocol over JSON, MessagePack, sockets, mpsc queues, or custom request IDs unless an actual limitation requires it.

## 1.9 Rust N-API definitions are authoritative for native binding types

Use napi-rs-generated declarations for native DTOs.

Do not introduce `ts-rs` initially.

Existing application-authoring contracts in `@plec/core` remain authoritative where they already exist.

## 1.10 Explicit lifecycle beats GC

Adopt the pattern proven by Rspack/Tauri:

```text id="ziddgk"
PlecApplication owns all long-lived JS callback references
```

and:

```text id="2rcg8j"
PlecApplication.close()
```

explicitly releases them.

Do not rely on JavaScript garbage collection to break Rust↔JS ownership cycles.

---

# 2. Proposed repository structure

The proposed implementation has two new Rust crates and one Node package. The
crate boundary is a dependency requirement; the exact name and number of
internal semantic crates are not architectural requirements. Prefer
`plec-server-engine` if it cleanly owns the shared execution implementation;
otherwise extract the minimum necessary existing modules while keeping the
same dependency direction: neither the Node bindings nor the semantic engine
may depend on Axum host machinery, and both hosts must call the same semantic
implementation.

## Rust semantic engine

```text id="dafweq"
crates/plec-server-engine/
```

Purpose:

> Host-independent server-side Plec execution.

It may depend on:

```text id="unxyx9"
plec-ir
plec-schema
plec-action
plec-eval
reqwest
tokio
http
bytes
futures-util
serde
serde_json
```

It must not depend on:

```text id="jhzbdt"
axum
tower
tower-http
napi
napi-derive
Node process management
```

The important definition of host-independent is therefore:

> Independent of Axum/N-API/public socket ownership.

It does **not** mean runtime-neutral or HTTP-concept-free.

Using `http::Method`, `http::HeaderMap` and `http::Uri` is acceptable.

## Native binding crate

```text id="m43oab"
crates/plec-node-bindings/
```

Purpose:

> napi-rs adapter over `plec-server-engine`.

It owns:

```text id="8cy8sq"
PlecApplication #[napi] class
N-API DTOs
ReadableStream conversion
PlecCallbackManager
JS callback wrappers
application close/admission state
Node-specific error conversion
```

## Node host package

```text id="pq6f3s"
packages/plec-node/
```

Published as:

```text id="gsfkkn"
@plec/node
```

It owns:

```text id="rn58f5"
public createPlecHandler API
optional serve convenience API
node:http adaptation
Web Request conversion
Web Response / ServerResponse conversion
static asset serving
app.mjs loading
host-provider loading
API dispatch
process signal handling for serve()
native addon loading
```

## Existing packages during migration

Keep:

```text id="vgocxe"
crates/plec-server
packages/plec-node-runtime
```

until parity is achieved.

After cutover:

```text id="96fmff"
packages/plec-node-runtime
```

should disappear.

The sidecar-specific modules inside `plec-server` become candidates for deletion.

---

# 3. Existing APIs that must not be redesigned

Preserve the existing application-facing surface in:

```text id="xu9es6"
packages/plec/src/server.ts
packages/plec/src/server-context.ts
```

Specifically:

```ts id="akjabt"
RequestContext;
ApiRouteHandler;
ApiMiddleware;
AppRequestHandler;
action();
requestContext();
withRequestContext();
```

The generated application bundle contract should also remain conceptually unchanged:

```ts id="ch3stj"
handleRequest(request, context);

invokeAction(id, args, context);

hasAction(id);
```

The architecture changes around these APIs; they should not change simply because the host changed.

---

# 4. New public Node API

Keep the public surface intentionally small.

Target:

```ts id="gnzcsa"
export interface PlecNodeOptions {
  dir: string;
  development?: boolean;
  trustProxy?: boolean;
}

export interface PlecHandler {
  fetch(request: Request): Promise<Response>;
  close(): Promise<void>;
}

export function createPlecHandler(
  options: PlecNodeOptions,
): Promise<PlecHandler>;
```

Add a convenience:

```ts id="19bglx"
export interface ServeOptions extends PlecNodeOptions {
  host?: string;
  port?: number;
}

export function serve(options: ServeOptions): Promise<void>;
```

`serve()` binds to `127.0.0.1` by default. Resolve the port as
`options.port`, then `PORT`, then `3000`; reject on bind failure without
retrying another port. Reject invalid/out-of-range port values; port `0` is
unsupported in 0.1. Resolve `serve()` only after a successful bind, and log the
bound URL. This default avoids accidentally exposing a development host on
every network interface.

`createPlecHandler()` is the architectural API.

`serve()` is an adapter/convenience.

`trustProxy` defaults to `false`. When false, ignore `x-forwarded-proto` and
derive the scheme from the actual transport. When true, use only the first
`x-forwarded-proto` token after trimming whitespace, and accept only `http` or
`https`; missing or malformed values fall back to the transport scheme. This
option is an explicit trust decision for deployments behind a proxy that
sanitizes forwarded headers.

This permits both:

```ts id="h5gaw3"
await serve({
  dir: './dist',
  port: 3000,
});
```

and:

```ts id="sjaee7"
const plec = await createPlecHandler({
  dir: './dist',
});

const server = createServer(async (req, res) => {
  // custom host behavior
  await dispatchWebHandler(plec, req, res);
});
```

Do not expose:

```text id="l8m6d3"
TSFN
Axum
Tower
native pointers
runtime sockets
runtime tokens
sidecar concepts
```

through this API.

---

# 5. Internal generated-application interface

Inside `@plec/node`, represent the generated `app.mjs` with an internal interface only:

```ts id="1kls0x"
interface GeneratedApplication {
  handleRequest: AppRequestHandler;

  invokeAction(
    id: string,
    args: unknown[],
    context: RequestContext,
  ): Promise<unknown>;

  hasAction(id: string): boolean;
}
```

Do not export this publicly in 0.1.

Load it once during:

```text id="ptyx8k"
createPlecHandler()
```

rather than dynamically importing it per request.

---

# 6. Establish the host-independent server execution boundary

Do this before moving production traffic.

The goal is not "move files into another crate."

The goal is:

> Convert the current semantic implementation from returning Axum responses into returning semantic outcomes.

## 6.1 Move `RequestContext`

Move the Rust `RequestContext` and `QueryValue` concepts from:

```text id="75ity1"
crates/plec-server/src/request.rs
```

into the engine.

Change imports from:

```rust id="0yr0wz"
axum::http::{HeaderMap, Method, Uri}
```

to:

```rust id="mayr5z"
http::{HeaderMap, Method, Uri}
```

No custom Plec replacements for those HTTP types are required.

Move:

```text id="k5p8wz"
URL construction
x-forwarded-proto handling
cookie parsing
query parsing
strict percent decoding
```

with their existing tests.

Do not move the Axum `Body`-specific `read_bounded_body()` unchanged.

Introduce a host-neutral bounded stream helper separately.

## 6.2 Move artifact loading/decoding

Move reusable artifact execution concerns currently under:

```text id="832xg4"
plec-server::artifact
```

to the engine.

Artifact byte-size validation remains Rust-owned.

At `PlecApplication.load()`, canonicalize the artifact path, read it once under
`MAX_ARTIFACT_JSON_BYTES`, parse and validate it, retain the parsed artifact /
graphs in application state, and close the file. Do not reopen artifacts per
request. Load manifest metadata once as well. `app.mjs` and provider modules are
imported once and cached. A development rebuild/reload must construct and swap
to a new application instance (or use an explicit snapshot-reload contract); it
must not silently reread files during an in-flight request. Static payloads
remain open-per-request.

Treat loaded state as an immutable `ApplicationSnapshot` owned by
`PlecApplication`, containing the parsed artifact, manifest, resolved roots,
server metadata, and provider metadata. `plec-server.json`, referenced
artifacts, declared `server/app.mjs`, declared provider manifests/modules, and
`publicRoot` must exist and pass containment checks at startup; missing required
inputs fail `createPlecHandler()`. Static asset files remain request-scoped and
may be absent (404). Reload creates and swaps a new application snapshot; it
does not mutate the active snapshot.

Separate:

```text id="whd2yd"
artifact path resolution
```

from:

```text id="5k0vct"
artifact parsing/validation
```

if path resolution is currently entangled with server-manifest handling.

## 6.3 Move route matching

Move:

```text id="17hmag"
RouteMatch
match_route()
```

and any route-chain resolution required for documents into the engine.

Route matching remains pure Rust and shared by both hosts.

## 6.4 Move route execution

Move:

```text id="xkkc7s"
RouteExecution
LoaderExecution
execute_route_loader()
```

into the engine.

Keep `reqwest` as the native loader fetch implementation.

Do not introduce a host-supplied JavaScript `fetch` abstraction merely for architectural purity.

## 6.5 Move SSR

Move the reusable SSR/snapshot implementation currently under:

```text id="j82ltx"
plec-server::ssr
```

to the engine.

Host-provider rendering must be expressed through an engine capability trait rather than through Node/private HTTP.

## 6.6 Introduce document semantic outcome

The engine must not return:

```rust id="j47a5r"
axum::Response<Body>
```

for document execution.

Introduce something approximately like:

```rust id="o94hkk"
pub enum DocumentOutcome {
    Rendered {
        status: u16,
        headers: HeaderMap,
        html: String,
    },

    Redirect {
        status: u16,
        location: String,
        headers: HeaderMap,
    },

    NotFound {
        headers: HeaderMap,
        html: String,
    },
}
```

Do not force body streaming into the engine's current SSR renderer yet.

Current SSR generates a `String`; preserve that behavior.

The N-API response transport may stream that string as one chunk.

True incremental SSR generation is explicitly out of scope for this migration.

## 6.7 Preserve SSR fallback semantics

Current `render_document()` has special development/fallback-shell behavior when SSR execution fails.

Decide explicitly where this policy belongs:

Recommended:

```text id="zjud32"
plec-server-engine
    → semantic SSR error

host adapter
    → fallback index.html / 500 policy
```

Reason:

Reading `public/index.html` is host/filesystem behavior rather than compiled Plec execution.

Both the Axum and Node hosts must receive parity tests for this policy.

Preserve the current response behavior exactly: if `public/index.html` is
available, return its bytes with status 200, `content-type: text/html;
charset=utf-8`, and `cache-control: no-cache`; in development also return
`x-plec-ssr-fallback` with the diagnostic. If the shell cannot be read, return
the generic 500 JSON response. Keep detailed errors out of production
responses.

---

# 7. Refactor the application runtime capability

The existing trait currently contains:

```rust id="jalslb"
dispatch()
render_host()
invoke_action()
```

The long-term engine-facing capability should contain only JS functionality genuinely needed by Rust semantics:

```rust id="fnb6l7"
pub trait ApplicationRuntime: Send + Sync + 'static {
    fn render_host(...);

    fn invoke_action(...);
}
```

`dispatch()` belongs to the old Axum→Node sidecar path.

Migration strategy:

1. Keep the existing trait intact while the Axum host still needs `dispatch()`.
2. Introduce an engine-level narrower trait, e.g.:

```rust id="1yv8dn"
pub trait ApplicationCapabilities {
    fn render_host(...);
    fn invoke_action(...);
}
```

3. Implement this for the existing sidecar runtime.
4. Implement it for the new N-API callback runtime.
5. Once the sidecar path is retired, remove the broader legacy trait or collapse it into the narrower engine trait.

Do not prematurely break the existing Axum host.

---

# 8. Introduce semantic server-action execution

Extract the semantic portion of:

```text id="98pkx9"
http.rs::handle_server_action
```

The engine should own:

```text id="ssauah"
POST requirement
same-origin validation
action ID grammar
action argument decoding
argument count limits
RuntimeValue limits
unknown/stale action semantics
action invocation
returned RuntimeValue validation
public failure classification
```

The host should own:

```text id="j7c2k1"
mapping request fields into semantic input
bounded body transport
mapping semantic outcome into HTTP status/headers/body
```

Introduce a semantic result, e.g.:

```rust id="0ovfbm"
pub enum ActionOutcome {
    Success(RuntimeValue),
    MethodNotAllowed,
    SameOriginRequired,
    UnknownAction,
    InvalidArguments,
    RequestTooLarge,
    ResultTooLarge,
    Failed,
}
```

Naming can follow current Plec conventions, but the key constraint is:

> No Axum `Response<Body>` inside action semantics.

The Node adapter supplies method, URL, ordered headers, and the inbound body
stream. Rust incrementally enforces the body ceiling, buffers only after the
limit has been enforced, decodes the JSON arguments, and performs action
validation. The host maps the resulting semantic outcome to the HTTP response.

### Public action HTTP response contract

Do not expose the private sidecar's `{ "ok": true, "value": ... }` envelope.
Preserve the current public action endpoint behavior:

| Outcome                       | Status and JSON body                                        |
| ----------------------------- | ----------------------------------------------------------- |
| Success                       | `200`; serialize the returned `RuntimeValue` directly       |
| Non-POST                      | `405`; `{ "error": "method not allowed" }`                  |
| Origin mismatch               | `403`; `{ "error": "same-origin action POST required" }`    |
| Invalid action ID             | `404`; `{ "error": "unknown server action" }`               |
| Body exceeds limit            | `413`; `{ "error": "server action request exceeds limit" }` |
| Invalid JSON arguments        | `400`; `{ "error": "invalid server action arguments" }`     |
| Argument count/value limit    | `400`; preserve the current corresponding message           |
| Unknown/stale registry action | `404`; `{ "error": "unknown or stale server action" }`      |
| Invalid/oversized result      | `500`; preserve the current result-limit message            |
| Action throws/rejects         | `500`; `{ "error": "server action failed" }`                |

Preserve current JSON content type and `cache-control: no-store` headers.
The bounded-body helper reports a semantic body-limit failure to the adapter;
it does not make the adapter the authority for the byte ceiling.

---

# 9. Introduce host-neutral bounded request streams

Current Axum logic calls:

```rust id="qpp6x0"
axum::body::to_bytes(body, MAX_REQUEST_BODY_BYTES)
```

The engine needs a reusable incremental equivalent.

Target helper:

```rust id="y037oo"
pub async fn read_bounded_stream<S, E>(
    stream: S,
    declared_content_length: Option<u64>,
    limit: usize,
) -> Result<Vec<u8>, BodyLimitError>
where
    S: Stream<Item = Result<Bytes, E>>;
```

Behavior:

```text id="s4q7dg"
if Content-Length > limit
    fail immediately

otherwise pull chunks incrementally

before extending:
    if accumulated + chunk > limit
        fail immediately

stop polling after failure
```

Use this for action bodies. On success it returns a bounded `Vec<u8>` for the
existing JSON `RuntimeValue` decoder. The limit is checked incrementally before
each append, and polling stops immediately on overflow.

Do not buffer an untrusted N-API request before invoking this logic.

---

# 10. Create `plec-node-bindings`

Follow the existing `plec-query-node` pattern.

## 10.1 Cargo structure

```text id="aphijg"
crates/plec-node-bindings/
├── Cargo.toml
├── build.rs
└── src/
    ├── lib.rs
    ├── application.rs
    ├── callbacks.rs
    ├── request.rs
    ├── response.rs
    └── error.rs
```

Dependencies:

```text id="28yvpf"
plec-server-engine
napi
napi-derive
tokio
tokio-util
http
bytes
futures-util
serde
serde_json
```

Enable napi-rs features needed for:

```text id="6sr651"
Tokio async runtime
Web Streams
serde-json
```

Do not enable broad features without need.

### Binding feasibility gate

Before committing to the proposed crate/API shape, run a minimal end-to-end
spike with the intended napi-rs v3 release, selected Tokio/Web Streams/serde
features, and minimum supported Node version. The spike must prove native class
loading, Promise-aware callback resolve/reject and re-entry, inbound Web Stream
consumption/cancellation, outbound stream backpressure/error propagation, and
native close while a callback is pending. Record the exact napi-rs version,
enabled features, Node support range, and verified stream/cancellation behavior
as implementation inputs. This is an architecture gate, not a standalone
production implementation; do not freeze DTO signatures or lifecycle design
until it passes.

### Feasibility gate result

The initial binding gate passes on the repository's Node floor, `22.20.0`, with
`napi 3.14.0`, `napi-derive 3.6.5`, and `napi-sys 3.4.0`. The binding crate
enables `napi8`, `tokio_rt`, `web_stream`, and `serde-json`, with napi default
features disabled. The supported Node floor remains 22.20.0; this gate was run
on that minimum version on Linux x64 GNU.

The native smoke suite verifies native class load, Promise callback resolve and
reject, callback re-entry into an async native method, close during a pending
callback, post-close rejection, multi-chunk inbound stream reads, demand-driven
outbound stream polling, and outbound stream error propagation. A napi-rs
`ReadableStream::read()` creates its JS reader synchronously; the resulting
reader can then be moved into a worker task. Dropping that reader does not call
the source's `cancel()` hook, so the Node adapter must retain a per-operation
cancellation bridge and call the underlying source reader's `cancel()` when
native consumption stops early. The overflow fixture verifies cancellation
through that explicit bridge and confirms no later source chunks are pulled.

Run the gate with `yarn workspace @plec/node test:native` under Node 22.20.0.
These are implementation inputs for the actual binding, not permission to omit
the cancellation bridge or the separately specified lifecycle/admission rules.
The exercised probe exports are behind the binding crate's
`feasibility-gate` feature and are not the production N-API surface.

## 10.2 Dedicated release profile

Do not build this addon using the current WASM-oriented:

```toml id="10h20o"
panic = "abort"
opt-level = "z"
```

Create a Node-native profile, approximately:

```toml id="alw4hn"
[profile.node-release]
inherits = "release"
opt-level = 3
panic = "unwind"
```

Ensure native package scripts explicitly select it.

Expected failures must still use `Result`; unwinding is a safety boundary, not normal error control flow.

---

# 11. `PlecApplication` native class

The binding should expose one stateful native object.

Conceptually:

```rust id="d8bu8j"
#[napi]
pub struct PlecApplication {
    inner: Arc<ApplicationInner>,
}
```

`ApplicationInner` should own:

```text id="300vuq"
plec-server-engine application state
callback manager
root cancellation token
task tracker / in-flight accounting
closed/admission state
```

Public native API should stay small:

```text id="rf4lfk"
PlecApplication.load(...)
PlecApplication.handleDocument(...)
PlecApplication.handleAction(...)
PlecApplication.close()
```

Do not export internal route/loader/SSR primitives through N-API individually.

Node should invoke the semantic operation, not orchestrate the Rust execution graph.

---

# 12. N-API request DTO

Do not attempt to expose JavaScript's `Request` class directly to Rust.

Use generated metadata DTOs, but pass Web Streams as dedicated N-API arguments
rather than assuming they can be fields of a `#[napi(object)]`:

```text id="yembjm"
NativeRequest
├── method
├── url
├── raw ordered headers
└── other request metadata

handleAction(metadata, bodyStream, cancellation)
```

Headers should use ordered pairs rather than:

```text id="uq1qu2"
Record<string, string>
```

to avoid losing duplicate HTTP semantics.

Conceptually:

```rust id="3coxhw"
#[napi(object)]
pub struct HeaderEntry {
    pub name: String,
    pub value: String,
}

#[napi(object)]
pub struct NativeRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<HeaderEntry>,
}
```

Use the napi-rs Web Stream type as a separate `handleAction` argument (optional
when the host request has no body). The document operation has no meaningful
inbound body today. Do not assume a stream can be embedded in an N-API object on
either input or output: prove the exact return shape in the binding spike,
using a napi-rs stream return or a small JS-thread wrapper if necessary.

---

# 13. N-API response DTO

Likewise return:

```text id="ku446n"
NativeResponse metadata
├── status
└── ordered raw headers

response body stream (separate stream value/return)
```

Do not force all native responses through a JS `Response` before writing to Node `ServerResponse`.

The host should support both:

```text id="8gyuea"
PlecHandler.fetch() → Web Response
```

and:

```text id="zn7eao"
node:http adapter → raw ServerResponse
```

The low-level Node adapter must preserve ordered header pairs wherever the
current host does. A Web `Request` has already normalized some repeated header
representations; `PlecHandler.fetch(Request)` guarantees Web-standard header
semantics, while the first-party `node:http` adapter should retain raw Node
headers through its internal path where required. Do not claim that
`fetch(Request)` can reconstruct raw headers it never received.

`Set-Cookie` requires explicit regression tests.

---

# 14. Adopt the Rolldown callback pattern

Do not use raw `ThreadsafeFunction` types throughout the binding.

Create one small abstraction inspired by Rolldown:

```text id="1aowgm"
JsCallback<Args, Ret>
MaybeAsyncJsCallback<Args, Ret>
```

Provide an operation approximately equivalent to:

```rust id="1l2atb"
await_call(args).await
```

Requirements:

- JavaScript may return a direct value where allowed;
- JavaScript may return `Promise<T>`;
- Promise rejection becomes a Rust `Result::Err`;
- thrown JS exceptions become controlled Rust errors;
- invalid return types become controlled Rust errors;
- do not permit N-API fatal exceptions for ordinary bad application callbacks.

Prefer napi-rs `call_async_catch()` semantics, following Rolldown.

---

# 15. Introduce `PlecCallbackManager`

Adopt Rspack's scoped-TSFN ownership model.

`PlecApplication` owns:

```text id="ab4lmw"
PlecCallbackManager
├── invokeAction
└── renderHost
```

Responsibilities:

```text id="4msv7q"
register callbacks during load
clone safe handles into operations
refuse calls once closing/closed
release every long-lived JS reference during close
prevent native→JS calls after release
```

The manager exists specifically to prevent:

```text id="5b3lgf"
Rust application
   ↓ retains
TSFN
   ↓ retains
JS callback
   ↓ closure retains
Node application
   ↓ retains
native Plec application
```

from forming an uncollectable cross-runtime cycle.

Do not rely on finalizers to break this cycle.

---

# 16. Action callback

During `PlecApplication.load()`, receive a callback matching the generated application action registry.

Internally:

```text id="l0upv0"
invokeAction(
  id,
  arguments,
  context
)
```

Rust flow:

```text id="15vp8h"
validate HTTP/action semantics
 ↓
construct ServerActionRequest
 ↓
callbackManager.invokeAction.await_call(...)
 ↓
Node app.mjs
 ↓
hasAction / invokeAction
 ↓
withRequestContext(...)
 ↓
user action()
 ↓
Promise result
 ↓
Rust
 ↓
RuntimeValue validation
```

Initially preserve the current distinction between:

```text id="gsqbl0"
unknown action
action execution failure
```

If `hasAction()` is required to preserve that cleanly, keep it.

A future optimisation may combine lookup and invocation into a tagged callback result, but do not make that a prerequisite.

---

# 17. Host-render callback

During load, pass:

```text id="3g1qe6"
renderHost(provider, component, props)
```

Rust calls it only when SSR reaches an explicit host-provider boundary.

Move existing provider loading logic from:

```text id="uoqr3i"
packages/plec-node-runtime/src/runtime.ts
```

into:

```text id="sgxgo1"
packages/plec-node/src/providers.ts
```

Preserve:

```text id="m43oap"
manifest validation
SSR allowlist
provider module containment
default-factory requirements
missing renderer → inert boundary
MAX_HOST_RENDER_BYTES
failure → inert CSR boundary behavior
```

Remove the private HTTP endpoint entirely from the new path.

---

# 18. Callback cancellation semantics

Adopt Rolldown's model.

When Rust is awaiting arbitrary JavaScript:

```text id="k3v74w"
await JS callback
       versus
request/application cancellation
```

If cancellation wins:

```text id="8fhsmk"
Rust stops awaiting the JS result
```

but:

```text id="fark4o"
JavaScript is not forcibly terminated
```

The JS Promise may continue.

Its eventual settlement must not:

```text id="jzq24b"
resume freed native request state
invoke released callbacks
write to a closed response
keep PlecApplication alive indefinitely
```

Document this explicitly as the 0.1 cancellation contract.

Do not pretend arbitrary JS can be safely pre-empted.

### Cancellation ownership and callback capacity

Distinguish three concepts:

```text
transport cancellation: client disconnect or Request.signal abort
native cancellation: stop loader work, native waits, response production, and
                     Rust's wait for a callback
JavaScript cancellation: best-effort only; arbitrary application Promises are
                         not forcibly terminated
```

A cancelled API handler, action callback, or provider callback may continue in
JavaScript. Plec ignores its eventual result, does not write it to a closed
response, and does not retain request-scoped native state for it. Callback
arguments crossing into JS are detached values, never references to native
request state. After cancellation, no Rust request-scoped state may be owned
exclusively by an unresolved JS Promise.

The API handler's request permit and any API body-buffer reservation remain
held until its Promise settles, even after the client disconnects; cancellation
detaches transport/response ownership, not the still-running JS work or memory
it retains. Its eventual result is ignored. This keeps active-request and
aggregate-memory bounds meaningful for cancelled handlers.

`CallbackPermit` capacity bounds actual outstanding JS callback Promises, not
only Rust waiters. Acquire before invoking JS and release only when the Promise
settles, including after Rust has detached due to cancellation. Cancellation
therefore stops native waiting but does not free capacity for JS work that is
still running. If all permits remain occupied by unresolved callbacks, reject
new callback-bearing work with 503 before invoking the callback; never admit
unbounded detached JS work. `close()` does not wait indefinitely for detached
Promises. It releases registration handles and ensures invocation-specific JS
work no longer retains `PlecApplication` or native request state.

Track JS work independently for capacity and shutdown. API handlers, action
callbacks, and provider callbacks retain the relevant request/body/callback
capacity permits until their Promises settle, even if native or transport
ownership has detached. `close()` cancels native work and waits for the defined
native drain/grace boundary, but never waits indefinitely for arbitrary JS
Promises; after detachment, those Promises must not retain request-scoped native
state or keep `PlecApplication` alive. A callback admitted before the
Open-to-Closing transition may finish or detach safely; one not yet admitted is
rejected with `PLEC_APPLICATION_CLOSED`. If TSFN scheduling fails before JS
starts, release its `CallbackPermit` immediately. Invocation-specific callback
handles remain valid until the admitted invocation finishes or detaches.

The binding feasibility gate must prove the callback thread and re-entry
contract: Rust schedules the callback onto Node's JS thread; the callback may
call another exported async Plec native method without blocking that thread;
the original Rust operation resumes when the Promise settles. Also test close
racing with callback admission/scheduling and verify there is no state where a
callback is scheduled but its lifetime owner has already been released.

## Cancellation bridge

The Node adapter creates an `AbortController` for each incoming request and
aborts it when the client disconnects. The binding must connect that
cancellation to the matching Rust `CancellationToken` without a global request
ID protocol.

First verify whether the selected napi-rs v3 API can directly accept and
observe an `AbortSignal` for an asynchronous native operation. If it can,
prefer that. If it cannot, expose one per-operation cancellation handle (for
example, an operation object with `result` and `cancel()`); do not add a
process-global request registry. In either case, test whether dropping the
napi-rs Web Stream reader cancels the JavaScript source or only releases its
lock. Explicitly call the source's cancel operation if reader drop is
insufficient.

---

# 19. Native request lifecycle

Each native request gets its own request state:

```text id="ptyr9o"
RequestExecution
├── child CancellationToken
└── tracked task lifetime
```

Root:

```text id="3epvts"
PlecApplication root CancellationToken
```

Request:

```text id="tzbw17"
root.child_token()
```

Cancellation sources:

```text id="7ol4oh"
client disconnect
request AbortSignal
PlecApplication.close()
```

Loader fetches and Rust waits should observe this token wherever practical.

Do not add cancellation points to pure short-running CPU evaluation merely for completeness unless required.

---

# 20. `PlecApplication.close()`

Implement lifecycle in this exact order.

## Step 1 — mark closing

Atomically transition:

```text id="u551pl"
Open → Closing
```

New calls return an explicit:

```text id="axfbi1"
PLEC_APPLICATION_CLOSED
```

style error.

## Step 2 — cancel native requests

Cancel the root `CancellationToken`.

This propagates to request child tokens.

## Step 3 — stop awaiting cancel-aware JS work

Any native tasks waiting on JS callbacks should race:

```text id="7dhonf"
callback completion
versus
cancellation
```

and detach from the callback result when cancellation wins.

## Step 4 — drain tracked Rust operations

Await tracked native work to reach the defined shutdown point.

Do not wait forever for arbitrary detached JavaScript.

## Step 5 — release TSFNs

Release all `PlecCallbackManager` callback references.

## Step 6 — drop engine resources

Release:

```text id="1qlc9h"
artifact state
reqwest client state
callback manager
native application data
```

## Step 7 — transition Closed

Repeated:

```text id="mfi71v"
close()
```

should be idempotent.

---

# 21. Node HTTP adapter

Implement in:

```text id="9c8048"
packages/plec-node/src/http.ts
```

The `node:http` adapter starts from Node's request and preserves raw header
pairs until route dispatch. For routing, it uses the validated encoded path
from the raw request target, not WHATWG `URL.pathname`: WHATWG parsing can
normalize dot segments and backslashes before Plec sees them. The path rule is
shared with Rust as a string-level semantic contract:

```text
CanonicalPlecPath:
  validated UTF-8 request path, excluding query
  percent escapes preserved; no percent-decoding
  no dot-segment or slash normalization
```

Split origin-form request targets into raw path and query without interpreting
fragment text; fragments are not valid in an HTTP request target. Validate the
request-target form and path syntax before classification. Use structured URL
parsing only for authority/origin and query semantics, never to derive the
route path. For `/_plec/actions/`, require a literal path prefix; the remaining
action identifier must be nonempty, contain no slash, and match only
`[A-Za-z0-9_-]`. Do not percent-decode an encoded identifier into a valid ID.
The parser/adapter contract must explicitly cover origin-form, absolute-form,
encoded dot segments, encoded slash/backslash, literal backslash, repeated
slash, malformed percent escapes, and malformed targets. The adapter does not
construct one universal Web `Request` before deciding whether the body belongs
to Node or Rust:

```text id="5gadhz"
IncomingMessage
 ↓
validated raw request target
 ↓
ordered raw header pairs
 ↓
shared route classifier
```

Ingress is path-specific:

```text
API request:
  GET/HEAD: ignore body; never consume it for Plec
  other methods: bounded pre-read (at most MAX_REQUEST_BODY_BYTES)
  → Web Request with the validated bounded body
  → invoke app.mjs only after successful read

Server action:
  ordered metadata + Readable.toWeb(incoming)
  → N-API stream argument
  → Rust incrementally checks the limit and then buffers only the bounded JSON

Document/static request:
  do not read an unused request body before routing
```

For API requests other than GET/HEAD, construct the Web `Request` only after
the bounded pre-read. GET/HEAD bodies are ignored and never exposed to
application code. If unread bytes prevent safe keep-alive reuse, close the
connection rather than draining an unbounded remainder.
For action requests, pass ordered metadata and the Web stream directly to
N-API. `PlecHandler.fetch(Request)` accepts an already-constructed Web Request;
it uses the same body contract for bytes it consumes: ignore GET/HEAD bodies;
for other API methods consume and bound the body before dispatch, returning 413
without invoking middleware or handlers on overflow. This limit does not bound
memory allocated before `fetch()` is called: if the caller buffered a large
body to construct the Request, Plec cannot retroactively constrain that
allocation. It cannot recover raw header details normalized before the call.

Do not call:

```text id="h016an"
Buffer.concat(...)
```

for general request bodies. Bounded API buffering is the explicit 0.1
exception; action streams and any future uncapped body path remain streaming.

Client abort:

```text id="pmv0os"
IncomingMessage aborted/close
 ↓
AbortController.abort()
```

That signal must ultimately cancel the corresponding native request.

## Ingress framing, deadlines, and admission

### Host ingress and ownership matrix

This matrix is normative for the 0.1 host boundary:

| Request path                                 | Header source                                      | Body handling                                                                | Plec admission                                                   | Deadline                                        | Cancellation                                                       | Execution/response owner          |
| -------------------------------------------- | -------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------- | ----------------------------------------------- | ------------------------------------------------------------------ | --------------------------------- |
| `/api`, `/api/*` through `serve()`           | Node `rawHeaders` until Web `Request` construction | GET/HEAD ignored; other methods bounded pre-read                             | request + weighted API pre-read budget                           | Node header and body deadlines                  | disconnect aborts host wait; arbitrary handler JS is not preempted | Node application code             |
| `/_plec/actions/*` through `serve()`         | ordered Node raw header pairs passed to Rust       | streamed to Rust; Rust enforces and buffers only within the action limit     | request + native-operation + callback permits                    | Node header/body deadlines                      | disconnect → Rust token; JS action is not forcibly stopped         | Rust semantics with Node callback |
| Document through `serve()`                   | Node `rawHeaders`                                  | no request body consumed                                                     | request + native-operation permits                               | Node header deadline                            | disconnect → Rust token and response-stream cancellation           | Rust                              |
| Static through `serve()`                     | Node `rawHeaders`                                  | no request body consumed                                                     | request permit                                                   | Node header deadline                            | disconnect destroys file/response stream                           | Node                              |
| API/action/document through `fetch(Request)` | Web-normalized `Headers`                           | Plec bounds bytes it consumes; caller owns prior allocations and raw framing | handler request, native, callback, and weighted pre-read permits | no transport/header deadline; caller owns these | `Request.signal` → corresponding Plec cancellation where available | route-specific as above           |

`PlecHandler.fetch(Request)` is a semantic host adapter, not an HTTP transport
implementation. It handles API, action, and document routes, but not static
files. It guarantees Plec route classification, limits on bytes it consumes,
handler/native/callback admission, aggregate API pre-read budgeting, action
validation, semantic execution, `close()` rejection, and Web-standard response
semantics. It cannot guarantee raw duplicate-header inspection, HTTP framing
validation, header-arrival or transport body deadlines, socket reuse/close
policy, or TCP disconnect detection beyond `Request.signal`; those belong to
the embedding host. The body limit does not cover memory allocated before the
call. Document this division in the public API documentation.

### Admission permit lifecycle

Use independently scoped permits with this normative lifecycle:

| Permit                    | Acquire                                                        | Release                                                                      | Survives cancellation?                                                              |
| ------------------------- | -------------------------------------------------------------- | ---------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| `RequestPermit`           | after route classification, before body allocation or dispatch | Plec request lifecycle ends                                                  | API: until handler Promise settles; native: until native work completes or detaches |
| `ApiPrereadPermit(bytes)` | before retaining body bytes                                    | bounded body is no longer reachable by handler, or request fails             | yes, while handler retains body                                                     |
| `NativeOperationPermit`   | before Rust execution                                          | native operation completes or detaches                                       | no after cancellation/detach                                                        |
| `CallbackPermit`          | before scheduling JS callback                                  | JS Promise settles; release immediately if scheduling fails before JS starts | yes, until actual Promise settlement                                                |
| `StaticRequestPermit`     | before opening a file                                          | response stream closes, errors, or is canceled                               | no after stream lifecycle ends                                                      |

Acquire `RequestPermit` immediately after route classification and before body
allocation, application middleware, native calls, or static-file open. Hold it
until the Plec-owned request lifecycle ends, including completion, error,
cancellation, or response-stream close/error/cancel. `serve()` owns socket-level
shutdown and may additionally track transport cleanup.

For API pre-reads, reserve
`min(validated Content-Length, MAX_REQUEST_BODY_BYTES)` before reading when a
valid length is available. For absent/chunked or untrusted lengths, acquire
additional weighted bytes before retaining each chunk; append only after the
reservation succeeds. Reject over-limit bodies before middleware/handler
invocation. Keep the reservation while the bounded body remains reachable by the
application handler, then release it when the handler completes or the request
fails. On disconnect, detach the response but retain the reservation until the
handler Promise settles. Release all reservations and permits in
`finally`/scope-guard paths for timeout, overflow, parse failure, close, handler
settlement, and stream error.
The aggregate byte budget must account for retained body buffers, not merely
bytes currently being read. Capacity tracking and shutdown tracking are
separate: a detached JS Promise continues to hold its capacity permit until
settlement, but does not have to block `close()` indefinitely.

Admission saturation returns 503 before middleware, Rust execution, callback
invocation, body allocation, or static-file open. If unread request bytes remain,
close the connection rather than draining an unbounded remainder. Limits are
per host process.

Canonical header handling for the raw Node adapter:

| Header              | Rule                                                                                                             |
| ------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `Host`              | Require exactly one accepted authority; reject duplicates or malformed values.                                   |
| `Origin`            | Action requests require exactly one valid value; reject missing, duplicate, or malformed values.                 |
| `Cookie`            | Preserve Node's accepted combined semantics; test repeated Cookie lines against the context parser.              |
| `Set-Cookie`        | Emit each value separately; never comma-join.                                                                    |
| `x-forwarded-proto` | Ignore unless `trustProxy` is true; then use the first trimmed `http`/`https` token, otherwise transport scheme. |
| `Content-Length`    | Trust Node parser acceptance; never reconstruct message framing.                                                 |
| `Transfer-Encoding` | Node parser is the sole authority.                                                                               |

The Node HTTP parser is the sole authority for HTTP/1 framing. Requests rejected
by Node's parser must not reach the route classifier. Do not reconstruct framing
from application-visible headers or add Plec-specific HTTP framing validation.
Keep focused raw-socket regression coverage for representative ambiguous
framing cases on supported Node versions; parser behavior itself is a Node
platform contract.

For accepted requests, preserve raw ordered header pairs where the
`node:http` path requires them. Define explicit handling for repeated
security-sensitive headers (`Host`, `Origin`, `Content-Length`,
`Transfer-Encoding`, `Cookie`, and `x-forwarded-proto`); never select a value by
an accidental object conversion or comma-join. Web `Request` construction may
normalize headers, and that normalization is part of the `fetch(Request)`
contract rather than a source from which raw headers can be recovered.

The Node path passes ordered `(name, value)` pairs derived from `rawHeaders` to
the semantic boundary and validates them before constructing Rust
`HeaderMap`/Web `Headers`. Never feed a comma-joined or object-converted value to
action-origin validation. `Host` must have exactly one accepted authority;
action `Origin` exactly one valid value; `Cookie` follows Node's accepted
combined semantics before RequestContext parsing; `Set-Cookie` response values
remain separate; forwarded-protocol evaluation uses raw values only when
`trustProxy` is enabled. Node's HTTP parser alone decides message framing;
application-visible `Content-Length` and `Transfer-Encoding` are not used to
reconstruct it.

Per-request byte ceilings do not bound aggregate resource use. Both
`createPlecHandler()` and `serve()` must enforce bounded admission for active
requests and for body pre-reads/native operations, with configurable or
documented defaults and an explicit overload response/connection policy. A
request rejected by admission must not start app middleware, actions, loaders,
or native callbacks. Bound the aggregate memory implied by concurrent 1 MiB API
pre-reads; do not rely on the per-body ceiling alone.

The initial 0.1 defaults are:

```text
MAX_ACTIVE_REQUESTS = 256
MAX_OUTSTANDING_APPLICATION_CALLBACKS = 64
MAX_AGGREGATE_API_PREREAD_BYTES = 32 MiB
HEADER_TIMEOUT = 10 seconds
BODY_TIMEOUT = 30 seconds
REQUEST_OPERATION_TIMEOUT = none by default
```

The aggregate API pre-read limit is enforced by a weighted byte semaphore, not
only a request-count limit. Admission saturation returns `503` before
middleware, Rust, or body-buffer allocation. If unread request bytes remain,
close the connection rather than draining an unbounded body. These limits apply
per host process. Header/body deadlines address ingress abuse; they do not
impose a blanket timeout on valid loader or action execution. The initial
defaults may remain internal rather than expanding the public API.

Apply finite header-read and body-read deadlines (or an equivalent minimum-rate
policy) in `serve()` to API pre-reads and streamed action bodies. On timeout or
disconnect, stop retaining/reading bytes, cancel the matching Plec operation,
and close the connection when unread request bytes make reuse unsafe. There is
no request-operation timeout by default: valid loaders/actions may take longer
than ingress deadlines. Callback permits continue to bound unresolved JS work.
The embedding API owns transport-level header/body deadlines and socket policy;
`PlecHandler` still bounds admitted Plec work and rejects after `close()`.

---

# 22. Top-level Node dispatch

Implement one obvious dispatch function.

Conceptually:

```ts id="iujlf1"
async function dispatchNode(
  incoming: IncomingMessage,
  outgoing: ServerResponse,
): Promise<void> {
  const pathname = canonicalPlecPath(incoming.url);

  if (pathname === '/api' || pathname.startsWith('/api/')) {
    const request = await makeBoundedApiRequest(incoming);
    return sendWebResponse(outgoing, await dispatchApi(request));
  }

  if (pathname.startsWith('/_plec/actions/')) {
    const body = Readable.toWeb(incoming);
    const response = await native.handleAction(
      actionRequestMetadata(incoming),
      body,
      cancellationFor(incoming, outgoing),
    );
    return sendNativeResponse(outgoing, response);
  }

  if (isDocumentRequest(pathname)) {
    return sendNativeResponse(
      outgoing,
      await native.handleDocument(documentRequestMetadata(incoming)),
    );
  }

  return serveStatic(incoming, outgoing);
}
```

`PlecHandler.fetch(request)` is a second adapter over the same classifier. It
returns a Web `Response`; the `node:http` adapter writes directly to
`ServerResponse` so it can preserve raw headers and avoid unnecessary
normalization. The two interfaces share route, body-limit, and semantic
contracts, but they do not promise identical raw-header behavior: callers of
`fetch(Request)` receive Web-standard normalized `Headers` semantics, while the
first-party adapter preserves raw pairs where required. Test these common and
adapter-specific contracts separately; in particular do not claim duplicate
header fidelity for `fetch(Request)`.

`isDocumentRequest` must use a platform-independent URL-path rule shared by
Rust and Node, not OS path utilities. A path is a document request when it is
`/`, or when its final URL path segment has no extension under Plec's explicit
URL rule. Define that rule in shared fixtures/helper semantics during
extraction; do not inherit host-OS differences from Rust `Path::extension()`:

```rust
pathname == "/" || final_url_segment_has_no_extension(pathname)
```

Do not replace it with filesystem-existence probing. Add shared parity fixtures
for `/`, `/foo`, `/foo/`, `/foo.bar`, `/foo.`, `/.well-known`,
`/foo.bar/baz`, `/foo%2Ebar`, and `/foo%2Fbar`.

The complete ordering is:

```text
/api or /api/...             → generated app.mjs
/_plec/actions/...           → Rust action semantics
document-classified path     → Rust document semantics
otherwise                    → Node static assets
```

Do not use "try document, then asset" or "try filesystem first".

Route normalization is one shared semantic rule: `canonicalPlecPath` extracts
the validated encoded path from the raw request target; it does not use
WHATWG-normalized `URL.pathname`, decode percent escapes, or normalize dot
segments, separators, or repeated slashes. Rust route matching receives that
same path representation. Thus `/foo%2Fbar` cannot be classified as one path
and later matched as `/foo/bar`. Shared fixtures cover origin-form and
absolute-form targets, encoded dot/separator/backslash, literal backslash,
repeated slash, query handling, malformed percent escapes, and malformed
targets. Classification and matching must agree; any mismatch is a release
blocker. The Node HTTP parser remains the authority for HTTP message framing;
Plec does not reimplement framing validation.

Method behavior is explicit per route class. APIs retain the GET/HEAD ignored
body rule and bounded pre-read for all other methods. Actions accept POST only;
reject other methods with 405 before body processing, and close the connection
if unread bytes make reuse unsafe. Documents and static requests are GET/HEAD
only, with other methods returning 405 and the same unread-body connection
policy, subject to a parity check against current Axum/ServeDir behavior before
this contract is frozen. HEAD responses never emit body bytes. Malformed targets
are rejected before route dispatch; invalid/duplicate Host is a client error,
and action Origin failures retain the action endpoint's documented 403
semantics. Add exact status/body/connection expectations to host tests.

---

# 23. API request path

The Node API path should use the existing generated application code directly.

Flow:

```text id="rpdkc4"
IncomingMessage
 ↓
GET/HEAD body ignored; otherwise bounded pre-read
 ↓
Web Request
 ↓
build RequestContext
 ↓
app.handleRequest(request, context)
 ↓
Web Response | null
```

If null:

```text id="2c1lln"
404 endpoint not found
```

Errors:

```text id="7gc3zd"
log real exception internally
return generic 500
```

Preserve the current redaction behavior.

API/Web responses use the same response state machine as native responses:

```text
NotStarted → HeadersSent → Completed
     └──────────────→ Aborted
```

An API handler throw or invalid response before headers are sent becomes the
generic redacted 500. If a response body fails before headers are sent, return
that same 500; after headers are sent, destroy the response/stream and log the
internal error without attempting a second response. Apply this consistently
to `fetch(Request)` and the raw `ServerResponse` adapter.

## API body ceiling

The current Rust host bounds all non-GET/HEAD bodies before application dispatch.
For 0.1, preserve that security contract even though API handlers execute
directly in Node. An arbitrary handler may perform side effects before it reads
the request body, so a streaming transform that discovers overflow only while
the handler consumes the body is not equivalent.

Direct Node APIs use a bounded pre-read for every non-GET/HEAD request, matching
the current native host. GET and HEAD bodies are ignored, never exposed to
application code, and not consumed by Plec. If unread bytes make keep-alive
reuse unsafe, close the connection rather than draining an unbounded remainder.

```text
IncomingMessage
  ↓ incremental Node reader
reject immediately if declared Content-Length exceeds the limit
  ↓ otherwise read chunks, checking before every append
reject and stop reading as soon as accumulated bytes exceed the limit
  ↓ only after EOF and successful validation
construct Web Request with the bounded body
  ↓
app.handleRequest(request, context)
```

The only buffering here is the deliberately capped API body, currently
`MAX_REQUEST_BODY_BYTES` (1 MiB). The API handler must not run before the
bounded read succeeds. Do not use `Buffer.concat` on an unbounded body.

The body-limit value must have one source of truth or a generated build-time
value. Rust owns the shared limit definition; Node enforces the same value
before API dispatch. Actions instead pass a ReadableStream through N-API and
Rust enforces the limit incrementally before its bounded JSON buffer.

Test that oversized API requests do not invoke application middleware or
handlers, including chunked requests with missing or false `Content-Length`.
After detecting overflow, stop retaining/reading body bytes and prevent reuse
of a connection with an unread request body (for example, return 413 with
connection-close behavior). Do not drain an unbounded remainder into memory.
The Node HTTP tests must verify this for API pre-reads and streamed action
bodies.

Apply the ingress body deadline and admission controls from section 21 to this
pre-read. Timeout, overload, disconnect, and size-limit failure must all occur
before `app.handleRequest()` and must not invoke middleware or handlers.

---

# 24. RequestContext parity

There are currently separate Rust and TS context-building implementations.

Do not introduce a third divergent implementation.

For Node-owned APIs, `@plec/node` must reproduce the canonical contract:

```text id="n2s1yz"
url
pathname
method
headers
cookies
params
query
```

Add parity fixtures shared between:

```text id="c0f12u"
Rust RequestContext
Node RequestContext
```

covering:

```text id="yk21y0"
x-forwarded-proto
host with non-default port
repeated query fields
+ form decoding
invalid percent escapes
cookie decoding
duplicate headers
empty query values
```

For 0.1, `trustProxy` defaults to false and controls forwarded-protocol use.
When false, ignore `x-forwarded-proto` and derive scheme from the actual
transport. When true, use the first token only after trimming whitespace;
accept only `http` or `https`, otherwise fall back to transport scheme. The
authority comes from exactly one accepted `Host` value; reject missing,
duplicate, or malformed Host values. Action requests require exactly one
well-formed `Origin`; missing or repeated Origin is rejected, and the single
origin must match canonical scheme/host/port exactly. Add parity fixtures for
both settings, repeated/malformed values, and default/non-default ports. A
trusted proxy must overwrite/sanitize forwarded headers.

Origin comparison is structural, not textual. Parse both request origin and
supplied Origin as URLs; compare `(scheme, hostname, effectivePort)`, normalizing
`http:80` and `https:443` to their scheme defaults. Parse IPv6 authorities
structurally, never by splitting on `:`. Reject `Origin: null`, multiple or
malformed Origin values, userinfo, and schemes other than HTTP/HTTPS. Use the
same helper/rule in Axum and Node during migration, with fixtures for IPv6,
default ports, explicit ports, malformed authorities, and userinfo.

If necessary, move shared fixtures into repository testdata rather than sharing implementation.

Use one `canonicalRequestAuthority()` rule for actual transport scheme, raw
Host, raw `x-forwarded-proto`, and `trustProxy`; its result `(scheme, hostname,
effectivePort, origin)` feeds RequestContext URL construction, action Origin
validation, redirect bases, and loader URL bases. With `trustProxy=false`, use
the actual transport scheme and ignore forwarded protocol. With it enabled, use
the first valid forwarded HTTP(S) token, otherwise the actual scheme; the
trusted proxy must sanitize forwarded headers. For embedded `fetch(Request)`,
derive scheme and authority from `request.url`, since there is no socket.

Do not make Node call Rust merely to construct API `RequestContext`.

---

# 25. Static asset host

Implement after document/action parity, not before. Node owns static lookup and
streams file bytes directly; static payloads never cross N-API.

Plec's 0.1 static contract is:

- `/...` resolves beneath canonical `publicRoot`; `/_plec/...` resolves beneath
  the canonical client root. `/_plec` itself and absent files return JSON 404
  `{ "error": "asset not found" }`.
- Decode each URL path segment once for filesystem lookup. Reject malformed
  escapes, decoded separators, dot segments, and candidates whose canonical
  target escapes the selected root with JSON 400
  `{ "error": "invalid asset path" }`. Symlinks are allowed only when their
  canonical target remains within that root. Explicit dotfiles are servable;
  directories are not implicitly mapped to index files.
- Only GET and HEAD are supported; other methods return 405 with
  `Allow: GET, HEAD`. Successful representations use `Cache-Control: no-cache`.
- MIME types follow Plec's explicit extension table; unknown extensions use
  `application/octet-stream`. The table covers AVIF, CSS, CSV, GIF, HTML/HTM,
  ICO, JPEG/JPG, JavaScript/MJS, JSON, MP3, MP4, OTF, PDF, PNG, SVG, TXT, WASM,
  web manifests, WebP, WOFF/WOFF2, and XML. Content type comes from the original
  asset path, not a `.br`/`.gz` sidecar.
- Present in-root `.br` and `.gz` sidecars are eligible representations.
  Parse `Accept-Encoding` quality values, select the available coding with the
  highest quality, and prefer Brotli on ties. Explicit coding quality overrides
  wildcard; identity is acceptable by default unless explicitly excluded. If no
  available representation is acceptable, return 406. Emit
  `Vary: Accept-Encoding` when sidecars make negotiation relevant and
  `Content-Encoding` for a selected sidecar.
- Support one byte range for identity representations. A satisfiable range
  returns 206 with `Accept-Ranges` and `Content-Range`; a valid unsatisfiable
  range returns 416 with `Content-Range: bytes */<size>`. Malformed or multiple
  ranges are ignored and served as a full 200. Compressed sidecars are served
  whole rather than applying ranges to encoded bytes.
- HEAD returns GET's status and representation headers without body bytes.

Tests assert this Plec contract directly. Exhaustive parity with `ServeDir`
defaults, directory-index behavior, and conditional-request behavior are not
acceptance criteria.

Do not implement naive:

```ts id="fy4wno"
join(publicDir, pathname);
```

serving.

Filesystem threat model for 0.1: remote requests are hostile; the deployed
distribution tree is trusted, immutable application output after startup.
Resolve and canonicalize `distRoot`, `publicRoot`, manifest, route artifacts,
`server/app.mjs`, and provider modules at startup; require each target to remain
under its expected canonical root. For static requests, resolve beneath the
canonical public root, canonicalize the candidate, verify containment, then
open it. Symlinks are allowed only when their final target remains inside the
expected root. Concurrent malicious replacement of deployment files after
startup is outside the 0.1 remote threat model; this is not a sandbox against a
hostile local filesystem. Test encoded traversal, separator variants, and
symlink-to-outside. If hostile-local-filesystem race resistance becomes a
requirement, use a platform-specific native descriptor-relative helper rather
than claiming portable TypeScript checks eliminate check/open races.

`server/app.mjs` and provider modules are trusted build output and execute
arbitrary server-side JavaScript when imported. Containment limits which files
are loaded; it does not sandbox their execution.

---

# 26. Response streaming

For native document/action responses:

```text id="zt2oc3"
Rust stream
 ↓
napi-rs ReadableStream
 ↓
Node
```

For `fetch()` API:

```text id="po31hr"
new Response(nativeBodyStream, {
  status,
  headers,
})
```

SSR currently constructs the complete HTML document as a `String`; the native
response stream may therefore contain that document as one chunk. This
migration establishes streaming transport and support for large/future stream
responses; it does not claim incremental SSR generation. Action JSON is also a
bounded semantic value serialized as a finite response.

For direct `node:http` serving, prefer:

```text id="gk8a5y"
Readable.fromWeb(stream)
 ↓
pipeline(...)
 ↓
ServerResponse
```

so Node backpressure controls Web Stream demand.

Define response-stream failure behavior: before headers are sent, the host may
return the generic redacted 500 response; after headers/body streaming begins,
destroy the response and log the internal error without attempting a second
HTTP response. Test both cases, along with slow-consumer backpressure.

Before writing a response, remove or reject hop-by-hop headers:
`Connection`, `Keep-Alive`, `Proxy-Authenticate`, `Proxy-Authorization`, `TE`,
`Trailer`, `Transfer-Encoding`, and `Upgrade`. Enforce body legality: HEAD,
1xx, 204, and 304 responses emit no body bytes. Preserve `Content-Length` only
when it is known and correct; remove stale lengths when converting to an
unknown-length stream. For a known finite body such as current SSR HTML, the
adapter may retain or generate the exact length. Preserve multiple `Set-Cookie`
values separately. Cover these rules in both Web `Response` and raw
`ServerResponse` adapter tests.

Do not implement manual loops around:

```text id="bq67gg"
reader.read()
res.write()
```

unless needed for exact semantics that `pipeline` cannot satisfy.

---

# 27. Manifest evolution

The current manifest's server section is sidecar-shaped:

```json id="52li0q"
{
  "server": {
    "entry": "server/app.mjs",
    "runtime": "server/runtime.mjs"
  }
}
```

The Node host reads only the application entry; it does not depend on the
sidecar runtime field or the copied `dist/node-runtime.mjs` compatibility
artifact. The build still emits that artifact and the manifest still names
`server/runtime.mjs` so Axum remains runnable as the Slice 7 parity host. These
legacy assets are not dependencies of the default production Node host and are
removed with the sidecar in Slice 8.

Before public 0.1, evolve it to:

```json id="4o89n5"
{
  "server": {
    "entry": "server/app.mjs"
  }
}
```

Plec has not released this manifest contract. Keep `SERVER_MANIFEST_VERSION =
1` and evolve the schema in place before 0.1; do not add a migration reader or
version solely to represent the Node cutover. The Node host validates v1 and
requires `server.entry` when a server bundle is present. `server.runtime`
remains available to Axum during Slice 7, but is ignored by Node and removed
with the sidecar in Slice 8. Revisit versioning only if a released artifact
establishes an external compatibility boundary.

---

# 28. Build output

Target production output:

```text id="lb1den"
dist/
├── plec-server.json
├── public/
│   ├── route-artifact.json
│   └── ...
└── server/
    └── app.mjs
```

The default Node production path does not need or load:

```text id="2gncpn"
server/runtime.mjs
```

The compatibility build may still emit this file for Axum during Slice 7;
remove it from generated output when Slice 8 retires the sidecar.

A generated startup file is optional.

If emitted:

```text id="1ho0tq"
server/start.mjs
```

it should simply import `@plec/node` and start the host.

Do not duplicate host implementation into generated output.

---

# 29. CLI migration

Current:

```text id="3apnab"
plec serve
 ↓
Rust process
 ↓
spawn Node sidecar
 ↓
Axum listener
```

Target:

```text id="uf72c6"
plec serve
 ↓
Node production host
 ↓
native Rust addon
```

For 0.1, the production `plec serve` command invoked from the npm package must
start the Node host directly. The Rust CLI must not remain the parent process
of the public HTTP server.

```text id="1b1oj1"
plec serve
 ↓
Node @plec/node host
```

The npm `plec` shim should route `serve` to Node. Rust CLI functionality for
build/compiler operations may remain Rust-owned. Do not leave a Rust launcher
process supervising production serving; this avoids ambiguous signal
forwarding, exit-code propagation, and shutdown ownership. If a platform
constraint later requires a launcher, treat it as a separate design decision
with explicit process/signal tests.

The production lifecycle is:

- Node is the application host;
- Node owns signal handling, the listener, active-request draining, native
  application close, and process exit;
- the Axum host remains an independent migration/parity path until removed.

`PlecHandler.close()` stops admission, cancels and drains native work; all
concurrent close calls await one shared Open → Closing → Closed transition.
An in-flight callback retains a safe cloned handle for its invocation until its
native wait completes or is detached by cancellation. Releasing the manager
prevents new calls and does not invalidate an active bridge call. JS callback
closures receive serialized callback arguments, not request-scoped native
state; after cancellation Rust drops its waiter, so late Promise settlement
cannot resume native request state. `close()` does not close embedding HTTP
sockets, forcibly abort arbitrary user API handlers, or cancel external static
streams; embedding servers own those resources. It does not wait indefinitely
for detached JS promises. Those promises may settle after the handler is closed,
but retain capacity permits until settlement and must not retain native request
state or `PlecApplication`.

`serve()` shutdown order is: stop listener admission; stop new Plec dispatch;
abort/close ingress bodies still being read; allow active responses up to a
finite 5-second shutdown grace; destroy remaining sockets/streams; await
`PlecHandler.close()`; then return. A client disconnect after response start
cancels the request token and native response stream, stops the pipeline, and
does not attempt a second response. Repeated close is idempotent. Do not call
`process.exit()` from the library; let Node exit after server and native
resources have closed.

---

# 30. Packaging

Use `@napi-rs/cli`.

Do not invent the native package loader.

The initial supported runtime floor is Node.js 22. Native packages use the
Node-API ABI supported by the selected napi-rs release. CI must test the minimum
supported Node 22 minor, the latest Node 22 release, and any newer LTS/current
line the package claims to support. Unsupported platform/architecture/libc
combinations must fail with a clear module-load error naming the detected
platform, architecture, libc where relevant, and supported package targets.
There is no source-build fallback unless separately specified and tested.

Target package model for the Node binding:

```text id="qqs2ae"
@plec/node
@plec/node-linux-x64-gnu
@plec/node-linux-x64-musl
@plec/node-linux-arm64-gnu
@plec/node-darwin-arm64
@plec/node-darwin-x64
@plec/node-win32-x64-msvc
```

Slice 7 declares all six target packages above. CI builds all six packages and
runtime-smokes the packed wrapper plus native platform package on Linux x64
GNU, macOS x64/arm64, and Windows x64. Linux musl and arm64 remain build/package
coverage only; they are not runtime-smoked by the current runner matrix.

Reuse lessons/scripts from:

```text id="o9ik6m"
plec-query-node
```

but recognise that its current local native build is not yet a full distribution pipeline.

CI must:

```text id="5ooqno"
build each supported target
collect @napi-rs artifacts
assemble platform packages
run native addon smoke test
verify generated declarations
verify package loader selects correct binary
```

---

# 31. Testing strategy

Do not create a separate "proof of napi-rs."

Test the actual architecture incrementally.

## Layer A — engine tests

Move semantic tests alongside extracted code.

Existing tests for:

```text id="9oocqh"
route matching
loader outcomes
redirects
not-found
SSR
snapshots
action limits
public failure semantics
```

must remain green without Axum.

## Layer B — binding tests

Test actual Plec binding behavior:

```text id="61zgqu"
multi-chunk request reaches Rust incrementally
body ceiling rejects before remainder is consumed
Rust response stream is demand-driven
native stream error rejects/aborts response
slow response consumer applies backpressure
Promise callback resolves
Promise callback rejects
invalid callback return is controlled error
close during callback is safe
callback invocation after close is refused
concurrent document requests do not deadlock
callback can re-enter native API asynchronously without deadlock
binding feasibility spike passes on minimum supported Node version
inbound stream reader release/cancel behavior is verified
selected napi-rs feature/version set is recorded and reproducible
```

## Layer C — Node host tests

Test:

```text id="lgqyw3"
API direct dispatch (API/action/document only; static serving belongs to `serve()`)
API middleware
API 404
API error redaction
bounded API body pre-read before handler invocation
API body overflow with missing/false Content-Length
static asset parity
large static asset streams without crossing N-API
client disconnect
HEAD
Set-Cookie
duplicate headers
ranges
precompressed assets
focused raw-socket regression coverage that representative ambiguous framing
is rejected by Node before Plec dispatch
duplicate Host/Origin/Content-Length/Transfer-Encoding behavior
fetch(Request) normalized-header contract versus node:http raw-header contract
bounded request admission and overload response
concurrent API pre-reads stay within aggregate memory budget
cancelled API handler retains request/body permits until its Promise settles
cancelled callback retains callback permit until its Promise settles
callback saturation returns 503 without invoking another callback
slow API/action body timeout and connection handling
raw request-target validation and shared encoded-path classifier/matcher parity
action/document/static method behavior and unread-body connection policy
response hop-by-hop headers and body legality for HEAD/1xx/204/304
```

## Layer D — dual-host parity

During migration run the same built `apps/fullstack` fixture through:

```text id="9lw9ht"
existing Axum host
new @plec/node host
```

Compare:

```text id="20dzlx"
status
headers
body
redirects
SSR markup
snapshot payload
not-found
loader public failures
action responses
provider SSR
```

Do not compare implementation-specific headers that are intentionally different unless they are part of the public contract.

## Layer E — E2E

Existing browser acceptance/E2E tests must run against the Node host before cutover.

Identity-sensitive SSR adoption is a release gate.

## Layer F — repository integration

Physical extraction and package additions must update and validate:

```text
Cargo workspace members and Cargo.lock
Rust dependency/license checks
CI cache keys and native/unit jobs
Turbo package graph and Node package scripts
generated napi declarations and a stale-generation check
build output/manifest fixtures and contract tests
plec workspace context/trace/marker/contract source references
fullstack build, acceptance, and E2E host selection
package files whitelist and native loader contents
```

In particular, update source-path references in
`crates/plec-cli/src/dev/context.rs`, `trace.rs`, `markers.rs`, and
`contract.rs` when semantic modules move. Run the relevant Plec workspace
contract/tooling checks after the move, not only Rust and browser tests.

---

# 32. Required failure tests

Explicitly test:

```text id="c5vcm5"
client disconnect during request stream
client disconnect during loader fetch
client disconnect during response streaming
close() while document execution is active
close() while invokeAction Promise is pending
close() while renderHost Promise is pending
serve shutdown stops admission, drains HTTP work, then closes PlecHandler
admission limit reached without starting application/native callbacks
concurrent bounded API reads respect configured aggregate budget
cancelled API handler retains request/body permits until its Promise settles
cancelled callback retains callback permit until its Promise settles
callback saturation returns 503 without invoking another callback
header/body/request deadline expires and releases resources
slow upload cannot hold a pre-read/native slot indefinitely
Node parser rejects representative ambiguous HTTP framing before dispatch
proxy-derived origin is used only when trustProxy is enabled
multiple and malformed x-forwarded-proto values follow explicit policy
structural action-Origin comparison covers IPv6, default ports, null, userinfo,
and malformed values
manifest/artifact/app/provider/static paths reject traversal and symlink escape
filesystem containment rejects traversal and symlink escapes under the 0.1 trusted-deployment-tree model
action Promise rejects
action Promise never resolves
host-provider Promise rejects
host-provider returns oversized markup
body exceeds Content-Length limit immediately
body lies about Content-Length and exceeds streamed limit
oversized API body does not invoke middleware or handler
native stream emits error
Node stops consuming native stream
response stream fails before headers are sent
response stream fails after headers are sent
double close()
request after close()
```

The expected semantics for "JS Promise never resolves" are:

```text id="9wa0lt"
cancellation detaches Rust from waiting
close() is allowed to complete
JS may continue independently
late result is ignored safely
```

---

# 33. Observability

Do not bundle a broad observability rewrite into this project.

Retain existing diagnostics initially.

Use:

```text id="wd85iw"
tracing
```

only where the new async native boundary materially benefits from structured request/lifecycle spans.

Do not globally install an invasive `tracing-subscriber` from the addon unless explicitly designed.

Node should continue to own user-visible host diagnostics.

---

# 34. Security invariants

The migration is not permitted to weaken:

```text id="1zggk0"
request body limits
loader response limits
artifact limits
RuntimeValue limits
action ID validation
same-origin action POST validation
unknown-action rejection
error redaction
provider allowlisting
provider output limits
path traversal protection
filesystem containment for all distribution-root inputs, including symlink policy
aggregate admission/memory bounds and ingress deadlines
trusted-proxy boundary for forwarded headers
HTTP framing rejection before Plec dispatch
SSR custom-element policy
snapshot validation
```

Every item above must have either:

```text id="57mr4l"
existing test reused
```

or:

```text id="ibfj7s"
new parity test
```

before the Node host becomes default.

---

# 35. Implementation sequence

Execute in this order.

## Vertical slices

Deliver the migration as end-to-end slices. Each slice should leave a usable,
testable path through the layers it touches; avoid completing all engine work,
then all bindings, then all host work as isolated horizontal phases. The
feasibility and shared-contract work in Slice 0 is a prerequisite, and the
existing Axum host remains available until parity and cutover are complete.

### Slice 0 — Prove the boundary and freeze host contracts

- Fixture-test canonical raw request-target parsing and shared path
  classification/matching rules.
- Freeze admission/permit lifetimes and the `fetch(Request)` versus `serve()`
  guarantees.
- Pass the napi-rs feasibility gate for native loading, async callback re-entry,
  Web Stream backpressure/cancellation, and close races; record Node/napi-rs
  versions and features.
- Verify representative HTTP framing rejection remains Node-owned.

**Exit:** these contracts and the binding gate pass before native DTOs or a
production Node-host option are treated as stable.

### Slice 1 — Shared engine + Node document response

- Extract the host-independent execution boundary and make Axum use it without
  changing observable behavior.
- Add the native application lifecycle and the minimum binding needed to load
  an application and execute a document request.
- Add the small `@plec/node` handler path needed to send document status,
  headers, and streamed response bytes; retain SSR's current complete-String
  generation and fallback-shell behavior.

**Exit:** a compiled document renders through Node → N-API → Rust, with Axum
parity and explicit close behavior. This slice does not depend on request-body
streaming.

### Slice 2 — Host-provider SSR callback

- Load and validate provider metadata/modules in `@plec/node`.
- Connect the engine's host-provider boundary to a scoped, Promise-aware native
  callback.
- Preserve allowlisting, containment, output limits, and inert fallback behavior.

**Exit:** provider SSR fixtures match the existing host, including failure and
oversize cases.

### Slice 3 — Server actions + streamed native request body

- Move action transport/value validation into the shared engine and expose
  `handleAction` through the binding.
- Pass action request bodies as Web Streams through N-API; enforce the
  authoritative body limit incrementally in Rust before bounded JSON decoding.
- Wire generated `hasAction`/`invokeAction` behavior and map semantic outcomes
  to the existing public HTTP contract.

**Exit:** action parity covers origin and ID validation, limits, errors,
concurrency/ALS isolation, callback settlement, and incremental body rejection.

### Slice 4 — Direct Node API dispatch

- Route `/api` and `/api/*` directly to cached `app.mjs` code.
- Build canonical Node `RequestContext` with shared parity fixtures.
- Ignore GET/HEAD bodies; for other methods enforce bounded pre-read and
  aggregate byte admission before middleware or handler invocation.
- Preserve API response streaming, redaction, overload, timeout, and abort
  behavior.

**Exit:** API routing/middleware and context behavior match the existing
contract, including `/api`, `/api/`, and `/api/foo`; oversized or overloaded
requests never invoke application code.

### Slice 5 — Node static-file serving

- Implement static lookup and streaming under the canonical public root.
- Implement section 25's explicit Plec static contract for containment, MIME,
  compression negotiation, HEAD, single ranges, caching, and path errors.

**Exit:** focused section 25 contract tests pass, including symlink escape and
encoded traversal; static payloads never cross N-API.

### Slice 6 — Complete lifecycle and transport integration

- Complete request cancellation, admission permits, deadlines, response-stream
  state handling, and deterministic `close()`/`serve()` shutdown ordering.
- Complete raw-header and request-target handling in the `node:http` adapter;
  keep `fetch(Request)` guarantees distinct where Web normalization loses data.
- Run dual-host parity and browser E2E against the Node host.

**Exit:** required failure/security tests and identity-sensitive SSR adoption
pass; the Node host is ready to become the default.

### Slice 7 — Ship and cut over

- Build/test supported native platform packages and generated declarations.
- Route npm `plec serve` directly to the Node host; verify production build output
  and manifest no longer require the sidecar runtime.

**Exit:** Node is the production process owner on all claimed targets, with the
full acceptance/E2E suite green. Keep Axum available temporarily as a parity
reference.

### Slice 8 — Retire the sidecar

- After cutover is green, remove `packages/plec-node-runtime`, private
  Rust↔Node transport/supervision, and generated `server/runtime.mjs` output.
- Update architecture/tooling references and rerun repository integration
  checks.

**Exit:** the production path has no sidecar process or private HTTP protocol,
and the Definition of Done in section 39 holds.

The slices are vertical delivery units, not permission to weaken the acceptance
criteria or security invariants in the detailed sections below. The existing
layer-specific test strategy remains the validation matrix for each slice.

## Milestone 0 — Validate N-API feasibility and host contracts

Before the semantic extraction locks in native API assumptions, settle only
these four implementation contracts:

1. Define and fixture-test the validated raw request-target and shared
   `CanonicalPlecPath` algorithm.
2. Freeze the permit acquisition/release table and distinguish capacity
   tracking from shutdown tracking.
3. Freeze `PlecHandler.fetch(Request)` guarantees versus `serve()` transport
   guarantees, including the limit on bytes consumed (not prior caller
   allocations) and static-file ownership.
4. Pass the section 10.1 napi-rs feasibility gate for callback scheduling,
   Promise re-entry, stream cancellation/backpressure, and close races.

The gate also records the supported Node floor and selected napi-rs version and
features. HTTP framing remains Node-owned; verify representative parser
rejections with focused regression coverage rather than designing a Plec
framing layer. Resolve proxy authority, filesystem containment, and
platform-specific static parity in their respective implementation milestones.
Do not freeze native DTOs or expose the Node host as a production option until
these four contracts and the binding gate pass.

## Milestone 1 — Establish the shared semantic boundary

Create a host-independent execution boundary (provisionally
`plec-server-engine`) if that is the cleanest dependency structure. The name
and crate count may change; the dependency invariant may not.

```text id="3n5ix4"
host-independent server execution boundary
```

Move:

```text id="mbqed6"
RequestContext
artifact execution
routing
loader execution
SSR
snapshot construction
host capability trait
action semantic execution
```

Make the existing Axum host use it.

Acceptance:

```text id="soj0h5"
all existing server tests green
no externally observable server behavior changed
the shared semantic execution target does not depend on Axum/Tower/NAPI
```

## Milestone 2 — Native class, callbacks, and close lifecycle

Create:

```text id="f8jfoj"
plec-node-bindings
```

Implement:

```text id="zw2e2l"
PlecApplication.load()
PlecApplication.close()
generated native declarations
PlecCallbackManager
Promise-aware action/provider callback plumbing
```

Milestone 2 tests the callback abstraction with controlled fixtures; it does
not yet connect production action or provider behavior. Those integrations are
covered by their later milestones.

Acceptance:

```text id="cd8xno"
Promise resolve/reject works
callback return mismatch is safe
callbacks release on close
request-after-close fails predictably
no TSFN ownership leak
```

## Milestone 3 — Node document path

Implement `handleDocument()` and connect the Node adapter to the native
semantic engine. SSR still produces a complete `String`; return it through the
native response stream without claiming incremental SSR.

Acceptance:

```text
real compiled document renders through Node → N-API → Rust
native response is exposed as a Web ReadableStream
headers/status and fallback-shell behavior match Axum
```

No request-body streaming criterion belongs to this milestone; documents do
not consume a meaningful inbound body today.

## Milestone 4 — Host-provider SSR

Move provider loading into `@plec/node`.

Wire:

```text id="bc0ziu"
renderHost
```

through scoped callback infrastructure.

Acceptance:

```text id="g089ad"
existing provider SSR fixtures match Axum/sidecar output
inert fallback behavior preserved
size limits preserved
```

## Milestone 5 — Server actions and native request streaming

Expose:

```text id="k92yon"
handleAction()
```

through native engine.

Wire generated:

```text id="6st0r8"
hasAction
invokeAction
withRequestContext
```

Acceptance:

```text id="qps8gt"
same-origin
unknown IDs
argument limits
result limits
error redaction
ALS isolation
concurrent actions
multi-chunk action body reaches Rust incrementally
body ceiling rejects before the remainder is consumed
```

all preserve current behavior.

This is the first production path that proves Node Web Stream → napi-rs → Rust
incremental request consumption. Rust applies the authoritative limit before
bounded buffering and JSON decoding.

## Milestone 6 — Direct Node APIs

Route:

```text id="jevq6s"
/api/*
```

directly to `app.mjs`.

Ignore GET/HEAD bodies; bounded-pre-read all other methods before handler
invocation and build canonical Node `RequestContext`. API bodies up to the
existing 1 MiB ceiling are buffered;
this preserves the current reject-before-application-dispatch contract. Do not
apply this API exception to the Rust action-body N-API stream.

Acceptance:

```text id="pkv373"
API routing/middleware parity
`/api`, `/api/`, and `/api/foo` routing
GET/HEAD bodies ignored and never exposed to application code
bounded body rejected before middleware/handler invocation
fallback 404
error redaction
context parity
aggregate pre-read budget and overload policy
slow-body timeout releases admission and prevents handler invocation
```

## Milestone 7 — Static assets

Implement Node static serving.

Acceptance against the explicit Plec static contract in section 25.
Containment and symlink policy applies equally to manifest, artifact, app entry,
provider modules, and static assets; escape and path-race tests pass.

## Milestone 8 — Full lifecycle/cancellation

Wire:

```text id="8h7nsz"
AbortSignal
client disconnect
CancellationToken
close()
task draining
TSFN release
```

Acceptance includes all shutdown/failure tests.

The Node host cannot become default until raw HTTP framing, resource admission,
deadlines, trusted-proxy deployment constraints, and filesystem containment
tests from Milestone 0 are green on supported targets.

The spike must settle the concrete AbortSignal-to-native-token mechanism and
whether dropping the napi-rs stream reader cancels its JS source. Prefer direct
napi-rs AbortSignal support if the selected v3 API provides it; otherwise use
one per-operation cancellation handle, never a global request-ID registry.

## Milestone 9 — Packaging

Ship native packages with `@napi-rs/cli`.

Add platform CI.

## Milestone 10 — Make Node host default

Run complete acceptance/E2E suite against Node.

Change the npm `plec serve` command to start the Node host directly; do not
retain a Rust parent process for production serving.

Keep Axum host temporarily available only for parity/reference if useful.

## Milestone 11 — Remove sidecar

Delete obsolete infrastructure only after Node host is default and green.

---

# 36. Code expected to disappear

After cutover, remove Node-host-obsolete code including:

```text id="rbqj3e"
crates/plec-server/src/runtime/internal.rs
crates/plec-server/src/runtime/protocol.rs
NodeApplicationRuntime sidecar spawn/supervision
private UDS/TCP client
runtime auth token
READY protocol
health route
sidecar shutdown protocol
packages/plec-node-runtime
generated server/runtime.mjs
sidecar request serialization
sidecar response buffering
sidecar host-render endpoint
sidecar action endpoint
```

The generated:

```text id="ssg5pi"
server/app.mjs
```

remains.

Provider loading logic is moved, not deleted.

---

# 37. Explicitly out of scope

Do not expand this migration into:

```text id="4xyxfr"
incremental streaming SSR generation
rewriting loader fetch through Node
new application authoring APIs
new server-action syntax
changing action()
rewriting RuntimeValue
native Windows/macOS UI targets
generic extension/plugin ABI
replacing reqwest
rewriting @plec/core server types
removing Axum before parity is established
```

Those can be separate projects.

---

# 38. Expected final request paths

## Document

```text id="ba0bt5"
Browser
 ↓
Node HTTP
 ↓
@plec/node
 ↓
native PlecApplication
 ↓
Rust route match
 ↓
Rust loaders
 ↓
Rust SSR
 ↓
Rust snapshot
 ↓
napi ReadableStream
 ↓
Node response
```

## API

```text id="26ha1a"
Browser
 ↓
Node HTTP
 ↓
generated app.mjs
 ↓
middleware
 ↓
API handler
 ↓
Web Response
```

## Server action

```text id="wdy1yc"
Browser
 ↓
Node HTTP
 ↓
native handleAction
 ↓
Rust validation
 ↓
PlecCallbackManager.invokeAction
 ↓
app.mjs
 ↓
withRequestContext
 ↓
user action()
 ↓
Rust result validation
 ↓
streamed Node response
```

## Host-provider SSR

```text id="xif0ds"
Rust SSR
 ↓
PlecCallbackManager.renderHost
 ↓
Node provider registry
 ↓
HTML
 ↓
Rust SSR resumes
```

## Static asset

```text id="4yqmnh"
Browser
 ↓
Node HTTP
 ↓
Node filesystem stream
```

No Rust crossing.

---

# 39. Definition of done for 0.1

The migration is complete when all of the following are true:

```text id="4wgfk4"
Node is the default production process owner.

Only one public application server process is required.

No Node sidecar is spawned.

No private Rust↔Node HTTP protocol exists in the production path.

Document routing/loaders/SSR/snapshots remain Rust-authored.

Server-action transport/value semantics remain Rust-authored.

Application APIs execute directly in Node.

Host-provider SSR uses direct Promise-aware native callbacks.

Server-action request bodies cross N-API as streams and native response bodies
use the stream interface. Node API bodies retain the bounded pre-read before
handler invocation; current SSR may emit its completed HTML as one stream chunk.

Client disconnects cancel native request work.

close() has deterministic lifecycle semantics.

All long-lived TSFNs are explicitly released.

Existing @plec/core server authoring APIs remain source-compatible.

Existing fullstack SSR/adoption/action/provider tests pass.

Security/resource ceilings remain at least as strict as before.

Native packages install and load on every supported 0.1 target.

Generated server output no longer contains runtime.mjs.

The old Node sidecar implementation is removed.
```

---

# 40. Architectural invariant after completion

Update repository architecture documentation to state:

> **Node owns Plec's production HTTP transport, process lifecycle, static asset transport, and arbitrary application JavaScript execution. Rust owns compiled Plec server semantics, including document routing, route-loader execution, SSR, snapshots, server-action validation, and native runtime limits. The two runtimes communicate in-process through a typed napi-rs boundary: action request bodies and native responses use Web Streams, while bounded Node API request bodies are pre-read before handler invocation; explicitly scoped Promise-aware callbacks handle the small number of operations requiring application JavaScript.**

That should replace the current invariant that the public socket is owned by Rust.
