use super::contract::{self, Kind};
use super::repo::Repo;
use super::trace::{self, Category, TraceMatch};
use serde::Serialize;

/// Change-impact companion to `trace`: for a protocol constant or symbol,
/// enumerate everything a change would touch, grouped by layer (Rust
/// definitions/consumers/fixtures, TypeScript glue/tests, e2e specs, docs).
///
/// Two engines are composed:
///
/// - the trace categorizer, regrouped into change-oriented layers, and
/// - the contract scanner site table, used as the impact registry for known
///   protocol boundaries. Registry rows include sites that hard-code version
///   literals without naming the symbol (loader fixtures, gates), which a
///   plain symbol search cannot find — the exact failure mode of the
///   snapshot v1-to-v2 bump that missed `typed_events.rs`.
const LAYER_ORDER: &[&str] = &[
    "RUST / DEFINITIONS",
    "RUST / CONSUMERS",
    "RUST / FIXTURES",
    "TYPESCRIPT / DEFINITIONS",
    "TYPESCRIPT / GLUE",
    "TYPESCRIPT / TESTS",
    "E2E / SPECS",
    "DOCS",
    "OTHER",
];

#[derive(Debug, Clone, Serialize)]
pub struct RegistryRow {
    pub site: String,
    pub file: String,
    pub line: usize,
    pub value: Option<u32>,
    pub status: String,
    /// Allowlisted legacy literal: not a conflict today, but a version bump
    /// must consciously re-decide this site (keep legacy or propagate).
    pub review_on_change: bool,
    pub conflict: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegistrySection {
    pub boundary: &'static str,
    pub canonical: Option<u32>,
    pub canonical_source: String,
    pub rows: Vec<RegistryRow>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Layer {
    pub name: &'static str,
    pub matches: Vec<TraceMatch>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImpactReport {
    pub query: String,
    pub related_contract: Option<String>,
    /// Contract-scanner registry rows when the query names a tracked
    /// protocol boundary; `None` for untracked symbols.
    pub registry: Option<RegistrySection>,
    /// Set when the registry could not be scanned (missing file, io error).
    pub registry_error: Option<String>,
    pub layers: Vec<Layer>,
    pub truncated: usize,
}

pub fn impact(repo: &Repo, query: &str) -> ImpactReport {
    let trace_report = trace::trace(repo, query);

    let (registry, registry_error) = match symbol_kind(query) {
        Some(kind) => match contract::scan(repo) {
            Ok(report) => (Some(registry_section(kind, &report)), None),
            Err(error) => (None, Some(error)),
        },
        None => (None, None),
    };

    ImpactReport {
        query: query.to_string(),
        related_contract: trace::related_contract(query),
        registry,
        registry_error,
        layers: group_layers(trace_report.matches),
        truncated: trace_report.truncated,
    }
}

/// Map a symbol or error-code query onto the tracked protocol boundary it
/// belongs to. `None` means the query is not a registry-tracked constant.
pub fn symbol_kind(query: &str) -> Option<Kind> {
    let lower = query.to_ascii_lowercase();
    if lower.contains("ssr_snapshot_version") || lower.contains("ssr-snapshot-version") {
        Some(Kind::Snapshot)
    } else if lower.contains("bootstrap_wrapper_version") || lower.contains("bootstrap") {
        Some(Kind::Bootstrap)
    } else if lower.contains("provider_manifest_version")
        || lower.contains("host-provider")
        || lower.contains("host_provider")
    {
        Some(Kind::HostProvider)
    } else if lower.contains("routemanifest") || lower.contains("route manifest") {
        Some(Kind::Manifest)
    } else {
        None
    }
}

fn kind_boundary(kind: Kind) -> &'static str {
    match kind {
        Kind::Snapshot => "snapshot",
        Kind::Bootstrap => "bootstrap",
        Kind::HostProvider => "host-provider",
        Kind::Manifest => "manifest",
    }
}

fn registry_section(kind: Kind, report: &contract::Report) -> RegistrySection {
    let section = match kind {
        Kind::Snapshot => &report.snapshot,
        Kind::Bootstrap => &report.bootstrap,
        Kind::HostProvider => &report.host_provider,
        Kind::Manifest => &report.manifest,
    };

    RegistrySection {
        boundary: kind_boundary(kind),
        canonical: section.canonical,
        canonical_source: section.canonical_source.clone(),
        rows: section.rows.iter().map(registry_row).collect(),
    }
}

fn registry_row(row: &contract::Row) -> RegistryRow {
    RegistryRow {
        site: row.site.clone(),
        file: row.file.clone(),
        line: row.line,
        value: row.value,
        status: row.status.clone(),
        review_on_change: row.status.starts_with('~'),
        conflict: row.status.starts_with("CONFLICT"),
    }
}

/// Regroup trace matches into change-oriented layers. Path rules follow the
/// trace categorizer: docs and e2e are recognized by path first, then the
/// trace category splits along the Rust/TypeScript boundary.
pub fn group_layers(mut matches: Vec<TraceMatch>) -> Vec<Layer> {
    matches.sort_by(|a, b| {
        LAYER_ORDER
            .iter()
            .position(|name| *name == layer_for(a))
            .cmp(&LAYER_ORDER.iter().position(|name| *name == layer_for(b)))
            .then_with(|| a.file.cmp(&b.file))
            .then_with(|| a.line.cmp(&b.line))
    });

    let mut layers: Vec<Layer> = Vec::new();
    for name in LAYER_ORDER {
        let owned: Vec<TraceMatch> = matches
            .iter()
            .filter(|m| layer_for(m) == *name)
            .cloned()
            .collect();
        if !owned.is_empty() {
            layers.push(Layer {
                name,
                matches: owned,
            });
        }
    }

    layers
}

fn layer_for(match_item: &TraceMatch) -> &'static str {
    let file = &match_item.file;
    if file.ends_with(".md") {
        return "DOCS";
    }
    if file.contains("packages/plec-e2e/") {
        return "E2E / SPECS";
    }

    let is_rust = file.ends_with(".rs");
    match match_item.category {
        Category::DefinedIn => {
            if is_rust {
                "RUST / DEFINITIONS"
            } else {
                "TYPESCRIPT / DEFINITIONS"
            }
        }
        Category::ProducedBy => {
            if is_rust {
                "RUST / CONSUMERS"
            } else {
                "TYPESCRIPT / GLUE"
            }
        }
        Category::AssertedBy => {
            if is_rust {
                "RUST / FIXTURES"
            } else {
                "TYPESCRIPT / TESTS"
            }
        }
        Category::DocumentedBy => "DOCS",
        Category::Other => "OTHER",
    }
}

pub fn print_report(report: &ImpactReport) {
    println!("{}\n", report.query);

    if let Some(contract) = &report.related_contract {
        println!("RELATED CONTRACT");
        println!("  {contract}\n");
    }

    if let Some(error) = &report.registry_error {
        println!("REGISTRY");
        println!("  scan failed: {error}\n");
    } else if let Some(registry) = &report.registry {
        println!("REGISTRY — {} boundary", registry.boundary);
        let canonical = registry
            .canonical
            .map(|value| value.to_string())
            .unwrap_or_else(|| "?".into());
        println!(
            "  canonical version       {canonical}  {}",
            registry.canonical_source
        );
        for row in &registry.rows {
            let value = row
                .value
                .map(|value| value.to_string())
                .unwrap_or_else(|| "—".into());
            let note = if row.conflict {
                format!("{}  ← fix before running suites", row.status)
            } else if row.review_on_change {
                format!("{}  (review on change)", row.status)
            } else {
                row.status.clone()
            };
            println!(
                "  {:22} {:>4}  {}:{}  {}",
                row.site, value, row.file, row.line, note
            );
        }
        println!();
    }

    if report.layers.is_empty() {
        println!("(no symbol matches)");
        if report.registry.is_none() && report.registry_error.is_none() {
            println!("  (query is not a registry-tracked protocol constant)");
        }
        return;
    }

    let mut current: Option<&str> = None;
    for layer in &report.layers {
        if current != Some(layer.name) {
            current = Some(layer.name);
            println!("{}", layer.name);
        }
        for match_item in &layer.matches {
            println!(
                "  {}:{}  {}",
                match_item.file, match_item.line, match_item.text
            );
        }
    }

    if report.truncated > 0 {
        println!(
            "\n  ... and {truncated} more matches (use --json for the full result)",
            truncated = report.truncated
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(file: &str, line: usize, category: Category) -> TraceMatch {
        TraceMatch {
            file: file.into(),
            line,
            text: format!("{file}:{line}"),
            category,
        }
    }

    #[test]
    fn known_constants_and_codes_map_to_boundaries() {
        assert_eq!(symbol_kind("SSR_SNAPSHOT_VERSION"), Some(Kind::Snapshot));
        assert_eq!(
            symbol_kind("unsupported:ssr-snapshot-version"),
            Some(Kind::Snapshot)
        );
        assert_eq!(
            symbol_kind("BOOTSTRAP_WRAPPER_VERSION"),
            Some(Kind::Bootstrap)
        );
        assert_eq!(symbol_kind("invalid:ssr-bootstrap"), Some(Kind::Bootstrap));
        assert_eq!(
            symbol_kind("PROVIDER_MANIFEST_VERSION"),
            Some(Kind::HostProvider)
        );
        assert_eq!(
            symbol_kind("host-provider manifest"),
            Some(Kind::HostProvider)
        );
        assert_eq!(symbol_kind("RouteManifest"), Some(Kind::Manifest));
        assert_eq!(symbol_kind("some_random_symbol"), None);
    }

    #[test]
    fn snapshot_wins_over_bootstrap_for_fully_qualified_codes() {
        // Error codes that name the snapshot boundary must not fall through
        // to the looser bootstrap keyword.
        assert_eq!(
            symbol_kind("unsupported:ssr-snapshot-version"),
            Some(Kind::Snapshot)
        );
    }

    #[test]
    fn layers_group_by_path_and_category() {
        let layers = group_layers(vec![
            m("docs/ssr-architecture.md", 10, Category::DocumentedBy),
            m(
                "packages/plec-e2e/tests/acceptance/adoption.playwright.ts",
                5,
                Category::AssertedBy,
            ),
            m(
                "crates/plec-runtime/tests/typed_events.rs",
                100,
                Category::AssertedBy,
            ),
            m(
                "packages/plec-browser/src/bootstrap.test.ts",
                7,
                Category::AssertedBy,
            ),
            m(
                "packages/plec-browser/src/index.ts",
                3,
                Category::ProducedBy,
            ),
            m("crates/plec-ir/src/lib.rs", 171, Category::DefinedIn),
            m(
                "crates/plec-runtime/src/runtime/lifecycle.rs",
                460,
                Category::ProducedBy,
            ),
            m("packages/plec/src/index.ts", 1, Category::DefinedIn),
        ]);

        let names: Vec<&str> = layers.iter().map(|l| l.name).collect();
        assert_eq!(
            names,
            vec![
                "RUST / DEFINITIONS",
                "RUST / CONSUMERS",
                "RUST / FIXTURES",
                "TYPESCRIPT / DEFINITIONS",
                "TYPESCRIPT / GLUE",
                "TYPESCRIPT / TESTS",
                "E2E / SPECS",
                "DOCS",
            ]
        );

        assert_eq!(layers[0].matches[0].file, "crates/plec-ir/src/lib.rs");
        assert_eq!(
            layers[2].matches[0].file,
            "crates/plec-runtime/tests/typed_events.rs"
        );
        assert_eq!(
            layers[6].matches[0].file,
            "packages/plec-e2e/tests/acceptance/adoption.playwright.ts"
        );
    }

    #[test]
    fn markdown_under_e2e_counts_as_docs() {
        assert_eq!(
            layer_for(&m("packages/plec-e2e/README.md", 1, Category::Other)),
            "DOCS"
        );
    }

    #[test]
    fn registry_rows_flag_review_and_conflict() {
        let report = contract::Report {
            snapshot: contract::Section {
                canonical: Some(2),
                canonical_source: "crates/plec-ir/src/lib.rs".into(),
                rows: vec![
                    contract::Row {
                        site: "definition".into(),
                        file: "crates/plec-ir/src/lib.rs".into(),
                        line: 171,
                        kind: Kind::Snapshot,
                        value: Some(2),
                        status: "definition ✓ (source line 171)".into(),
                        text: "pub const SSR_SNAPSHOT_VERSION".into(),
                    },
                    contract::Row {
                        site: "runtime fixtures".into(),
                        file: "crates/plec-runtime/tests/typed_events.rs".into(),
                        line: 2630,
                        kind: Kind::Snapshot,
                        value: Some(1),
                        status: "~ intentional legacy fixture".into(),
                        text: r#"snapshot["version"] = 1;"#.into(),
                    },
                    contract::Row {
                        site: "browser gate".into(),
                        file: "packages/plec-browser/src/index.ts".into(),
                        line: 40,
                        kind: Kind::Snapshot,
                        value: Some(1),
                        status: "CONFLICT: found 1, expected 2".into(),
                        text: "parsed.version === 1".into(),
                    },
                ],
            },
            bootstrap: contract::Section {
                canonical: None,
                canonical_source: String::new(),
                rows: Vec::new(),
            },
            host_provider: contract::Section {
                canonical: None,
                canonical_source: String::new(),
                rows: Vec::new(),
            },
            manifest: contract::Section {
                canonical: None,
                canonical_source: String::new(),
                rows: Vec::new(),
            },
        };

        let section = registry_section(Kind::Snapshot, &report);
        assert_eq!(section.boundary, "snapshot");
        assert_eq!(section.canonical, Some(2));
        assert_eq!(section.rows.len(), 3);
        assert!(!section.rows[0].review_on_change && !section.rows[0].conflict);
        assert!(section.rows[1].review_on_change && !section.rows[1].conflict);
        assert!(!section.rows[2].review_on_change && section.rows[2].conflict);
    }
}
