# `fullstack`

The demo full-stack Plec application: typed TSX routes compiled by the Rust
compiler into a route manifest + graphs, executed in the browser by the WASM
runtime started through `startPlecRouter`.

Routes: root layout with sidebar, home, about, todos (live collection CRUD —
the acceptance target), runtime stress, and not-found.

This app must remain React-free. `check:no-react` enforces it at dev, build,
test, and typecheck time.

## Run

```sh
yarn workspace plec build:runtime             # once: build the WASM runtime
yarn workspace fullstack dev    # build + plec serve dist
```

Serves on `PORT` (default `3000`). TSX edits require a rebuild — the Rust
compiler emits graphs at build time; there is no HMR.

## Test

```sh
yarn workspace fullstack test  # vitest + check:no-react
yarn test:e2e                                # Playwright smoke gate (repo root)
yarn test:acceptance                         # Playwright full behavioral suites
```

Playwright (`packages/plec-e2e`) owns the native server lifecycle for e2e
tiers — never start a server manually for tests.

Prerequisites, the WASM rebuild loop, and the stale-`.br` trap:
[docs/getting-started.md](../../docs/getting-started.md).
