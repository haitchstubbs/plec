# Getting started

Contributor and developer documentation for prerequisites, the first build,
the development loop, WASM runtime rebuilds, validation, and benchmarks. The
short version lives in the root [README](../README.md).

## Prerequisites

- **Node.js 22.20.0** and **Yarn 4.17.1**. `.node-version` and the root
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

Build the runtime before the application so the required runtime assets are
available to the fullstack build. The fullstack scripts in
`apps/fullstack/package.json` are authoritative.

```sh
yarn install --immutable
yarn install:build-tools
yarn workspace @plec/core build:runtime   # cargo check + wasm-pack into packages/plec/dist/runtime
yarn build
```

The fullstack build runs `plec build` (defaults to `src/app.tsx` and `dist`).
Plec owns the framework build and deployment contract; the first-party Vite
adapter owns generic browser bundling and development orchestration. Vite
handles modules and assets; Plec handles application semantics. To override
the entry or output directory, pass a source path or `--out-dir`:

```sh
plec build src/custom-app.tsx --out-dir build
plec dev src/custom-app.tsx --out-dir build
```

The output roots
separate application ownership from Plec's browser implementation:

```text
dist/public/         public static files, served at /
dist/client/         browser assets, served at /_plec/
dist/server/         private server/application-runtime output
dist/plec-server.json  Node host deployment manifest
```

`/_plec/` is a reserved framework URL prefix; deployment hosts should expose
`dist/client/*` only below that prefix. Browser artifacts remain client-visible.
For ordinary
application-owned static files, put them under the app's `public/` directory;
`plec build` copies them to the same relative path under `dist/public/` and
removes old output on each build. The complete `dist/` output is relocatable
and consumed by `plec serve dist`; production serving does not require Vite,
Vite preview, a development server, or application source files. `server/` is
private to the host; `client/` and `public/` are browser-visible. Plec
bootstrap/runtime resources are under `/_plec/`; app assets keep their
ordinary paths.

Use `public/` when an asset has an intentionally stable, manually addressed
URL—for example `public/favicon.svg` is served as `/favicon.svg`. Use a source
import when the asset is a dependency of application code:

```tsx
import logoUrl from './logo.svg';

export function Logo() {
  return <img src={logoUrl} alt="Logo" />;
}
```

Plec currently accepts opaque `.svg`, `.png`, `.jpg`, `.jpeg`, `.webp`, `.gif`,
`.ico`, `.woff`, `.woff2`, and `.ttf` files from relative static default imports.
It emits the original bytes once under `/assets/compiled/<sha256-prefix>.<ext>`
and lowers the fingerprinted URL to a string in the route graph. SSR and browser
rendering therefore consume the same URL. A missing file, unsupported type, or
path escaping the importing module's approved source root fails the build.
Use `public/` for unsupported formats or when a stable hand-authored URL is
preferred. Imports do not transform files; dynamic, named, namespace, and
query-string asset imports are not supported. `dist/plec-assets.json` records
application-relative source paths and emitted URLs for build/dev dependency
tracking. It stays outside `dist/public/` and is not required by `plec serve`.

## Dev loop

```sh
yarn workspace fullstack dev
```

`plec dev` starts Vite on the public development port and attaches `@plec/node`
request dispatch to that same HTTP server, so development uses one listening
port. Vite owns file watching, CSS/assets,
and the browser development connection. Plec builds into an isolated candidate
directory so a failed build prints its structured diagnostic while leaving the
last successful application available; fixing the source triggers another
build automatically. Client-only replacements keep the native host running.
Server-owned changes restart the Node application host when needed. After the
replacement is ready, Vite sends one full-page reload.
Vite HMR is not Plec semantic HMR. A successful Plec source change recompiles
and triggers a full-page reload; application state is not preserved and Plec
does not hot-replace semantic modules.

For the default path, no `vite.config.*` is needed. If an app supplies one,
Vite loads it normally and composes its generic plugins/settings with the
first-party Plec adapter. The adapter keeps ownership of Plec's required
entries, output layout, semantic artifact integration, and dependency checks;
use `plec.toml` for Plec-specific settings rather than duplicating them in
Vite configuration. The generated app includes `esbuild` as a build-time
dependency for its server bundle; the browser graph is bundled by Vite through
the `@plec/core` → `@plec/vite` dependency chain.

## Rebuilding the WASM runtime

After changing the runtime crate (`crates/plec-runtime`):

```sh
yarn workspace @plec/core build:wasm   # wasm-pack -> packages/plec/dist/runtime
yarn workspace fullstack build   # regenerates the application artifact and staged assets
```

Verify what you just built (and what the app stages) before testing:

```sh
plec workspace artifact stale   # non-zero when dist/staged WASM is stale or protocol-drifted
```

**Stale `.br` trap:** the dev server serves `.br` brotli variants when the
client sends `accept-encoding: br`. If you hand-copy fresh `runtime.js` /
`runtime_bg.wasm` into `apps/fullstack/dist/client/runtime/` without
regenerating the `.br` files, the browser silently runs the old code. Delete
the `.br` files or re-run the fullstack build instead of hand-copying.

For continuous rebuilds: `yarn workspace @plec/core dev:wasm`.

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

Both variants report the same Plec product SemVer via `plec --version`. It
is declared once in the Cargo workspace and mirrored into all JS workspace
package manifests; update the complete release bundle with
`plec workspace version --set` and verify with `--check`. Protocol/IR and
schema versions remain separate compatibility contracts and are never bumped
by it.

## Tests

```sh
yarn toolchain:verify          # pinned Rust and WASM toolchain
yarn toolchain:verify:browser  # includes pinned browser tooling
yarn format:check
yarn typecheck
yarn test                      # unit and workspace tests
yarn test:wasm                 # WASM browser test suite
yarn test:e2e                  # Playwright smoke gate (E2E_PORT, default 3216)
yarn test:acceptance           # larger opt-in behavioral suite
yarn build
```

`packages/plec-e2e` is the canonical end-to-end runner. Playwright owns the
fullstack server for every tier: turbo builds the app, the `webServer`
config starts `plec serve dist`, waits for HTTP readiness, and kills the
process group afterwards — no manual spawning, no leftover ports. The port
comes from `E2E_PORT` in `.env.devports` (the canonical port registry);
override it per run with `E2E_PORT=…`. Specs are named `*.playwright.ts`.
See `packages/plec-e2e/README.md` and the E2E section of `AGENTS.md`.

`yarn typecheck` runs the workspace type checks. The fullstack typecheck also
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
