# Plec Semantic Cutover Knowledge

## Frontier

Rust: TSX parser + semantic graph -> canonical root component -> HIR -> `plec-ir` 0.9 -> JSON -> typed WASM runtime -> DOM.
Proven: scalar state text update; keyed collection row insert/update/move/remove and standalone/row-local conditional lifecycle.
Current worktree: `useCollection("name")` HIR input + executable IR input.

## Ownership

Rust owns parser, semantic graph, component discovery, HIR, scalar/collection/row lowering, and conditional lowering.
TypeScript owns production component expansion, rich actions, router, and TanStack integration/delta production.
Runtime owns typed artifact validation, state/delta dispatch, keyed-loop reconciliation, conditional lifecycle, direct listener ownership.

## Proven slices

Rust `Counter`: source -> HIR -> artifact -> WASM mount/click -> state-targeted text mutation; browser fixture asserts one binding/text and text-node identity.
Rust collection rows: source -> input/loop/row conditional artifact -> WASM deltas; browser fixture proves row/text identity on update/move and branch/row removal.
Rust text records point to their binding; runtime writes text with `Text.set_data`, not parent `textContent`.
Standalone conditional: Rust artifact -> WASM state switch; browser fixture proves selected branch, node replacement, parent removal, and inert stale listener.

## Capability status

Rust HIR represents `useCollection("name")`, component calls/props, fragments, conditionals, keyed `ForEach`, loop-item bindings, callable parameters.
Rust lowering executes collection inputs, keyed rows, named row fields/edges, row-owned inline events, standalone/row-local conditionals, scalar state; rejects components, callable parameters/props, generic action expressions.

## Semantic-loss boundaries

Component call target/props/parameters stop at Rust executable lowering.

## Contract drift

`plec-ir` serializes Rust artifact; runtime separately deserializes typed 0.9 schema. Counter and collection-row fixtures are exact cross-boundary checks.
2026-08-23: compiler fixture tests 3/3 pass; WASM suite 15/18 passes, including all Rust artifacts. Three pre-existing typed-event failures: two route-loader output assertions, one row fetch-frame duplicate text assertion.

## Runtime invariants

Listeners direct; owners `Static`, keyed `Row`, or `Conditional` with inherited row owner; dispose before region/row/route/runtime removal.
Moves and binding-only row updates retain listener generation; stale owner callbacks inert.
Text bindings must target text nodes and mutate only data.

## Graph observations

`g-vxnfoy` (`TodosPage`) contains one keyed loop, 3 conditionals (one row-local), 9 row-owned events, 14 rowField->binding and 10 rowField->propProgram edges; keyed row is one lifecycle/dependency/frame contract, not isolated loop syntax.
Other local graphs are static or state-only; graph artifacts are current local evidence.

## Source landmarks

Rust lowering: `crates/plec-compiler/src/lowering.rs`; HIR nodes: `crates/plec-hir/src/node.rs`; Rust IR: `crates/plec-ir/src/lib.rs`.
Rust vertical fixtures: `crates/plec-compiler/tests/rust_counter_fixture.rs`; runtime artifacts: `packages/plec-runtime/crates/runtime/tests/fixtures/rust-*.json`; browser proof: `typed_events.rs`.

## Candidate clusters

1. Component call: canonical target + input/prop slots + parameter binding + cross-instance dependencies + component lifecycle.

## Corrections

`Text.binding` is required by runtime typed mounting; Rust emitter now supplies it. Numeric `add` must preserve numbers; runtime VM fixed locally.
Detached test roots make `Node.is_connected()` false; assert parent removal or root selection instead.
