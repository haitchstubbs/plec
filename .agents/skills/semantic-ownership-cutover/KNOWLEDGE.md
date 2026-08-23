# Plec Semantic Cutover Knowledge

## Frontier

Rust: TSX parser + semantic graph -> canonical root component -> reachable `HirApplication` -> per-component HIR -> executable IR 0.9/0.10 static components -> JSON -> typed WASM runtime -> DOM.
Proven: scalar state text update; keyed collection row insert/update/move/remove; standalone/row-local conditional lifecycle; static component prop refresh without remount.
`useCollection("name")` is a named collection HIR input; lowering preserves it as executable input and links its keyed loop.
Next frontier: keyed component calls, where row identity/frame must own child mount, prop refresh, and disposal.

## Ownership

Rust owns parser, semantic graph, component discovery, HIR, scalar/collection/row/conditional lowering, static 0.10 component calls, scalar prop lowering, and parent state->component dependency edges. Runtime owns component-instance identity, prop refresh, named-input fan-out, and orphan disposal.
TypeScript owns production component expansion, rich actions, router, and TanStack integration/delta production.
Runtime owns typed artifact validation, state/delta dispatch, keyed-loop reconciliation, conditional lifecycle, direct listener ownership.

## Proven slices

Rust `Counter`: source -> HIR -> artifact -> WASM mount/click -> state-targeted text mutation; browser fixture asserts one binding/text and text-node identity.
Rust collection rows: source -> input/loop/row conditional artifact -> WASM deltas; browser fixture proves row/text identity on update/move and branch/row removal.
Rust text records point to their binding; runtime writes text with `Text.set_data`, not parent `textContent`.
Standalone conditional: Rust artifact -> WASM state switch; browser fixture proves selected branch, node replacement, parent removal, and inert stale listener.
Static component props: Rust 0.10 artifact -> WASM parent-state update; browser fixture proves child span/text identity survives prop refresh.

## Capability status

Rust HIR represents reachable acyclic component applications, canonical calls/props, `useCollection("name")`, fragments, conditionals, keyed `ForEach`, loop-item bindings, callable parameters.
Rust lowering executes collection inputs, keyed rows, named row fields/edges, row-owned inline events, standalone/row-local conditionals, scalar state, static component props, and `LoadProp` in 0.10; preserves `prop` and row-field component edges; rejects callable/direct props and JSX children. Runtime loads 0.10, mounts recursive calls at comment anchors, refreshes prop-dependent sinks, fans named input snapshots/deltas to live instances, and drops detached child instances.

## Semantic-loss boundaries

Reachable component identity/call props/parameters survive into `HirApplication` and 0.10 IR. Child sinks retain `prop` edges. Runtime-local component anchors connect parent call nodes to child instances; state refresh re-evaluates props without remount. Keyed component identity/browser lifecycle proof remains pending.

## Contract drift

`plec-ir` serializes Rust artifact; runtime separately deserializes typed 0.9/0.10 schema. Counter, collection-row, static-conditional, and static-component fixtures are exact cross-boundary checks. Rust/runtime independently define 0.10 components; loader decodes/validates 0.10, mounts child runtimes, and refreshes static scalar props without remount.
2026-08-24: compiler fixture tests 4/4 pass; WASM suite 16/19 passes, including all Rust artifacts. Known unrelated failures: two route-loader/fetch assertions; one row fetch-frame duplicate-text assertion.

## Runtime invariants

Listeners direct; owners `Static`, keyed `Row`, or `Conditional` with inherited row owner; dispose before region/row/route/runtime removal.
Moves and binding-only row updates retain listener generation; stale owner callbacks inert.
Text bindings must target text nodes and mutate only data.

## Graph observations

`g-vxnfoy` (`TodosPage`) contains one keyed loop, 3 conditionals (one row-local), 9 row-owned events, 14 rowField->binding and 10 rowField->propProgram edges; keyed row is one lifecycle/dependency/frame contract, not isolated loop syntax. It is legacy 0.9 and flattened: no component-call graph evidence. Static-component ownership is proven by Rust fixture; keyed-component scope remains source/runtime evidence, not graph evidence.
Other local graphs are static or state-only; graph artifacts are current local evidence.

## Source landmarks

Application discovery: `crates/plec-compiler/src/hir_builder.rs::lower_application`; executable lowering: `crates/plec-compiler/src/lowering.rs`; HIR component/input model: `crates/plec-hir/src/{component,node}.rs`; Rust IR: `crates/plec-ir/src/lib.rs`.
Rust vertical fixtures: `crates/plec-compiler/tests/rust_counter_fixture.rs`; runtime artifacts: `packages/plec-runtime/crates/runtime/tests/fixtures/rust-*.json`; browser proof: `typed_events.rs` (`rust_component_fixture_refreshes_child_without_remounting`). Component prototype: `crates/plec-compiler/src/lowering.rs::lower_application_to_executable`; runtime typed schema/mount: `schema/typed.rs`, `typed/runtime.rs`.

## Candidate clusters

1. Keyed component call: row key/frame + per-row child instance identity + row-prop refresh + stale-row child disposal. Lowering already emits `rowField` component edges; runtime stores row-local call anchors, queues row prop refresh, then culls detached child runtimes. Missing only Rust artifact/browser proof. Do not split key/frame from lifecycle. Acceptance: Rust source fixture with keyed `Child` rows; update/move retain matching child DOM identity, remove culls child/listener, only affected child prop sink mutates.

## Corrections

`Text.binding` is required by runtime typed mounting; Rust emitter now supplies it. Numeric `add` must preserve numbers; runtime VM fixed locally.
Detached test roots make `Node.is_connected()` false; assert parent removal or root selection instead.
2026-08-24: 0.10 preserves scalar prop dependency edges and runtime prop reads; schema rejects invalid component targets/props. Rust fixture/browser proof confirms parent-state prop refresh updates existing child text node without remount. Next proof must add row-scoped component identity and disposal.
