# Rust compiler workspace

The Rust crates compile typed TSX source into the Plec application artifact:
a route manifest plus semantic application graphs consumed by the WASM
runtime. Rust is the compiler authority.

## Pipeline

```text
TSX source
  plec-parser    -> raw syntax
  plec-model      -> name/symbol resolution          (depends on parser)
  plec-hir       -> high-level app model (components, nodes, exprs)
  plec-lowering  -> HIR to IR 0.10                  (depends on hir, ir)
  plec-ir        -> shared IR contract (versioned)
  plec-compiler  -> driver + plec-route-manifest binary (depends on all)
  plec-diagnostics -> diagnostic reporting (standalone, no dependents yet)
```

`plec-parser`, `plec-hir`, and `plec-ir` are leaves; `plec-compiler` is the
only entry point. Its `plec-route-manifest` binary is what the fullstack
build invokes (`apps/fullstack/scripts/build.mjs`) to emit the route
manifest and per-route graphs.

`plec-build` assembles the application artifact around those compiler
outputs: clean, in-process artifact emission, browser/server esbuild
bundles, dependency-boundary validation, revision hashing + brotli
sidecars, runtime staging (workspace build or installed `plec` package),
and the document shell. `plec-cli` owns the command surface only: its
build/compile subcommands parse arguments and delegate.

## The runtime crates are workspace members too

The WASM runtime is split across workspace members under the root
`Cargo.toml`. `crates/plec-runtime` is the cdylib: the `#[wasm_bindgen]`
`PlecRuntime` facade whose exported methods are the JS contract, the
SSR snapshot tooling (`runtime/snapshots.rs`), and the embedded
`plec-protocol` custom section. Execution lives in the crates below it:

```text
plec-schema   -> artifact data model (typed graphs, deltas, routing)
plec-dom      -> browser host access (window/document/now, cookie policy)
plec-eval     -> expression VM for typed graph instructions
plec-client   -> typed graph runtime: RuntimeState, events, cookies,
                 fetch actions, keyed reorder, action VM, binding sinks,
                 route-instance transitions
plec-router   -> URL matching, navigation/adoption, browser listeners
plec-runtime  -> wasm facade (cdylib) + snapshots + protocol marker
```

Dependencies point strictly downward (`plec-router` -> `plec-client` -> ...);
the facade delegates to the crates above it. The `fetch` feature is declared
by `plec-schema` (validation gating) and forwarded by `plec-client` and
`plec-runtime`; `router` remains a vestigial no-op kept for compatibility.

## Where to look next

- [docs/plec-missing-features-implementation.md](../docs/plec-missing-features-implementation.md)
  — authoritative map of feature to implementation files and tests
- [docs/typed-action-ir.md](../docs/typed-action-ir.md) — the typed action
  IR contract
- [docs/getting-started.md](../docs/getting-started.md) — build and test
  commands
