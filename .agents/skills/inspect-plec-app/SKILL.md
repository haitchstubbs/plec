---
name: inspect-plec-app
description: Guide for inspecting Plec compiler graphs and route artifacts using the Plec CLI.
---

# Plec Inspector

Use the Plec CLI to inspect compiler graphs and route artifacts.

## Purpose

Plec compiler artifacts can be large and highly connected. Do not manually dump, grep, traverse, or reconstruct entire compiler graphs when the Plec inspector can answer the question directly.

Prefer targeted graph queries through:

```bash
plec inspect <source> '<query>'
```

The inspector queries the compiled `ComponentApplication` directly in memory. It does not require an HTTP server or external GraphQL service.

Use this skill whenever investigating:

- executable IR
- components
- bindings
- state
- actions
- events
- dependencies
- keyed collections
- component calls
- runtime-facing compiler output
- relationships between compiled entities
- whether a compiler construct was lowered correctly

## Core rule

**Inspect the graph through `plec inspect` before manually reading or traversing serialized graph structures.**

Do not begin graph debugging by:

- dumping large JSON artifacts
- recursively opening generated fixtures
- grepping serialized IR
- writing ad-hoc scripts to traverse compiler output
- reconstructing graph relationships from Rust struct definitions
- reading large snapshots line-by-line

Those approaches are fallback mechanisms, not the normal inspection workflow.

## Commands

### Targeted inspection

```bash
plec inspect <source> '<query>'
```

Example:

```bash
plec inspect apps/fullstack/src/main.tsx '
{
  components {
    id
  }
}'
```

Use this whenever the required information can be expressed as an inspector query.

Prefer the smallest query that answers the current question.

For example, if investigating a component's dependencies, query that component and its dependency fields rather than requesting the entire application.

### Raw executable IR

```bash
plec raw <source>
```

This prints the complete executable `ComponentApplication`.

Use `raw` only when:

1. the inspector schema does not expose the required information;
2. debugging serialization itself;
3. validating the complete emitted artifact;
4. comparing exact executable IR output against a fixture or snapshot.

Do not use `raw` merely because its output is familiar.

### Routes

```bash
plec routes <source>
```

Use this to inspect the application's lowered route manifest.

Do not infer the route manifest by manually tracing route declarations through compiler internals when the command can produce the canonical lowered result.

## Investigation workflow

When debugging compiler behavior:

1. Identify the source entry being investigated.
2. Form a specific question about the compiled graph.
3. Query it with `plec inspect`.
4. Narrow or expand the query based on the result.
5. Trace the returned IDs or relationships with additional targeted queries.
6. Only inspect compiler source once the graph evidence identifies the relevant lowering or semantic boundary.

The intended flow is:

```text
source behaviour
      ↓
plec inspect
      ↓
compiled graph evidence
      ↓
identify incorrect/missing relationship
      ↓
inspect relevant compiler implementation
      ↓
make change
      ↓
plec inspect again
      ↓
tests
```

Avoid this flow:

```text
source behaviour
      ↓
read many compiler files
      ↓
guess graph structure
      ↓
dump large JSON
      ↓
grep generated output
      ↓
infer what probably happened
```

## Query strategy

Treat the inspector as a graph exploration interface.

Start narrow.

Prefer:

```graphql
{
  components {
    id
  }
}
```

over requesting every available field.

Once an interesting entity is identified, query the fields needed to investigate that entity and its edges.

Follow stable compiler IDs where possible rather than matching arbitrary serialized text.

Good queries answer questions such as:

- Which component owns this binding?
- What state does this binding depend on?
- Which action is attached to this event?
- Which component call receives this prop?
- What dependencies cause this component to refresh?
- Which collection owns this keyed loop?
- What executable node did this source construct lower into?
- Is this dependency edge present in executable IR?
- Is this construct absent entirely, or merely connected incorrectly?

Prefer graph relationships over textual coincidence.

## Evidence requirements

When reporting findings from compiler inspection, describe the relevant graph evidence explicitly.

For example:

```text
`TodoRow.title` is present as a binding, but its dependency set contains only
the component prop slot and no row-field dependency. The failure is therefore
in dependency lowering rather than DOM reconciliation.
```

Do not report conclusions such as:

```text
It looks like dependency lowering might be broken.
```

without first obtaining graph evidence where practical.

## Source inspection

The inspector does not replace reading compiler code.

It changes **when** source inspection happens.

Use the graph first to determine:

- what was emitted;
- what is missing;
- which relationship is incorrect;
- which semantic stage likely owns the defect.

Then inspect the smallest relevant compiler/runtime surface.

This prevents broad exploratory reading from replacing evidence-driven debugging.

## After changes

After modifying compiler behavior, rerun the same inspector query that demonstrated the problem.

Confirm that the expected graph shape changed before moving on to broader tests.

Where appropriate:

```text
1. reproduce with `plec inspect`
2. modify compiler
3. verify with `plec inspect`
4. run focused Rust tests
5. run browser/runtime proof if behaviour crosses the runtime boundary
```

Do not treat passing tests alone as proof that the intended graph relationship now exists when it can be directly inspected.

## CLI availability

Assume `plec` is available on `PATH`.

If it is unavailable in the current environment, use the workspace equivalent:

```bash
cargo run -p plec-cli -- inspect <source> '<query>'
```

Likewise:

```bash
cargo run -p plec-cli -- raw <source>
cargo run -p plec-cli -- routes <source>
```

Do not abandon the inspector workflow merely because the globally installed CLI is unavailable.

## Fallback hierarchy

Use the following order:

```text
plec inspect
    ↓
plec routes                 # route-specific questions
    ↓
plec raw                    # exact/full artifact required
    ↓
focused source inspection
    ↓
temporary/ad-hoc tooling    # last resort
```

If manual graph traversal or temporary tooling becomes necessary, first establish why `plec inspect` cannot answer the question.

If the limitation is generally useful, prefer extending `plec-inspect` so future investigations can remain query-driven.

## Design principle

The Plec compiler produces a graph.

Debug it as a graph.

Use the inspector as the canonical human/agent interface to compiler artifacts rather than repeatedly building one-off methods for understanding the same structure.

Protocol, artifact, and test-suite questions are a different domain: use the `plec-dev-cli` skill (`plec dev contract/trace/artifact/test/doctor`) for those.
