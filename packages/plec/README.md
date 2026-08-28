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

## Commands

```sh
yarn workspace plec build
yarn workspace plec test
yarn workspace plec typecheck
```

This package is client-side TypeScript; it contains no server code. Browser
startup (`startPlecRouter`) lives in [`plec-browser`](../plec-browser).
