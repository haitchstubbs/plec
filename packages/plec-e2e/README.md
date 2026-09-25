# @plec/e2e

Canonical Playwright runner for the monorepo. Playwright owns the fullstack
server: turbo builds `fullstack`, the `webServer` entry starts
`plec serve dist`, waits for HTTP readiness, and kills the process group
afterwards. Never spawn the server manually.

## Tiers

`test:wasm` runs the wasm-bindgen browser suite with the pinned Playwright
Chromium and ChromeDriver toolchain. `plec workspace test wasm` uses this
runner and owns output capture separately.

| Package script     | Repo root              | Contents                                                                      |
| ------------------ | ---------------------- | ----------------------------------------------------------------------------- |
| `test:e2e`         | `yarn test:e2e`        | `tests/smoke` — fast render/mount gate (the default agents run)               |
| `test:acceptance`  | `yarn test:acceptance` | `tests/acceptance` — full behavioral suites (opt-in)                          |
| `test:bench`       | `yarn bench`           | `tests/bench` — cold navigation benchmark into `benchmarks/results/` (opt-in) |
| `test:bench:smoke` | —                      | one-sample bench                                                              |
| `test`             | —                      | `vitest run` over pure helpers such as `bench-utils.ts`                       |

## Conventions

- Specs are named `*.playwright.ts`; `testMatch` enforces it in every
  config.
- The port comes from `E2E_PORT` in `.env.devports` (the canonical port
  registry). Override for parallel spikes without editing the file:
  `E2E_PORT=3311 yarn test:e2e`.
- `reuseExistingServer: false` is intentional: a stale server on the port
  fails the run loudly instead of being silently adopted.
- One-time setup: `yarn install --immutable && yarn install:build-tools` from
  the repository root. This installs the Playwright-pinned Chromium and
  matching ChromeDriver into repository-local caches; the runner rejects
  mismatched browser tooling before starting tests.
