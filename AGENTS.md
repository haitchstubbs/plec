# AGENTS.md

Plec is a compiler/runtime project for authoring full-stack applications in TS/TSX and executing their compiled semantics through a Rust/WASM runtime.

This file defines repository-wide agent invariants. Prefer current code, tests, protocol checks, and `plec workspace` output over historical assumptions or speculative architecture.

## Instruction priority

When instructions conflict, use this order:

1. The current user/orchestrator request.
2. Repository-local instructions closest to the files being changed.
3. This file.
4. Issue text, old plans, comments, and historical documentation.

Never use a stale plan or issue description to override code/tests that establish the current contract.

## Core invariants

- `apps/fullstack` is a Plec application and **must remain React-free**.
  - Do not add `react` or `react-dom` dependencies.
  - Do not import either package there.
  - `check:no-react` enforces this during development, build, test, and typecheck.
- The production compiler authority is Rust under `crates/*`.
- The browser runtime is Rust compiled to WASM (`crates/plec-runtime`).
- Preserve compiler/runtime ownership boundaries. Do not move semantic authority into TypeScript merely because it is easier locally.
- Prefer deterministic, inspectable IR and explicit diagnostics over hidden fallback behavior.
- Preserve targeted updates and DOM identity. Do not solve a local problem by re-rendering broad subtrees unless the architecture explicitly requires it.
- SSR/CSR adoption and their protocol/version contracts are part of the current system. Treat them as compatibility surfaces, not disposable implementation detail.
- Do not add React compatibility, arbitrary JS execution, persistence/sync infrastructure, or a new bundler unless the task explicitly requires it.

## Current architecture

Conceptually:

```text
TS / TSX Plec source
        ↓
Rust parser + semantic analysis
        ↓
HIR
        ↓
Executable Plec IR
        ↓
serialized application artifact
        ↓
Rust/WASM runtime
        ↓
DOM
```

Supporting host/browser/server layers may load artifacts, provide capabilities, route requests, or perform SSR, but they do not own Plec language semantics.

The central architectural property is:

```text
source dependency
        ↓
known IR dependency edge
        ↓
owned runtime region / binding
        ↓
minimal required mutation
```

A change to one value should update only the runtime state, bindings, regions, and DOM nodes that depend on it.

## Repository map

Use the repository itself as the authority if this map drifts.

```text
apps/
  fullstack/              Demo full-stack Plec application; React-free

packages/
  plec/                   Plec authoring/runtime-facing TS APIs
  plec-browser/           Browser glue and graph/artifact loading
  plec-node-runtime/      Node sidecar: imports the app server bundle over a private socket
  plec-e2e/               Canonical Playwright E2E runner
  ui/                     React/shadcn UI package; do not leak into fullstack
  lucide-plec/            Generated Plec icon components

crates/
  plec-parser/            TS/TSX parsing
  plec-model/              Semantic graph / analysis
  plec-hir/               High-level IR
  plec-lowering/          Lowering
  plec-ir/                Executable IR authority
  plec-compiler/          Compiler driver
  plec-diagnostics/       Compiler diagnostics
  plec-runtime/           Rust/WASM runtime
  plec-server/            Native HTTP host (assets/SSR/loaders in Rust); `ApplicationRuntime` sidecar boundary
  plec-cli/               Plec CLI (`build`, `serve`, `workspace`) and `plec workspace` workflows
```

## Server host boundary

The public socket is owned by Rust (`crates/plec-server`). Application
server code lives entirely in the app (e.g. `apps/fullstack/src/server.ts`
exporting `handleRequest`); the framework knows only contracts: the
`handleRequest` export, `plec.toml` -> `dist/plec-server.json`, and the
sidecar protocol. `/api/*` traffic crosses the `ApplicationRuntime` trait
into the Node sidecar (private UDS/loopback+token); loaders, SSR
expressions, and snapshots execute in Rust. E2E runs the same native host and
`dist/server/app.mjs` application artifact used in production.

Do not infer ownership from an old package path. Inspect the current workspace before introducing a new package or crate.

## Compiler rules

Compiler changes should preserve a clear pipeline rather than bypassing layers for convenience.

Prefer:

```text
parse → semantic representation → HIR → executable IR → runtime
```

Rules:

- Keep source syntax and runtime representation separate.
- Represent semantics structurally when the runtime needs to reason about them.
- Prefer stable IDs/handles and deterministic lowering.
- Unsupported constructs should produce precise diagnostics at the earliest layer that can identify the semantic problem.
- Do not silently reinterpret unsupported source as a different semantic construct.
- Do not add TypeScript-side production lowering that duplicates Rust compiler authority.
- Reuse existing HIR/IR concepts before adding parallel representations.
- When changing IR, trace all producers, consumers, snapshots, protocol versions, and fixtures.
- Do not generalize a narrow semantic feature into “general JavaScript support” unless explicitly requested.

### Compiler investigation

Before changing a semantic path, identify at minimum:

1. where the source form is parsed,
2. where its semantic/HIR form is produced,
3. where executable IR is lowered,
4. where the runtime consumes it,
5. which tests assert the contract.

Use `plec workspace trace` before reconstructing this manually with repeated grep pipelines.

## Runtime rules

The runtime owns execution, identity, lifecycle, and targeted mutation of compiled application semantics.

- Preserve stable identity for reused DOM nodes, component instances, keyed rows, conditions, outlets, effects, and other owned regions.
- Disposal must only remove resources owned by the disposed runtime region.
- Prefer direct node/handle ownership over document-wide queries or scans.
- Do not introduce a VDOM or subtree rerender as a shortcut around ownership/reconciliation problems.
- Reject stale callbacks/events using the runtime's ownership/generation model where applicable.
- Keep host-specific capabilities behind explicit host/runtime boundaries.
- Avoid broad runtime APIs. Add the minimum operation required by executable semantics.

If a runtime change affects SSR adoption, markers, snapshots, or protocol constants, treat it as a cross-layer contract change and validate it accordingly.

## SSR and adoption

SSR is implemented and compatibility-sensitive.

- Do not treat SSR as optional cleanup or remove adoption paths while fixing CSR behavior.
- Preserve DOM identity when adopting server-rendered output.
- Protocol/marker/version changes must be deliberate and tested across Rust, browser/server integration, fixtures, and E2E coverage.
- After touching an SSR protocol constant or contract, run:

```bash
plec workspace contract ssr --check
```

- After rebuilding runtime WASM, verify the staged artifact is current before trusting browser/E2E failures:

```bash
plec workspace artifact stale
```

## Browser and server glue

Browser/server TypeScript should adapt the compiled runtime to its host environment, not become a second runtime.

Good responsibilities include:

- artifact loading/staging,
- host capability adapters,
- router/server integration,
- SSR orchestration,
- development diagnostics and tooling.

Avoid:

- duplicating compiler semantics,
- maintaining a parallel DOM ownership model,
- scanning the entire rendered graph to recover information the runtime can own directly,
- creating a second canonical representation of executable application state.

## Dev CLI: prefer encoded workflows

The dev-only `plec workspace` commands encode recurring repository investigation and validation workflows. **Use them before hand-rolled shell reconstruction.**

Install/refresh the dev CLI frontend when needed:

```bash
yarn install:plec-cli:dev
```

Important commands:

```text
plec workspace compile [--profile p] [--features f] [--no-optimize]
plec workspace test wasm [filters] [--failures]
plec workspace test last [--failure <substr>]
plec workspace contract ssr [--check]
plec workspace contract limits [--write]
plec workspace version [--set <semver> | --check]
plec workspace trace <symbol|error-code>
plec workspace impact <symbol|protocol-constant>
plec workspace artifact provenance runtime
plec workspace artifact stale
plec workspace doctor adoption [--html f] [--snapshot f] [--route p]
```

Conventions:

- `plec workspace test wasm` captures a run. Use `plec workspace test last` to inspect it instead of rerunning merely to reread output.
- Run `plec workspace artifact stale` after runtime/WASM rebuilds before trusting browser or E2E results.
- Use `plec workspace contract ssr --check` after changing protocol/version constants.
- If the same investigation is repeatedly reconstructed by hand, consider extending `plec workspace` rather than creating another ad hoc script.

Do not forbid ordinary tools entirely: `rg`, compiler search, and direct file inspection are appropriate for local code reading. The rule is to prefer an existing purpose-built Plec command when it already answers the question.

## Testing

Run the narrowest relevant validation first, then expand when the change crosses boundaries.

### Rust/compiler/runtime

Use package/crate-specific tests while iterating. Add or update deterministic snapshots when changing compiler/HIR/IR output.

Prefer tests that assert semantics and identity, not implementation trivia.

### E2E

`packages/plec-e2e` is the canonical Playwright runner. Playwright owns the fullstack server lifecycle.

Never manually spawn an application server for E2E tests and do not leave a test server running.

From the repository root:

```bash
yarn test:e2e          # fast smoke gate
yarn test:acceptance   # deeper behavioral suites
yarn bench             # benchmark suite
```

Rules:

- Smoke tests stay fast.
- Deep suites belong under acceptance.
- E2E ports come from `.env.devports`; override `E2E_PORT` for parallel work instead of editing shared configuration.
- `reuseExistingServer: false` is intentional. A stale server should fail loudly.

### What to test for runtime changes

Where relevant, assert:

- rendered result,
- DOM/node identity preservation,
- keyed ordering/identity,
- lifecycle/disposal behavior,
- stale event rejection,
- SSR adoption identity,
- protocol compatibility,
- mutation/operation counts when performance semantics are part of the contract.

## Performance and benchmarks

Optimize architecture only after correctness and ownership are proven.

Useful measurements include:

- initial mount/adoption,
- one-value update,
- keyed row insert/update/move/remove,
- navigation,
- DOM operation count,
- runtime WASM size,
- application artifact size,
- generated/host JS size.

The meaningful question is not only “how many milliseconds?” but:

```text
one application change
        ↓
how much work did Plec perform?
```

Do not trade away identity or semantics to improve a synthetic benchmark.

## Dependencies and infrastructure

Prefer an existing focused dependency over custom infrastructure when it preserves Plec's experiment and ownership model.

Before adding a dependency:

1. confirm the repository does not already solve the problem,
2. prefer small, well-scoped libraries,
3. keep semantics in Plec when semantics are the thing being validated,
4. avoid introducing frameworks that take ownership of routing, rendering, state, or compilation by accident.

Do not add foundational persistence/sync/offline infrastructure, a custom binary format, a new bundler, or a generalized plugin system unless directly required by the task.

## Scope discipline

Keep changes narrow.

- Fix the layer that owns the problem.
- Do not opportunistically redesign adjacent systems.
- Do not create speculative interfaces, registries, providers, or abstractions for hypothetical future implementations.
- Prefer obvious ownership and concrete types.
- Comment non-obvious invariants and architectural decisions, not routine code.
- If you discover adjacent work that is real but not required, create a linked issue instead of expanding the current task.

## Multi-agent safety

Assume other agents may be working in the same worktree or nearby files.

- Inspect `git status --short` before making broad edits.
- Do not revert, reset, stash, overwrite, or “clean up” changes you did not create unless explicitly instructed.
- Stay within the claimed issue/task boundary. If another issue owns adjacent work, link it rather than absorbing it.
- Prefer additive/local edits over sweeping rewrites when unrelated working-tree changes exist.
- Do not use destructive Git commands (`reset --hard`, `clean -fd`, forced checkout, history rewrite) without explicit authorization.
- If concurrent changes make a required edit ambiguous, preserve both intents where possible and report the collision at handoff.

## No temporary files (hard rule)

Never create scratch/intermediate files solely to feed another tool: no temporary markdown plans, JSON payload files, throwaway scripts, or repo-local notes.

Pass arguments inline, pipe stdin, or use the tool's batching/interface support.

Files are allowed when they are actual task outputs: source, tests, fixtures, config, docs, benchmark results required by the repository, etc.

## Issue tracking

Issue tracking lives in **GitHub Issues**. Do not create markdown TODO lists or a parallel local issue system.

Rules:

- Task-tracking guidance never grants permission to commit or push.

## Git and sync policy

Default to **conservative** repository mutation.

Unless the current user/orchestrator or an explicit repository profile authorizes it:

- do not commit,
- do not push Git,
- do not rewrite history.

At handoff, report changed files, validation performed, issue status, and any suggested commit/sync commands.

Explicit current instructions always win over generic session-completion guidance.

## Before finishing implementation work

If code changed:

1. run the narrow relevant tests/checks,
2. run broader gates when the change crosses compiler/runtime/browser/server boundaries,
3. verify runtime artifact freshness when WASM changed,
4. update/close the tracking issue if the active workflow calls for it,
5. create linked issues for genuine remaining work rather than hiding TODOs in prose/code,
6. report what changed and what was validated.

Do not claim a test passed unless you ran it in the current worktree/session.
