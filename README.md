# Plec

A full-stack framework for building data-intensive web applications around a
compiled semantic application graph.

Most frameworks compile your components into application-specific JavaScript
that owns rendering, reconciliation, and runtime behavior. Plec instead
compiles application semantics — UI structure, state, dependencies, actions,
routing — into a portable, inspectable graph that a generic runtime executes.
The result is closer to a UI ABI than a traditional frontend bundle:
behavior is validated before it runs, and a change to one piece of data
updates exactly the DOM nodes that depend on it.

The runtime is written in Rust and compiled to WebAssembly. WebAssembly is an
implementation detail, not the point — the point is the boundary between
application source and application execution.

## How it works

```text
TSX source (apps are ordinary typed components)
  |
  v
Rust compiler pipeline (crates/*)
  parse -> sema -> HIR -> lower -> IR
  |
  v
Application artifact (route manifest + graph JSON)
  |
  v
WASM runtime assets (packages/plec/dist/runtime)
  reactive update handling, instruction execution, scheduler
  |
  v
Targeted DOM mutations
```

One changed tuple flows through a known dependency edge to one or a few DOM
mutations, without re-rendering a component subtree.

## Repository layout

```text
apps/
  fullstack/          Demo full-stack Plec application (the runnable example)
packages/
  plec/               Framework runtime: jsx-runtime, state hooks, router
  plec-ir/            Shared IR schema (Zod) and validation
  plec-browser/       Browser glue: startPlecRouter, graph loading
  plec/               Release package and Rust -> WASM runtime assets
  ui/                 React/shadcn UI kit used by tooling pages
  lucide-plec/        Generated Lucide icon components for Plec
crates/               Rust compiler workspace (parser -> sema -> HIR -> IR)
docs/                 Working notes, contracts, and guides
```

## Quickstart

Prerequisites: Node.js with yarn 4, a Rust toolchain with the
`wasm32-unknown-unknown` target, and wasm-pack. The full list and first-build
troubleshooting live in [docs/getting-started.md](docs/getting-started.md).

```sh
yarn install
yarn workspace plec build:runtime   # cargo check + wasm-pack -> packages/plec/dist/runtime
yarn build                          # turbo: compile routes, bundle, brotli
yarn workspace @wasm-runtime/fullstack dev
```

The demo app serves on `http://localhost:3000` (override with `PORT`).

## Documentation

- [docs/getting-started.md](docs/getting-started.md) — prerequisites, build
  order, dev loop, WASM rebuild loop, tests, benchmarks
- [crates/README.md](crates/README.md) — the Rust compiler pipeline
- [AGENTS.md](AGENTS.md) — experiment goals, constraints, and milestones
- [docs/](docs/) — IR contracts, feature maps, and handoff notes
