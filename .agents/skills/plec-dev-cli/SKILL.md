---
name: plec-dev-cli
description: Use when running or debugging Plec's WASM test suite, SSR protocol/version questions, stale runtime WASM, artifact provenance, adoption health checks, or tracing where a symbol or adoption error code comes from. Teaches the `plec dev` workflow commands that replace hand-rolled grep/tail pipelines.
---

# Plec Dev CLI

`plec dev` is the workspace's own debugging interface. It exists because
sessions repeatedly burned context on: re-running the noisy WASM suite to
re-read failures, reconstructing which protocol version each layer
implements, checking whether the browser served stale WASM, and manually
reconstructing graph registration semantics.

**Core rule: prefer a `plec dev` command over hand-rolled
`grep`/`tail`/`grep -A`/`grep -B` pipelines for these questions.**

## Availability

The workspace's default `.env.plec` builds the release frontend, which has
no `dev` group. For workspace work:

```bash
yarn install:plec-cli:dev
```

Verify with `plec --help` — the `dev` command must be listed. If it is not,
the binary on PATH is the release frontend.

## Command selection

| Question                                           | Command                                              |
| -------------------------------------------------- | ---------------------------------------------------- |
| Which WASM tests failed and why?                   | `plec dev test wasm --failures`                      |
| Re-read the last run's failures without re-running | `plec dev test last [--failure <substr>]`            |
| Do all SSR protocol versions agree?                | `plec dev contract ssr [--check]`                    |
| Where does this symbol/error code come from?       | `plec dev trace <query>`                             |
| Is the built/staged WASM current?                  | `plec dev artifact stale`                            |
| Full artifact identity + protocol report           | `plec dev artifact provenance runtime`               |
| Why is SSR adoption failing?                       | `plec dev doctor adoption [--html f] [--snapshot f]` |

All commands accept `--json`.

## Workflows

### After a WASM test failure

```bash
plec dev test wasm --failures          # run once, parse the noise
plec dev test last --failure <substr>  # re-read without re-running
```

Do not pipe `yarn test:wasm` through `grep -A`/`grep -B`. The capture lives
in `.cache/plec/test-wasm/` and `test last` queries it.

### After touching a protocol constant (`*_VERSION`)

```bash
plec dev contract ssr --check
```

Must pass before running the suites. Intentional legacy fixtures are marked
`~`; anything marked `CONFLICT` is a real propagation miss. Then rebuild the
runtime and verify the built binaries:

```bash
yarn workspace plec-runtime build:wasm
plec dev artifact stale
```

`artifact stale` must pass before trusting browser or e2e results. A stale
binary reports `STALE: implements snapshot protocol N, source is M` — the
"real app still behaves as if v1" failure mode comes from exactly this.

### When SSR adoption fails in the browser/e2e

```bash
plec dev doctor adoption
```

Read the sections in order: Protocol (version disagreement), Graph
resolution (unresolved graph reference — note the `✗ unknown ssr snapshot
graph` rows), Nested execution (what snapshot v2 must record), DOM markers
(pass `--html <file>` with the rendered page or fixture), Artifact
provenance (stale WASM). Follow the printed hints.

### When investigating a symbol or error code

```bash
plec dev trace unsupported:ssr-snapshot-version
plec dev trace missing:ssr-loop
plec dev trace SSR_SNAPSHOT_VERSION
```

Matches are categorized: DEFINED IN / PRODUCED BY / ASSERTED BY / DOCUMENTED
BY, with a RELATED CONTRACT row for known adoption codes. This answers
"where can this come from and which tests expect it?" without opening five
files.

## Extending the CLI

When an investigation reveals a recurring question that none of these
commands answer, extend `crates/plec-cli/src/dev/` rather than solving it ad
hoc again. Deferred command ideas already tracked in beads: `impact`,
`context <domain>`, `verify adoption`, `graph resolve/tree`,
`markers explain/validate`.

## Boundaries

- `dev` is dev-frontend only; never reference `plec dev` from app-facing
  docs or code.
- The commands read workspace state (git, built artifacts, compiled IR);
  they do not start servers. Playwright owns long-running browser processes
  for e2e — never spawn `dist/server.mjs` manually.
- Compiler-graph questions (not protocol/artifact/test questions) still
  belong to the `inspect-plec-app` skill's `plec inspect` workflow.
