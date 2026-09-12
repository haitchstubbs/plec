## Objective

Implement a simple, fail-closed Rust dependency security layer for the `feat/ssr` branch.

The implementation should:

1. Pin Rust/Cargo tooling by:

   * semantic version
   * upstream Git repository
   * exact full Git commit SHA
2. Install pinned Cargo tools from that exact Git commit, not merely from crates.io by version.
3. Install those tools repo-locally under `.tools/`, not globally.
4. Add:

   * `cargo-deny`
   * `cargo-audit`
   * `cargo-lock`
5. Add a committed `deny.toml`.
6. Add one repository-owned Rust dependency security entrypoint.
7. Ensure all meaningful Rust builds use `--locked`.
8. Run fast deterministic dependency checks before normal compilation.
9. Run current RustSec vulnerability checks at release/CI/security-audit boundaries.
10. Keep the implementation small and consistent with the existing `scripts/`, `cli-tools.json`, `setup.mjs`, and `verify-toolchain.mjs` architecture.

Do not introduce `cargo-vet` in this issue.

---

# Existing repository architecture

Work against `feat/ssr`.

Important existing files:

* root `Cargo.toml`
* root committed `Cargo.lock`
* root `cli-tools.json`
* `scripts/setup.mjs`
* `scripts/toolchain.mjs`
* `scripts/verify-toolchain.mjs`
* `scripts/build-wasm.mjs`
* `scripts/browser-harness.mjs`
* `scripts/watch-wasm.mjs`
* root `package.json`
* `packages/plec-runtime/package.json`
* `packages/plec/scripts/build-artifact.mjs`
* root `turbo.json`

The repository already follows a useful pattern:

```text
cli-tools.json
      │
      ▼
scripts/setup.mjs
      │
      ▼
scripts/verify-toolchain.mjs
      │
      ▼
build/test tooling
```

Extend that architecture rather than creating a parallel tool-management system.

Existing hardening already present:

* root `Cargo.lock` is committed
* `verify-toolchain.mjs` runs `cargo metadata --locked`
* `plec-runtime` cargo check/test commands use `--locked`
* `scripts/build-wasm.mjs` forwards `--locked` through `wasm-pack`

Known gaps:

* `packages/plec/scripts/build-artifact.mjs` currently performs a release `cargo build` without `--locked`
* root `install:plec-cli` commands omit `--locked`
* `scripts/browser-harness.mjs` does not currently forward `--locked` through `wasm-pack test`

Fix these as part of this issue.

---

# Core design decision: commit pins must be enforceable

Do not merely add a `"commit"` string next to the existing version and continue accepting arbitrary globally installed binaries.

That would provide little security because this:

```text
wasm-pack --version
→ 0.13.1
```

cannot establish whether the binary was actually built from the expected Git commit.

Instead, generalize the repo-local tooling pattern already used for `wasm-bindgen-cli`.

Pinned Cargo tools should be installed into `.tools/` from their exact upstream Git commit.

Conceptually:

```text
.tools/
└── cargo/
    ├── wasm-pack/<commit>/bin/wasm-pack
    ├── wasm-tools/<commit>/bin/wasm-tools
    ├── wasm-bindgen-cli/<commit>/bin/wasm-bindgen
    ├── cargo-deny/<commit>/bin/cargo-deny
    ├── cargo-audit/<commit>/bin/cargo-audit
    └── cargo-lock/<commit>/bin/cargo-lock
```

`.tools/` is already ignored by Git.

Build scripts should use these exact paths rather than whichever matching executable appears first in the user's global PATH.

---

# Phase 1 — Extend `cli-tools.json`

Migrate Cargo-installed tools from simple version strings to structured records.

Use a schema along these lines:

```json
{
  "build-tools": {
    "wasm-pack": {
      "version": "0.13.1",
      "repository": "UPSTREAM_GIT_REPOSITORY",
      "commit": "FULL_40_CHARACTER_COMMIT_SHA"
    },
    "wasm-tools": {
      "version": "1.258.0",
      "repository": "UPSTREAM_GIT_REPOSITORY",
      "commit": "FULL_40_CHARACTER_COMMIT_SHA"
    },
    "wasm-bindgen-cli": {
      "version": "0.2.128",
      "repository": "UPSTREAM_GIT_REPOSITORY",
      "commit": "FULL_40_CHARACTER_COMMIT_SHA"
    }
  },

  "security-tools": {
    "cargo-deny": {
      "version": "<PINNED_VERSION>",
      "repository": "UPSTREAM_GIT_REPOSITORY",
      "commit": "FULL_40_CHARACTER_COMMIT_SHA"
    },
    "cargo-audit": {
      "version": "<PINNED_VERSION>",
      "repository": "UPSTREAM_GIT_REPOSITORY",
      "commit": "FULL_40_CHARACTER_COMMIT_SHA"
    },
    "cargo-lock": {
      "version": "<PINNED_VERSION>",
      "repository": "UPSTREAM_GIT_REPOSITORY",
      "commit": "FULL_40_CHARACTER_COMMIT_SHA",
      "features": ["cli"]
    }
  }
}
```

Keep the existing `runtime` and `browser-tools` sections conceptually unchanged.

Optional fields supported by the Cargo-tool schema should be limited to what we actually need:

```text
version
repository
commit
package?
binary?
features?
```

Default `package` and `binary` to the JSON object key.

Do not turn `cli-tools.json` into a generic package manager.

### Commit resolution requirements

For every migrated tool:

1. Identify the canonical upstream Git repository.
2. Find the Git commit corresponding to the pinned released version/tag.
3. Record the full 40-character SHA.
4. Verify that the package at that commit reports the declared version.
5. Do not guess a SHA.
6. Do not use abbreviated SHAs.
7. Do not assume a tag name itself is an immutable pin.

If an upstream release tag is annotated, resolve it to the actual commit object.

The version and commit serve separate purposes:

```text
commit  → immutable source identity
version → sanity check that the pinned source is the expected release
```

---

# Phase 2 — Generalize repo-local Cargo tool paths

Update `scripts/toolchain.mjs`.

Introduce helpers similar to the existing wasm-bindgen helpers:

```js
cargoToolRoot(name)
cargoToolPath(name, binary?)
cargoToolBinPath(name)
```

The root should include the pinned commit so changing the pin naturally creates a new isolated installation.

For example:

```text
.tools/cargo/cargo-deny/<commit>/
```

or an equivalently deterministic structure.

The path must derive entirely from `cli-tools.json`.

Keep the wasm-bindgen helpers if useful for compatibility, but preferably implement them through the generalized Cargo-tool helper.

Build code should not manually reconstruct `.tools/...` paths.

---

# Phase 3 — Harden `scripts/setup.mjs`

Replace the existing global Cargo CLI installation behavior with repo-local exact-revision installation.

For a normal pinned tool, setup should conceptually execute:

```bash
cargo install \
  --git <repository> \
  --rev <full-commit-sha> \
  --locked \
  --root <repo-local-tool-root> \
  <package>
```

Add:

```text
--features feature1,feature2
```

where configured.

Do not use `--version` as the source selector when installing from Git.

After installation:

1. execute the repo-local binary with `--version`
2. confirm the reported version equals `cli-tools.json`
3. fail setup if it does not
4. never silently fall back to a global binary

### Existing installations

A repo-local installation may be reused only when:

* expected executable exists
* it lives under the expected commit-specific tool root
* reported version matches the configured version

Do not consider a matching global binary sufficient.

### Optional installation receipt

Add a small machine-generated receipt inside the tool root if it keeps verification clean, e.g.:

```json
{
  "package": "cargo-deny",
  "version": "...",
  "repository": "...",
  "commit": "...",
  "features": []
}
```

This receipt is local state under ignored `.tools/`.

It is supplementary metadata; the trusted source of configuration remains committed `cli-tools.json`.

Do not commit generated receipts.

---

# Phase 4 — Extend `verify-toolchain.mjs`

Verify all pinned Cargo tooling from its repo-local path.

For each Cargo tool:

* executable exists
* executable is the expected repo-local executable
* reported version exactly matches `cli-tools.json`

Do not use PATH discovery for these tools.

Continue validating:

* Node
* Yarn
* Rust
* Cargo.lock resolution
* browser tooling when `--browser` is supplied

The existing:

```bash
cargo metadata --locked --format-version 1 --no-deps
```

lockfile validation should remain.

If appropriate, drop `--no-deps` only if there is a concrete reason to validate the complete resolved graph here; otherwise keep toolchain verification lightweight.

Do not run vulnerability database updates from `verify-toolchain.mjs`.

Toolchain verification and dependency security are separate concerns.

---

# Phase 5 — Add `deny.toml`

Create root:

```text
deny.toml
```

Start with a strict but maintainable policy.

Use `cargo-deny` for:

* dependency source provenance
* licenses
* explicit bans
* wildcard version detection
* build-time executable/interpreted-content checks
* RustSec advisories where applicable

Policy principles:

### Sources

Fail closed on unexpected dependency sources.

Allow crates.io.

For Git dependencies:

* deny unknown Git sources by default
* if Git dependencies are intentionally introduced later, require review
* require immutable revision pins rather than branches/tags wherever cargo-deny supports enforcement

### Versions

Deny wildcard dependency versions.

Do not initially make duplicate package versions a hard failure unless the existing graph is already clean enough to support that without substantial unrelated cleanup.

Warning is acceptable initially for duplicate versions.

### Licenses

Build the license allowlist from licenses actually used by the existing Plec dependency graph.

Do not paste a giant generic license allowlist.

Any exception should:

* be narrow
* include a reason
* be reviewable in source control

### Build-time behaviour

Enable cargo-deny's build-time checking approximately along these lines:

```toml
[bans.build]
executables = "deny"
interpreted = "deny"
enable-builtin-globs = true
include-dependencies = true
include-archives = true
```

Confirm exact syntax against the pinned cargo-deny version.

Do not initially create a giant explicit build-script allowlist.

Document the limitation that cargo-deny is a policy/static-content check, not a sandbox: malicious ordinary Rust inside a `build.rs` or proc macro cannot be comprehensively detected this way.

---

# Phase 6 — Add one Rust dependency security entrypoint

Create:

```text
scripts/check-rust-deps.mjs
```

This is the repository's public façade for Rust dependency security.

Other scripts should not need to understand three separate tools.

Support two modes.

## Default / fast mode

Designed to run before normal compilation.

Run:

```text
1. Cargo lockfile resolution check
2. cargo-deny deterministic policy checks
```

Conceptually:

```bash
cargo metadata --locked --format-version 1

cargo deny check \
  licenses \
  bans \
  sources
```

Use exact repo-local tool paths from `toolchain.mjs`.

The normal compile gate should avoid unnecessary live network-dependent advisory refreshes.

The fast gate should answer:

```text
Is the lockfile stable?
Are dependency sources permitted?
Are licenses permitted?
Are dependency policies satisfied?
Are obvious prohibited build-time contents present?
```

## Full mode

Expose:

```bash
node scripts/check-rust-deps.mjs --full
```

Run everything in fast mode plus current vulnerability/advisory checks.

Use both:

```text
cargo-deny advisories
cargo-audit
```

Configure `cargo-audit` to fail on vulnerabilities and other categories we intentionally decide should be release blocking, including yanked/unsound advisories where supported by the pinned version.

Avoid duplicate output where practical, but using both tools is intentional:

```text
cargo-deny  → repository dependency policy
cargo-audit → dedicated RustSec vulnerability audit
```

Do not treat `cargo-lock` as another vulnerability scanner.

---

# Phase 7 — Add useful root commands

Update root `package.json`.

Add simple developer-facing commands along these lines:

```json
{
  "scripts": {
    "rust:deps:check": "node scripts/check-rust-deps.mjs",
    "rust:deps:audit": "node scripts/check-rust-deps.mjs --full",
    "rust:deps:list": "<repo-local cargo-lock> list --source",
    "rust:deps:tree": "<repo-local cargo-lock> tree"
  }
}
```

Prefer small Node wrappers/helpers rather than hard-coding `.tools/...` paths into `package.json` if needed.

`cargo-lock` is an inspection/debugging utility.

Its purpose here is to make dependency investigations easy, e.g.:

```text
cargo lock list
cargo lock tree
```

For inverse dependency reasoning, normal Cargo can still be used:

```bash
cargo tree -i <crate>
```

---

# Phase 8 — Put the fast gate before compilation

The desired invariant is:

```text
Rust dependency policy
        │
        ▼
Rust compilation
```

At minimum, root build should become logically:

```text
yarn rust:deps:check
        │
        ▼
turbo run build
```

However, account for direct package entrypoints.

A developer can currently run things such as:

```bash
yarn workspace plec-runtime build
```

without going through root `yarn build`.

Ensure the relevant direct Rust build entrypoints also invoke the fast dependency gate.

Avoid blindly inserting the check before every individual Cargo invocation.

The goal is one check per meaningful build boundary, not repeated scanning throughout the same Turbo build.

Be conscious of existing duplication in:

```text
verify-toolchain
plec-runtime build
build:wasm
```

Do not make this substantially worse.

If useful, use an environment marker for an already-completed dependency check within the current process tree, but only if this is actually necessary. Prefer simpler wiring first.

---

# Phase 9 — Enforce `--locked` everywhere Rust resolution matters

Audit the branch for Rust build/test/install entrypoints.

Fix known gaps.

## `packages/plec/scripts/build-artifact.mjs`

Change the release CLI build from conceptually:

```bash
cargo build --release -p plec-cli
```

to:

```bash
cargo build --locked --release -p plec-cli
```

## root CLI installation scripts

Change:

```bash
cargo install --path crates/plec-cli
```

to include:

```text
--locked
```

Likewise for the dev install.

## `scripts/browser-harness.mjs`

Ensure `wasm-pack test` forwards:

```text
--locked
```

to Cargo.

Preserve existing test arguments correctly.

## `scripts/build-wasm.mjs`

This already forwards `--locked`.

Preserve it.

## `packages/plec-runtime/package.json`

Existing `cargo check` and `cargo test` commands already use `--locked`.

Preserve them.

### Final invariant

After this issue:

> No repository-owned command that builds/tests/installs workspace Rust code may silently update dependency resolution.

---

# Phase 10 — Release hardening

The actual Plec release artifact should require a full Rust dependency audit.

`packages/plec/scripts/build-artifact.mjs` produces the release package and native CLI binary, so make the full dependency audit a release boundary.

Before the release Cargo compilation, ensure:

```text
scripts/check-rust-deps.mjs --full
```

has succeeded.

Expected flow:

```text
build-artifact.mjs
        │
        ├─ dependency/tool prerequisites
        │
        ▼
full Rust dependency audit
        │
        ├─ lockfile valid
        ├─ dependency sources valid
        ├─ license policy valid
        ├─ dependency bans valid
        ├─ build-time policy valid
        ├─ RustSec advisories clean
        └─ cargo-audit clean
        │
        ▼
cargo build --locked --release -p plec-cli
        │
        ▼
assemble artifact
        │
        ▼
existing artifact self-containment checks
```

A releasable Plec artifact must not be producible by the normal release build while bypassing the dependency audit.

---

# Phase 11 — CI / scheduled audit

There does not appear to currently be a `.github/workflows` setup on `feat/ssr`.

If introducing CI is within scope, add a small workflow for Rust dependency security.

Run full auditing when any of these change:

```text
Cargo.lock
Cargo.toml
crates/**/Cargo.toml
deny.toml
cli-tools.json
scripts/check-rust-deps.mjs
scripts/setup.mjs
scripts/toolchain.mjs
scripts/verify-toolchain.mjs
```

Also run it on a schedule.

A dependency can become vulnerable long after `Cargo.lock` was committed, so dependency-change-only CI is insufficient.

A daily or weekly scheduled RustSec check is acceptable.

If GitHub Actions creation would substantially expand this issue, leave a clearly scoped follow-up rather than bloating the core implementation.

---

# Phase 12 — Security exception policy

Keep exceptions visible and reviewable.

For any ignored advisory or policy exception:

* identify the exact crate/advisory
* include a reason
* avoid broad wildcard ignores
* remove the exception when the underlying dependency is upgraded

For Git dependencies:

* exact immutable revisions only
* no branch-based trust

For tool pins:

* use full 40-character commits
* version must match the source at that commit
* updating a tool requires reviewing both:

  * new version
  * new commit SHA

This makes a tool update an explicit source-controlled security change.

---

# Expected responsibility split

Maintain this mental model:

```text
cli-tools.json
    immutable tool source + version policy

setup.mjs
    provision exact tools

verify-toolchain.mjs
    prove expected local tools are present

Cargo.lock
    exact application dependency resolution

--locked
    prevent silent resolution changes

cargo-deny
    dependency policy / provenance / licenses / bans /
    build-time static red flags

cargo-audit
    current RustSec vulnerability intelligence

cargo-lock
    lockfile inspection / incident investigation
```

Do not blur these responsibilities unnecessarily.

---

# Expected files changed

Likely:

```text
cli-tools.json
deny.toml                              new
scripts/toolchain.mjs
scripts/setup.mjs
scripts/verify-toolchain.mjs
scripts/check-rust-deps.mjs            new
scripts/build-wasm.mjs                 maybe path plumbing only
scripts/browser-harness.mjs
package.json
packages/plec-runtime/package.json
packages/plec/scripts/build-artifact.mjs
```

Possibly:

```text
.github/workflows/rust-deps.yml        new, if CI is in scope
```

Do not modify unrelated Plec compiler/runtime functionality.

---

# Tests / acceptance criteria

The implementation is complete when all of the following hold.

### Tool source pinning

Changing a tool's configured commit causes setup to install/use the new commit-specific tool root.

A random globally installed `cargo-deny`, `wasm-pack`, etc. cannot satisfy toolchain verification.

A configured tool with:

```text
version = X
commit = Y
```

must:

* originate from commit `Y` via setup
* report version `X`

Invalid/mismatched metadata fails closed.

### Setup

From a clean `.tools/`:

```bash
yarn setup
```

provisions all pinned Cargo tooling repo-locally.

Running setup again does not unnecessarily reinstall valid tools.

### Toolchain verification

```bash
yarn toolchain:verify
```

validates the repo-local Cargo tools and existing runtime/toolchain pins.

Removing one expected local security binary causes verification to fail with an actionable message.

### Dependency checks

```bash
yarn rust:deps:check
```

passes on the current accepted dependency graph.

```bash
yarn rust:deps:audit
```

runs the current advisory checks and returns non-zero for a blocking RustSec finding.

### Build gating

Normal Rust build entrypoints cannot proceed when deterministic dependency policy fails.

Release artifact construction cannot proceed when the full dependency audit fails.

### Lockfile integrity

All repository-owned Rust build/test/install flows use `--locked` or otherwise demonstrably preserve equivalent locked semantics.

Deliberately making `Cargo.toml` inconsistent with `Cargo.lock` causes build/security commands to fail rather than rewrite the lockfile.

### Source policy

A deliberately introduced unapproved Git dependency fails policy checks.

A wildcard dependency version fails policy checks.

### Build-time policy

A fixture/test demonstrating prohibited executable/interpreted dependency content should fail cargo-deny if reasonably practical with the pinned cargo-deny version.

Do not create a large synthetic security test framework solely for this.

---

# Non-goals

Do not:

* introduce `cargo-vet`
* build a custom malware scanner
* attempt to sandbox `build.rs`
* attempt to sandbox proc macros
* globally install the new security tooling
* replace Cargo's lockfile model
* duplicate RustSec databases
* implement automatic dependency upgrades
* introduce broad unrelated dependency cleanup
* refactor the Plec build architecture beyond what is necessary for this gate
* treat commit pinning as proof that upstream source itself is trustworthy

The purpose is narrower:

> Make Plec's Rust dependency resolution reproducible, its allowed dependency graph explicit, its known vulnerabilities detectable, and the tooling enforcing those rules itself pinned to immutable upstream source commits.

---

# Implementation quality constraint

Prefer one clear path over layers of wrappers.

The desired developer experience should remain approximately:

```bash
yarn setup
yarn build
```

with optional explicit security commands:

```bash
yarn rust:deps:check
yarn rust:deps:audit
```

Developers should not need to know where repo-local Cargo security binaries live or manually compose cargo-deny/cargo-audit invocations.

Before finishing, run the relevant setup, toolchain verification, Rust dependency checks, normal build, Rust tests, and release-artifact build paths and report:

1. exact files changed
2. final `cli-tools.json` schema
3. upstream repositories + full commit SHAs selected for each Cargo tool
4. commands used to verify release-version ↔ commit correspondence
5. any existing dependency-policy violations discovered
6. any exceptions added to `deny.toml` and why
7. any remaining Rust command that intentionally does not use `--locked`, with justification
