# Untrusted-input resource limits

Executable Plec artifacts, runtime values, snapshots, fetch responses, source
modules, and execution work are treated as untrusted. Transport byte envelopes
bound decode allocation; structural validation then rejects pathological decoded
shapes before runtime execution. Values are generous: they must accept any
legitimate compiled output and reject only pathological inputs.

Canonical constants live in `crates/plec-ir/src/limits.rs` (re-exported by
`plec-schema`). TypeScript host mirrors are tracked for generated replacement
in Beads `wasm-runtime-a08`.

## Artifact and manifest decode (WASM boundary)

| Boundary                                     | Limit                              | Where                                                                                                                                                         |
| -------------------------------------------- | ---------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Application artifact / lazy graph JSON bytes | 16 MiB (`MAX_ARTIFACT_JSON_BYTES`) | `decode_untrusted_json` in `crates/plec-runtime/src/lifecycle.rs`; browser fetch in `packages/plec-browser`                                                   |
| Route manifest JSON bytes                    | 1 MiB (`MAX_MANIFEST_JSON_BYTES`)  | same                                                                                                                                                          |
| Manifest route count                         | 2,048 (`MAX_MANIFEST_ROUTES`)      | `RouteManifest::validate` (`crates/plec-ir`)                                                                                                                  |
| JS-value normalization depth                 | 128 (`MAX_DECODE_JS_DEPTH`)        | all decode paths normalize JS Map/object/array values before stringification, so hostile nesting fails before Rust deserialization can exhaust the WASM stack |
| JS-value normalization width / nodes         | 1,000,000 (`MAX_DECODE_JS_NODES`)  | all decode paths normalize JS Map/object/array values before stringification, so hostile width fails before allocating every member                           |

## Structural budgets (`TypedApplication::validate_contract`)

| Boundary                        | Limit                                                                                                                                                                                                                                                          |
| ------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Component definitions           | 65,536 (`MAX_COMPONENT_COUNT`); pathological-shape guard only                                                                                                                                                                                                  |
| Any per-component IR collection | 100,000 entries (`MAX_COMPONENT_COLLECTION_LEN`)                                                                                                                                                                                                               |
| Total IR entries                | 1,000,000 (`MAX_TOTAL_IR_ENTRIES`)                                                                                                                                                                                                                             |
| Total instructions              | 1,000,000 (`MAX_TOTAL_INSTRUCTIONS`)                                                                                                                                                                                                                           |
| String pool                     | 1 MiB per entry / 8 MiB aggregate (`MAX_COMPONENT_STRING_BYTES`, `MAX_TOTAL_STRING_POOL_BYTES`)                                                                                                                                                                |
| Constant values                 | 1,000,000 aggregate nodes (`MAX_TOTAL_CONSTANT_NODES`)                                                                                                                                                                                                         |
| Expression program              | 10,000 instructions (`MAX_EXPRESSION_INSTRUCTIONS`)                                                                                                                                                                                                            |
| Action program                  | 10,000 instructions (`MAX_ACTION_INSTRUCTIONS`)                                                                                                                                                                                                                |
| Constant runtime values         | depth 64 (`MAX_VALUE_DEPTH`), 100,000 nodes (`MAX_VALUE_NODES`), 1 MiB per string (`MAX_VALUE_STRING_BYTES`)                                                                                                                                                   |
| Node graph topology             | every structural handle in range; ownership edges form a forest rooted at the graph root and loop row templates: acyclic, no node claimed twice, fully reachable, and no tree deeper than 128 (`MAX_NODE_GRAPH_DEPTH`) (`TypedApplication::validate_topology`) |
| Component call graph            | acyclic over static `Component` nodes and component-valued props (`validate_component_call_graph_acyclic`); dynamic component targets resolve at runtime and are bounded by the mounted-region budget                                                          |
| Action frame slots              | 4,096 (`MAX_FRAME_SLOTS`) — `frameSlots` sizes the per-frame allocation directly and is capped before slot-index checks                                                                                                                                        |
| Expression control flow         | jump targets within the program, Filter/Map program handles in range (`validate_contract`)                                                                                                                                                                     |

## Runtime values (host inputs, rows, fetch bodies)

| Boundary                      | Limit                                    | Where                                                                                                            |
| ----------------------------- | ---------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| Host-input payload JSON bytes | 1 MiB (`MAX_HOST_INPUT_JSON_BYTES`)      | `set_host_inputs`, `apply_delta(s)`, `initialize_input`                                                          |
| Runtime value trees           | depth 64 / 100,000 nodes / 1 MiB strings | `RuntimeValue::check_limits` (`crates/plec-schema/src/delta.rs`), enforced in `runtime_from_json` and validation |

## SSR snapshots

| Boundary                                                              | Limit                               | Where                                                                   |
| --------------------------------------------------------------------- | ----------------------------------- | ----------------------------------------------------------------------- |
| Snapshot payload JSON bytes                                           | 4 MiB (`MAX_SNAPSHOT_JSON_BYTES`)   | `import_ssr_snapshot` (WASM); inline `#plec-bootstrap` script (browser) |
| Routes / loaders / structure graphs / nested records / public exports | 2,048 each (`MAX_SNAPSHOT_ENTRIES`) | `PlecSsrSnapshot::validate` (`crates/plec-ir`)                          |
| Loop keys per loop node                                               | 10,000 (`MAX_SNAPSHOT_LOOP_KEYS`)   | same                                                                    |
| Export and loader-resolved values                                     | depth 64 / 100,000 nodes            | `ensure_value_is_bounded` (same file)                                   |

## Snapshot input facades

| Boundary                          | Limit                                    | Where                                                                                        |
| --------------------------------- | ---------------------------------------- | -------------------------------------------------------------------------------------------- |
| Snapshot value/shape JSON bytes   | 1 MiB (`MAX_HOST_INPUT_JSON_BYTES`)      | `initialize_snapshot_input`, `apply_input_snapshot` (`crates/plec-runtime/src/snapshots.rs`) |
| Snapshot value trees              | depth 64 / 100,000 nodes / 1 MiB strings | `check_value_budget` (same file)                                                             |
| Observed paths per snapshot shape | 1,024 (`MAX_SNAPSHOT_SHAPE_PATHS`)       | `validate_shape` (same file)                                                                 |
| Segments per observed path        | 64 (`MAX_SNAPSHOT_SHAPE_PATH_SEGMENTS`)  | same                                                                                         |

## Fetch responses

| Boundary                  | Limit                              | Where                                                                                                                                                                                                                                                                                |
| ------------------------- | ---------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Declared `content-length` | 8 MiB (`MAX_FETCH_RESPONSE_BYTES`) | `declared_length_failure` in `crates/plec-client/src/fetch.rs`; rejected before the body is read. A fast path only: never trusted as the sole ceiling                                                                                                                                |
| Streamed body bytes       | 8 MiB                              | enforced chunk-by-chunk before whole-body buffering; the stream is cancelled mid-flight when the budget is exceeded — `bounded_body_bytes` (client fetches), `boundedResponseBytes` (browser artifact/graph/runtime-JS loading), `read_bounded_stream` (native server route loaders) |
| Decoded JSON payload      | 8 MiB                              | `bounded_response_value` (JS Map normalization + bounded JSON parse); native server route loaders parse the bounded text (`crates/plec-server`)                                                                                                                                      |

Absent, forged, and lying `content-length` declarations therefore cannot
buffer a response body past the ceiling: the byte budget is applied while
the body streams, before any `text()`/`json()`-style whole-body read.

## Source modules and import graph (compiler)

| Boundary                 | Limit                           | Where                                           |
| ------------------------ | ------------------------------- | ----------------------------------------------- |
| Source file size         | 2 MiB (`MAX_SOURCE_FILE_BYTES`) | `crates/plec-compiler/src/read_source_graph.rs` |
| Aggregate source size    | 32 MiB (`MAX_TOTAL_SOURCE_BYTES`) | same                                          |
| Import-chain depth       | 128 (`MAX_IMPORT_DEPTH`)        | same                                            |
| Module count per graph   | 4,096 (`MAX_MODULE_COUNT`)      | same                                            |
| Workspace package count  | 1,024 (`MAX_WORKSPACE_PACKAGE_COUNT`) | same (`WorkspaceIndex::load`)             |
| Workspace manifest bytes | 1 MiB (`MAX_MANIFEST_JSON_BYTES`) | same (`read_workspace_manifest`)              |

Cycles are already rejected by the existing `seen` set; depth, count, and
aggregate byte bounds extend this to deep chains and file-count/size
exhaustion.

Containment is scope-based, not repository-wide: relative imports may only
reach sources beneath the application root (`root_dir` as passed by the
build pipeline — the app directory), and workspace imports may only reach
sources beneath the resolved package's own directory. One application can
therefore never read another application's sources through the shared
repository root, and a package cannot pull in its siblings' files.

Source reads are also resistant to concurrent path replacement: the
canonicalized path is opened and both the size accounting and the bytes are
taken from the open file handle (fstat + read), then the original pathname
must still canonicalize to the opened file or the compile fails
(`read_bounded_source`). This narrows the symlink-swap window dramatically
but cannot eliminate it against a hostile writer with arbitrary filesystem
access; production builds must compile from an isolated, immutable
workspace.

## Compiler HIR and lowering budgets

| Boundary                    | Limit                                       | Where                                        |
| --------------------------- | ------------------------------------------- | -------------------------------------------- |
| Nodes/expressions per HIR component | 100,000 (`MAX_COMPONENT_COLLECTION_LEN`) | `crates/plec-compiler/src/hir_builder.rs`  |
| HIR nodes+expressions per application | 1,000,000 (`MAX_TOTAL_HIR_ENTRIES`) | same (`ensure_hir_aggregate_budgets`)      |
| Component count             | 65,536 (`MAX_COMPONENT_COUNT`)              | `hir_builder.rs` and `plec-lowering`         |
| Component nesting depth     | 128 (`MAX_COMPONENT_NESTING_DEPTH`)         | same                                         |
| Entries per lowered component collection | 100,000 (`MAX_COMPONENT_COLLECTION_LEN`) | `crates/plec-lowering/src/application.rs` |
| Total lowered IR entries    | 1,000,000 (`MAX_TOTAL_IR_ENTRIES`)          | same                                         |
| Total lowered instructions  | 1,000,000 (`MAX_TOTAL_INSTRUCTIONS`)        | same (`component_budget_usage`)              |
| Total string pool bytes     | 8 MiB (`MAX_TOTAL_STRING_POOL_BYTES`)       | same                                         |

These mirror the artifact-boundary aggregates so a pathological application
fails in the compiler instead of lowering and serializing without bound.
Constant-value tree nodes remain bounded by `MAX_TOTAL_CONSTANT_NODES` at
the artifact decode boundary.

## Execution budgets

| Boundary              | Limit                                                                                                                                                                                              | Where                                                                                 |
| --------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| Expression evaluation | 100,000 shared steps (`MAX_EXPRESSION_STEPS`), Filter/Map nesting 32 (`MAX_EVAL_NESTING`), stack of at most 1,000 values / 8 MiB estimated bytes (`MAX_EVAL_STACK_VALUES`, `MAX_EVAL_STACK_BYTES`) | `crates/plec-eval/src/eval.rs` (SSR mirror in `crates/plec-server/src/ssr/render.rs`) |
| Action continuations  | 1,000,000 steps (`MAX_ACTION_STEPS`), call depth 64 (`MAX_CALL_DEPTH`)                                                                                                                             | `crates/plec-client/src/vm.rs`                                                        |
| Reaction drain        | 10,000 executions (`MAX_REACTION_STEPS`), nesting 32 (`MAX_REACTION_DRAIN_DEPTH`)                                                                                                                  | same                                                                                  |
| Graph mount recursion | depth 128 (`MAX_MOUNT_DEPTH`), stack watermark 512 KiB (`MAX_MOUNT_STACK_BYTES`)                                                                                                                   | `crates/plec-client/src/runtime.rs`                                                   |
| SSR row adoption      | depth 128 + 512 KiB stack watermark (shared mount budgets)                                                                                                                                         | `crates/plec-client/src/runtime.rs` (`adopt_row_node`)                                |
| SSR render walk       | depth 256 (`MAX_SSR_RENDER_DEPTH`)                                                                                                                                                                 | `crates/plec-server/src/ssr/render.rs` (`render_node`)                                |

Because validation caps node-graph depth at `MAX_NODE_GRAPH_DEPTH` (128), a
validated graph can never push the recursive mount, adoption, or SSR render
walks past their depth budgets; the runtime guards remain as defense in depth
for native/WASM stack safety. Fuel is shared across nested Filter/Map
predicate evaluation, so crafted backward-jump loops and self-referential
predicates exhaust a documented budget instead of pinning the tab, growing
the heap, or overflowing the stack. The expression value stack additionally
tracks live value count and estimated bytes — a `Constant` instruction
deep-clones whole constant-pool trees, so instruction fuel alone would bound
steps but not the memory one step may enqueue; the SSR mirror enforces the
same ceilings (failing soft to `null`). Tail calls (`Call`/`CallFrame`
without continuations) run inside the same continuation loop and its
call-depth budget instead of native recursion, so chained or
self-referential tail calls exhaust the same documented bound.

## Amplification budgets

| Boundary               | Limit                                                                                                 | Where                                                                                                                                            |
| ---------------------- | ----------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| Loop expansion         | 10,000 rows (`MAX_LOOP_ROWS`)                                                                         | Projection builders reject before row allocation; `reconcile_loop` rejects before mutation                                                       |
| Live runtime ownership | 100,000 regions (`MAX_MOUNTED_REGIONS`)                                                               | Shared application-wide region tracker; graph instances, rows, and conditional regions own idempotent lifecycle slots                            |
| Reconcile work         | 500,000 DOM operations (`MAX_DOM_OPERATIONS_PER_RECONCILE`)                                           | One reservation spans the top-level reconcile and all deferred component work until flush quiescence                                             |
| Concurrent fetches     | 128 (`MAX_IN_FLIGHT_FETCHES`)                                                                         | Shared application-wide fetch tracker                                                                                                            |
| Logical action fetches | 128 fetches / 16 MiB decoded response values (`MAX_FETCHES_PER_ACTION`, `MAX_FETCH_BYTES_PER_ACTION`) | Accounting is carried by the action continuation across nested calls, continuations, and finalizers; concurrent actions receive distinct records |

Product-policy caps such as manifest routes, source modules, and snapshot loop
keys remain documented separately from these primary exhaustion defences.

## Inbound request bodies (dev server)

| Boundary                  | Limit                            | Where                                                                         |
| ------------------------- | -------------------------------- | ----------------------------------------------------------------------------- |
| Request body bytes        | 1 MiB (`MAX_REQUEST_BODY_BYTES`) | `read_bounded_body` in `crates/plec-server/src/request.rs` (413 on violation) |
| Application artifact file | 16 MiB                           | `read_bounded` in `crates/plec-server/src/artifact.rs`                        |

## Boundary tests

- `crates/plec-schema/src/typed.rs` — oversized collections, deep constants,
  oversized programs, oversized string pool entries, cyclic/self-child/shared
  and unrooted node graphs, over-deep node graphs and chains exactly at the
  depth limit, component-call-graph cycles, action frame-slot caps,
  out-of-range structural handles, out-of-range expression jump targets and
  Filter/Map program handles.
- `crates/plec-ir/src/lib.rs` — snapshot loop-key/depth/size caps, manifest
  route count.
- `crates/plec-compiler/src/read_source_graph.rs` — oversized source file,
  over-deep import chain, chain just within the limit still compiles,
  cross-application relative-import traversal, package-scope relative
  escape, symlink escape, symlink swap detected between canonicalization
  and the post-read verification, aggregate source exhaustion, workspace
  package-count and manifest-byte caps.
- `crates/plec-compiler/src/hir_builder.rs` — per-component node budget,
  component nesting beyond/within the depth budget, aggregate HIR entry and
  component count budgets.
- `crates/plec-lowering/src/lib.rs` — per-collection lowered budget,
  aggregate IR entry exhaustion, lowered component count.
- `crates/plec-runtime/tests/untrusted_input_limits.rs` — oversized and
  over-deep host inputs, artifacts, snapshots, nested JS Maps, undefined
  fields, expression/action loops, self-tail-call actions, self-child and
  unrooted node graphs, over-deep node graphs, component call cycles,
  oversized `frameSlots`, expression stacks beyond the value/byte ceilings,
  reaction cycles, and oversized/over-deep/structurally excessive snapshot
  input values plus snapshot shape path limits at the WASM boundary.
- `crates/plec-server/tests/server.rs` — oversized request body (413) and
  oversized artifact file (500).
