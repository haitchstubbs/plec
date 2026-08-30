# `@wasm-runtime/fullstack`

The demo full-stack Plec application: typed TSX routes compiled by the Rust
compiler into a route manifest + graphs, executed in the browser by the WASM
runtime started through `startPlecRouter`.

Routes: root layout with sidebar, home, about, todos (live collection CRUD —
the acceptance target), runtime stress, and not-found.

This app must remain React-free. `check:no-react` enforces it at dev, build,
test, and typecheck time.

## Run

```sh
yarn workspace plec-runtime build             # once: build the WASM runtime
yarn workspace @wasm-runtime/fullstack dev    # build + node --watch dist/server.mjs
```

Serves on `PORT` (default `3000`). TSX edits require a rebuild — the Rust
compiler emits graphs at build time; there is no HMR.

## Test

```sh
yarn workspace @wasm-runtime/fullstack test             # vitest + check:no-react
yarn workspace @wasm-runtime/fullstack test:acceptance  # build + headless-Chrome todos acceptance (port 3201)
yarn workspace @wasm-runtime/fullstack bench:navigation
```

Prerequisites, the WASM rebuild loop, and the stale-`.br` trap:
[docs/getting-started.md](../../docs/getting-started.md).
