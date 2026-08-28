# `plec-browser`

Browser-side glue: starts a Plec application in the DOM and coordinates
loading of the compiled artifacts.

## Exports

- `plec-browser` —
  `startPlecRouter({ root, manifestUrl, graphUrl, inputs, onQueryUpdate })`:
  fetches the route manifest, owns route matching, outlet replacement, and
  instance disposal; returns a handle with `dispose()`
- `plec-browser/compiled-subtree` — compiled-subtree entry

Depends on `comlink` and [`plec-ir`](../plec-ir).

## Commands

```sh
yarn workspace plec-browser build
yarn workspace plec-browser test
yarn workspace plec-browser typecheck
```
