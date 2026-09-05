# `plec`

The Plec framework runtime: the package applications import. It provides the
`plec` jsx-import-source, client state hooks, and the router primitives.

## Exports

- `plec` — `Fragment`, `jsx`, `jsxs`, `createRoot`, `useState`, `useRef`,
  `useHostRef`, `useReaction`, `useListener`, `useLocation`, `cookie`,
  router primitives (`createRootRoute`, `createRoute`, `createRouter`,
  `Link`, `Outlet`, `RouterProvider`, `useNavigate`)
- `plec/jsx-runtime` — the automatic JSX runtime (set
  `jsxImportSource: "plec"`)
- `plec/client/effects/development-memory-hud` — development-only memory HUD

## CLI

The `plec` command is owned by this package (`"bin"` → `bin/plec.js`): a
dependency-free Node shim that resolves the native CLI binary and execs it,
forwarding argv, stdio, and exit codes (Windows resolves `plec.exe`).

Resolution order:

1. `PLEC_BIN` — explicit binary override; authoritative, so a set-but-
   unusable value is an error instead of a silent fallback
2. `dist/bin/plec[.exe]` — the packaged binary assembled by
   `scripts/build-artifact.mjs` (**release** variant)
3. `~/.cargo/bin/plec[.exe]` — a cargo-installed binary (repo contributors;
   `yarn install:plec-cli:dev` installs the **dev** variant)

Which variant wins where:

- Inside `yarn`/`npx` in a project that depends on `plec`, the shim wins and
  serves the packaged **release** variant (app commands only: `inspect`,
  `raw`, `routes`, `build`).
- A bare `plec` in the shell resolves via the system `PATH`, typically the
  cargo-installed **dev** variant, which additionally carries the `workspace`
  command group used for repo development.
- `PLEC_BIN` forces a specific binary anywhere.

Out-of-repo apps get the same command by adding the built artifact folder as
a file dependency (`"plec": "file:../path/to/packages/plec"`): yarn links
`node_modules/.bin/plec` and package scripts invoke the shim like any other
bin. Calling `node <artifact>/bin/plec.js …` directly works too.

If no binary is found, the shim prints every searched path and points at the
source build (`yarn workspace plec build`; `PLEC_CLI_VERSION` in `.env.plec`
selects the variant).

## Commands

```sh
yarn workspace plec build
yarn workspace plec test
yarn workspace plec typecheck
```

This package is client-side TypeScript; it contains no server code. Browser
startup (`startPlecRouter`) lives in [`plec-browser`](../plec-browser).
