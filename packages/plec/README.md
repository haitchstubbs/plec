# `@plec/core`

The Plec framework runtime: the package applications import. It provides the
`@plec/core` jsx-import-source, client state hooks, and the router primitives.

## Exports

- `@plec/core` — `Fragment`, `jsx`, `jsxs`, `createRoot`, `useState`, `useRef`,
  `useHostRef`, `useReaction`, `useListener`, `useLocation`, `cookie`,
  router primitives (`createRootRoute`, `createRoute`, `createRouter`,
  `Link`, `Outlet`, `RouterProvider`, `useNavigate`)
- `@plec/core/jsx-runtime` — the automatic JSX runtime (set
  `jsxImportSource: "@plec/core"`)
- `@plec/core/client/effects/development-memory-hud` — development-only memory HUD

## CLI

The `plec` command is owned by this package (`"bin"` → `bin/plec.js`). In an
npm install, `plec serve` starts `@plec/node` directly in the CLI process;
build/compiler and inspection commands continue to use the native Plec CLI.
An explicit `PLEC_BIN` keeps the Rust `serve` path available for development
and Axum parity tests.

Resolution order:

For commands other than `serve`, resolution order is:

1. `PLEC_BIN` — explicit binary override; authoritative, so a set-but-
   unusable value is an error instead of a silent fallback
2. `dist/bin/plec[.exe]` — the packaged binary assembled by
   `scripts/build-artifact.mjs` (**release** variant)
3. `~/.cargo/bin/plec[.exe]` — a cargo-installed binary (repo contributors;
   `yarn install:plec-cli:dev` installs the **dev** variant)

Which variant wins where:

- Inside an npm project that depends on `@plec/core`, `plec serve` runs the
  Node host. Other app commands use the packaged Rust **release** variant
  (`inspect`, `raw`, `routes`, `build`).
- A bare `plec` in the shell resolves via the system `PATH`, typically the
  cargo-installed **dev** variant, which additionally carries the `workspace`
  command group used for repo development.
- `PLEC_BIN` forces a specific binary anywhere.

Out-of-repo apps get the same command by adding the built artifact folder as
a file dependency (`"@plec/core": "file:../path/to/packages/plec"`): yarn links
`node_modules/.bin/plec` and package scripts invoke the shim like any other
bin. Calling `node <artifact>/bin/plec.js …` directly works too.

If no Rust CLI binary is found for a non-`serve` command, the shim prints
every searched path and points at the release artifact build
(`yarn workspace @plec/core build:artifact`; `PLEC_CLI_VERSION` in `.env.plec`
selects the variant). `plec serve` uses the Node host and does not need that
binary.

## Version

The package carries the canonical Plec product SemVer in its `version` field.
`plec workspace version --set` / `--check` (dev CLI) keeps it aligned with the
Cargo workspace declaration and every Rust crate and JS workspace package.
The artifact build fails before assembling anything when the core package and
Cargo declaration drift, and `plec --version` prints the same value from the
compiled binary. Protocol versions (IR, route manifest, SSR snapshot)
are compatibility contracts and are deliberately not coupled to this SemVer.

## Commands

```sh
yarn workspace @plec/core build
yarn workspace @plec/core build:artifact
yarn workspace @plec/core test
yarn workspace @plec/core typecheck
```

## Runtime development

The Rust runtime crate remains [`crates/plec-runtime`](../../crates/plec-runtime).
`plec` publishes its validated WASM assets into `dist/runtime`, which is the
same package directory applications stage from through `node_modules/@plec/core`.

```sh
yarn workspace @plec/core build:runtime      # cargo check + WASM publish
yarn workspace @plec/core build:wasm         # WASM publish only
yarn workspace @plec/core dev:wasm           # watch and republish
yarn workspace @plec/core test:runtime       # cargo test --lib
yarn workspace @plec/core test:runtime:core  # cargo test --no-default-features
yarn workspace @plec/core test:wasm          # @plec/e2e browser harness
```

WASM builds use a temporary release directory, verify hashes, Brotli sidecars,
and the protocol marker, then publish `dist/runtime` as one complete tree.
Browser startup (`startPlecRouter`) lives in [`@plec/browser`](../plec-browser).
