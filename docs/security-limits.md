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

## Structural budgets (`TypedApplication::validate_contract`)

| Boundary                        | Limit                                                                                                        |
| ------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| Component definitions           | 65,536 (`MAX_COMPONENT_COUNT`); pathological-shape guard only                                                |
| Any per-component IR collection | 100,000 entries (`MAX_COMPONENT_COLLECTION_LEN`)                                                             |
| Total IR entries                | 1,000,000 (`MAX_TOTAL_IR_ENTRIES`)                                                                           |
| Total instructions              | 1,000,000 (`MAX_TOTAL_INSTRUCTIONS`)                                                                         |
| String pool                     | 1 MiB per entry / 8 MiB aggregate (`MAX_COMPONENT_STRING_BYTES`, `MAX_TOTAL_STRING_POOL_BYTES`)              |
| Constant values                 | 1,000,000 aggregate nodes (`MAX_TOTAL_CONSTANT_NODES`)                                                       |
| Expression program              | 10,000 instructions (`MAX_EXPRESSION_INSTRUCTIONS`)                                                          |
| Action program                  | 10,000 instructions (`MAX_ACTION_INSTRUCTIONS`)                                                              |
| Constant runtime values         | depth 64 (`MAX_VALUE_DEPTH`), 100,000 nodes (`MAX_VALUE_NODES`), 1 MiB per string (`MAX_VALUE_STRING_BYTES`) |
| Node graph topology              | every structural handle in range; ownership edges form a forest rooted at the graph root and loop row templates: acyclic, no node claimed twice, fully reachable (`TypedApplication::validate_topology`) |
| Expression control flow          | jump targets within the program, Filter/Map program handles in range (`validate_contract`)                   |

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

| Boundary                             | Limit                                    | Where                                                                                             |
| ------------------------------------ | ---------------------------------------- | ------------------------------------------------------------------------------------------------- |
| Snapshot value/shape JSON bytes      | 1 MiB (`MAX_HOST_INPUT_JSON_BYTES`)      | `initialize_snapshot_input`, `apply_input_snapshot` (`crates/plec-runtime/src/snapshots.rs`)      |
| Snapshot value trees                 | depth 64 / 100,000 nodes / 1 MiB strings | `check_value_budget` (same file)                                                                  |
| Observed paths per snapshot shape    | 1,024 (`MAX_SNAPSHOT_SHAPE_PATHS`)       | `validate_shape` (same file)                                                                      |
| Segments per observed path           | 64 (`MAX_SNAPSHOT_SHAPE_PATH_SEGMENTS`)  | same                                                                                              |

## Fetch responses

| Boundary                   | Limit                              | Where                                                                                                                                                            |
| -------------------------- | ---------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Declared `content-length`  | 8 MiB (`MAX_FETCH_RESPONSE_BYTES`) | `declared_length_failure` in `crates/plec-client/src/fetch.rs`; rejected before the body is read                                                                 |
| Body length / JSON payload | 8 MiB                              | post-read check and `bounded_response_value` (JS Map normalization + bounded JSON parse); server route loaders enforce the same ceiling (`packages/plec-server`) |

## Source modules and import graph (compiler)

| Boundary               | Limit                           | Where                                           |
| ---------------------- | ------------------------------- | ----------------------------------------------- |
| Source file size       | 2 MiB (`MAX_SOURCE_FILE_BYTES`) | `crates/plec-compiler/src/read_source_graph.rs` |
| Import-chain depth     | 128 (`MAX_IMPORT_DEPTH`)        | same                                            |
| Module count per graph | 4,096 (`MAX_MODULE_COUNT`)      | same                                            |

Cycles are already rejected by the existing `seen` set; depth and count bounds
extend this to deep chains and file-count exhaustion.

## Execution budgets

| Boundary              | Limit                                                                                     | Where                          |
| --------------------- | ----------------------------------------------------------------------------------------- | ------------------------------ |
| Expression evaluation | 100,000 shared steps (`MAX_EXPRESSION_STEPS`), Filter/Map nesting 32 (`MAX_EVAL_NESTING`) | `crates/plec-eval/src/eval.rs` |
| Action continuations  | 1,000,000 steps (`MAX_ACTION_STEPS`), call depth 64 (`MAX_CALL_DEPTH`)                    | `crates/plec-client/src/vm.rs` |
| Reaction drain        | 10,000 executions (`MAX_REACTION_STEPS`), nesting 32 (`MAX_REACTION_DRAIN_DEPTH`)         | same                           |
| Graph mount recursion | depth 128 (`MAX_MOUNT_DEPTH`)                                                             | `crates/plec-client/src/runtime.rs` |

Mount recursion flows through one depth-guarded `instantiate_node` entry, so a
validated acyclic chain up to `MAX_COMPONENT_COLLECTION_LEN` nodes deep fails
with a `mount depth exceeds limit` diagnostic instead of overflowing the WASM
stack.

Fuel is shared across nested Filter/Map predicate evaluation, so crafted
backward-jump loops and self-referential predicates exhaust a documented
budget instead of pinning the tab, growing the heap, or overflowing the stack.
Tail calls (`Call`/`CallFrame` without continuations) run inside the same
continuation loop and its call-depth budget instead of native recursion, so
chained or self-referential tail calls exhaust the same documented bound.

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

| Boundary                  | Limit                            | Where                                                                             |
| ------------------------- | -------------------------------- | --------------------------------------------------------------------------------- |
| Request body bytes        | 1 MiB (`MAX_REQUEST_BODY_BYTES`) | `readBody` in `packages/plec-server` (413 on violation, enforced while streaming) |
| Application artifact file | 16 MiB                           | `readBoundedArtifact` in `packages/plec-server`                                   |

## Boundary tests

- `crates/plec-schema/src/typed.rs` — oversized collections, deep constants,
  oversized programs, oversized string pool entries, cyclic/self-child/shared
  and unrooted node graphs, out-of-range structural handles, out-of-range
  expression jump targets and Filter/Map program handles.
- `crates/plec-ir/src/lib.rs` — snapshot loop-key/depth/size caps, manifest
  route count.
- `crates/plec-compiler/src/read_source_graph.rs` — oversized source file,
  over-deep import chain, chain just within the limit still compiles.
- `crates/plec-runtime/tests/untrusted_input_limits.rs` — oversized and
  over-deep host inputs, artifacts, snapshots, nested JS Maps, undefined
  fields, expression/action loops, self-tail-call actions, self-child and
  unrooted node graphs, deep linear node chains, reaction cycles, and
  oversized/over-deep/structurally excessive snapshot input values plus
  snapshot shape path limits at the WASM boundary.
- `packages/plec-server/src/index.test.ts` — oversized request body (413) and
  oversized artifact file (500).
