# Plec Semantic Cutover Knowledge

## Frontier

Rust: TSX parser + semantic graph -> canonical root component -> reachable `HirApplication` -> per-component HIR -> executable IR 0.9/0.10 components -> JSON -> typed WASM runtime -> DOM.
Proven: scalar state text update; keyed collection row insert/update/move/remove; standalone/row-local conditional lifecycle; static, keyed, and nested direct component prop refresh without remount.
`useCollection("name")` is a named collection HIR input; lowering preserves it as executable input and links its keyed loop.
Slots now HIR/IR/runtime static-mount: `{children}` -> `Slot`; call owns child templates. Next: slot range/lifecycle ownership.

## Ownership

Rust owns parser, semantic graph, component discovery, HIR, scalar/collection/row/conditional lowering, 0.10 direct component calls and zero-arg callable props, and parent state/row-field/prop->component dependency edges. Runtime owns component-instance identity, prop refresh, callback dispatch, named-input fan-out, and orphan disposal.
TypeScript owns production component expansion, rich actions, router, and TanStack integration/delta production.
Runtime owns typed artifact validation, state/delta dispatch, keyed-loop reconciliation, conditional lifecycle, direct listener ownership.

## Proven slices

Rust `Counter`: source -> HIR -> artifact -> WASM mount/click -> state-targeted text mutation; browser fixture asserts one binding/text and text-node identity.
Rust collection rows: source -> input/loop/row conditional artifact -> WASM deltas; browser fixture proves row/text identity on update/move and branch/row removal.
Rust text records point to their binding; runtime writes text with `Text.set_data`, not parent `textContent`.
Standalone conditional: Rust artifact -> WASM state switch; browser fixture proves selected branch, node replacement, parent removal, and inert stale listener.
Static component props: Rust 0.10 artifact -> WASM parent-state update; browser fixture proves child span/text identity survives prop refresh.
Keyed component props: Rust 0.10 artifact -> WASM collection deltas; browser fixture proves child text/row identity survives update/move, local child action remains live, and remove detaches child/listener.
Nested direct component props: Rust 0.10 `App -> Child -> Grandchild` artifact -> WASM mount/state update; browser fixture proves transitive prop refresh keeps section/span/text identity and grandchild local action live.
Keyed callable component props: Rust 0.10 artifact -> WASM child click -> parent action; browser fixture proves parent row capture, child identity across move, and removed child callback inert.
Static slot: Rust HIR/0.10 `Component.children` + callee `Slot` -> WASM anchor mount; browser fixture proves caller `<p>` mounts inside callee `<section>`.

## Capability status

Rust HIR represents reachable acyclic component applications, canonical calls/props, `useCollection("name")`, fragments, conditionals, keyed `ForEach`, loop-item bindings, callable parameters.
Rust lowering executes static slot templates plus prior slices; rejects callable arguments/conditionals. Runtime mounts caller templates via `DocumentFragment` between callee slot anchors. Dynamic slot row/frame/lifecycle ownership not proven.

## Semantic-loss boundaries

Reachable component identity/call props/parameters survive into `HirApplication` and 0.10 IR. Child sinks retain `prop` edges. Runtime-local component anchors connect parent call nodes to child instances; row roots retain component start/end range so keyed move/remove owns child DOM and lifecycle.
Slot template node IDs survive 0.10, but mount currently has no component-call slot range/listener owner or row frame; do not call dynamic/keyed slot disposal correct.

## Contract drift

`plec-ir` serializes Rust artifact; runtime separately deserializes typed 0.9/0.10 schema. Rust/runtime independently define 0.10 value/callable component props; loader decodes/validates 0.10, mounts child runtimes, refreshes values without remount, and carries runtime-only callbacks.
2026-08-24: compiler fixtures 7/7 pass; WASM suite 19/22 passes, all Rust artifacts pass. Known unrelated failures: two route-loader/fetch assertions; one row fetch-frame duplicate-text assertion.

## Runtime invariants

Listeners direct; owners `Static`, keyed `Row`, or `Conditional` with inherited row owner; dispose before region/row/route/runtime removal.
Moves and binding-only row updates retain listener generation; stale owner callbacks inert.
Component-root rows own full start/end anchor range; move/remove must move/remove child DOM between anchors.
Slot ranges need equivalent caller-owned range/listener/frame ownership before keyed or removable slots are enabled.
Text bindings must target text nodes and mutate only data.

## Graph observations

`g-vxnfoy` (`TodosPage`) contains one keyed loop, 3 conditionals (one row-local), 9 row-owned events, 14 rowField->binding and 10 rowField->propProgram edges; keyed row is one lifecycle/dependency/frame contract, not isolated loop syntax. It is legacy 0.9 and flattened: no component-call graph evidence. Static-component ownership is proven by Rust fixture; keyed-component scope remains source/runtime evidence, not graph evidence.
Other local graphs are static or state-only; graph artifacts are current local evidence.

## Source landmarks

Application discovery: `crates/plec-compiler/src/hir_builder.rs::lower_application`; executable lowering: `crates/plec-compiler/src/lowering.rs`; HIR component/input model: `crates/plec-hir/src/{component,node}.rs`; Rust IR: `crates/plec-ir/src/lib.rs`.
Rust vertical fixtures: `crates/plec-compiler/tests/rust_counter_fixture.rs`; runtime artifacts: `packages/plec-runtime/crates/runtime/tests/fixtures/rust-*.json`; browser proof: `typed_events.rs` (`rust_nested_component_fixture_refreshes_grandchild_without_remounting`). Component prototype: `crates/plec-compiler/src/lowering.rs::lower_application_to_executable`; runtime typed schema/mount: `schema/typed.rs`, `typed/runtime.rs`.

## Candidate clusters

1. Complete component slots: call-site range + caller frame + listener disposal + keyed move/remove. Static anchor mount is landed; do not split range/lifecycle.

## Corrections

`Text.binding` is required by runtime typed mounting; Rust emitter now supplies it. Numeric `add` must preserve numbers; runtime VM fixed locally.
Detached test roots make `Node.is_connected()` false; assert parent removal or root selection instead.
2026-08-24: 0.10 preserves scalar/row-field component edges and runtime prop reads; schema rejects invalid component targets/props. Rust keyed-component fixture/browser proof confirms update/move identity, child-local action, and removal disposal.
2026-08-24: 0.10 tags value/callable props; validates parameter kind; `CallProp` queues runtime-only callback descriptors. Keyed callback fixture/browser proof covers parent action dispatch and stale removed child inertness.
