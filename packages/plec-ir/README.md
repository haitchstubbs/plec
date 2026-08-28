# `plec-ir`

The shared application IR contract: a versioned Zod schema plus validation
helpers. Both the TypeScript tooling and the Rust runtime (via its own typed
decoder) agree on this contract; see `crates/plec-ir` for the Rust side.

## Exports

- `plec-ir` — the IR schema and types
- `plec-ir/runtime` — runtime-side schema slice
- `plec-ir/executable` — executable (typed action program) schema
- `plec-ir/validate-route-manifest` — route manifest validation
- `plec-ir/is-plec-value` — Plec value guards

## Commands

```sh
yarn workspace plec-ir build
yarn workspace plec-ir test
yarn workspace plec-ir typecheck
```

Depends only on `zod`; no workspace dependencies.
