# Getting started

This is the internal walkthrough: prerequisites, the first build, the dev
loop, the WASM rebuild loop, and how to test and benchmark. The quickstart
version lives in the root [README](../README.md).

## Prerequisites

- **Node.js** with **yarn 4** (`corepack enable` picks up the version pinned
  in the root `package.json`).
- **Rust** (stable) with the `wasm32-unknown-unknown` target:
  `rustup target add wasm32-unknown-unknown`
- **wasm-pack**: `cargo install wasm-pack`
- **wasm-tools** (optional): the WASM build strips debug info with it when it
  is on `PATH`, and prints a warning and skips optimization when it is not
  (`scripts/build-wasm.mjs`).
- **Chrome + a matching ChromeDriver** for the WASM browser tests. Run
  `yarn install:build-tools`: it reads the Chromium version Playwright
  manages and provisions an exactly matching ChromeDriver into
  `.tools/chromedriver-<platform>/` (git-ignored), so the driver
  can never drift from the browser. The browser harness resolves the driver
  automatically; override with `CHROMEDRIVER` and `PLEC_CHROME_EXECUTABLE`
  if needed.

## First build

Build order matters: the fullstack build copies pre-built WASM artifacts and
invokes the Rust route compiler, so the runtime and toolchain must exist
first. Source of truth: `apps/fullstack/scripts/build.mjs`.

```sh
yarn install
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

This rebuilds and then runs `node --watch dist/server.mjs`. The server listens
on `PORT` (default `3000`).

There is no vite/HMR: the Rust compiler emits the route manifest and graphs
at build time. TSX edits require re-running the fullstack build — the `dev`
script does that before restarting the server.

## Rebuilding the WASM runtime

After changing the runtime crate (`crates/plec-runtime`):

```sh
yarn workspace plec-runtime build:wasm   # wasm-pack -> packages/plec-runtime/dist/runtime
yarn workspace @wasm-runtime/fullstack build   # copies wasm into the app and regenerates brotli
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
config starts `dist/server.mjs`, waits for HTTP readiness, and kills the
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
