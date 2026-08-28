# `plec-compiler`

Legacy TypeScript-side compiler utilities. The compiler authority is the
Rust workspace ([`crates/`](../../crates) — `plec-parser` through
`plec-compiler`), which the fullstack build invokes via its
`plec-route-manifest` binary. This package remains for
executable-lowering/trace tooling until that cutover completes; see
[docs/rust-authority-capstone-handoff.md](../../docs/rust-authority-capstone-handoff.md).

## Exports

- `plec-compiler` — executable lowering against the `plec-ir` contract
- `plec-compiler/node-entry` — Node entry point
- `plec-compiler/trace` — lowering trace utilities

## Commands

```sh
yarn workspace plec-compiler build
yarn workspace plec-compiler test
yarn workspace plec-compiler typecheck
```
