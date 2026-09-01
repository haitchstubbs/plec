# AGENTS.md

You are scaffolding an experimental browser application runtime that compiles a constrained React + TanStack DB application into a declarative application graph, then renders that graph through a WASM runtime.

## Core architecture

Target this conceptual pipeline:

```text
React / TanStack source
        │
        ├── TanStack DB queries / collections
        └── JSX view structure
        │
        ↓
Application compiler
        │
        ├── data graph
        ├── view graph
        └── bindings between them
        │
        ↓
Application IR
        │
        ↓
WASM runtime
        │
        ├── reactive update handling
        ├── UI instruction execution
        └── scheduler
        │
        ↓
DOM
```

Do NOT make SQLite/OPFS, replication, streaming, offline sync, service workers, or a custom binary format foundational to the first implementation.

The application IR may initially be JSON.

Persistence can be added later once the compiler/runtime model is proven.

## Repository structure

`apps/fullstack` is a Plec application and must remain React-free. Do not add
`react` or `react-dom` dependencies, or import either package there. Its
`check:no-react` script enforces this at development, build, test, and
typecheck time.

The monorepo is organized roughly like:

```text
apps/
  fullstack/            Demo full-stack Plec application (React-free)

packages/
  plec/                 Framework runtime: jsx-runtime, state hooks, router
  plec-ir/              Shared IR schema (Zod)
  plec-browser/         Browser glue: startPlecRouter, graph loading
  plec-runtime/         Rust -> WASM runtime (crate at crates/plec-runtime)
  ui/                   React/shadcn UI kit
  lucide-plec/          Generated Lucide icon components for Plec

crates/                 Rust compiler workspace (the compiler authority)
  plec-parser/ plec-sema/ plec-hir/ plec-lowering/ plec-ir/
  plec-compiler/        Driver + plec-route-manifest binary
  plec-diagnostics/
```

The build integration described below for `packages/vite-plugin` is
currently realized by `apps/fullstack/scripts/build.mjs` (esbuild + the
`plec-route-manifest` binary); no vite plugin exists. The runtime crate is `crates/plec-runtime`, a member of the root Cargo
workspace alongside `crates/*`.

Responsibilities:

### Compiler (`crates/*` — Rust)

This is one of the two important experimental components.

Responsibilities:

- parse TS/TSX
- identify supported JSX structures
- identify supported TanStack DB query usage
- construct a view graph
- construct bindings from query results/state into the view graph
- emit application IR
- explicitly identify unsupported constructs

Start syntax-first.

Use SWC rather than the TypeScript compiler unless type information is genuinely required.

Initial supported constructs should be deliberately narrow:

- JSX intrinsic elements
- simple function components
- props
- text bindings
- property bindings
- simple conditionals
- `.map()` list rendering
- `useLiveQuery`
- straightforward event handlers
- simple local scalar state if inexpensive

Unsupported or dynamic constructs should fail clearly or be marked as future JS-island candidates.

Do NOT attempt general React compilation.

Do NOT support arbitrary effects, refs, dynamic imports, runtime component lookup, or arbitrary imperative DOM manipulation.

### `packages/plec-runtime`

This is the second important experimental component.

Implement it in Rust and compile it to WASM.

Responsibilities:

- load application IR
- instantiate a view graph
- create DOM elements
- maintain node/binding IDs
- apply targeted DOM mutations
- accept data/query deltas
- update only bindings affected by those deltas

Use:

- `wasm-bindgen`
- `serde`
- `serde-wasm-bindgen`
- `web-sys`
- `js-sys`

The runtime should initially expose a very small API, conceptually similar to:

```ts
loadApplication(ir);
mount(root);
applyDelta(delta);
```

Avoid inventing a broad runtime API.

The key experiment is whether:

```text
one changed tuple
    ↓
known dependency edge
    ↓
one/few DOM mutations
```

can occur without re-rendering an entire component subtree.

### `packages/plec-ir`

Define the shared IR schema.

Use Zod on the TypeScript side.

Keep the IR readable and intentionally boring.

A rough conceptual schema may include:

```text
Application
  components
  elements
  queries
  bindings
  events
  loops
  conditions
  dependencyEdges
```

Possible operations:

```text
CREATE_ELEMENT
CREATE_TEXT
SET_ATTRIBUTE
BIND_TEXT
BIND_ATTRIBUTE
IF
FOR_EACH
CALL_COMPONENT
EVENT
```

Do not prematurely optimize the representation.

JSON is acceptable for the MVP.

### Build integration (currently `apps/fullstack/scripts`)

Provide the integration point.

Use Vite rather than building a custom bundler.

Responsibilities:

- inspect/transform relevant source modules
- invoke the compiler
- emit the application IR
- expose compiler diagnostics during development
- integrate the WASM runtime
- preserve normal Vite development ergonomics as much as possible

Use:

- `vite`
- `@vitejs/plugin-react`
- `@swc/core`
- `magic-string`

The plugin should eventually make enabling the experiment feel approximately like:

```ts
plugins: [react(), experimentalRuntime()];
```

Do not attempt sophisticated HMR initially unless it falls out naturally.

### `packages/plec-browser`

Keep browser-specific glue here.

Use existing packages rather than inventing protocols.

Use `comlink` if Worker RPC is useful.

Workers are optional for the first milestone. A main-thread runtime is acceptable if that produces a faster path to proving the compiler/runtime model.

Do not optimize concurrency prematurely.

## Dependencies

Prefer the following existing packages.

Application:

```text
react
react-dom
@tanstack/react-start
@tanstack/react-router
@tanstack/react-db
@tanstack/db
@tanstack/query-db-collection
```

Compiler/build:

```text
vite
@vitejs/plugin-react
@swc/core
magic-string
zod
```

Optional serialization:

```text
@msgpack/msgpack
```

Do not use MessagePack until JSON size becomes a meaningful problem.

Browser glue:

```text
comlink
idb-keyval
```

`idb-keyval` is optional and should only be used for trivial experiment metadata or caching.

Do not introduce SQLite/OPFS yet unless needed for a clearly defined later milestone.

Testing:

```text
vitest
@vitest/browser
playwright
happy-dom
tinybench
```

Rust testing/benchmarking:

```text
wasm-bindgen-test
insta
criterion
```

## Important architectural principle

TanStack DB already provides a declarative/reactive data model.

Do not reimplement its data semantics prematurely.

The compiler's main job is initially to connect:

```text
TanStack data graph
        ↓
query result
        ↓
view dependency
        ↓
specific DOM binding
```

Think of DOM nodes as sinks in a reactive dependency graph.

For example:

```text
todosCollection
    ↓
filter(done = false)
    ↓
openTodos
    ↓
TodoList loop
    ↓
TodoRow
    ↓
todo.title
    ↓
specific DOM text node
```

If a single todo title changes, the ideal runtime behaviour is to update the corresponding text node directly rather than rerender `TodoList`.

## Compatibility philosophy

The developer-facing source should remain ordinary React/TanStack code.

Do not introduce a new JSX dialect.

Do not require a Rust frontend.

Do not require users to write explicit IR.

The long-term compatibility model may eventually look like:

```text
supported declarative source
    → compiled IR

unsupported dynamic source
    → JS island / compatibility fallback
```

Do not implement JS islands unless needed to keep the demo working.

For this MVP, unsupported constructs may simply produce clear compiler diagnostics.

## Explicit non-goals

Do NOT build any of the following unless necessary to complete the core experiment:

- custom router
- custom bundler
- custom sync engine
- SQLite abstraction
- OPFS abstraction
- replication protocol
- service worker framework
- offline-first framework
- custom binary format
- JS-to-WASM compiler
- full React compatibility
- custom state-management library
- visual devtools
- complex worker scheduler
- production security model
- SSR implementation
- component library

Do not turn this into a general framework project.

## Development/debugging requirements

Compiler output should be inspectable.

Emit something like:

```text
dist/
  application.ir.json
  runtime.wasm
```

The IR should be readable enough to inspect manually.

Add compiler snapshot tests such as:

```ts
expect(compile(source)).toMatchSnapshot();
```

Use `insta` where useful for Rust runtime representations.

Prefer explicit IDs and deterministic output so snapshots remain stable.

## E2E testing

`packages/plec-e2e` is the canonical Playwright runner. Playwright owns the
fullstack server: turbo builds `@wasm-runtime/fullstack`, the `webServer`
config starts `dist/server.mjs`, waits for HTTP readiness, and kills the
process group afterwards.

Never spawn `dist/server.mjs` manually, and never leave a dev server
running for tests. Playwright starts, watches, and stops the server.

Tiers (run from the repo root):

```text
yarn test:e2e          # smoke gate — fast; run this while spiking
yarn test:acceptance   # full behavioral suites — opt-in
yarn bench             # navigation benchmark into benchmarks/results/ — opt-in
```

Rules:

- Specs live in `packages/plec-e2e/tests/{smoke,acceptance,bench}` and are
  named `*.playwright.ts`. New deep suites go under `acceptance/`; keep the
  smoke tier fast.
- The port comes from `E2E_PORT` in `.env.devports` — the canonical port
  registry. For parallel spikes override it instead of editing the file:
  `E2E_PORT=3311 yarn test:e2e`.
- `reuseExistingServer: false` is intentional: a stale server on the port
  must fail the run loudly instead of being silently adopted.

## Benchmark requirements

Create a small benchmark harness comparing the normal implementation and compiled runtime where practical.

Measure at minimum:

- initial mount time
- update of one list row
- addition/removal of one row
- generated JS size
- WASM + IR size

Do not optimize based on synthetic benchmarks before the architecture works.

The most important benchmark is conceptually:

```text
change one item
```

versus:

```text
how much application work occurred?
```

Instrument the runtime so we can observe how many DOM operations were performed.

## Milestones

Work in this order.

### Milestone 1 — scaffold

Create the monorepo, packages, build configuration, test infrastructure, and working demo application.

The demo must run normally before introducing compilation.

### Milestone 2 — static JSX compiler

Compile a tiny JSX subset into application IR.

Example:

```tsx
function Hello({ name }) {
  return <div>Hello {name}</div>;
}
```

should produce deterministic IR representing the element and text binding.

Add snapshot tests.

### Milestone 3 — WASM renderer

Load the IR into the Rust/WASM runtime and render it into the DOM.

At this point React should not be responsible for rendering the compiled component.

### Milestone 4 — list rendering

Support a simple loop such as:

```tsx
{
  todos.map((todo) => <li>{todo.title}</li>);
}
```

Represent the loop and row bindings explicitly in the IR.

### Milestone 5 — TanStack DB binding

Connect one `useLiveQuery` result to the compiled view graph.

A TanStack DB collection mutation should result in a targeted runtime update.

Instrument DOM mutation counts.

### Milestone 6 — simple actions

Support one straightforward event path such as:

```tsx
<button onClick={() => complete(todo.id)}>Complete</button>
```

Keep the supported action semantics narrow.

### Milestone 7 — evaluate

Compare the normal React/TanStack implementation against the compiled implementation.

Document:

- what compiled cleanly
- what required special handling
- what could not compile
- bundle/runtime overhead
- DOM operation counts
- runtime performance
- compiler complexity
- whether the architecture still appears worth pursuing

Do not proceed into persistence or sync until this evaluation is complete.

## Code quality

Keep abstractions minimal.

Do not create interfaces merely because future implementations might exist.

Prefer straightforward code with obvious ownership.

Avoid large dependency-injection systems, generic plugin frameworks, registries, or premature extensibility.

Comment unusual compiler/runtime decisions, not obvious code.

Prefer small commits or clearly separated implementation steps if your environment supports them.

## No temporary files (hard rule)

Never create temporary, scratch, or intermediate files as part of agent work — no markdown drafts, notes, JSON payloads, or scripts written to the repo, `/tmp`, or elsewhere to feed into a CLI.

Work directly with the primary tool's interface instead: pass arguments/flags inline, pipe stdin when a tool requires it, and use the tool's own batching features (e.g. `bd create --graph` or repeated `bd` calls — not a hand-written plan file).

The only files you write are files the task itself requires in the repository (source, tests, config, docs).

## Decision rule

Whenever there is a choice between:

A. implementing infrastructure ourselves

or

B. using an existing package while preserving the experiment

choose B.

The only novel work we are intentionally validating is:

```text
React/TanStack source
        ↓
application dependency graph
```

and:

```text
application dependency graph
        ↓
efficient WASM-driven DOM updates
```

Keep the project aggressively focused on proving or disproving those two ideas.

<!-- BEGIN BEADS INTEGRATION v:1 profile:full hash:19cc25d9 -->

## Issue Tracking with bd (beads)

**IMPORTANT**: This project uses **bd (beads)** for ALL issue tracking. Do NOT use markdown TODOs, task lists, or other tracking methods.

### Why bd?

- Dependency-aware: Track blockers and relationships between issues
- Git-friendly: Dolt-powered version control with native sync
- Agent-optimized: JSON output, ready work detection, discovered-from links
- Prevents duplicate tracking systems and confusion

### Quick Start

**Check for ready work:**

```bash
bd ready --json
```

**Create new issues:**

```bash
bd create "Issue title" --description="Detailed context" -t bug|feature|task -p 0-4 --json
bd create "Issue title" --description="What this issue is about" -p 1 --deps discovered-from:bd-123 --json
```

**Claim and update:**

```bash
bd update <id> --claim --json
bd update bd-42 --priority 1 --json
```

**Complete work:**

```bash
bd close bd-42 --reason "Completed" --json
```

### Issue Types

- `bug` - Something broken
- `feature` - New functionality
- `task` - Work item (tests, docs, refactoring)
- `epic` - Large feature with subtasks
- `chore` - Maintenance (dependencies, tooling)

### Priorities

- `0` - Critical (security, data loss, broken builds)
- `1` - High (major features, important bugs)
- `2` - Medium (default, nice-to-have)
- `3` - Low (polish, optimization)
- `4` - Backlog (future ideas)

### Workflow for AI Agents

1. **Check ready work**: `bd ready` shows unblocked issues
2. **Claim your task atomically**: `bd update <id> --claim`
3. **Work on it**: Implement, test, document
4. **Discover new work?** Create linked issue:
   - `bd create "Found bug" --description="Details about what was found" -p 1 --deps discovered-from:<parent-id>`
5. **Complete**: `bd close <id> --reason "Done"`

### Quality

- Use `--acceptance` and `--design` fields when creating issues
- Use `--validate` to check description completeness

### Lifecycle

- `bd defer <id>` / `bd supersede <id>` for issue management
- `bd stale` / `bd orphans` / `bd lint` for hygiene
- `bd human <id>` to flag for human decisions
- `bd formula list` / `bd mol pour <name>` for structured workflows

### Sync

bd stores issue history in Dolt:

- Each write auto-commits to Dolt history
- Use `bd dolt push`/`bd dolt pull` for remote sync
- Do not treat `.beads/issues.jsonl` as the sync protocol

**Architecture in one line:** issues live in a local Dolt DB; sync uses `refs/dolt/data` on your git remote; `.beads/issues.jsonl` is a passive export. See https://github.com/gastownhall/beads/blob/main/docs/SYNC_CONCEPTS.md for details and anti-patterns.

### Important Rules

- ✅ Use bd for ALL task tracking
- ✅ Always use `--json` flag for programmatic use
- ✅ Link discovered work with `discovered-from` dependencies
- ✅ Check `bd ready` before asking "what should I work on?"
- ❌ Do NOT create markdown TODO lists
- ❌ Do NOT use external issue trackers
- ❌ Do NOT duplicate tracking systems

For more details, see README.md and docs/QUICKSTART.md.

## Agent Context Profiles

The managed Beads block is task-tracking guidance, not permission to override repository, user, or orchestrator instructions.

- **Conservative (default)**: Use `bd` for task tracking. Do not run git commits, git pushes, or Dolt remote sync unless explicitly asked. At handoff, report changed files, validation, and suggested next commands.
- **Minimal**: Keep tool instruction files as pointers to `bd prime`; use the same conservative git policy unless active instructions say otherwise.
- **Team-maintainer**: Only when the repository explicitly opts in, agents may close beads, run quality gates, commit, and push as part of session close. A current "do not commit" or "do not push" instruction still wins.

## Session Completion

This protocol applies when ending a Beads implementation workflow. It is subordinate to explicit user, repository, and orchestrator instructions.

1. **File issues for remaining work** - Create beads for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **Handle git/sync by active profile**:
   ```bash
   # Conservative/minimal/default: report status and proposed commands; wait for approval.
   git status

   # Team-maintainer opt-in only, unless current instructions forbid it:
   git pull --rebase
   bd dolt push
   git push
   git status
   ```
5. **Hand off** - Summarize changes, validation, issue status, and any blocked sync/commit/push step

**Critical rules:**

- Explicit user or orchestrator instructions override this Beads block.
- Do not commit or push without clear authority from the active profile or the current user request.
- If a required sync or push is blocked, stop and report the exact command and error.

<!-- END BEADS INTEGRATION -->

<!-- BEGIN BEADS CODEX SETUP: generated by bd setup codex -->

## Beads Issue Tracker

Use Beads (`bd`) for durable task tracking in repositories that include it. Use the `beads` skill at `.agents/skills/beads/SKILL.md` (project install) or `~/.agents/skills/beads/SKILL.md` (global install) for Beads workflow guidance, then use the `bd` CLI for issue operations.

### Quick Reference

```bash
bd ready                # Find available work
bd show <id>            # View issue details
bd update <id> --claim  # Claim work
bd close <id>           # Complete work
bd prime                # Refresh Beads context
```

### Rules

- Use `bd` for all task tracking; do not create markdown TODO lists.
- Run `bd prime` when Beads context is missing or stale. Codex 0.129.0+ can load Beads context automatically through native hooks; use `/hooks` to inspect or toggle them.
- Keep persistent project memory in Beads via `bd remember`; do not create ad hoc memory files.

**Architecture in one line:** issues live in a local Dolt DB; sync uses `refs/dolt/data` on your git remote; `.beads/issues.jsonl` is a passive export. See https://github.com/gastownhall/beads/blob/main/docs/SYNC_CONCEPTS.md for details and anti-patterns.
<!-- END BEADS CODEX SETUP -->
