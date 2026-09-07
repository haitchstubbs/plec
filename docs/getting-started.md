# Getting started

This is the internal walkthrough: prerequisites, the first build, the dev
loop, the WASM rebuild loop, and how to test and benchmark. The quickstart
version lives in the root [README](../README.md).

## Prerequisites

- **Node.js 24.20.0** and **Yarn 4.17.1**. `.node-version` and the root
  `packageManager` are authoritative.
- **Rust 1.98.0** with the `wasm32-unknown-unknown` target. `rust-toolchain.toml`
  installs the target for rustup-managed toolchains.
- **Pinned WASM/browser tools**. `cli-tools.json` is the authority for
  wasm-pack, wasm-tools, Playwright Chromium, and ChromeDriver.

Install the pinned dependencies and local browser tools before building:

```sh
yarn install --immutable
yarn install:build-tools
```

`install:build-tools` installs the exact Rust tools, Playwright Chromium into
`.cache/ms-playwright/`, and its exact ChromeDriver into `.tools/`. It fails
instead of falling back to a stable browser or driver. Every browser runner
verifies this toolchain before executing.

The WASM crates pin `serde-wasm-bindgen` with the compatible `wasm-bindgen`,
`js-sys`, `web-sys`, and test family in the workspace manifest and lockfile.
`install:build-tools` preinstalls that exact wasm-bindgen CLI and test runner
under `.tools/`; test runs use `wasm-pack --mode no-install` and never install
either binary themselves.

## First build

Build order matters: the fullstack build copies pre-built WASM artifacts and
invokes the Rust route compiler, so the runtime and toolchain must exist
first. Source of truth: `apps/fullstack/scripts/build.mjs`.

```sh
yarn install --immutable
yarn install:build-tools
yarn workspace plec-runtime build   # cargo check + wasm-pack into packages/plec-runtime/dist/runtime
yarn build                          # turbo: compile routes via plec-route-manifest, esbuild client/server, copy wasm, brotli
```

`yarn build` for the fullstack app compiles the app's TSX with the **Rust**
compiler (`cargo run -p plec-compiler --bin plec-route-manifest`), emits
`dist/public/route-manifest.json` and `dist/public/graphs/*.json`, bundles
`src/client.tsx` and `src/server.ts` with esbuild, and enforces that no
compiler/zod/typescript code leaks into the browser bundle.

## Dev loop

```sh
yarn workspace @wasm-runtime/fullstack dev
```

This rebuilds and then runs `plec serve dist`. The native server listens on
`PORT` (default `3000`).

There is no vite/HMR: the Rust compiler emits the route manifest and graphs
at build time. TSX edits require re-running the fullstack build — the `dev`
script does that before restarting the server.

## Rebuilding the WASM runtime

After changing the runtime crate (`crates/plec-runtime`):

```sh
yarn workspace plec-runtime build:wasm   # wasm-pack -> packages/plec-runtime/dist/runtime
yarn workspace @wasm-runtime/fullstack build   # copies wasm into the app and regenerates brotli
```

Verify what you just built (and what the app stages) before testing:

```sh
plec workspace artifact stale   # non-zero when dist/staged WASM is stale or protocol-drifted
```

**Stale `.br` trap:** the dev server serves `.br` brotli variants when the
client sends `accept-encoding: br`. If you hand-copy fresh `runtime.js` /
`runtime_bg.wasm` into `apps/fullstack/dist/public/runtime/` without
regenerating the `.br` files, the browser silently runs the old code. Delete
the `.br` files or re-run the fullstack build instead of hand-copying.

For continuous rebuilds: `yarn workspace plec-runtime dev:wasm`.

**Silent failures:** some browser-side wasm failure paths early-return
without logging (for example generation mismatches in typed fetch). When a
change silently does nothing, add `web_sys::console::error_1` markers at
each early-return and reproduce with a Playwright probe capturing
`page.on('console')`.

## Dev CLI

The `plec` binary ships a dev-only workflow group for workspace
investigation — capturing/querying WASM test runs, checking SSR protocol
versions, tracing symbols and error codes, artifact provenance, and an SSR
adoption doctor. Install it with `yarn install:plec-cli:dev`; the command
reference lives in [crates/plec-cli/README.md](../crates/plec-cli/README.md)
and the agent-facing rules in `AGENTS.md` ("Dev CLI").

Two `plec` variants exist. `yarn` scripts resolve the shim in
[`packages/plec/bin/plec.js`](../packages/plec/README.md#cli), which prefers
the packaged release binary (app commands only). The dev `workspace` group
lives in the cargo-installed binary, so run those as bare `plec …` on the
shell `PATH`, or set `PLEC_BIN` to force a binary.

## Tests

```sh
yarn test                                        # turbo: all workspaces
yarn workspace plec-runtime test                 # cargo test --lib
yarn workspace plec-runtime test:core            # cargo test --no-default-features
yarn workspace plec-runtime test:wasm            # browser harness (wasm-pack test --headless --chrome)
yarn workspace @wasm-runtime/fullstack test      # vitest + check:no-react
yarn test:e2e                                    # Playwright smoke gate (E2E_PORT, default 3216)
yarn test:acceptance                             # Playwright full behavioral suites — opt-in
```

`packages/plec-e2e` is the canonical end-to-end runner. Playwright owns the
fullstack server for every tier: turbo builds the app, the `webServer`
config starts `plec serve dist`, waits for HTTP readiness, and kills the
process group afterwards — no manual spawning, no leftover ports. The port
comes from `E2E_PORT` in `.env.devports` (the canonical port registry);
override it per run with `E2E_PORT=…`. Specs are named `*.playwright.ts`.
See `packages/plec-e2e/README.md` and the E2E section of `AGENTS.md`.

Type checking: `yarn typecheck` (turbo). For the fullstack app this also
runs `cargo check -p plec-compiler`.

## Benchmarks

```sh
yarn measure:runtime-baseline           # runtime baseline metrics
yarn measure:runtime-baseline:verify    # verify against committed baselines
yarn bench                              # Playwright navigation benchmark into benchmarks/results/
```

The navigation benchmark runs through `packages/plec-e2e` with one browser
context per sample, cache disabled via CDP, and an optional fast-4G
throttled phase. `PLEC_BENCH_SAMPLES` (default 10) controls sample count;
`PLEC_BENCHMARK_ORIGIN` adds a deployed-origin phase. Reports land in
`benchmarks/results/`.

## Formatting and lint

```sh
yarn format          # prettier --write . + cargo fmt (runtime crate)
yarn format:check    # both in check mode
```
