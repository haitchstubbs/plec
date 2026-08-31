---
name: beads-next
description: Skill to pick up and complete the next unblocked Beads issue with minimal context burn. Use this skill automatically whenever a user vaguely requests work to be done.
---

# Beads Next

## Start

Run:

```bash
bd prime
node .agents/skills/beads-next/scripts/bd-next.mjs --claim --show
```

Treat the issue as the authoritative discovery cache for this slice.

Its:

* context
* repository facts
* design decisions
* ownership/files
* non-goals
* acceptance criteria
* verification commands

are established constraints.

**Do not rediscover facts already recorded in the issue.**

Only inspect source when:

* you need the exact code being changed;
* the issue explicitly leaves a decision unresolved;
* a cited fact no longer matches the repository;
* implementation exposes contradictory evidence.

Prefer exact/ranged reads over whole-file reads. Never broad-search the repository for a fact already cited by the issue without a concrete reason.

### Claim without echoing the issue

```bash
bd update <id> --claim --json | jq -r '.[] | "\(.id) \(.status)"'
```

### Implement directly

Before a non-trivial edit, inspect the exact existing type/fixture/helper being consumed. Prefer one coherent edit over speculative code generation followed by repair.

Before editing, reduce the task mentally to:

```text
acceptance criterion -> code location -> proof/test
```

Do not produce another architecture document or lengthy implementation plan.

Stay within the issue contract.

Do not invent additional invariants, abstractions, cleanup, or future-proofing unless required by:

1. an acceptance criterion;
2. an existing repository invariant;
3. evidence discovered while implementing.

Useful adjacent work should become another Beads issue rather than expanding the current slice.

### Verify only what matters

Run the verification commands specified by the issue.

Fix failures caused by the change.

Do not add broad validation, benchmarking, clippy/lint passes, repository archaeology, or unrelated cleanup unless there is a concrete reason.

### 7. Close compactly

When acceptance criteria are satisfied:

```bash
bd close <id> --reason="<concise evidence of completion>" --json \
  | jq -r '.[] | "\(.id) \(.status)"'
```

Then:

```bash
git status --short
```

Follow the repository's active commit/push policy.

### 8. Stop

Report:

* issue completed;
* important implementation result;
* verification outcome;
* changed files;
* any newly discovered follow-up issue.

Do not automatically begin another issue unless instructed to continue.

## Context discipline

**Beads is compressed context. Preserve that compression.**

Bad:

```text
Bead -> reread repository -> re-prove every fact -> redesign task -> implement
```

Good:

```text
Bead -> inspect changed code -> implement -> verify
```

When the issue already contains the answer, use it.

Avoid re-deriving facts or re-evaluating decisions that are already captured in the issue. This ensures minimal context burn and efficient progress on the next unblocked issue.

## Helpful Bash Commands

### Find information about installed chrome and chromedriver
**browser-info.sh**
```bash
.agents/skills/beads-next/scripts/browser-info.sh
```

### Summarize a fixture JSON
**fixture-summary.sh**
```bash
.agents/skills/beads-next/scripts/fixture-summary.sh \
  crates/plec-runtime/tests/fixtures/rust-nested-component-0.10.json
```

### Plec CLI
```bash
# Usage
plec --help
```

No need to invent cd, filtering, tail, etc.