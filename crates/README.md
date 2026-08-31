# Rust compiler workspace

The Rust crates compile typed TSX source into the Plec application artifact:
a route manifest plus semantic application graphs consumed by the WASM
runtime. Rust is the compiler authority.

## Pipeline

```text
TSX source
  plec-parser    -> raw syntax
  plec-sema      -> name/symbol resolution          (depends on parser)
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

## The runtime crate is a workspace member too

The WASM runtime lives at `crates/plec-runtime` and is a
member of this same Cargo workspace (root `Cargo.toml`), under the crate
name `plec-runtime`. It decodes and validates the IR these crates emit, with
feature profiles `full` (default; router + fetch), `core`, `router`, and
`fetch`.

## Where to look next

- [docs/plec-missing-features-implementation.md](../docs/plec-missing-features-implementation.md)
  — authoritative map of feature to implementation files and tests
- [docs/typed-action-ir.md](../docs/typed-action-ir.md) — the typed action
  IR contract
- [docs/getting-started.md](../docs/getting-started.md) — build and test
  commands
