---
name: semantic-ownership-cutover
description: >-
  Maintain and advance Plec's semantic-ownership cutover from the legacy
  TypeScript compiler into the Rust compiler/runtime path. Reuse a persistent
  sibling KNOWLEDGE.md so each agent starts from established repository
  knowledge, validates only what matters for the current frontier, inspects
  generated application graphs for coherent semantic clusters, and plans
  large end-to-end Rust ownership slices without repeatedly rediscovering the
  compiler.
---

# Semantic Ownership Cutover

Use this skill when deciding what Plec semantics should move to Rust next, validating the current cutover frontier, or planning a new ownership slice after previous work has landed.

This is a stateful discovery skill.

The skill directory should contain:

```text
<skill-directory>/
├── SKILL.md
├── KNOWLEDGE.md
├── cutover-contracts.json
└── cutover-progress.mjs
```

`SKILL.md` defines the method.
`KNOWLEDGE.md` is the compact, cumulative repository knowledge produced by previous discovery passes.

The primary objective is to make each invocation continue from established context rather than rediscovering the compiler from scratch.

## Core principle

Cut over semantic ownership, not files, syntax nodes, isolated IR fields, or checklist items.
A good slice moves a connected capability cluster from:

```text
TypeScript-authoritative semantics
        ↓
Rust parser / sema / HIR / lowering / IR
        ↓
existing runtime contract
```

and leaves one fewer semantic decision owned by the TypeScript compiler.
Prefer a larger connected slice when splitting it would require temporary shims, duplicated semantic decisions, lossy translations, throwaway IR shapes, or an immediately superseded intermediate architecture.
Do not expand scope merely because adjacent syntax is easy to support.

## Measurable cutover progress

`cutover-contracts.json` is the versioned, fixed cutover denominator for the
declared deprecation scope. Each weighted contract declares its current semantic
`owner`: `rust` or `typescript`. A TypeScript-owned contract is always a visible
`GAP`, even when nearby Rust probes pass. It may move to `rust` only after source,
HIR, IR, runtime, and browser probes verify a Rust-produced artifact; representation,
snapshots, TypeScript-produced JSON, and agent judgement never earn partial ownership credit.

The score is an ownership score, not a feature-presence score. `100%` means only
that every contract in the manifest is Rust-owned. It means the legacy compiler
can be deprecated only when the manifest's deprecation scope has no
TypeScript-owned contracts and the legacy evidence for each former gap has been
removed or made unreachable. Expand or reweight that scope only through a
`corrections` entry with repository evidence.

Run from repository root:

```text
node .agents/skills/semantic-ownership-cutover/cutover-progress.mjs
node .agents/skills/semantic-ownership-cutover/cutover-progress.mjs --targets route-aware-async-actions
```

The report is authoritative for Rust-owned percentage, cutover remaining,
failed/blocked contracts, and the maximum points a target slice can retire.
The current denominator is frozen. Add, remove, or reweight a contract only with
a `corrections` entry containing repository evidence; never redefine scope to
improve the score. When the user asks whether TypeScript can be deprecated,
include every known TypeScript-authoritative semantic boundary in this scope.

## Endgame mode

When the report leaves only a few TypeScript-authoritative ownership boundaries,
recommend one capstone rather than a menu of small slices. Combine adjacent work
when it shares action frames, async continuation/result ownership, route or
structural disposal, identity, dependency representation, or an HIR-to-IR loss
boundary. Reject a split if the next slice would replace its IR, frame,
lifecycle, validation contract, or prevent source-to-browser proof.

Investigate failed contracts and their declared prerequisites first. The next
frontier is the highest-weight unowned dependency-ready contract or capstone,
not a fresh capability inventory.

## Persistent knowledge protocol

KNOWLEDGE.md is mandatory working state
Before broad repository discovery, read the sibling `KNOWLEDGE.md`.
If it exists, use it as the starting model of the codebase.
If it does not exist, create it as part of the first discovery pass.
Do not begin by independently inventorying every Plec capability if the knowledge file already contains that inventory.
The knowledge file exists specifically to avoid repeated work across agents.
Treat knowledge entries as:
established context when unchanged and unrelated to the current cutover;
revalidation targets when they directly affect the current candidate;
stale hypotheses when repository evidence contradicts them.
Do not blindly trust `KNOWLEDGE.md`, but do not re-prove stable facts without a reason.
Invoke the caveman skill when writing knowledge
Whenever creating or materially updating `KNOWLEDGE.md`, invoke the caveman skill to compress the discovered state into the smallest durable representation that preserves future reasoning value.
Use caveman for:
first creation of `KNOWLEDGE.md`;
consolidation after a substantial discovery pass;
removal of duplicated or superseded facts;
compression when the file is becoming verbose;
turning raw findings into terse reusable state.
The knowledge file should optimize for future agent context cost, not narrative readability.
Prefer dense factual statements over prose.
Do not copy investigation logs, command output, speculative reasoning, or full implementation plans into `KNOWLEDGE.md`.
Incremental validation rule
On each invocation:
Read `KNOWLEDGE.md`.
Identify what changed since the last recorded frontier.
Validate only the facts needed to understand those changes and select the next ownership chunk.
Inspect additional code only when the existing knowledge is missing, stale, contradictory, or insufficient.
Update `KNOWLEDGE.md` with newly established durable facts.
Use caveman to keep the resulting state compact.
The default behavior is incremental discovery, not full rediscovery.
What belongs in KNOWLEDGE.md
Maintain compact sections for durable cutover state.
Recommended structure:

```markdown
# Plec Semantic Cutover Knowledge

## Frontier

## Ownership

## Proven slices

## Capability status

## Semantic-loss boundaries

## Contract drift

## Runtime invariants

## Graph observations

## Source landmarks

## Candidate clusters

## Corrections
```

Keep sections terse.
Frontier
Record the currently proven authoritative pipeline, for example:

```text
source
→ parser
→ semantic graph
→ HIR
→ executable IR
→ runtime load
→ runtime execution
→ DOM
```

Include only the boundaries that matter to semantic ownership.
Ownership
Record which system currently owns each important semantic decision:
Rust compiler;
TypeScript compiler;
shared/duplicated contract;
runtime.
Do not record implementation trivia unless it changes ownership.
Proven slices
Record only end-to-end Rust-owned slices that have actually been demonstrated.
Examples:

```text
scalar state → event → targeted text mutation: proven
conditional branch lifecycle: not proven
component input propagation: not proven
```

Capability status
Track capabilities compactly using the following meanings:
represented — concept exists structurally;
lowered — Rust can emit executable representation;
preserved — identity/dependencies required downstream survive lowering;
executed — runtime behavior exists;
proven — a Rust-produced artifact demonstrates the behavior end to end.
Do not equate representation with ownership.
Do not reproduce a full capability matrix on every invocation. Update only changed rows or rows relevant to the current frontier.
Semantic-loss boundaries
Record places where information is currently discarded or cannot cross a boundary.
Examples:

```text
component identity survives HIR but component calls cannot lower
loop source lowers but lacks runtime input identity
dependency edges preserve state dependencies only
```

These are particularly valuable because they often define future semantic clusters.
Contract drift
Record independently maintained schemas, duplicated semantics, adapters, or compatibility boundaries that can drift.
Only retain drift that affects correctness or future cutovers.
Runtime invariants
Record runtime requirements discovered while proving slices, especially:
identity;
ownership;
dependency targeting;
disposal;
frame/capture behavior;
input identity;
mount/remount rules.
These invariants should prevent future agents from rediscovering runtime expectations by reading the same execution code repeatedly.
Graph observations
Record durable conclusions extracted from generated application graphs, not raw graph statistics.
Good:

```text
keyed loop nodes carry row-scoped event ownership and targeted delta paths
branch nodes own removable descendants and listeners
component inputs feed dependency edges across component instance boundaries
```

Bad:

```text
graph X has 412 nodes
graph Y contains 18 loops
```

Counts belong only when they materially support prioritization.
Source landmarks
Record stable source locations that make future revalidation cheap.
Prefer file-level or symbol-level landmarks.
Avoid line numbers unless they are genuinely useful; they become stale quickly.
Candidate clusters
Keep only plausible next ownership clusters and the dependency between them.
Remove candidates once completed, invalidated, or superseded.
Corrections
Use only for important previously-held assumptions that were wrong and likely to recur.
Do not preserve a historical changelog.
Repository evidence hierarchy
Use evidence in this order:
`KNOWLEDGE.md` for already-established context.
Recent code changes and tests around the current frontier.
Relevant Rust compiler/runtime source.
Relevant TypeScript compiler source where semantic authority remains.
Generated application graphs.
Broader repository inspection only when needed.
The repository remains authoritative.
`KNOWLEDGE.md` is a cache of validated architectural knowledge, not a substitute for source.
Generated graph evidence
Use graphs under:

```text
apps/fullstack/dist/public/graphs/
```

to identify large, naturally connected semantic ownership chunks.
This directory is git ignored. Its contents are local evidence, not canonical artifacts.
If it is absent or stale, record that fact and continue using source/tests.
Do not treat missing graphs as a reason to rediscover the entire compiler.
Graph inspection should answer semantic questions
Inspect representative graphs to discover:
node kinds that form one runtime execution path;
dependency-edge families that travel together;
identity fields consumed together;
ownership scopes such as branch, loop, component, route, or query;
state/input/row/host dependencies feeding the same mutation target;
event/action/callable relationships;
mount/dispose/remount relationships;
metadata the runtime consumes but Rust currently drops;
real subgraphs showing where splitting a capability would create an artificial boundary.
Prefer automated graph inspection where useful.
Useful analyses include:
node-kind and edge-kind inventories;
co-occurrence around structural nodes;
inbound/outbound dependency patterns;
ownership IDs and consumers;
smallest representative connected subgraph for a candidate capability;
comparison between Rust-emittable graphs and TypeScript-produced graphs.
Do not store raw inspection dumps in `KNOWLEDGE.md`.
Store only the semantic conclusion.
Discovery procedure

1. Load existing cutover context
   Read `KNOWLEDGE.md` first.
   Summarize internally:
   what Rust already owns end to end;
   what was most recently proven;
   known information-loss boundaries;
   known runtime invariants;
   unresolved candidate semantic clusters;
   known contract drift;
   relevant graph observations.
   Do not start by rebuilding an exhaustive capability inventory.
   If the knowledge file is missing, bootstrap it once using repository evidence and caveman compression.
2. Establish what changed
   Inspect the changes since the recorded frontier.
   Use recent commits, diffs, tests, or user-provided implementation summaries where available.
   Ask:

```text
What became newly represented?
What became newly lowered?
What identity/dependency information is now preserved?
What newly executes?
What became proven end to end?
What old loss boundary disappeared?
What new loss boundary is now exposed?
```

Update only affected knowledge. 3. Revalidate the active frontier
Write the current authoritative path in concrete terms:

```text
source
→ parser
→ semantic graph
→ HIR
→ executable IR
→ runtime load
→ runtime execution
→ DOM
```

For the semantics relevant to the next cutover, identify who owns each decision.
Revalidate source only where the answer matters to candidate selection.
Do not re-open unrelated capability implementations merely to reconfirm old facts. 4. Inspect unresolved semantic-loss boundaries
Start from known loss boundaries in `KNOWLEDGE.md`.
These are usually more useful than a generic "missing features" list.
For each relevant boundary determine:
what semantic information exists before the boundary;
what is discarded, rejected, duplicated, or reconstructed;
what runtime behavior depends on it;
what adjacent semantics share the same identity, ownership, dependency, or lifecycle machinery.
This is the primary input to candidate construction. 5. Inspect real graph shapes
Use generated graphs to test whether candidate boundaries correspond to real application execution structures.
Inspect multiple representative graphs when available.
Ask:

> What semantics form one indivisible runtime contract in practice?
> Use graph evidence to enlarge, merge, split, or reject candidate chunks.
> Do not choose work solely because a graph node kind is common.
> A rare construct may be architecturally foundational.

6. Build candidate semantic clusters
   A candidate chunk should establish one coherent execution model.
   Examples:

```text
conditional expression
+ branch identity
+ branch dependencies
+ structural mount/unmount
+ branch-owned listener/node disposal
```

```text
component call
+ component identity
+ prop/input slots
+ parameter binding
+ cross-component dependencies
+ component-owned lifecycle
```

```text
keyed loop
+ loop input identity
+ row/key/index bindings
+ row-owned events
+ targeted row delta handling
+ stale-row disposal
```

Avoid candidates shaped like:

```text
add three missing IR fields
```

unless those fields complete a semantic ownership boundary. 7. Choose chunk size using shared invariants
Prefer combining semantics when they share:
identity model;
ownership/lifecycle boundary;
dependency representation;
runtime delta path;
parameter/frame model;
HIR→IR information-loss boundary.
A useful heuristic:

> If the first slice introduces a representation that the next slice must immediately replace, combine them.
> Split semantics when each slice leaves behind a stable, independently useful execution contract.

8. Select and score the capstone
   Evaluate candidates against:
   Semantic coherence
   Does the slice establish one understandable execution model?
   Runtime completeness
   Can the runtime execute the Rust-produced form without TypeScript making semantic decisions on Rust's behalf?
   Information preservation
   Do required identities, dependencies, ownership relations, frames, and inputs survive source → HIR → IR?
   Architectural leverage
   Does the slice establish machinery reused by later cutovers?
   Real-application relevance
   Do generated graphs confirm this cluster exists as a coherent runtime shape?
   End-to-end provability
   Can one focused source fixture prove the entire cluster through the actual runtime?
   Temporary architecture cost
   Would cutting it smaller require throwaway schema, compatibility logic, duplicated lowering, or knowingly incomplete semantics?
   Prefer high coherence, leverage, and provability over minimum line count.
Run `cutover-progress.mjs` before selection. Name manifest target IDs, current
remaining percentage, planned percentage-point delta, and projected remaining
percentage. Treat `GAP` as an unowned candidate, not as a probe regression.
In endgame mode choose one capstone; mention an alternative only when it is a
concrete blocker.
9. Trace the selected contract end to end
   For the preferred candidate, trace every required semantic invariant through:

```text
source syntax
→ semantic symbols/bindings
→ HIR
→ lowering
→ plec-ir
→ serialization
→ runtime decoding/validation
→ runtime execution
→ observable behavior
```

Call out any boundary where Rust and runtime independently encode the same contract.
Do not redesign the entire IR unless the duplicated contract blocks safe progress. 10. Define the vertical acceptance fixture
Every ownership cutover must end in at least one real Rust-produced artifact executing through the runtime.
The fixture should be the smallest program that exercises the whole semantic cluster.
Assert Plec semantics, not merely final visible output.
Examples:
same DOM node is updated rather than remounted;
inactive branch-owned nodes/listeners are disposed;
keyed row identity survives reorder/update;
only the affected row receives a delta;
component input changes target the correct component instance;
captured loop/event frame refers to the correct row;
route replacement disposes previous route-owned resources.
Where useful, assert emitted runtime/DOM operations in addition to final DOM state. 11. Define explicit deferrals
List adjacent semantics intentionally left TypeScript-authoritative.
A deferral is valid when it has an independent semantic boundary.
A deferral is suspicious when the selected slice cannot be correct without emulating or duplicating it. 12. Persist and measure the new state
Before finishing:
Compare findings with `KNOWLEDGE.md`.
Update only durable new facts and changed facts.
Remove or replace superseded facts.
Invoke caveman to compress the result.
Ensure the next agent can understand the new frontier without repeating this discovery pass.
Run target probes and `cutover-progress.mjs` again. A cutover is incomplete
until its contract owner changes from `typescript` to `rust` through passing
probes, its legacy evidence is no longer authoritative, and persistent knowledge
records only durable baseline/result, not raw logs.
Architectural guardrails
HIR must remain semantic rather than a mirror of runtime IR.
Do not make HIR depend on executable IR details solely to ease lowering.
Preserve canonical component/binding/node identities until the last stage that needs them.
Do not reconstruct information in lowering that was available earlier but discarded.
Avoid compatibility logic that lets Rust emit semantically incomplete artifacts which TypeScript repairs later.
Treat dependency edges as executable semantics, not optimization metadata.
Treat ownership/lifecycle metadata as correctness requirements when nodes, listeners, async work, branches, rows, or components can disappear.
Prefer failing during Rust lowering/validation over emitting an artifact whose semantics are ambiguous.
Keep the legacy compiler available as a reference during cutover, but do not make new Rust semantics depend on it at runtime.
IR/runtime drift check
Because `plec-ir` and runtime types may independently describe the same artifact version, every relevant slice must verify:

```text
Rust IR
→ serialize
→ runtime decode/validate
```

Prefer tests that consume the exact artifact produced by the Rust compiler fixture rather than manually maintained duplicate JSON.
When duplicated schema becomes the dominant source of friction or correctness risk, recommend consolidation as its own architectural slice.
Do not recommend consolidation merely for cleanliness while substantive semantic cutover remains unblocked.
Testing expectations
For the selected chunk, identify the minimum useful test pyramid:
focused HIR tests for semantic preservation;
lowering/IR tests for executable representation;
serialization/runtime compatibility;
native runtime execution tests where useful;
WASM/browser E2E proving the Rust-produced artifact;
regression tests for existing semantics affected by the ownership change.
Do not call a cutover complete because snapshots pass.
Existing unrelated failures should be recorded separately.
Required output
Return a concise discovery report.
Current frontier
State what changed since `KNOWLEDGE.md`, what Rust now owns end to end, and where TypeScript remains authoritative.
Do not repeat unchanged capability inventory unless required for the recommendation.
Graph evidence
Summarize only graph observations relevant to the candidate chunks.
Name representative artifacts when useful.
If graphs are absent or stale, say so.
Candidate chunks
Name selected manifest target IDs and only blockers that could prevent the
capstone. Explain semantic boundary, current information-loss boundary, shared
runtime invariants, architectural leverage, and why it cannot be split.
Recommended cutover
Choose one candidate and state the semantic ownership boundary that moves to Rust.
Include computed current remaining percentage, planned percentage-point delta,
and projected post-slice remaining percentage. Do not default to the smallest patch.
Contract changes
List required HIR, IR, serialization, runtime, and validation changes as concepts and invariants.
Avoid speculative file-by-file implementation plans.
Acceptance fixture
Describe one source-level fixture and the exact behavior, identity, delta, ownership, frame, or lifecycle assertions that prove the cutover.
Explicit deferrals
List adjacent semantics that remain TypeScript-authoritative after the slice.
Knowledge update
State the durable facts added, changed, or removed from `KNOWLEDGE.md`.
Do not dump the file contents unless requested.
Score report
Include owned/total weight, remaining percentage, target IDs, planned delta,
and each failed, blocked, or TypeScript-owned `GAP` contract. State whether the
score is sufficient for legacy-compiler deprecation. Do not estimate these values.
Completion condition
Finish with:

```text
After this slice, Rust is authoritative for <semantic cluster> from source through runtime execution; TypeScript no longer decides <specific semantics> for that cluster.
```

What not to do
Do not:
rediscover the full capability surface on every invocation;
regenerate a complete capability matrix when only a few rows changed;
scan the entire repository before reading `KNOWLEDGE.md`;
preserve raw discovery logs in persistent state;
let `KNOWLEDGE.md` become a narrative changelog;
choose work solely because it has few files or few lines;
recommend one IR field at a time without a semantic boundary;
infer runtime behavior from type names without reading relevant execution code;
infer graph semantics from one artifact when several are available;
treat generated graphs as canonical source;
add broad feature parity unrelated to the selected semantic cluster;
introduce generalized abstractions before the selected vertical slice demonstrates the need;
silently preserve TypeScript semantic authority behind a Rust facade;
call a cutover complete without a Rust-produced runtime/browser execution test.
Working style
Be cumulative first, investigative second, prescriptive third.
Start from established knowledge.
Spend discovery effort only where the frontier changed or the next ownership decision is uncertain.
Follow identities, dependencies, ownership, and lifecycle across layers.
Leave the next agent with more durable context and less rediscovery work than this invocation required.
The desired result is a sequence of cutovers where each step leaves Plec with a larger coherent Rust-owned language and a smaller, denser, more accurate persistent knowledge state.
