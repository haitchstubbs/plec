# Plec Node Host Target Implementation Architecture

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

## 1.3 `/api/*` remains JavaScript-owned

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

## 1.7 The N-API boundary is streaming

Do not introduce a permanent buffer-all bridge.

Request:

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
```

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

# 2. New repository structure

Introduce exactly two new implementation units.

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
RequestContext
ApiRouteHandler
ApiMiddleware
AppRequestHandler
action()
requestContext()
withRequestContext()
```

The generated application bundle contract should also remain conceptually unchanged:

```ts id="ch3stj"
handleRequest(request, context)

invokeAction(id, args, context)

hasAction(id)
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

`createPlecHandler()` is the architectural API.

`serve()` is an adapter/convenience.

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

# 6. Extract `plec-server-engine`

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

Use this for action bodies.

Do not buffer an untrusted N-API request completely before invoking this logic.

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

Use a generated DTO:

```text id="yembjm"
NativeRequest
├── method
├── url
├── raw ordered headers
├── optional ReadableStream<Uint8Array>
└── cancellation/abort integration
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
    pub body: Option<ReadableStream<...>>,
}
```

Exact napi-rs stream types should follow the library API rather than inventing wrappers.

---

# 13. N-API response DTO

Likewise return:

```text id="ku446n"
NativeResponse
├── status
├── ordered raw headers
└── ReadableStream<Uint8Array>
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

The low-level Node adapter may preserve ordered header pairs more accurately than the Web `Headers` abstraction.

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

Request conversion:

```text id="5gadhz"
IncomingMessage
 ↓
absolute request URL
 ↓
Headers / raw header pairs
 ↓
Readable.toWeb(incoming)
 ↓
Web Request
```

Do not call:

```text id="h016an"
Buffer.concat(...)
```

for general request bodies.

Client abort:

```text id="pmv0os"
IncomingMessage aborted/close
 ↓
AbortController.abort()
```

That signal must ultimately cancel the corresponding native request.

---

# 22. Top-level Node dispatch

Implement one obvious dispatch function.

Conceptually:

```ts id="iujlf1"
async function dispatch(request: Request): Promise<Response> {
  const pathname = new URL(request.url).pathname;

  if (pathname.startsWith('/api/')) {
    return dispatchApi(request);
  }

  if (pathname.startsWith('/_plec/actions/')) {
    return native.handleAction(...);
  }

  if (isStaticPath(pathname)) {
    return serveStatic(...);
  }

  return native.handleDocument(...);
}
```

Exact asset/document classification must reproduce current Plec semantics.

Do not use "try document, then asset" or "try filesystem first" unless that is proven equivalent to `http.rs`.

---

# 23. API request path

The Node API path should use the existing generated application code directly.

Flow:

```text id="rpdkc4"
IncomingMessage
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

## API body ceiling

The current Rust host bounds all non-GET/HEAD bodies before application dispatch.

Direct Node APIs therefore need an equivalent streaming limit.

Implement a Node transform/reader that enforces:

```text id="ke4brz"
MAX_REQUEST_BODY_BYTES
```

without pre-buffering the request.

Do not weaken the current resource-safety contract by bypassing Rust.

The constant should have one source of truth or a generated build-time value; do not hand-maintain unrelated numeric copies.

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

If necessary, move shared fixtures into repository testdata rather than sharing implementation.

Do not make Node call Rust merely to construct API `RequestContext`.

---

# 25. Static asset host

Implement after document/action parity, not before.

The Node static host must match current `ServeDir` behavior for:

```text id="88seip"
public root containment
/_plec client assets
content types
.br/.gz sidecars
Accept-Encoding
HEAD
ranges
not found
malformed/unsafe paths
cache-control
```

Prefer an established, narrowly scoped Node static-serving primitive if it matches the required contract.

Do not implement naive:

```ts id="fy4wno"
join(publicDir, pathname)
```

serving.

Symlink/path containment must be reviewed explicitly.

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

For direct `node:http` serving, prefer:

```text id="gk8a5y"
Readable.fromWeb(stream)
 ↓
pipeline(...)
 ↓
ServerResponse
```

so Node backpressure controls Web Stream demand.

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

The final Node-host manifest should not reference a sidecar runtime.

Before public 0.1, evolve it to:

```json id="4o89n5"
{
  "server": {
    "entry": "server/app.mjs"
  }
}
```

Because 0.1 has not established a stable public deployment contract yet, prefer a clean manifest version bump over indefinite compatibility hacks.

Target:

```text id="xq5b6s"
SERVER_MANIFEST_VERSION = 2
```

Migration sequence:

1. New readers temporarily understand v1/v2 if useful during development.
2. New builds emit v2.
3. Node host requires v2.
4. Once the sidecar migration is complete, remove v1 compatibility before 0.1 unless there is an explicit compatibility requirement.

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

Do not emit:

```text id="2gncpn"
server/runtime.mjs
```

after sidecar retirement.

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

Because the current `plec` binary is Rust-owned, explicitly decide how it launches the Node host.

Recommended for 0.1:

```text id="1b1oj1"
plec serve
 ↓
exec/spawn Node once as the public application process
```

This is different from the current sidecar:

- Node is the application host;
- Rust CLI performs launcher/tooling responsibility only;
- there is no Rust HTTP server process remaining alongside it.

Alternative:

The npm `plec` shim may eventually invoke the Node host directly for `serve`.

Do not keep:

```text id="dqg60i"
Rust HTTP process + Node HTTP process
```

as the final architecture.

---

# 30. Packaging

Use `@napi-rs/cli`.

Do not invent the native package loader.

Target package model:

```text id="qqs2ae"
@plec/node
@plec/node-linux-x64-gnu
@plec/node-linux-x64-musl
@plec/node-linux-arm64-gnu
@plec/node-darwin-arm64
@plec/node-darwin-x64
@plec/node-win32-x64-msvc
```

Only claim platforms actually tested by CI.

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
Promise callback resolves
Promise callback rejects
invalid callback return is controlled error
close during callback is safe
callback invocation after close is refused
```

## Layer C — Node host tests

Test:

```text id="lgqyw3"
API direct dispatch
API middleware
API 404
API error redaction
streaming API body limit
static asset parity
client disconnect
HEAD
Set-Cookie
duplicate headers
ranges
precompressed assets
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

---

# 32. Required failure tests

Explicitly test:

```text id="c5vcm5"
client disconnect during request stream
client disconnect during loader fetch
close() while document execution is active
close() while invokeAction Promise is pending
close() while renderHost Promise is pending
action Promise rejects
action Promise never resolves
host-provider Promise rejects
host-provider returns oversized markup
body exceeds Content-Length limit immediately
body lies about Content-Length and exceeds streamed limit
native stream emits error
Node stops consuming native stream
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

## Milestone 1 — Extract semantic engine

Create:

```text id="3n5ix4"
plec-server-engine
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
plec-server-engine does not depend on Axum/Tower/NAPI
```

## Milestone 2 — Native binding skeleton

Create:

```text id="f8jfoj"
plec-node-bindings
```

Implement:

```text id="zw2e2l"
PlecApplication.load()
PlecApplication.handleDocument()
PlecApplication.close()
generated native declarations
```

No actions/providers yet.

Acceptance:

```text id="cd8xno"
real compiled document renders through Node → NAPI → engine
request body is not pre-buffered by bridge
native result returns through ReadableStream
```

## Milestone 3 — Callback infrastructure

Implement:

```text id="sc5unx"
JsCallback abstraction
MaybeAsync callback support
PlecCallbackManager
closed-state protection
Promise error handling
```

Acceptance:

```text id="oa1qlz"
Promise resolve/reject works
callback return mismatch is safe
callbacks release on close
request-after-close fails predictably
no TSFN ownership leak
```

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

## Milestone 5 — Server actions

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
```

all preserve current behavior.

## Milestone 6 — Direct Node APIs

Route:

```text id="jevq6s"
/api/*
```

directly to `app.mjs`.

Implement streaming body ceiling and canonical Node `RequestContext`.

Acceptance:

```text id="pkv373"
API routing/middleware parity
body limits
fallback 404
error redaction
context parity
```

## Milestone 7 — Static assets

Implement Node static serving.

Acceptance against current ServeDir contract.

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

## Milestone 9 — Packaging

Ship native packages with `@napi-rs/cli`.

Add platform CI.

## Milestone 10 — Make Node host default

Run complete acceptance/E2E suite against Node.

Change production `plec serve`/generated startup to Node host.

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

Request and response bodies cross the native boundary as streams.

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

> **Node owns Plec's production HTTP transport, process lifecycle, static asset transport, and arbitrary application JavaScript execution. Rust owns compiled Plec server semantics, including document routing, route-loader execution, SSR, snapshots, server-action validation, and native runtime limits. The two runtimes communicate in-process through a typed napi-rs boundary using Web Streams for body transport and explicitly scoped Promise-aware callbacks for the small number of operations requiring application JavaScript.**

That should replace the current invariant that the public socket is owned by Rust.