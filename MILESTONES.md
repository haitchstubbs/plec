# Milestones

This project uses beads for issue tracking.
While in stealth mode, issues are centralised locally and backed up remotely.
A pre-commit hook ensures updates to the local issue tracker are captured here for visibility.

Once this project opens to new contributors, issues will be tracked publically.
In the meantime, issues can be subbmited via github and will be prioritised into the beads issue tracker.

## Issue Tracker

| ID | Title | Status | Priority | Type | Contributor | Labels |
| --- | --- | --- | --- | --- | --- | --- |
| wasm-runtime-0yt | Global typed_host_inputs loaderData slot goes stale across route navigations | open | P3 | bug |  |  |
| wasm-runtime-1nw | Bound mount recursion depth for deep acyclic node graphs | closed | P2 | task | haitchstubbs |  |
| **wasm-runtime-1wj** | **packages/plec: the release package — source-buildable CLI + framework toolkit** | open | P1 | epic |  |  |
| &nbsp;&nbsp;↳ wasm-runtime-1wj.1 | packages/plec release-package layout: self-contained artifact build | closed | P1 | task | haitchstubbs |  |
| &nbsp;&nbsp;↳ wasm-runtime-1wj.10 | Implement canonical Plec SemVer release versioning | open | P2 | task |  |  |
| &nbsp;&nbsp;↳ wasm-runtime-1wj.2 | Rename internal plec dev group to plec workspace | closed | P2 | task | haitchstubbs |  |
| &nbsp;&nbsp;↳ wasm-runtime-1wj.3 | Out-of-repo plec build: runtime staging + esbuild from the app's node_modules | closed | P1 | feature |  |  |
| &nbsp;&nbsp;↳ wasm-runtime-1wj.4 | bin shim: plec command owned by packages/plec | closed | P2 | task | haitchstubbs |  |
| &nbsp;&nbsp;↳ wasm-runtime-1wj.5 | plec init + application template | in_progress | P1 | feature | haitchstubbs |  |
| &nbsp;&nbsp;↳ wasm-runtime-1wj.6 | plec dev — consumer watch + serve loop | open | P1 | feature |  |  |
| &nbsp;&nbsp;↳ wasm-runtime-1wj.7 | Docs: install-from-source workflow for the release package | open | P2 | task |  |  |
| &nbsp;&nbsp;↳ wasm-runtime-1wj.8 | apps/fullstack as the canonical example built from real release assets | open | P1 | task |  |  |
| &nbsp;&nbsp;↳ wasm-runtime-1wj.9 | Eliminate the plec-runtime staging workspace package | in_progress | P1 | task | haitchstubbs |  |
| wasm-runtime-2fl | Sync cookie policy denial silently falls back to host inputs | closed | P1 | bug | haitchstubbs |  |
| wasm-runtime-43g | Split plec-runtime into plec-dom/plec-eval/plec-client/plec-router crates | closed | P1 | task |  |  |
| wasm-runtime-472 | plec dev impact: change-impact report for protocol constants and symbols | open | P2 | feature |  |  |
| wasm-runtime-4iq | plec-cli: select dev/app CLI at compile time from .env.plec | closed | P2 | task |  |  |
| **wasm-runtime-5pm** | **Implement shared Plec application build pipeline in plec-cli** | closed | P1 | task | haitchstubbs |  |
| &nbsp;&nbsp;↳ wasm-runtime-15c | Move into `plec-cli/src/com` | closed | P2 | task |  |  |
| &nbsp;&nbsp;↳ wasm-runtime-3az | Goal | closed | P2 | task |  |  |
| &nbsp;&nbsp;↳ wasm-runtime-8gq | Clean builds | closed | P2 | task |  |  |
| &nbsp;&nbsp;↳ wasm-runtime-h9f | HTML revision | closed | P2 | task |  |  |
| &nbsp;&nbsp;↳ wasm-runtime-it3 | Shared CLI ownership | closed | P2 | task |  |  |
| &nbsp;&nbsp;↳ wasm-runtime-j9d | Browser dependency validation | closed | P2 | task |  |  |
| &nbsp;&nbsp;↳ wasm-runtime-lmq | Leave application-owned | closed | P2 | task |  |  |
| &nbsp;&nbsp;↳ wasm-runtime-syk | Output | closed | P2 | task |  |  |
| wasm-runtime-6am | Track host-provider manifest as a first-class versioned boundary in the SSR contract scanner | closed | P3 | chore | haitchstubbs |  |
| wasm-runtime-6t2 | test:core fails to compile: typed/vm.rs unit test reads .body on () without fetch feature | closed | P2 | bug | haitchstubbs |  |
| wasm-runtime-7g4 | plec dev CLI: workflow commands (test capture, contract, trace, provenance, doctor) | closed | P1 | feature |  |  |
| wasm-runtime-8cd | Lowered responseJson fetch actions receive {ok,status,body} but programs consume the value directly | closed | P2 | bug | haitchstubbs |  |
| wasm-runtime-8ci | SSR pages paint styleless/flicker: unused Google Fonts @import render-blocks styles.css | closed | P1 | bug | haitchstubbs |  |
| wasm-runtime-8eo | Migrate plec-e2e to canonical Playwright runner | closed | P1 | task | haitchstubbs |  |
| wasm-runtime-8or | Fullstack client bundle imports the whole lucide icon set | closed | P2 | task | haitchstubbs |  |
| wasm-runtime-8vi | Stream-limit fetched and loader response bodies | closed | P2 | bug | haitchstubbs |  |
| wasm-runtime-8yy | Design file-based API routes (apps/fullstack/api) like Next's app/api | open | P3 | feature |  |  |
| wasm-runtime-96v | Rework router listener closures to drop raw-pointer state capture | closed | P3 | task |  |  |
| wasm-runtime-a08 | Generate TypeScript resource-limit constants from Rust | closed | P2 | task | haitchstubbs |  |
| **wasm-runtime-a2n** | **Deferred host-renderer capabilities** | closed | P2 | epic |  |  |
| &nbsp;&nbsp;↳ wasm-runtime-a2n.1 | Configure host import bindings | closed | P2 | task | haitchstubbs |  |
| &nbsp;&nbsp;↳ wasm-runtime-a2n.2 | Add SSR-capable host providers | closed | P2 | task | haitchstubbs |  |
| &nbsp;&nbsp;↳ wasm-runtime-a2n.3 | Support callback and event props in host components | closed | P2 | task | haitchstubbs |  |
| &nbsp;&nbsp;↳ wasm-runtime-a2n.4 | Scope host provider capability registries to Plec runtime instances | closed | P2 | task | haitchstubbs |  |
| wasm-runtime-a7h | Bound JS normalization width before JSON stringify | closed | P2 | bug | haitchstubbs |  |
| wasm-runtime-ahw | Client-side route navigation never seeds loader data into the mounted graph (notes page renders empty after graph swap) | closed | P2 | bug | haitchstubbs |  |
| wasm-runtime-ayk | Bound runtime amplification and aggregate application budgets | closed | P2 | bug | haitchstubbs |  |
| wasm-runtime-b1i | Wire the native plec-server crate into dev/build workflows and retire packages/plec-server | closed | P2 | task |  |  |
| wasm-runtime-b50 | Unify route matching and support nested route-chain SSR | closed | P2 | feature | haitchstubbs |  |
| wasm-runtime-buy | Remove or implement crates/plec-diagnostics stub | closed | P3 | chore |  |  |
| wasm-runtime-ctb | plec dev graph resolve/tree: registry resolution debugger | closed | P2 | feature | haitchstubbs |  |
| wasm-runtime-d5q | Make loader error retry acceptance deterministic with SSR | closed | P2 | task | haitchstubbs |  |
| wasm-runtime-dvs | plec dev context <domain>: condensed architecture packets for agents | open | P3 | feature |  |  |
| wasm-runtime-e7f | Move WASM test orchestration under packages/plec-e2e Playwright ownership | open | P3 | chore |  |  |
| wasm-runtime-e92 | test:core fails to compile: typed/vm.rs unit test reads .body on () without fetch feature | closed | P2 | bug | haitchstubbs |  |
| wasm-runtime-f93 | Navigation froze location-derived component props (sidebar aria-current stuck on first-load page) | closed | P1 | bug |  |  |
| wasm-runtime-fe3 | Update loader success acceptance for SSR | closed | P2 | task | haitchstubbs |  |
| wasm-runtime-gca | Retire packages/plec-server TS host once the Rust host is canonical | closed | P3 | task |  |  |
| wasm-runtime-hv3 | Adopted loop rows re-evaluate with lost row scope (editing branch + empty title + dead rebinds) | closed | P1 | bug | haitchstubbs |  |
| wasm-runtime-ieh | Finish lifting compile/build pipeline from plec-cli into plec-compiler and plec-build | closed | P1 | task | haitchstubbs |  |
| wasm-runtime-ipk | node_chain_within_mount_depth_limit_still_mounts crashes the wasm test harness | closed | P1 | bug | haitchstubbs |  |
| wasm-runtime-ivo | Fix pre-existing prettier failures in scripts/watch-wasm.mjs and apps/fullstack/scripts/check-no-react.mjs | closed | P3 | chore | haitchstubbs |  |
| **wasm-runtime-ixk** | **Harden SSR adoption and DOM identity invariants** | closed | P1 | epic |  | dom-runtime, ssr |
| &nbsp;&nbsp;↳ wasm-runtime-ixk.1 | Record SSR execution state for nested component conditionals and loops | closed | P1 | feature | haitchstubbs | dom-runtime, ssr |
| **wasm-runtime-ixk.2** | **Make structural DOM address an explicit protocol: canonical SSR/CSR grammar, retire data-runtime-node competitors** | closed | P1 | task | haitchstubbs | dom-runtime, ssr |
| &nbsp;&nbsp;&nbsp;&nbsp;↳ wasm-runtime-ixk.2.1 | ixk.2a: Measure CSR instantiation cost baseline (pre-migration) | closed | P1 | task | haitchstubbs | dom-runtime, ssr |
| &nbsp;&nbsp;&nbsp;&nbsp;↳ wasm-runtime-ixk.2.2 | ixk.2b: Emit canonical path-qualified addresses from CSR typed runtime | closed | P1 | task | haitchstubbs | dom-runtime, ssr |
| &nbsp;&nbsp;&nbsp;&nbsp;↳ wasm-runtime-ixk.2.3 | ixk.2c: Encode one-shot adoption lifecycle invariant + mixed-DOM test | closed | P1 | task | haitchstubbs | dom-runtime, ssr |
| &nbsp;&nbsp;&nbsp;&nbsp;↳ wasm-runtime-ixk.2.4 | ixk.2d: Classify and migrate data-runtime-node consumers | closed | P1 | task | haitchstubbs | dom-runtime, ssr |
| &nbsp;&nbsp;&nbsp;&nbsp;↳ wasm-runtime-ixk.2.5 | ixk.2e: Write authoritative structural DOM address protocol document | closed | P1 | task | haitchstubbs | dom-runtime, ssr |
| &nbsp;&nbsp;&nbsp;&nbsp;↳ wasm-runtime-ixk.2.6 | ixk.2f: Update tests to canonical addresses and verify gates | closed | P1 | task | haitchstubbs | dom-runtime, ssr |
| &nbsp;&nbsp;↳ wasm-runtime-ixk.3 | Fail loudly on typed graph instance id collisions instead of silently overwriting | closed | P2 | bug | haitchstubbs | dom-runtime, ssr |
| &nbsp;&nbsp;↳ wasm-runtime-ixk.4 | Minimise DOM movement in keyed loop reorder | closed | P2 | task | haitchstubbs | dom-runtime, performance, ssr |
| &nbsp;&nbsp;↳ wasm-runtime-ixk.5 | Reserve and validate Plec's DOM metadata namespace | closed | P2 | task | haitchstubbs | compiler, dom-runtime, ssr |
| &nbsp;&nbsp;↳ wasm-runtime-ixk.6 | Test and document the SSR text-marker adjacency invariant | closed | P3 | task | haitchstubbs | dom-runtime, ssr, testing |
| &nbsp;&nbsp;↳ wasm-runtime-ixk.7 | Isolate and document the legacy 0.8/0.9 marker and adoption schemes | closed | P3 | chore | haitchstubbs | dom-runtime, ssr, tech-debt |
| wasm-runtime-jfd | plec-e2e: pre-existing tsc strict errors in tests | closed | P3 | task | haitchstubbs |  |
| wasm-runtime-lcy | plec dev markers explain/validate: executable DOM address protocol | open | P2 | feature |  |  |
| **wasm-runtime-moq** | **Plec SSR execution/adoption contract: resumable HTML + typed execution snapshot** | closed | P1 | epic |  | ssr |
| &nbsp;&nbsp;↳ wasm-runtime-moq.1 | Freeze typed SSR snapshot contract v1 in plec-ir (Rust-owned schema + validation) | closed | P1 | feature | haitchstubbs | rust, ssr |
| &nbsp;&nbsp;↳ wasm-runtime-moq.10 | Retire fallback-only SSR assumptions superseded by the execution contract | closed | P3 | task | haitchstubbs | cleanup, ssr |
| &nbsp;&nbsp;↳ wasm-runtime-moq.11 | SSR snapshot contract v1: typed schema + validation in plec-ir | closed | P1 | feature | haitchstubbs | ssr |
| &nbsp;&nbsp;↳ wasm-runtime-moq.2 | Browser + WASM snapshot import pipeline (validated initial execution state before adoption) | closed | P1 | feature | haitchstubbs | browser, rust, ssr |
| &nbsp;&nbsp;↳ wasm-runtime-moq.3 | Canonical route execution identity: server-published route chain + params, browser stops re-matching | closed | P1 | feature | haitchstubbs | browser, rust, server, ssr |
| &nbsp;&nbsp;↳ wasm-runtime-moq.4 | SSR public export collection + server-only gating (wire ExecutionOwner/PublicExport) | closed | P1 | feature | haitchstubbs | rust, server, ssr |
| &nbsp;&nbsp;↳ wasm-runtime-moq.5 | Loader state transfer: SSR executes route loaders, browser resumes instead of re-running | closed | P2 | feature | haitchstubbs | rust, server, ssr |
| &nbsp;&nbsp;↳ wasm-runtime-moq.6 | Indexed adoption ownership map with duplicate-marker validation | closed | P2 | task | haitchstubbs | rust, ssr |
| &nbsp;&nbsp;↳ wasm-runtime-moq.7 | Conditional structural ownership: adopt the instantiated branch | closed | P2 | feature | haitchstubbs | rust, server, ssr |
| &nbsp;&nbsp;↳ wasm-runtime-moq.8 | Keyed loop structural ownership: server renders rows, runtime claims them | closed | P2 | feature | haitchstubbs | rust, server, ssr |
| &nbsp;&nbsp;↳ wasm-runtime-moq.9 | SSR adoption contract test matrix + divergence diagnostics | closed | P2 | task | haitchstubbs | ssr, testing |
| wasm-runtime-n2l | Execute route loaders through shared Rust action semantics | closed | P2 | task | haitchstubbs |  |
| **wasm-runtime-omk** | **Security hardening for executable runtime** | closed | P1 | epic |  |  |
| &nbsp;&nbsp;↳ wasm-runtime-omk.1 | Constrain executable DOM binding sinks | closed | P1 | bug | haitchstubbs |  |
| &nbsp;&nbsp;↳ wasm-runtime-omk.10 | Restrict executable IR element tags | closed | P1 | bug | haitchstubbs |  |
| &nbsp;&nbsp;↳ wasm-runtime-omk.11 | Apply bounded decoding to snapshot facade APIs | closed | P2 | bug | haitchstubbs |  |
| &nbsp;&nbsp;↳ wasm-runtime-omk.12 | Close residual executable IR allocation and recursion DoS | closed | P1 | bug | haitchstubbs |  |
| &nbsp;&nbsp;↳ wasm-runtime-omk.13 | Require bounded streamed response decoding | closed | P1 | bug | haitchstubbs |  |
| &nbsp;&nbsp;↳ wasm-runtime-omk.14 | Finish compiler source-boundary and aggregate resource controls | closed | P2 | bug | haitchstubbs |  |
| &nbsp;&nbsp;↳ wasm-runtime-omk.2 | Remove router listener raw-pointer lifetime hazard | closed | P1 | bug | haitchstubbs |  |
| &nbsp;&nbsp;↳ wasm-runtime-omk.3 | Bound runtime and compiler untrusted resources | closed | P2 | bug | haitchstubbs |  |
| &nbsp;&nbsp;↳ wasm-runtime-omk.4 | Make host capabilities default-deny | closed | P1 | bug | haitchstubbs |  |
| &nbsp;&nbsp;↳ wasm-runtime-omk.5 | Contain compiler source-graph filesystem traversal | closed | P2 | bug | haitchstubbs |  |
| &nbsp;&nbsp;↳ wasm-runtime-omk.6 | Validate IR topology and execution bounds | closed | P1 | bug | haitchstubbs |  |
| &nbsp;&nbsp;↳ wasm-runtime-omk.7 | Harden SSR tag and attribute serialization | closed | P1 | bug | haitchstubbs |  |
| &nbsp;&nbsp;↳ wasm-runtime-omk.8 | Escape build metadata and verify staged runtime assets | closed | P2 | bug | haitchstubbs |  |
| &nbsp;&nbsp;↳ wasm-runtime-omk.9 | Isolate synchronous cookie grants per runtime | closed | P1 | bug | haitchstubbs |  |
| wasm-runtime-pp1 | Author and enforce shared, server, and client execution ownership | closed | P2 | epic |  |  |
| wasm-runtime-r0l | Compiler SSR drops island component props: SVG icons paint at 24px unclassed, then resize to 16px after adoption | closed | P2 | bug | haitchstubbs |  |
| wasm-runtime-rz7 | Anchor-click interception intermittently falls through to a full page navigation during client-side route swaps | closed | P2 | bug | haitchstubbs |  |
| wasm-runtime-sum | Fix chromedriver/playwright Chromium version drift breaking test harness | closed | P1 | bug | haitchstubbs |  |
| wasm-runtime-tfk | plec dev verify adoption: one-shot validation pipeline | open | P2 | feature |  |  |
| wasm-runtime-ujj | Investigate runtime baseline nondeterminism | open | P2 | bug |  |  |
| wasm-runtime-vdd | Wire the native plec-server crate into dev/build workflows and retire packages/plec-server | closed | P2 | task |  |  |
