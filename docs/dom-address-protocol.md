# The Structural DOM Address Protocol

> Authoritative reference for Plec's structural DOM identity. Every element
> address and structural boundary marker emitted anywhere in the system —
> server or client — follows this document. The SSR audit in
> [ssr-architecture.md](./ssr-architecture.md) describes how the protocol is
> consumed by adoption; this document _defines_ the grammar.
>
> Decision of record: 2026-09-03 (wasm-runtime-ixk.2) — one canonical
> path-qualified structural address grammar across SSR and CSR.

## Principle

A **structural address** identifies a position in the _execution structure_
of an application graph: which route outlet, which component call, which
loop row, which graph node. Addresses are derived exclusively from:

1. graph topology (node handles, component calls, outlets), and
2. execution structure (route chain, loop row keys).

They must **never** encode runtime allocation counters (instance ids,
generation numbers, mount order) and must **never** depend on DOM position
(query order, sibling index). Two executions of the same graph with the same
row keys produce byte-identical addresses, regardless of which side
(server renderer or WASM runtime) created the DOM.

Structural identity is deliberately separate from _runtime allocation
identity_ (component instance keys, `TypedRow` generations,
`data-runtime-row-key` attributes). Merging the two is a non-goal: an
address names a position, not an instance.

## Canonical grammar

Paths compose by descent, mirroring the graph walk both renderers perform:

```
root                    → root graph instance
{path}/outlet:{id}      → route graph mounted in outlet {id}
{path}/component:{i}    → component instance created by call node {i}
{path}/loop:{i}/key:{k} → one row of loop node {i}, keyed {k}
```

Row keys `{k}` are escaped exactly like the server renderer's
`escapeInstanceSegment`: `%` → `%25` first, then `/` → `%2F`.

| DOM construct      | Address / marker                                                                                                       | Emitted by      |
| ------------------ | ---------------------------------------------------------------------------------------------------------------------- | --------------- |
| Element            | attribute `data-plec-node="{path}/node:{i}"`                                                                           | SSR **and** CSR |
| Text (server side) | comment `<!--plec:text:{path}:{i}-->` immediately before the text                                                      | SSR only¹       |
| Conditional region | `<!--plec:conditional:{path}:{i}-->` … `<!--plec:conditional-end:{path}:{i}-->`                                        | SSR **and** CSR |
| Component call     | `<!--plec:component:{path}:{i}-->` … `<!--plec:component-end:{path}:{i}-->`                                            | SSR **and** CSR |
| Slot               | `<!--plec:slot:{path}:{i}-->` … `<!--plec:slot-end:{path}:{i}-->`                                                      | SSR **and** CSR |
| Loop row           | `<!--plec:loop:{rowPath}-->` … `<!--plec:loop-end:{rowPath}-->` per row, where `{rowPath}` = `{path}/loop:{i}/key:{k}` | SSR **and** CSR |

¹ CSR-created text nodes carry no marker: the runtime owns them by direct
reference, and text is never a claim boundary. Text markers exist only
because server text cannot carry attributes; the marker-before-text
adjacency contract is specified in wasm-runtime-ixk.6.

There is deliberately **no loop-position anchor**. The server emits none;
rows are self-delimiting regions. A bare `plec:loop:{index}` comment is a
retired competitor grammar, not a valid address.

## Uniqueness and ownership

- **Uniqueness**: within one live DOM tree, every structural address names
  at most one node. The `TypedAdoptionIndex` enforces this for claims: a
  repeated marker fails the whole adoption (`duplicate:ssr-marker:*`),
  never last-wins.
- **Ownership of emission**: only a runtime executing a graph (server
  renderer, WASM typed runtime) may emit `plec:*` markers or
  `data-plec-node` attributes for the graph positions it creates.
  Application code must not synthesize them.
- **Address derivation**: CSR emission derives addresses from the instance
  path recorded at mount time (`root`, `{path}/outlet:{id}` for route
  children, `{path}/component:{i}` from the parent's emission) plus the
  row prefixes recorded at loop instantiation (`{path}/loop:{i}/key:`).
  No counter participates.
- **Provenance is not observable from the grammar** — by design. Whether a
  node was server- or client-created is a property of the adoption
  lifecycle (below), never of a different marker scheme.

## Reserved DOM metadata namespace

The attributes and comment grammar this protocol emits are **reserved**:
application code must never author them. A user-written
`data-plec-node="x"` reaches adoption as a duplicate structural marker and
fails the whole page closed (`duplicate:ssr-marker:*`) — a downstream
failure for what is really an authoring mistake. Ownership comments
(`<!--plec:…-->`) cannot collide with attributes, but the `plec:*` grammar
is reserved anyway so no future attribute form can split the namespace.

| Reservation        | Scope            | Covers                                                                         |
| ------------------ | ---------------- | ------------------------------------------------------------------------------ |
| `data-plec-*`      | **permanent**    | `data-plec-node`, `data-plec-spread-keys`                                      |
| `plec:*`           | **permanent**    | boundary comments (`plec:text/conditional/component/slot/loop`) and any future `plec:`-prefixed name |
| `data-runtime-*`   | **migration window only** | `data-runtime-row-key`, `data-runtime-action`, `data-runtime-event`, `data-runtime-field`, and the legacy `data-runtime-node` |

`data-runtime-node` is excluded from the protocol (see the retired-grammars
table above) but stays reserved until its legacy consumers are removed by
wasm-runtime-ixk.7; after that the `data-runtime-*` reservation shrinks to
the typed runtime surface still in use. `data-plec-*` and `plec:*` never
shrink. Plain `data-*` attributes remain available to applications.

### Enforcement

- **Compiler (authoring time)**: a literal JSX attribute whose name matches
  the reservation — on intrinsic elements and component calls alike — is a
  compile error naming the attribute (`Reserved Plec DOM attribute '…'`).
- **Server renderer (serialize time backstop)**: attribute writes whose
  names are not statically visible — spread bags — are checked where they
  are serialized. A reserved name fails the document render closed with
  `RESERVED_ATTRIBUTE:{name}` (HTTP 500), never a silently degraded page.
- **CSR runtime**: not enforced at runtime (deliberate — the runtime does
  not police graph IR). The compile-time and serialize-time checks above are
  the contract boundary.

## Adoption lifecycle invariant (one-shot)

Adoption claims server-rendered DOM **at most once per application
lifetime**, and only while no client-rendered DOM exists in the claim
scope. This is an explicit, checked precondition — `start_adopt_snapshot`
fails closed with `invariant:adoption-once` when a typed instance forest is
already live — never an implied consequence of call order or of
distinguishing marker schemes.

Rationale: streamed/progressive adoption, subtree adoption, partial
recovery, re-adoption, and marker-based ownership tooling would otherwise
blend server-created and client-created DOM. The grammar alone cannot
prevent that (one grammar means no provenance markers), so the lifecycle
must.

## Retired grammars and consumer classification

`data-runtime-node` is **not** part of this protocol. Historical consumers
are classified:

| Consumer                                                              | Classification          | Disposition                                                                    |
| --------------------------------------------------------------------- | ----------------------- | ------------------------------------------------------------------------------ |
| `packages/plec-browser` `outlet()`                                    | structural graph lookup | migrated: canonical `data-plec-node="root/node:{node}"` reference, root-scoped |
| `packages/plec-browser` `mountIslands()` placeholder lookup           | host/island bridging    | legacy (no IR producer exists); root-scoped; isolated by wasm-runtime-ixk.7    |
| `crates/plec-runtime/src/dom/instantiate.rs` (string-id renderer)     | legacy 0.8/0.9 scheme   | isolated by wasm-runtime-ixk.7                                                 |
| `crates/plec-runtime/src/runtime/lifecycle.rs` `adopt()` (string ids) | legacy 0.8/0.9 scheme   | isolated by wasm-runtime-ixk.7                                                 |
| `crates/plec-runtime/src/runtime/deltas.rs` conditional markers       | legacy 0.8/0.9 scheme   | isolated by wasm-runtime-ixk.7                                                 |

Legacy queries are always scoped to their owning root; no consumer may
resolve markers through document-global first-match `querySelector`.

## Diagnostics

Adoption diagnostics keep their contract (see the SSR audit's table):
`missing:ssr-*`, `mismatch:ssr-*`, `duplicate:ssr-*`, `detached:ssr-text`.
The protocol adds:

| Code                              | Meaning                                                                                                                                             |
| --------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- |
| `invariant:adoption-once`         | Adoption attempted while typed DOM is already materialized (one-shot lifecycle invariant).                                                          |
| `address:loop-row-prefix-missing` | A delta row insert found no recorded structural prefix for its loop — the loop was never instantiated or adopted through the protocol. Fail-closed. |

## Related contracts

- Reserved attribute namespace (`data-plec-*`, `data-runtime-*`, `plec:*`):
  defined and enforced above ("Reserved DOM metadata namespace").
- Text-marker adjacency: wasm-runtime-ixk.6.
- Legacy 0.8/0.9 marker/adoption isolation: wasm-runtime-ixk.7.
