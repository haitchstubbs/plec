# Plec CLI

Command-line tooling for compiling and inspecting Plec applications.

The CLI exposes the Plec compiler pipeline directly, making it possible to inspect executable IR, route manifests, and compiler state without starting a browser runtime or development server.

## Dev CLI vs app CLI

The `plec` binary contains two frontends: the **dev CLI** (compiler inspection: `inspect`, `raw`, `routes`, `build`) and the **app CLI**. Which frontend handles a command is selected at **compile time**, not process startup:

- `crates/plec-cli/build.rs` lifts `PLEC_CLI_VERSION` from `.env.plec` at the workspace root (parsed with `dotenvy`) and bakes it into the crate with `cargo:rustc-env`.
- `lib.rs` selects the frontend with `option_env!("PLEC_CLI_VERSION")`: `release` selects the app CLI, any other value (or an absent variable) keeps the dev CLI.

Because the selection is baked in, changing `.env.plec` triggers a rebuild of the CLI through the build script's `rerun-if-changed` directive, and an explicitly exported `PLEC_CLI_VERSION` in the build environment wins over the file:

```bash
# .env.plec
PLEC_CLI_VERSION='release'

# pick up the change in the binary
cargo build -p plec-cli
```

## Usage

```bash
plec <command>
```

Available commands:

```text
inspect   Query the compiled application
raw       Print the compiled executable application
routes    Print the application's route manifest
build     Compile a routed application into deployable artifacts
dev       Developer workflow helpers (dev frontend only)
```

## `raw`

Compile a Plec application and print its executable component graph as JSON.

```bash
plec raw <source>
```

Example:

```bash
plec raw apps/fullstack/src/main.tsx
```

The command runs the application through the normal compiler pipeline:

```text
source graph
    ↓
semantic graph
    ↓
root component discovery
    ↓
application HIR
    ↓
executable IR
```

The resulting `ComponentApplication` is written to stdout as formatted JSON.

This is useful when debugging lowering, inspecting generated IR, or verifying compiler output directly.

---

## `inspect`

Compile an application and query its executable graph through the Plec inspector.

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

The inspector operates directly against the in-memory compiled application. It does not start an HTTP or GraphQL server.

Query results are emitted as JSON:

```json
{
  "components": [
    {
      "id": "App"
    }
  ]
}
```

Inspector/query errors are written to stderr.

This command is intended for targeted compiler debugging where dumping the entire IR with `raw` would be unnecessarily noisy.

---

## `routes`

Parse an application and print its compiled route manifest.

```bash
plec routes <source>
```

Example:

```bash
plec routes apps/fullstack/src/main.tsx
```

The command performs source discovery and semantic analysis before lowering route declarations into Plec's route representation and serializing the resulting manifest.

Example output:

```json
{
  "routes": [
    {
      "path": "/"
    },
    {
      "path": "/todos"
    }
  ]
}
```

The exact manifest structure is defined by the current Plec route IR.

---

## Compiler pipeline

The CLI intentionally uses the same compiler crates as the rest of Plec rather than maintaining a separate inspection path.

For application compilation it performs:

1. `read_source_graph`
2. `build_semantic_graph`
3. `discover_root_component`
4. `lower_application`
5. `lower_application_to_executable`

For route inspection it performs:

1. `read_source_graph`
2. `build_semantic_graph`
3. `lower_routes`
4. `lower_route_manifest`

This makes the CLI useful as a thin debugging surface over the canonical compiler implementation.

## Commands at a glance

| Command        | Input                | Output              | Purpose                            |
| -------------- | -------------------- | ------------------- | ---------------------------------- |
| `plec raw`     | Source entry         | Executable IR JSON  | Inspect complete compiler output   |
| `plec inspect` | Source entry + query | Query result JSON   | Targeted inspection of compiled IR |
| `plec routes`  | Source entry         | Route manifest JSON | Inspect router compilation         |
| `plec dev …`   | Workspace state      | Reports/captures    | Developer workflow helpers (below) |

---

# `plec dev` — developer workflow helpers

The `dev` group encodes the validation and investigation workflows of the
Plec workspace itself. Each command exists because agents and developers
kept rebuilding the same context by hand: repo topology, protocol
invariants, test-failure details, and artifact provenance.

`dev` belongs to the **dev CLI frontend only** — the release (app) frontend
never exposes it. Which frontend is built is selected at compile time (see
[Dev CLI vs app CLI](#dev-cli-vs-app-cli)). For workspace work install the
dev frontend:

```bash
yarn install:plec-cli:dev
```

Every `dev` command accepts `--json` for machine-readable output.

## `plec dev test wasm` / `plec dev test last`

Run the WASM browser suite once, capture the output, and query it without
re-running.

```bash
plec dev test wasm                        # full suite, live output + capture
plec dev test wasm nested_component       # filters forwarded to wasm-pack
plec dev test wasm --failures             # print only the parsed failures
plec dev test last                        # summary of the last run
plec dev test last --failure nested_loop  # failures matching a substring
plec dev test last --json                 # the captured report as JSON
```

The runner spawns `scripts/browser-harness.mjs` (which single-sources
ChromeDriver/Chrome resolution for wasm-pack), tees output live, and parses
the wasm-bindgen-test noise into a structured report:

```text
2 / 87 failed

nested_component_loop_without_record_fails_closed
  crates/plec-runtime/tests/typed_events.rs:3857
  called `Result::unwrap()` on an `Err` value: JsValue("missing:ssr-loop:…")
```

Captures live in `.cache/plec/test-wasm/` (`last.json` + `last.log`), which
is gitignored. Agents should never re-run the whole suite just to re-read a
failure — query the capture instead.

> Note: when wasm test orchestration moves under `packages/plec-e2e`'
> Playwright ownership, only the spawn step changes; the capture/parse layer
> is invocation-independent.

## `plec dev artifact provenance` / `plec dev artifact stale`

The runtime flows through a pipeline — source crate → wasm-pack →
`packages/plec-runtime/dist/runtime` → staged copy in
`apps/fullstack/dist/public/runtime` — and sessions keep tripping over
"am I testing source, package dist, or staged dist?". Two staleness shapes
are checked:

- **build identity** — the file on disk vs the hash recorded in
  `provenance.json` at build time, and the staged copy vs package dist;
- **protocol drift** — which SSR snapshot protocol the binary actually
  implements, read from the `plec-protocol` WASM custom section
  (crates/plec-runtime/src/lib.rs embeds it from the plec-ir constants;
  `scripts/build-wasm.mjs` guarantees it survives optimization).

```bash
plec dev artifact provenance runtime   # full report
plec dev artifact stale                # terse gate; non-zero on staleness
```

```text
package dist               OK
app staged                 STALE
  ✗ STALE: implements snapshot protocol 1, source is 2
```

Both commands exit non-zero on problems, so they gate scripts.

## `plec dev contract ssr`

The SSR protocol has three versioned boundaries — snapshot
(`SSR_SNAPSHOT_VERSION`), bootstrap wrapper, route manifest — that must
agree across Rust sources/fixtures, the TypeScript server and browser glue,
e2e tests, and the docs. A bump that misses one site (a loader fixture
hard-coding the old version) historically surfaced only as a confusing
runtime failure.

```bash
plec dev contract ssr            # report every site's version
plec dev contract ssr --check    # exit non-zero on any conflict
```

```text
SSR protocol contract

Snapshot
  canonical version       2  crates/plec-ir/src/lib.rs (compiled into plec-cli: 2)
  definition                2  crates/plec-ir/src/lib.rs:171  definition ✓
  runtime gate              —  crates/plec-runtime/src/runtime/lifecycle.rs:460  constant ref
  runtime fixtures          2  crates/plec-runtime/tests/typed_events.rs:2630  ✓
  server producer           1  packages/plec-server/src/index.ts:529  ~ legacy non-snapshot fallback
  …

✓ no stale hard-coded protocol versions
```

Intentional legacy literals (fixtures exercising the fail-closed gates) are
allowlisted per site and reported with `~` rather than treated as conflicts.

## `plec dev trace <symbol-or-error-code>`

Categorized search: where a symbol or adoption error code is defined,
produced, asserted, and documented.

```bash
plec dev trace unsupported:ssr-snapshot-version
plec dev trace SSR_SNAPSHOT_VERSION
```

```text
unsupported:ssr-snapshot-version

RELATED CONTRACT
  SSR_SNAPSHOT_VERSION = 2 (crates/plec-ir/src/lib.rs)

PRODUCED BY
  crates/plec-runtime/src/runtime/lifecycle.rs:461  return Err(…)

ASSERTED BY
  crates/plec-runtime/tests/typed_events.rs:2747  "unsupported:ssr-snapshot-version"

DOCUMENTED BY
  docs/ssr-architecture.md:81  > **Contract evolution:** …
```

## `plec dev doctor adoption`

Health check for the SSR adoption pipeline, composing the checks above plus
graph resolution:

1. **Protocol** — do all versioned boundaries agree?
2. **Graph resolution** — compiles the app (default:
   `apps/fullstack/src/router.tsx`) and resolves every graph reference
   through the runtime's registry semantics: direct registry key, or via a
   registered application's components.
3. **Nested execution** — which conditionals/loops each component contains,
   i.e. what a snapshot v2 must record.
4. **DOM markers** — with `--html <file>`: validates `plec:*` boundary
   pairing and `data-plec-node` address grammar against
   `docs/dom-address-protocol.md`.
5. **Artifact provenance** — is built/staged WASM current and
   protocol-consistent?

```bash
plec dev doctor adoption
plec dev doctor adoption --route /
plec dev doctor adoption --html page.html
plec dev doctor adoption --snapshot captured-snapshot.json
plec dev doctor adoption --json
```

Exits non-zero when any section finds problems, and prints hints pointing at
the specific follow-up command.

## Running from the workspace

During development, the CLI can be run directly through Cargo (note: with
the workspace default `.env.plec`, plain `cargo run` builds the **release**
frontend — override the variant for dev commands):

```bash
PLEC_CLI_VERSION=dev cargo run -p plec-cli -- dev doctor adoption
```

## Status

The CLI currently focuses on compiler inspection and diagnostics.

It is expected to grow alongside Plec's toolchain, with higher-level commands such as application compilation, development workflows, and build orchestration remaining separate concerns until their interfaces are stable.
