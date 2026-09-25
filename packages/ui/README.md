# `@plec/ui`

React/shadcn-based UI kit: Tailwind v4 styles, atoms (components), hooks,
and lib utilities. This is the React side of the workspace — ordinary React
components, not compiled by Plec — used by tooling and design work.

## Usage

Import via subpath exports or the `#` imports configured in
`package.json`:

- `@plec/ui/globals.css`
- `@plec/ui/atoms/*` (or `#atoms/*`)
- `@plec/ui/hooks/*` (or `#hooks/*`)
- `@plec/ui/lib/*` (or `#lib/*`)

Peer dependencies: `react`, `react-dom`.

Apps that consume Plec-compiled routes must stay React-free (the fullstack
app's `check:no-react` script enforces this); keep React usage inside this
package.
