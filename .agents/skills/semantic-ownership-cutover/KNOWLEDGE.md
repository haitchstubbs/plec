# Plec Semantic Cutover Knowledge

Target: Complete reliance on IR0.10, with 0.9 & prior being completely deprecated
End State: Typescript plec should basically be deletable

## Frontier

Rust: TSX parser + semantic graph -> canonical root component -> reachable `HirApplication` -> per-component HIR -> executable IR 0.9/0.10 components/actions -> JSON -> typed WASM runtime -> DOM.
Proven: scalar state text update; keyed collection row insert/update/move/remove; standalone/row-local conditional lifecycle; static, keyed, and nested direct component prop refresh without remount.
`useCollection("name")` is a named collection HIR input; lowering preserves it as executable input and links its keyed loop.
Slots: Rust artifact -> keyed caller template -> callee `Slot` -> WASM DOM; row text/conditional/event retain caller ownership through update/move/remove.
Next: callable component-prop arguments need cross-instance argument/frame ownership; keep distinct from local calls.

## Ownership

Rust owns parser, semantic graph, component discovery, HIR, scalar/collection/row/conditional lowering, direct calls, parameterized local action frames/calls, implicit slot validity/lowering, zero-arg callable props, and parent state/row-field/prop->component dependency edges. Runtime owns component-instance identity, local action-frame execution, slot row context, prop refresh, callback dispatch, named-input fan-out, and orphan disposal.
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
Keyed slot: Rust 0.10 artifact -> WASM caller button/text/conditional inside callee `<li>`; update/move retain identities, remove detaches slot and stale callback inert.
Parameterized local action: Rust 0.10 artifact -> WASM `LoadFrame`/`Call`; scalar update retains text identity and keyed row argument retains row/button identity through move.

## Capability status

Rust HIR represents reachable acyclic component applications, canonical calls/props, `useCollection("name")`, fragments, conditionals, keyed `ForEach`, loop-item bindings, callable parameters.
Rust lowers one implicit `{children}` slot; slotless child calls fail. Runtime validates one target slot, mounts templates with keyed row context, merges nodes/conditionals/listeners into caller row. Rust fixture/WASM proof complete.
Rust lowers callable parameters into deterministic action frame slots and direct local calls into typed `Call` argument programs; runtime schema/VM validates and executes this contract.

## Semantic-loss boundaries

Reachable component identity/call props/parameters survive into `HirApplication` and 0.10 IR. Child sinks retain `prop` edges. Runtime-local component anchors connect parent call nodes to child instances; row roots retain component start/end range so keyed move/remove owns child DOM and lifecycle.
Resolved: slot template IDs, row frame, listener ownership, and conditional regions survive caller -> callee anchors. Calls with children and no target `Slot` fail lowering/validation.
Resolved: local callable parameter identity survives HIR -> 0.10 action frame -> runtime `LoadFrame`. Still lost: callback arguments at child `CallProp` -> parent action frame boundary.

## Contract drift

`plec-ir` serializes Rust artifact; runtime separately deserializes typed 0.9/0.10 schema. Rust/runtime independently define 0.10 value/callable component props; loader decodes/validates 0.10, mounts child runtimes, refreshes values without remount, and carries runtime-only callbacks.
Runtime schema now requires one target `Slot` for component children and bounds child IDs.
2026-08-24: compiler fixtures 8/8 pass; WASM suite 21/24 passes. Known unrelated failures: two route-loader/fetch assertions; one row fetch-frame duplicate-text assertion.

## Runtime invariants

Listeners direct; owners `Static`, keyed `Row`, or `Conditional` with inherited row owner; dispose before region/row/route/runtime removal.
Moves and binding-only row updates retain listener generation; stale owner callbacks inert.
Component-root rows own full start/end anchor range; move/remove must move/remove child DOM between anchors.
Slot templates execute in caller runtime; keyed row context owns bindings, conditional regions, listeners, move/remove lifecycle while callee anchors supply physical DOM range.
Text bindings must target text nodes and mutate only data.

## Graph observations

`g-vxnfoy` (`TodosPage`) contains one keyed loop, 3 conditionals (one row-local), 9 row-owned events, 14 rowField->binding and 10 rowField->propProgram edges; keyed row is one lifecycle/dependency/frame contract, not isolated loop syntax. It is legacy 0.9 and flattened: no component-call graph evidence. Static-component ownership is proven by Rust fixture; keyed-component scope remains source/runtime evidence, not graph evidence.
Other local graphs are static or state-only; graph artifacts are current local evidence.

## Source landmarks

Application discovery: `crates/plec-compiler/src/hir_builder.rs::lower_application`; executable lowering: `crates/plec-compiler/src/lowering.rs`; HIR component/input model: `crates/plec-hir/src/{component,node}.rs`; Rust IR: `crates/plec-ir/src/lib.rs`.
Rust vertical fixtures: `crates/plec-compiler/tests/rust_counter_fixture.rs`; runtime artifacts: `packages/plec-runtime/crates/runtime/tests/fixtures/rust-*.json`; browser proof: `typed_events.rs` (`rust_keyed_slot_fixture_retains_caller_row_identity_and_disposes_slots`). Component prototype: `crates/plec-compiler/src/lowering.rs::lower_application_to_executable`; runtime typed schema/mount: `schema/typed.rs`, `typed/runtime.rs`.
Action-frame lowering: `crates/plec-compiler/src/lowering.rs::{action,local_action_call}`; Rust/WASM fixtures: `rust-local-action-0.10.json`, `rust-keyed-local-action-0.10.json`, `typed_events.rs::rust_keyed_local_action_fixture_retains_row_identity`.

## Candidate clusters

Callable component-prop arguments + cross-instance callback frame/payload ownership; separate from local action frames.

## Corrections

`Text.binding` is required by runtime typed mounting; Rust emitter now supplies it. Numeric `add` must preserve numbers; runtime VM fixed locally.
Detached test roots make `Node.is_connected()` false; assert parent removal or root selection instead.
2026-08-24: 0.10 preserves scalar/row-field component edges and runtime prop reads; schema rejects invalid component targets/props. Rust keyed-component fixture/browser proof confirms update/move identity, child-local action, and removal disposal.
2026-08-24: 0.10 tags value/callable props; validates parameter kind; `CallProp` queues runtime-only callback descriptors. Keyed callback fixture/browser proof covers parent action dispatch and stale removed child inertness.
2026-08-24: static slot WASM test uses hand-authored typed IR; Rust lowering proof is structural only, not end-to-end.
2026-08-24: keyed-slot Rust fixture/browser proof passes; initial row reconciliation defers slot conditionals until component mount, then ordinary row deltas reconcile them.
