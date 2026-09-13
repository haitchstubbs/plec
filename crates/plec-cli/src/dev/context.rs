use super::repo::Repo;
use super::{artifact, contract};
use serde::Serialize;
use std::fs;

#[derive(Clone, Copy, Debug, clap::ValueEnum, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Domain {
    SsrAdoption,
    Routing,
    TypedEvents,
    Cookies,
    Artifacts,
}

impl Domain {
    fn name(self) -> &'static str {
        match self {
            Self::SsrAdoption => "ssr-adoption",
            Self::Routing => "routing",
            Self::TypedEvents => "typed-events",
            Self::Cookies => "cookies",
            Self::Artifacts => "artifacts",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ContextReport {
    pub domain: &'static str,
    pub summary: &'static str,
    pub protocols: Vec<Protocol>,
    pub entry_points: Vec<EntryPoint>,
    pub invariants: Vec<&'static str>,
    pub workflows: Vec<&'static str>,
    pub tests: Vec<&'static str>,
    pub documentation: Vec<EntryPoint>,
}

#[derive(Debug, Serialize)]
pub struct Protocol {
    pub name: &'static str,
    pub value: Option<u32>,
    pub source: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct EntryPoint {
    pub label: &'static str,
    pub file: &'static str,
    pub line: usize,
}

struct Spec {
    summary: &'static str,
    protocols: &'static [(&'static str, &'static str)],
    entry_points: &'static [EntryPoint],
    invariants: &'static [&'static str],
    workflows: &'static [&'static str],
    tests: &'static [&'static str],
    documentation: &'static [EntryPoint],
}

const SSR_ENTRIES: &[EntryPoint] = &[
    ep(
        "browser adoption gate",
        "packages/plec-browser/src/index.ts",
        499,
    ),
    ep(
        "browser bootstrap validation",
        "packages/plec-browser/src/index.ts",
        558,
    ),
    ep(
        "runtime adoption entry",
        "crates/plec-runtime/src/lifecycle.rs",
        332,
    ),
    ep("typed claim walk", "crates/plec-client/src/runtime.rs", 841),
    ep("route adoption", "crates/plec-router/src/navigation.rs", 24),
    ep(
        "adoption abandonment",
        "crates/plec-runtime/src/lifecycle.rs",
        381,
    ),
    ep("SSR renderer", "crates/plec-server/src/ssr/render.rs", 1),
    ep(
        "snapshot producer",
        "crates/plec-server/src/ssr/snapshot.rs",
        1,
    ),
];
const SSR_DOCS: &[EntryPoint] = &[
    ep("architecture contract", "docs/ssr-architecture.md", 46),
    ep("marker grammar", "docs/dom-address-protocol.md", 1),
];
const SSR_PROTOCOLS: &[(&str, &str)] = &[
    ("snapshot", "snapshot"),
    ("bootstrap", "bootstrap"),
    ("manifest", "manifest"),
];
const SSR_INVARIANTS: &[&str] = &[
    "Adoption is router-only; standalone mounts start fresh.",
    "Server and browser walk identical compiled graph structure.",
    "Snapshot revision must equal RouteManifest revision.",
    "Snapshot public state seeds client inputs before claims.",
    "Structural disagreement fails adoption closed.",
    "Binding-value divergence is allowed and counted.",
    "Every marker address names at most one DOM node.",
    "Text marker must be immediately adjacent to text node.",
    "Nested components consume records keyed by marker path.",
    "Conditional branch selection comes from snapshot records.",
    "Loop row order and identity come from snapshot records.",
    "Abandonment leaves SSR DOM for one destructive remount.",
];
const SSR_WORKFLOWS: &[&str] = &[
    "plec workspace doctor adoption",
    "plec workspace doctor adoption --html <file>",
    "plec workspace doctor adoption --snapshot <file>",
    "plec workspace contract ssr --check",
    "plec workspace artifact stale",
];
const SSR_TESTS: &[&str] = &[
    "cargo test -p plec-runtime --lib",
    "wasm adoption and snapshot tests in crates/plec-runtime/tests/typed_events.rs",
    "packages/plec-e2e/tests/acceptance/adoption.playwright.ts",
    "packages/plec-e2e/tests/acceptance/adoption-mismatch.playwright.ts",
];

const ROUTING_ENTRIES: &[EntryPoint] = &[
    ep("route discovery", "crates/plec-compiler/src/routes.rs", 46),
    ep(
        "route artifact lowering",
        "crates/plec-compiler/src/routes.rs",
        197,
    ),
    ep(
        "stable route revision",
        "crates/plec-compiler/src/routes.rs",
        474,
    ),
    ep(
        "typed route matcher",
        "crates/plec-schema/src/routing.rs",
        38,
    ),
    ep("navigation", "crates/plec-router/src/navigation.rs", 291),
    ep(
        "router listeners",
        "crates/plec-router/src/listeners.rs",
        14,
    ),
    ep("browser router", "packages/plec-browser/src/index.ts", 499),
];
const ROUTING_DOCS: &[EntryPoint] = &[
    ep("SSR route architecture", "docs/ssr-architecture.md", 46),
    ep("getting started", "docs/getting-started.md", 1),
];
const ROUTING_PROTOCOLS: &[(&str, &str)] = &[("manifest", "manifest"), ("snapshot", "snapshot")];
const ROUTING_INVARIANTS: &[&str] = &[
    "Route paths must be static string literals during lowering.",
    "Each route phase becomes a self-contained executable graph.",
    "Parent graphs declare child mount points with RouteOutlet records.",
    "Artifact ordering and node indices remain deterministic.",
    "Stable revision hashes serialized route artifacts.",
    "Manifest version is validated before navigation.",
    "Server and typed browser matchers must agree on route identity.",
    "Snapshot route chain is cross-validated against current URL.",
    "Static routes take precedence over parameter routes.",
    "Navigation preserves owned runtime regions where possible.",
];
const ROUTING_WORKFLOWS: &[&str] = &[
    "plec routes <source>",
    "plec workspace graph resolve <graph-id>",
    "plec workspace graph tree <graph-id>",
    "plec workspace doctor adoption --route <path>",
    "plec workspace trace stale-revision",
];
const ROUTING_TESTS: &[&str] = &[
    "cargo test -p plec-schema",
    "cargo test -p plec-router",
    "route lowering tests in crates/plec-compiler/src/routes.rs",
    "typed route tests in crates/plec-runtime/tests/typed_events.rs",
    "packages/plec-e2e/tests/acceptance/home-layout.playwright.ts",
    "packages/plec-e2e/tests/acceptance/notes-loader.playwright.ts",
];

const EVENT_ENTRIES: &[EntryPoint] = &[
    ep("event schema", "crates/plec-ir/src/lib.rs", 1045),
    ep("event lowering", "crates/plec-lowering/src/node.rs", 60),
    ep(
        "client event dispatch",
        "crates/plec-client/src/events.rs",
        210,
    ),
    ep(
        "typed action completion",
        "crates/plec-client/src/events.rs",
        504,
    ),
    ep(
        "runtime listener queue",
        "crates/plec-client/src/runtime.rs",
        1519,
    ),
];
const EVENT_DOCS: &[EntryPoint] = &[ep("SSR event boundary", "docs/ssr-architecture.md", 162)];
const EVENT_PROTOCOLS: &[(&str, &str)] = &[("snapshot", "snapshot")];
const EVENT_INVARIANTS: &[&str] = &[
    "Compiler emits typed event records, not arbitrary JavaScript callbacks.",
    "Lowering preserves event target and handler identity.",
    "Runtime dispatch resolves listener ownership before execution.",
    "Queued callbacks run through the owning runtime region.",
    "Stale callbacks are rejected by runtime ownership checks.",
    "Event actions may produce host fetch and cookie requests.",
    "Pending host requests complete through typed result channels.",
    "SSR drops event handlers from emitted HTML.",
    "Failed adoption remount restores interactivity client-side.",
];
const EVENT_WORKFLOWS: &[&str] = &[
    "plec workspace trace <event-symbol>",
    "plec workspace test wasm typed_event",
    "plec workspace test last --failure <event-test>",
    "plec workspace graph tree <graph-id>",
];
const EVENT_TESTS: &[&str] = &[
    "cargo test -p plec-client",
    "typed event tests in crates/plec-runtime/tests/typed_events.rs",
    "event tests in crates/plec-client/src/events.rs",
    "typed schema tests in crates/plec-schema/src/typed.rs",
];

const COOKIE_ENTRIES: &[EntryPoint] = &[
    ep(
        "browser host policy",
        "packages/plec-browser/src/index.ts",
        307,
    ),
    ep(
        "browser provider registration",
        "packages/plec-browser/src/index.ts",
        346,
    ),
    ep("DOM cookie gate", "crates/plec-dom/src/cookie.rs", 1),
    ep(
        "client cookie state",
        "crates/plec-client/src/runtime.rs",
        361,
    ),
    ep(
        "cookie request queue",
        "crates/plec-client/src/cookie.rs",
        25,
    ),
    ep(
        "SSR render boundary",
        "crates/plec-server/src/ssr/render.rs",
        979,
    ),
];
const COOKIE_DOCS: &[EntryPoint] = &[
    ep("SSR cookie boundary", "docs/ssr-architecture.md", 58),
    ep("security limits", "docs/security-limits.md", 1),
];
const COOKIE_PROTOCOLS: &[(&str, &str)] = &[("snapshot", "snapshot"), ("bootstrap", "bootstrap")];
const COOKIE_INVARIANTS: &[&str] = &[
    "Cookie authority belongs to explicit host capability grants.",
    "Browser glue adapts host policy; runtime owns request execution.",
    "Cookie values never enter SSR markup.",
    "Cookie values never enter the SSR bootstrap snapshot.",
    "Server render rejects cookie access at the render boundary.",
    "Client cookie writes leave runtime as typed pending requests.",
    "Host responses re-enter runtime through typed state updates.",
    "Cookie capability is not inferred from arbitrary source access.",
    "Adoption fallback purges seeded public state only.",
];
const COOKIE_WORKFLOWS: &[&str] = &[
    "plec workspace trace cookies",
    "plec workspace doctor adoption",
    "plec workspace contract ssr --check",
    "plec workspace test wasm cookie",
];
const COOKIE_TESTS: &[&str] = &[
    "cookie tests in crates/plec-runtime/tests/typed_events.rs",
    "cookie state tests in crates/plec-client/src/cookie.rs",
    "packages/plec-e2e adoption mismatch cookie coverage",
    "apps/fullstack/src/components/fullstack-layout.tsx",
];

const ARTIFACT_ENTRIES: &[EntryPoint] = &[
    ep(
        "artifact emission",
        "crates/plec-build/src/modules/artifacts.rs",
        22,
    ),
    ep(
        "runtime staging",
        "crates/plec-build/src/modules/artifacts.rs",
        138,
    ),
    ep(
        "build orchestration",
        "crates/plec-build/src/modules/build.rs",
        1,
    ),
    ep("WASM build pipeline", "scripts/build-wasm.mjs", 267),
    ep(
        "CLI provenance inspection",
        "crates/plec-cli/src/dev/artifact.rs",
        73,
    ),
    ep(
        "runtime protocol section",
        "crates/plec-cli/src/dev/wasm_section.rs",
        1,
    ),
];
const ARTIFACT_DOCS: &[EntryPoint] = &[
    ep(
        "CLI workflow documentation",
        "crates/plec-cli/README.md",
        199,
    ),
    ep("build order", "docs/getting-started.md", 36),
];
const ARTIFACT_PROTOCOLS: &[(&str, &str)] = &[("snapshot", "snapshot")];
const ARTIFACT_INVARIANTS: &[&str] = &[
    "Runtime WASM is a Plec asset, not an application compiler output.",
    "Build and verification remain separate operations.",
    "Package dist is the release runtime source.",
    "Fullstack dist is the staged runtime served by the app.",
    "Provenance hashes identify runtime.js and runtime_bg.wasm.",
    "The plec-protocol WASM section identifies implemented snapshot version.",
    "Staged WASM hash must equal package dist WASM hash.",
    "Runtime staging validates paths remain inside selected source directory.",
    "Missing or stale artifacts fail loudly with a repair command.",
    "Application builds do not silently compile replacement WASM.",
];
const ARTIFACT_WORKFLOWS: &[&str] = &[
    "plec workspace compile",
    "plec workspace artifact provenance runtime",
    "plec workspace artifact stale",
    "plec workspace doctor adoption",
    "yarn workspace plec build:wasm",
];
const ARTIFACT_TESTS: &[&str] = &[
    "cargo test -p plec-build",
    "cargo test -p plec-cli",
    "packages/plec-e2e/tests/smoke/artifacts.playwright.ts",
    "apps/fullstack/src/performance.test.ts",
];

const fn ep(label: &'static str, file: &'static str, line: usize) -> EntryPoint {
    EntryPoint { label, file, line }
}

fn spec(domain: Domain) -> Spec {
    match domain {
        Domain::SsrAdoption => Spec { summary: "SSR renders once on server; browser adopts marked DOM or fails closed.", protocols: SSR_PROTOCOLS, entry_points: SSR_ENTRIES, invariants: SSR_INVARIANTS, workflows: SSR_WORKFLOWS, tests: SSR_TESTS, documentation: SSR_DOCS },
        Domain::Routing => Spec { summary: "Rust lowers deterministic route graphs; browser navigation consumes same manifest.", protocols: ROUTING_PROTOCOLS, entry_points: ROUTING_ENTRIES, invariants: ROUTING_INVARIANTS, workflows: ROUTING_WORKFLOWS, tests: ROUTING_TESTS, documentation: ROUTING_DOCS },
        Domain::TypedEvents => Spec { summary: "Typed IR event records connect DOM listeners to owned runtime actions.", protocols: EVENT_PROTOCOLS, entry_points: EVENT_ENTRIES, invariants: EVENT_INVARIANTS, workflows: EVENT_WORKFLOWS, tests: EVENT_TESTS, documentation: EVENT_DOCS },
        Domain::Cookies => Spec { summary: "Cookie access crosses explicit browser/server host boundaries; it never becomes SSR state.", protocols: COOKIE_PROTOCOLS, entry_points: COOKIE_ENTRIES, invariants: COOKIE_INVARIANTS, workflows: COOKIE_WORKFLOWS, tests: COOKIE_TESTS, documentation: COOKIE_DOCS },
        Domain::Artifacts => Spec { summary: "Compiler output and runtime binaries move through verified, provenance-bearing build layers.", protocols: ARTIFACT_PROTOCOLS, entry_points: ARTIFACT_ENTRIES, invariants: ARTIFACT_INVARIANTS, workflows: ARTIFACT_WORKFLOWS, tests: ARTIFACT_TESTS, documentation: ARTIFACT_DOCS },
    }
}

pub fn build(repo: &Repo, domain: Domain) -> Result<ContextReport, String> {
    let selected = spec(domain);
    for entry in selected.entry_points.iter().chain(selected.documentation) {
        verify_entry(repo, entry)?;
    }
    let contracts = contract::scan(repo)?;
    let artifacts = artifact::inspect(repo);
    let protocols = selected
        .protocols
        .iter()
        .map(|(name, section)| Protocol {
            name,
            value: match *section {
                "snapshot" => contracts.snapshot.canonical,
                "bootstrap" => contracts.bootstrap.canonical,
                "manifest" => contracts.manifest.canonical,
                _ => None,
            },
            source: match *section {
                "snapshot" => contracts.snapshot.canonical_source.clone(),
                "bootstrap" => contracts.bootstrap.canonical_source.clone(),
                "manifest" => contracts.manifest.canonical_source.clone(),
                _ => "unknown".into(),
            },
        })
        .collect();
    let mut report = ContextReport {
        domain: domain.name(),
        summary: selected.summary,
        protocols,
        entry_points: selected.entry_points.to_vec(),
        invariants: selected.invariants.to_vec(),
        workflows: selected.workflows.to_vec(),
        tests: selected.tests.to_vec(),
        documentation: selected.documentation.to_vec(),
    };
    if matches!(domain, Domain::Artifacts) {
        report.invariants.push(if artifacts.ok {
            "Current package and staged runtime artifacts pass provenance checks."
        } else {
            "Current runtime artifacts need `plec workspace artifact stale` before browser tests."
        });
    }
    Ok(report)
}

fn verify_entry(repo: &Repo, entry: &EntryPoint) -> Result<(), String> {
    let path = repo.root.join(entry.file);
    let content = fs::read_to_string(&path)
        .map_err(|error| format!("context entry missing {}: {error}", entry.file))?;
    if content.lines().count() < entry.line {
        return Err(format!(
            "context entry line missing {}:{}",
            entry.file, entry.line
        ));
    }
    Ok(())
}

pub fn print_report(report: &ContextReport) {
    println!("Plec workspace context: {}\n", report.domain);
    println!("SUMMARY\n  {}\n", report.summary);
    println!("PROTOCOL VERSIONS");
    for protocol in &report.protocols {
        println!(
            "  {} = {:?} ({})",
            protocol.name, protocol.value, protocol.source
        );
    }
    print_entries("ENTRY POINTS", &report.entry_points);
    print_lines("INVARIANTS", &report.invariants);
    print_lines("WORKFLOWS", &report.workflows);
    print_lines("TESTS", &report.tests);
    print_entries("DOCUMENTATION", &report.documentation);
}

fn print_entries(title: &str, entries: &[EntryPoint]) {
    println!("\n{title}");
    for entry in entries {
        println!("  {}: {}:{}", entry.label, entry.file, entry.line);
    }
}

fn print_lines(title: &str, lines: &[&str]) {
    println!("\n{title}");
    for line in lines {
        println!("  {line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn repo() -> Repo {
        Repo {
            root: Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
        }
    }

    #[test]
    fn all_domains_have_current_entries() {
        let repo = repo();
        for domain in [
            Domain::SsrAdoption,
            Domain::Routing,
            Domain::TypedEvents,
            Domain::Cookies,
            Domain::Artifacts,
        ] {
            let report = build(&repo, domain).expect("context metadata stays valid");
            assert!(!report.entry_points.is_empty());
            assert!(!report.invariants.is_empty());
            assert!(!report.tests.is_empty());
        }
    }

    #[test]
    fn human_packet_has_expected_sections() {
        let report = build(&repo(), Domain::SsrAdoption).unwrap();
        assert_eq!(report.domain, "ssr-adoption");
        assert_eq!(report.protocols[0].value, Some(2));
        assert!(report
            .documentation
            .iter()
            .any(|entry| entry.file.ends_with("ssr-architecture.md")));
    }
}
