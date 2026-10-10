# Plec browser WASM runtime size investigation

**Scope:** measurement and analysis only. No production runtime implementation was changed.  
**Checkout:** `ad884fe` (`feat/no-serde-in-wasm` merge), clean before the production build.  
**Date:** 2026-10-11.

## Executive finding

The normal production pipeline independently reproduces the reported artifact exactly: `runtime_bg.wasm` is **753,605 bytes raw, 303,282 bytes gzip -9, and 239,018 bytes Brotli quality 11**. The reported “239 KB” means Brotli-compressed WASM alone, not JS glue. The adjacent generated `runtime.js` is 58,783 raw / 8,081 Brotli bytes, putting the two-file compressed runtime payload at **247,099 bytes** (before HTTP headers or app-specific assets).

The binary is not accidentally carrying the old Serde decoding path. The production runtime is a full interpreter/runtime artifact, rather than a small per-app bundle: it exports 39 functions, has 1,884 defined functions, 684,832 bytes of code-section payload and 54,486 bytes of data-section payload. A symbol-preserving analysis build now maps the largest linked functions back to crates (details below); it is intentionally not byte-comparable to the shipped artifact. The strongest measured optional-capability lever is `fetch`: the core (no defaults) profile saves **19,958 Brotli WASM bytes** and **20,847 Brotli bytes including generated JS glue**. Router-only versus core produced no size delta in this build.

The best next step before v0.1 is not another serializer rewrite or aggressive compiler flags. Keep full semantics by default, consider an explicitly opt-in core runtime only if its feature contract is already suitable for applications, and investigate focused code/data reduction around typed decode/validation and error text with per-change build measurements. Further large reductions are likely to require capability/deployment trade-offs or changing the runtime delivery model.

## A. Production baseline and reproduction

### Artifact and sizes

| Delivered file                               | Raw bytes | gzip -9 bytes | Brotli Q11 bytes |
| -------------------------------------------- | --------: | ------------: | ---------------: |
| `packages/plec/dist/runtime/runtime_bg.wasm` |   753,605 |       303,282 |          239,018 |
| `packages/plec/dist/runtime/runtime.js`      |    58,783 |             — |            8,081 |
| Combined compressed payload                  |         — |             — |      **247,099** |

The WASM is composed of 684,832 bytes in the code section, 54,486 bytes in data, 9,474 bytes of imports, 1,557 bytes of exports, 671 bytes of types, plus table/memory/global/element framing and the custom `plec-protocol` section. Those are section payloads/section ranges from `wasm-tools objdump`, not exclusive Rust feature attributions. The final 33 bytes over the 753,572-byte optimized/stripped output are the protocol section (17-byte section record plus 16-byte payload).

### Build settings and commands

Production artifact was produced with:

```sh
source ~/.nvm/nvm.sh && nvm use 22.20.0
yarn workspace @plec/core build:wasm
```

`packages/plec/scripts/build-runtime.mjs` invokes `scripts/build-wasm.mjs` with the default `full` profile into a temporary release directory, then publishes to `packages/plec/dist/runtime`. The shared builder invokes `wasm-pack build crates/plec-runtime --target web --out-name runtime -- --locked`; `full` leaves Cargo default features enabled. It then runs the pinned wasm-pack optimization, followed by Binaryen `wasm-opt -Oz --strip-debug --strip-producers`, `wasm-tools strip --all`, protocol stamping, and Brotli sidecar generation at quality 11. The code uses maximum Node Brotli quality. The generated JS glue is separate from the `.wasm` number.

Relevant Cargo profile (`Cargo.toml`): `release` uses `opt-level = "z"`, fat LTO, one codegen unit, and `panic = "abort"`. There is no production `debug = true` setting; final metadata is stripped. `wasm-pack`’s release build and explicit second `-Oz` pass both apply. Source profile includes `router` and `fetch` by default (`crates/plec-runtime/Cargo.toml`). The protocol custom section is reintroduced/verified after strip by `scripts/build-wasm.mjs`.

Measured toolchain: Node 22.20.0, rustc 1.98.0 (88d9e12ae, 2026-08-18), Cargo 1.98.0, wasm-pack 0.13.1, wasm-tools 1.258.0, twiggy-opt 0.8.0, Binaryen wasm-opt from `node_modules/binaryen` (pinned by the Yarn lockfile dependency). `rust-toolchain.toml` selects the Rust toolchain; `cli-tools.json` pins the build tools.

The build printed 753,572 bytes before protocol stamping and 753,605 after stamping. Recompression independently measured 303,282 gzip and 239,018 Brotli. SHA-256 matches the post-Serde benchmark record: `d971618201d35c9e4bffe289bd367584151971d91894e018df734ab2d1415f27`.

## B. Binary composition and attribution limits

### Directly measured ranking

| Rank | Contributor                                                          | Evidence / byte amount                                                                                                                              | Attribution quality                                                                                         |
| ---- | -------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| 1    | Compiled executable code, all retained runtime and wasm-bindgen code | 684,832-byte code section; twiggy reports 2,258 rows, with many modest code items rather than a single huge one                                     | Measured section size, not crate-exclusive; function names are stripped                                     |
| 2    | Retained static data, tables and string/metadata payload             | 54,486-byte data section; many user-facing schema, validation, action, route, tag-policy, and panic/error strings are visible in `wasm-tools print` | Measured section size; data combines shared strings and compiler/runtime tables                             |
| 3    | Imports/exports and binding ABI                                      | 179 imports / 39 exports; 9,474 import bytes and 1,557 export bytes in objdump                                                                      | Measured section size, not equal to total wasm-bindgen implementation overhead                              |
| 4    | Runtime root/adoption exports                                        | `twiggy dominators` attributes 76,181 retained bytes (10.11%) to `plecruntime_start_adopt_snapshot`; top shallow item is 34,529 bytes               | Measured retaining subtree, not removable savings; includes shared callees and indirect table paths         |
| 5    | Typed IR decoding and structural validation                          | `plec-schema/src/typed_decode.rs` is the direct decoder; binary contains typed-IR field/variant strings and payload/depth/count limits              | Reachability confirmed from runtime exports, but per-module byte contribution not separable after stripping |
| 6    | Fetch capability                                                     | Controlled build feature experiment: full vs core saves 19,958 Brotli WASM bytes; raw savings 69,727 bytes                                          | Measured difference; shared compiler/runtime optimization means not an additive object-size sum             |

On the stripped production artifact, `twiggy top` gives maximum shallow items of 34,529 bytes (4.58%), 18,349 (2.43%), 18,091 (2.40%), 16,425 (2.18%), 15,464 (2.05%), and 13,919 (1.85%), but only as `code[N]` indices. `twiggy monos` on that artifact found no reportable generics. The separate symbol-preserving profiling image below resolves the identity issue and reports many useful Rust symbols/monomorphizations, although its code generation and name-section overhead prevent treating its function sizes as exact production sizes.

### Symbol-preserving analysis image

Built without touching the production artifact:

```sh
wasm-pack build crates/plec-runtime --target web --profiling --no-opt \
  --mode no-install --out-dir ../../.tmp/size-study/symbolized \
  --out-name runtime -- --locked
twiggy top -n 50 .tmp/size-study/symbolized/runtime_bg.wasm
```

The toolchain was the same as production. This kept the Wasm `name` custom section, enabling `twiggy` to report crate paths. The image has 4,095 functions, 763,842 code bytes and 1,220,884 bytes of function names. The latter alone is analysis metadata and must be stripped; the output is not a delivery artifact. Compared with production's 684,832 code-section bytes it also lacks the final custom `-Oz`+strip pass. Therefore the ranking and relative scale are diagnostic, not a production size ledger.

| Symbol / crate function                                            | Shallow bytes in symbolized image | Share of that image's code section | Note                                                                                                 |
| ------------------------------------------------------------------ | --------------------------------: | ---------------------------------: | ---------------------------------------------------------------------------------------------------- |
| `plec_client::runtime::TypedRuntime::adopt`                        |                            14,933 |                              1.96% | Largest named Plec function in `twiggy top`                                                          |
| `plec_eval::core::evaluate_bounded<TypedProgram>`                  |                            10,238 |                              1.34% | Expression evaluation                                                                                |
| `plec_router::navigation::adopt_typed_route`                       |                             9,607 |                              1.26% | Route/SSR adoption                                                                                   |
| `plec_schema::typed::js_decode::decode_typed_application`          |                             9,331 |                              1.22% | Top-level typed IR decoder                                                                           |
| `plec_client::runtime::TypedRuntime::instantiate_node`             |                             9,002 |                              1.18% | DOM/runtime node setup                                                                               |
| `plec_client::runtime::TypedRuntime::adopt_row_node_bounded`       |                             7,630 |                              1.00% | Bounded row adoption                                                                                 |
| `plec_client::runtime::TypedRuntime::apply_delta`                  |                             7,421 |                              0.97% | Runtime delta application                                                                            |
| `plec_action::drive<BrowserActionHost>` / `drive<TypedLoaderHost>` |                     7,339 / 7,143 |                      0.96% / 0.94% | The action VM is instantiated for both hosts; sharing is a candidate, not an additive 14.5 KB saving |
| Decoder array/iterator path for `TypedActionInstruction`           |                             7,018 |                              0.92% | A large generated iterator/try-fold body associated with decoding instruction arrays                 |
| `plec_schema::typed::TypedApplication::validate_with_policy`       |                             6,868 |                              0.90% | Post-decode schema validation                                                                        |
| Decoder array/iterator path for `TypedExpressionInstruction`       |                             3,072 |                              0.40% | Expression instruction list decode                                                                   |
| Decoder array/iterator path for `TypedNode`                        |                             2,392 |                              0.31% | Node list decode                                                                                     |

Summing all 65 shallow rows whose symbols reference `plec_schema::typed::js_decode` gives approximately **41,456 bytes** in this symbolized image (about 5.4% of its code section). This includes per-type `array`/`try_fold`/`collect` machinery associated with decoder results; it is not a direct “remove this module and save 41 KB” measure and includes source-profile inflation. The high-ranked functions and these per-type iterator instantiations show that handwritten decode/collect paths are real, measurable code, especially tagged instruction lists. `ObjectDecoder` core methods are comparatively small in the same listing (`new` 404 B, `get` 149 B, `optional` 157 B, `get_alias` 231 B, `optional_alias` 190 B); generic decoding and per-result vector handling are the more relevant repetition signal than a single oversized field helper.

The 76,181-byte retained tree for `start_adopt_snapshot` does **not** mean adoption alone costs 76 KB or that this is reclaimable: `twiggy` retained size includes reachable/shared functions and the function table's indirect-call reachability. Other exports and code can share that tree. Treat it only as a lower-level graph observation that SSR adoption is connected to a significant part of the retained runtime graph.

### Static data and string compression experiments

The production data section is exactly **54,486 raw bytes**. I parsed the final artifact's Wasm data section and applied in-memory, length-preserving substitutions to selected NUL-terminated printable spans, then Brotli-compressed the modified whole module with the same Q11 setting. No temporary patched binary was written and no production artifact was changed. Because these perturbations change the contents consumed by runtime logic, they are _compression attribution probes_, not valid builds or safe optimization patches. Repeated `x` replacement changes local Brotli context, so the deltas estimate how much compressed payload those spans carry, not guaranteed savings from deleting them.

| In-memory replacement class                                                                                | Matched spans | Matched raw characters | Whole-file Brotli | Delta from production |
| ---------------------------------------------------------------------------------------------------------- | ------------: | ---------------------: | ----------------: | --------------------: |
| Production baseline                                                                                        |             — |                      — |           239,018 |                     — |
| Broad validation/diagnostic phrases (`invalid`, `unknown`, `missing`, `limit`, `failed`, `expected`, etc.) |           196 |                 19,572 |           235,128 |       **3,890 bytes** |
| Selected typed-IR field/variant keys                                                                       |             8 |                  1,528 |           238,707 |         **311 bytes** |
| Selected DOM tag/policy vocabulary                                                                         |             9 |                    922 |           238,555 |         **463 bytes** |
| All matched NUL-terminated printable ASCII strings (length ≥4)                                             |           227 |                 20,833 |           234,304 |       **4,714 bytes** |

Each class is independently patched against the original; the rows overlap and are **not additive**. The broad message matcher is heuristic and catches data strings containing those terms, including some strings that are not user-facing diagnostics. It should be read as an upper-ish estimate for the selected diagnostic-like span set, not an exact sum of all error strings. The typed field/variant matcher is also a targeted sample, not the complete protocol-key inventory. Even replacing every matched printable string with repetitive bytes reduces Brotli by only 4.7 KB, around 2.0% of the 239 KB total. The 54 KB raw data section is therefore not 54 KB of removable English text: much of it is tables, compact binary constants, and non-text bytes; retained strings compress well, and many are required for schema compatibility, errors, tag policies, and runtime behavior.

This answers the compressed-payload question more directly than the raw section size: validation/diagnostic strings are the largest tested string group at ~3.9 KB Brotli under this perturbation. Typed key names are modest (~0.3 KB for this sampled set). A further source-level split by precise string ownership would require mapping each literal's address and full reference set; these measurements do not justify stripping public errors or changing field names.

### Functional assessment (not byte-exclusive)

- **Interpreter / runtime:** expressions, action instructions, reactive state and dependency graph updates, component/conditional/loop lifecycle, and targeted DOM mutation all execute from the same retained wasm runtime. The binary retains their semantics across applications, rather than specializing to one app.
- **Typed decode and validation:** the direct decoder lives in `crates/plec-schema/src/typed_decode.rs`; limits and graph checks live in the schema/IR code. It is reachable because browser startup consumes typed application IR. Strings for decode width/depth, application count limits, malformed fields, unsafe sinks, etc. remain in static data. Current evidence does not say direct decoding is a leading share of the 239 KB; that requires an instrumented before/after build that isolates the decoder.
- **Typed decode follow-up:** the symbolized image places top-level application decoding at 9.3 KB and decoder-associated functions/iterator code at ~41.5 KB summed shallow. This is material enough to profile and consider a targeted refactor, but still a minority of code in the non-production profiling image. The 7 KB instruction-list iterator function and separate ~3 KB expression list / ~2.4 KB node list bodies point to per-variant construction and generic collection as specific investigation targets. Sharing more primitives might help, but aggressive generic consolidation could instead create new monomorphizations; only a release-profile Brotli A/B can settle the net benefit.
- **Serialization:** `plec-schema/src/json_encode.rs` and runtime value conversion are browser-used paths for some features. Their precise share is not separately measured. No evidence indicates a linked Serde or `serde_json` runtime decoder.
- **WASM/JS bindings and DOM bindings:** `wasm-bindgen`, `js-sys`, and selectively enabled `web-sys` are expected to be linked where reachable. Cargo graph membership alone is not size evidence. The wasm binary has 179 imported host functions and 39 wasm-bindgen exports; the generated glue adds 8,081 Brotli bytes. `web-sys` declares broad workspace feature sets, while linker dead-code elimination means this does not imply every declared binding is retained.
- **Formatting and floats:** `zmij` appears through `plec-ir`/schema paths in the normal dependency tree and is plausibly live for JSON number formatting (`RuntimeValue::Number` serialization). It is not evidence of a large share by itself. Panic and display strings are visible in static data, but their raw/compressed totals cannot be isolated without rebuild variants.

## C. Dependencies and suspicious retention

### Serde is not retained in the production wasm

`plec-client` has default features off when used from the runtime, and runtime dependencies disable defaults on the compiler IR/schema/action/dom/eval crates. `decode-bench` is not enabled in the runtime production build. Serde/serde_json/serde-wasm-bindgen appear in `cargo tree -e features` because that command includes development dependencies and unified workspace feature details; they do not appear in `cargo tree -e normal --target wasm32-unknown-unknown --no-default-features` as Serde packages. The final binary's visible strings include no clear Serde/serde_json decoding identifiers, and the matching artifact SHA is the post-Serde `after` binary recorded by the repository benchmark. The production linked artifact is therefore not the benchmark build with both decode paths.

There is still a confusing graph presentation: plain Cargo feature trees show Serde through dev-dependencies (`plec-runtime` test dependencies include Serde and the wasm-bindgen test harness) and portions of the workspace graph. This is not a feature-unification leak into a normal wasm-pack release build; it is a tree-query scope issue. Use `-e normal` when auditing production dependencies.

### Other findings

- Server-only crates (`plec-server-engine`, Node bindings, compiler/parser crates) are not normal dependencies of the runtime crate; no reason was found to believe their implementations are linked. Normal-target Cargo dependency tree has runtime crates plus wasm host libraries.
- `wasm-bindgen-test`, Serde dev tools, and `decode-bench` are not production linked functionality.
- The build intentionally exports a broad runtime API for browser startup, adoption/snapshot startup, mutation/event delivery, diagnostics/validation policy, and teardown. Exports and JS imports are compatibility surface, not unexplained dead code.
- The enabled `web-sys` declarations include capabilities beyond the narrow wasm import list. The binary section reports 179 actual imports; not all declared API bindings are retained.
- `wasm-opt --print-function-map/--print-call-graph` was attempted but the installed Binaryen rejects modern bulk-memory/reference-types instructions without explicit feature enablement. `wasm-tools print/objdump` and twiggy succeeded. `cargo-bloat` was not installed, and native-target bloat would not establish wasm contribution in any case.

## D. Controlled experiments

All builds used the same Rust/wasm-pack/wasm-opt/wasm-tools/Brotli toolchain and the same `scripts/build-wasm.mjs` pipeline. Builds wrote into ignored `.tmp/size-study/*`, not package dist. `full` is the reproducible production baseline. Sizes below are exact file measurements; JS glue is shown separately where it changed.

| Experiment                                | Raw WASM |    gzip |  Brotli | Brotli delta vs full |
| ----------------------------------------- | -------: | ------: | ------: | -------------------: |
| Production full/default features          |  753,605 | 303,282 | 239,018 |                    — |
| `--profile core` (no defaults/features)   |  683,878 | 276,042 | 219,060 |          **-19,958** |
| `--profile router` (router only)          |  683,878 | 276,042 | 219,060 |          **-19,958** |
| `--profile fetch` (fetch only)            |  753,605 | 303,282 | 239,098 |                  +80 |
| Full, Rust release `opt-level=s` override |  878,458 | 348,445 | 269,319 |              +30,301 |

Feature profile artifacts all had protocol stamps. Router-only matched core byte for byte in this experiment; fetch-only matched full raw/gzip, but Brotli was 80 bytes larger (likely differences in compression/layout or reproducibility details; it is not a meaningful regression). Thus fetch accounts for the measured 69,727 raw / 19,958 Brotli delta. Router code was not independently isolated by that test: `plec-router` is an unconditional runtime dependency, and the top-level `router` feature currently did not remove linked code in the tested profile. Treat “router optional” as currently ineffective for size, not as proof that all routing implementation can be removed safely.

With full semantics and only `CARGO_PROFILE_RELEASE_OPT_LEVEL=s` changed, the output increased 124,853 raw and 30,301 Brotli. This demonstrates that the current `z` release profile plus Binaryen `-Oz` is more effective than `s` for this artifact. No change was made to checked-in profile settings.

Exact commands:

```sh
node scripts/build-wasm.mjs crates/plec-runtime .tmp/size-study/core --profile core
node scripts/build-wasm.mjs crates/plec-runtime .tmp/size-study/router --profile router
node scripts/build-wasm.mjs crates/plec-runtime .tmp/size-study/fetch --profile fetch
CARGO_PROFILE_RELEASE_OPT_LEVEL=s node scripts/build-wasm.mjs crates/plec-runtime .tmp/size-study/release-s --profile full
```

Measurement commands:

```sh
wasm-tools objdump packages/plec/dist/runtime/runtime_bg.wasm
twiggy top -n 60 packages/plec/dist/runtime/runtime_bg.wasm
twiggy top --retained -n 40 packages/plec/dist/runtime/runtime_bg.wasm
twiggy dominators -r 80 packages/plec/dist/runtime/runtime_bg.wasm
twiggy monos --all-generics packages/plec/dist/runtime/runtime_bg.wasm
```

Node compression reproduction:

```sh
node -e "const fs=require('fs'),z=require('zlib'),p='packages/plec/dist/runtime/runtime_bg.wasm',b=fs.readFileSync(p); console.log({raw:b.length,gzip:z.gzipSync(b,{level:9}).length,brotli:z.brotliCompressSync(b,{params:{[z.constants.BROTLI_PARAM_QUALITY]:11}}).length});"
```

The `--no-optimize` exploration yielded 761,765 raw / 239,096 Brotli, but wasm-pack still applied its own optimization. It is not a valid “no Binaryen at all” counterfactual and is excluded from the experiment table. This also illustrates why the actual shipping pipeline matters: removing only the second `-Oz` pass does not establish the cost of Binaryen optimization.

## E. Prioritised opportunities

Savings are delivered compressed WASM unless stated. Estimates without an experiment are explicitly uncertain. Complexity: low / medium / high; risk refers to compatibility/semantic risk.

| Priority / opportunity                                                                                 |                                                                                                Estimated Brotli saving | Complexity                                      | Risk                                                        | Recommendation                                                                                                                                                                                                                                                              |
| ------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------: | ----------------------------------------------- | ----------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1. Offer a core/no-fetch build for apps that do not need browser fetch actions                         |                                                             **Measured 19,958 bytes WASM** (20,847 bytes including JS) | Low-medium; feature integration and app testing | Medium: application feature contract/capability consistency | Worth pursuing if app authors can declare capabilities and deployment cache/artifact identity stays clear. Do not silently omit fetch in the generic default.                                                                                                               |
| 2. Investigate making the `router` feature actually gate router runtime code                           |                                                             Unknown; current router-only build saved **0** beyond core | Medium                                          | Medium-high: routing/SSR/runtime contract                   | First map `crates/plec-runtime/src/lib.rs`, router crate usage, feature tests, and app capability assumptions. Fix feature semantics only if a real no-router artifact has value; current name implies optionality that sizing did not verify.                              |
| 3. Reduce duplicate typed-decode field/variant plumbing where source-level duplication is demonstrated | Symbolized build associates ~41.5 KB shallow code with decoder symbols, but production Brotli delta remains unmeasured | Medium                                          | Medium: malformed-input diagnostics/limits                  | Profile at module/function level before refactoring. Candidate files: `crates/plec-schema/src/typed_decode.rs`, `js_decode.rs`, and validation helpers. Keep explicit checks and structural budget enforcement; avoid replacing this with a heavyweight generic serializer. |
| 4. Audit retained diagnostic strings and production error construction                                 |                             Probe delta **3.89 KB Brotli** for selected diagnostic-like spans; not a guaranteed saving | Low-medium                                      | Medium: public error stability and debuggability            | Consider compact stable public codes plus development-only context only after identifying exact string/function ownership. Preserve deterministic useful errors and SSR/adoption fail-closed codes.                                                                         |
| 5. Inspect wasm-bindgen exports/host ABI and actual imported wrappers                                  |                                                   Unknown; measured 179 imports, 39 exports, 8,081-byte Brotli JS glue | Medium-high                                     | High: ABI/adoption compatibility                            | A measured export-caller map is required. Remove only exports proven unused by browser/server staging and contract tests; do not move semantics to TS.                                                                                                                      |
| 6. Runtime splitting/lazy capability chunks                                                            |                                                                                         Not measured; total may worsen | High                                            | High: initialization, caching, compatibility                | Research only. Extra JS, requests, startup and cache fragmentation may erase `.wasm` savings. Require total-payload and cold-start benchmark.                                                                                                                               |
| 7. Change float/JSON number formatter (`zmij`)                                                         |                                                                                     Unknown, not demonstrated dominant | Medium                                          | Medium: JSON semantics and correctness                      | Low priority until symbol-level size evidence. Existing formatter is shared; replacing it based only on Cargo graph presence is unjustified.                                                                                                                                |
| 8. Alternative release profile (`opt-level=s`)                                                         |                                                                  **-30,301 bytes not saved; it grew Brotli by 30,301** | Low                                             | Performance/build behavior                                  | Do not adopt based on size. Current `z` profile wins for the measured artifact.                                                                                                                                                                                             |

## F. Architectural assessment

The remaining payload is predominantly **necessary generic runtime execution functionality plus host bindings and retained static data**. The section-level measurement says code is 90.9% of the WASM file before compression and data 7.2%; it cannot say how much code is each subsystem. There is no evidence that Serde, server-side Rust, test helpers, or benchmark-only paths are a significant retained contributor. Direct typed decoding does add real executable code and data, but its magnitude relative to the runtime is unmeasured; this investigation cannot responsibly call it the main remaining cost.

Build configuration is already aggressively size-oriented (`z`, fat LTO, one codegen unit, abort, `-Oz`, stripping). A tested weaker compiler optimization level made the output much larger. Feature separation has one validated opportunity (fetch, about 20 KB Brotli) but the router profile did not provide a reduction. Big savings beyond that likely require changing what a given deployed runtime supports, or a different delivery model. Removing lifecycle, keyed collection, adoption, validation, or expression/action semantics would be a semantic compromise and is not recommended to chase 239 KB.

The 239 KB figure is a Brotli representation. Optimizing raw bytes alone can mislead; the release-s experiment increased raw by 16.6% and Brotli by 12.7%. Track raw, gzip, Brotli, JS glue, and startup/performance together.

## G. Recommended next steps before v0.1

### Quick wins

1. Keep the current production optimization pipeline and establish a CI artifact-size baseline with both WASM and generated JS sizes, compression settings, toolchain, and hash. Relevant: `scripts/build-wasm.mjs`, `packages/plec/scripts/build-runtime.mjs`, `benchmarks/results/serde-wasm-removal-2026-10-10.json`.
2. Use normal-dependency scope in audits (`cargo tree -e normal --target wasm32-unknown-unknown`) so test/dev Serde entries are not mistaken for production linkage.
3. Document `core`/`fetch` profile implications for deployment and confirm generated application manifests select matching runtime bundles, if the feature profiles are intended as supported user-facing options. Test the real app capability contract before shipping profiles.

### Moderate refactors

1. Establish symbolized attribution before code refactoring: preserve a name-bearing intermediate artifact before final strip, or generate a `twiggy`/DWARF map in an analysis-only reproducible workflow. Avoid modifying production output just to improve diagnostics. Track `twiggy top`, dominators, and per-crate `cargo bloat` only as supplementary evidence.
2. If per-function data confirms material decode duplication, consolidate common property/variant parsing in `plec-schema/src/typed_decode.rs`. Verify typed IR decoder tests, invalid/missing/unknown field behavior, and all resource-limit tests in `crates/plec-schema` and runtime integration tests.
3. Audit router feature wiring, where top-level `router` currently has no measured delta. Relevant: `crates/plec-runtime/Cargo.toml`, runtime exports in `crates/plec-runtime/src/lib.rs`, `crates/plec-router`, and route/adoption wasm tests.

### Higher-risk research

1. Capability-tiered browser artifacts with explicit application metadata and compatibility/cache keys. Measure total delivery (WASM + glue + requests) and cold start; preserve one generic runtime contract where possible.
2. Narrower wasm-bindgen ABI or module split only if export/import caller paths prove specific wrappers are unnecessary. Validate browser glue, SSR adoption identity, routing, events, and public authoring API.

### Not recommended

- Reintroducing Serde or replacing direct decoding with a general-purpose format framework without a same-pipeline size/runtime benchmark.
- Removing resource budgets, validation, stable errors, SSR adoption, routing semantics, lifecycle ownership, keyed updates, or action/expression semantics.
- Moving decoding/execution into browser TypeScript to shrink the wasm file; total JS and duplicated semantics would rise.
- Switching the release profile to `opt-level=s` or changing optimization settings based on assumed compression ratios; measured result was larger.
- Treating every dependency in Cargo.lock or workspace-wide feature output as linked wasm size.

## Verification and worktree state

Production reproduction and four controlled builds succeeded. `twiggy`, `wasm-tools objdump/print`, Cargo normal-dependency inspection, and independent gzip/Brotli measurements ran. Binaryen's direct function-map/call-graph diagnostic was incompatible with enabled Wasm features on the installed invocation; no conclusion relies on that output. Build products went to ignored package dist and `.tmp/size-study` paths; no Rust production files or runtime semantics were changed.
