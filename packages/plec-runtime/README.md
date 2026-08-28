# `plec-runtime`

The Rust → WebAssembly runtime. Loads the compiled application artifact,
instantiates the view graph, and applies targeted DOM mutations from
reactive deltas — one changed tuple reaches the few DOM nodes that depend on
it instead of rerendering a subtree.

The crate lives at `crates/runtime` (crate name `plec_runtime`) and is a
member of the root Cargo workspace. Docs for the compiler crates:
[`crates/README.md`](../../crates/README.md).

## Build profiles

wasm-pack builds target `web`. Feature profiles
(`scripts/build-wasm.mjs`): `full` (default: router + fetch), `core`,
`router`, `fetch`.

## Commands

```sh
yarn workspace plec-runtime build        # cargo check + wasm-pack
yarn workspace plec-runtime build:wasm   # wasm-pack only
yarn workspace plec-runtime dev:wasm     # watch mode
yarn workspace plec-runtime test         # cargo test --lib
yarn workspace plec-runtime test:core    # cargo test --no-default-features
yarn workspace plec-runtime test:wasm    # browser harness (matching chromedriver required)
```

**Stale `.br` trap:** after rebuilding, refresh the app via
`yarn workspace @wasm-runtime/fullstack build` rather than hand-copying
wasm files — stale brotli variants are served silently otherwise. See
[docs/getting-started.md](../../docs/getting-started.md).
