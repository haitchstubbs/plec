# @haitchstack/query

The public TypeScript package for the haitchstack query builder. This package is a typed facade over Rust-backed native (Node.js) and WASM bindings. All runtime query logic lives in [`crates/query`](../../crates/query).

## Entry points

| Export                    | Path                 | Purpose                        |
| ------------------------- | -------------------- | ------------------------------ |
| `@haitchstack/query`      | `./dist/index.js`     | Main API — Node native runtime |
| `@haitchstack/query/wasm` | `./dist/wasm.js`      | WASM runtime (browser/bundler) |
| `@haitchstack/query/zod`  | `./dist/zod/index.js` | Zod schema integration         |

## Architecture

```
TypeScript layer (apps/query)
  ├── compile-time types, overloads, schema inference
  ├── Database API facade
  └── binds to ──► Rust (crates/query)
                      ├── query_core  — SQL builder, dialect policy, validation
                      ├── query_node  — napi-rs native .node binding
                      └── query_wasm  — wasm-bindgen WASM bundle
```

## Building

TypeScript (required for type checking):

```sh
yarn build:ts
# or: tsc
```

Rust bindings (required to actually run queries):

```sh
# Both native and WASM:
yarn build:rust

# Native only (Node.js):
yarn compile:native

# WASM only (browser/bundler):
yarn compile:wasm
```

## Testing

```sh
yarn test           # runtime + type tests
yarn test:runtime   # vitest runtime tests only
yarn test:types     # tsc type tests only
```

## Docs

```sh
# Regenerate the dialect capability matrix from Rust metadata:
yarn docs:generate-dialect-matrix

# Verify matrix is current (used in CI):
yarn docs:check-dialect-matrix
```

The generated matrix lives in [`docs/dialect-matrix.md`](docs/dialect-matrix.md).
