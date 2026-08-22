---
name: "discover-crate-capabilities"
description: "Inspect the Plec Rust compiler cutover, map the current source-to-runtime pipeline, identify capability gaps and semantic-loss risks, and recommend one smallest next cutover slice. Discovery only; do not modify files."
---

# Plec Cutover Discovery

Use this skill after each compiler cutover slice.

## Mode

Discovery only.

Do not:

- modify files
- implement code
- create patches
- refactor
- change manifests
- fix tests
- add tests
- generate placeholder implementations

Read the repository, run safe existing tests where useful, and report what actually exists.

## Goal

Determine the current state of the Plec compiler cutover:

```text
source
→ plec-parser
→ plec-sema
→ plec-compiler
→ plec-hir
→ plec-lowering
→ plec-ir
→ runtime
```

Do not assume every stage is complete.

The purpose is to identify the **next smallest meaningful cutover boundary**.

---

## 1. Map the current pipeline

Trace the actual code path from source toward runtime.

For every active boundary identify:

- crate
- file
- entry function
- input representation
- output representation

Show the real current pipeline.

Example:

```text
source
↓ read_source_graph()
ParsedModule
↓ build_semantic_graph()
SemanticGraph
↓ build_hir()
HirComponent
↓ lower_component_to_executable()
ExecutableApplication
```

If a stage is skipped, duplicated, or still handled by TypeScript/runtime code, show that explicitly.

---

## 2. Inspect these crates

Always inspect where present:

- `plec-parser`
- `plec-sema`
- `plec-compiler`
- `plec-hir`
- `plec-lowering`
- `plec-ir`
- `plec-diagnostics`

Also inspect:

- workspace `Cargo.toml`
- crate manifests
- relevant tests
- relevant runtime schema/contracts
- old TypeScript compiler implementation when needed for cutover comparison

Do not inspect unrelated application code unless needed to understand a boundary.

---

## 3. Build a capability matrix

For each meaningful compiler capability classify it:

- `source supported`
- `represented in HIR`
- `lowered to IR`
- `runtime supported`
- `tested end-to-end`
- `missing`
- `intentionally deferred`

Include relevant capabilities such as:

- literals
- arrays/objects
- lexical bindings
- local derived values
- state
- state setters
- actions
- callable references
- inline callables
- action control flow
- events
- text bindings
- props
- component calls
- conditionals
- keyed `ForEach`
- loop-row bindings
- dependency edges
- contexts
- refs
- effects
- host values
- capabilities
- fetch/cookies
- routes/loaders

Only include capabilities supported by repository evidence.

Use a table:

| Capability | Source | HIR | IR  | Runtime | Tests | Notes |
| ---------- | ------ | --- | --- | ------- | ----- | ----- |

---

## 4. Find current rejection boundaries

Search for places where valid HIR/source currently fails.

Identify:

- unsupported HIR variants in `plec-lowering`
- HIR variants that cannot yet be produced from source
- IR variants without a HIR/source producer
- runtime schema features unused by Rust lowering
- source constructs deliberately rejected

For each rejection say whether it belongs to:

```text
HIR expansion
lowering expansion
IR expansion
runtime cutover
```

Do not propose a new representation unless required.

---

## 5. Compare against the previous implementation

Where useful, inspect the old TypeScript compiler/runtime path.

Determine:

- semantics Rust already replaces
- semantics still owned by TypeScript
- semantics present in TypeScript but absent in Rust
- semantics intentionally dropped or redesigned
- runtime contracts Rust must remain compatible with

Do not blindly port TypeScript architecture.

Treat it as evidence of existing Plec behaviour.

---

## 6. Check semantic preservation

Look specifically for silent information loss.

Examples:

- unresolved identifiers becoming strings/null
- callable bodies being truncated
- component identities reverting to display names
- state relationships inferred from spelling
- unsupported syntax becoming `Empty`
- dependency information being discarded
- AST nodes leaking into HIR
- lowering rereading source/SWC
- IR recreating semantic resolution

Any semantic loss should be called out explicitly.

---

## 7. Check architecture boundaries

Protect these invariants:

```text
plec-compiler → plec-hir

plec-lowering → plec-hir
plec-lowering → plec-ir

plec-hir ↛ plec-ir
plec-ir ↛ plec-hir
```

Also check:

- module resolution remains above HIR
- lexical identity remains semantic, not string-based
- HIR contains program meaning
- lowering contains representation conversion
- IR contains execution contracts
- runtime executes IR rather than rediscovering semantics

Call out concrete violations only.

---

## 8. Inspect tests

Use tests as architectural evidence.

For each important feature determine:

- unit coverage
- cross-module coverage
- lowering coverage
- executable/runtime coverage

Run safe existing targeted tests if useful.

Do not fix failures.

Report failures as evidence.

---

## 9. Determine the cutover frontier

Identify the exact point where the Rust implementation currently stops being authoritative.

Examples:

```text
source → HIR        Rust complete
HIR → IR            partial
IR → runtime        TypeScript/runtime still authoritative
```

or:

```text
source → IR         Rust complete for structural/state subset
runtime execution   next cutover boundary
```

This is the most important finding.

---

## 10. Recommend one next slice

Choose **one** next cutover slice.

It should be:

- small
- end-to-end where possible
- architecturally coherent
- testable
- based on existing semantics
- not blocked by another missing representation

Prefer completing a vertical capability over adding disconnected schema.

Examples:

```text
derived locals → expression IR → dependency edge → executable binding
```

or:

```text
conditional HIR → conditional IR → runtime branch execution
```

Do not produce a giant roadmap.

---

# Output

## Current pipeline

Concise real pipeline with function/crate names.

## Cutover frontier

Where Rust currently stops being authoritative.

## Capability matrix

Current source/HIR/IR/runtime/test coverage.

## Newly completed since last slice

Infer this from code/history where possible.

## Current gaps

Only real gaps supported by code.

Group by:

- HIR
- lowering
- IR
- runtime

## Semantic-loss risks

Anything currently discarded, guessed, or reconstructed.

## Architecture drift

Concrete boundary violations only.

## Next slice

One recommended cutover slice.

Include:

- why this is next
- exact boundary it completes
- what should remain deferred

## Confidence

For uncertain findings use:

- `confirmed`
- `likely`
- `unclear`

Never fill gaps with guesses.

---

# Final rule

Do not implement.

The job is to answer:

> What part of Plec has actually crossed into the new Rust compiler now, where does the cutover frontier sit, and what is the smallest next slice that moves that frontier forward?
