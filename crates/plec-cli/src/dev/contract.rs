use super::repo::Repo;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// The SSR protocol spans four independently-versioned boundaries that must
/// agree across the Rust compiler/runtime/server, browser glue, test
/// fixtures, and the docs:
///
/// - **snapshot** — `PlecSsrSnapshot.version`, canonical in
///   `crates/plec-ir/src/lib.rs` (`SSR_SNAPSHOT_VERSION`)
/// - **bootstrap** — the `{ version, snapshot }` HTML payload wrapper,
///   produced by `crates/plec-server` and gated by `packages/plec-browser`
/// - **host-provider** — the `/host-providers.json` manifest, produced by
///   `crates/plec-build` (`PROVIDER_MANIFEST_VERSION`) and gated by
///   `registerPlecProviders` in `packages/plec-browser`. It versions
///   independently of snapshot/bootstrap: producer and consumer ship in the
///   same build (revision cache-busted), so provider shape changes never
///   ride a snapshot version bump.
/// - **manifest** — `RouteManifest.version`, frozen in `plec-ir::validate`
///
/// The scanner finds version literals near these boundaries and reports
/// disagreements. Intentional legacy literals (fixtures exercising the
/// fail-closed gates) are allowlisted per site with a note, so a bump like
/// v1 → v2 surfaces every site the change must touch.

pub const CANONICAL_FILE: &str = "crates/plec-ir/src/lib.rs";
pub const HOST_PROVIDER_CANONICAL_FILE: &str = "crates/plec-build/src/modules/host.rs";
const HOST_PROVIDER_DEFINITION: &str = "const PROVIDER_MANIFEST_VERSION";
const HOST_PROVIDER_DEFINITION_NAME: &str = "PROVIDER_MANIFEST_VERSION";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Snapshot,
    Bootstrap,
    HostProvider,
    Manifest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Row {
    pub site: String,
    pub file: String,
    pub line: usize,
    pub kind: Kind,
    pub value: Option<u32>,
    pub status: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Section {
    pub canonical: Option<u32>,
    pub canonical_source: String,
    pub rows: Vec<Row>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub snapshot: Section,
    pub bootstrap: Section,
    pub host_provider: Section,
    pub manifest: Section,
}

impl Report {
    pub fn conflicts(&self) -> Vec<&Row> {
        [
            &self.snapshot,
            &self.bootstrap,
            &self.host_provider,
            &self.manifest,
        ]
        .into_iter()
        .flat_map(|section| section.rows.iter())
        .filter(|row| row.status.starts_with("CONFLICT"))
        .collect()
    }
}

struct SiteSpec {
    label: &'static str,
    path: &'static str,
    /// Refine matches by nearby keywords instead of forcing one kind.
    mixed: bool,
    force: Option<Kind>,
    /// Kind for mixed-site matches whose surrounding window carries no
    /// recognizable boundary keyword. `None` drops the match: the literal
    /// belongs to a manifest this scanner does not track on that site.
    fallback: Option<Kind>,
    allow: &'static [(u32, &'static str)],
    /// Path substrings to skip (relative to the site root).
    exclude: &'static [&'static str],
}

const SITES: &[SiteSpec] = &[
    SiteSpec {
        label: "runtime gate",
        path: "crates/plec-runtime/src/lifecycle.rs",
        mixed: false,
        force: Some(Kind::Snapshot),
        fallback: None,
        allow: &[],
        exclude: &[],
    },
    SiteSpec {
        label: "runtime fixtures",
        path: "crates/plec-runtime/tests/typed_events.rs",
        mixed: true,
        force: None,
        fallback: Some(Kind::Snapshot),
        allow: &[
            (1, "intentional legacy fixture"),
            (999, "version gate fixture"),
        ],
        exclude: &[],
    },
    SiteSpec {
        label: "server producer",
        path: "crates/plec-server/src/ssr/snapshot.rs",
        mixed: true,
        force: None,
        fallback: Some(Kind::Snapshot),
        allow: &[],
        exclude: &[],
    },
    SiteSpec {
        // host.rs also emits `dist/plec-server.json` (the server host
        // manifest), whose version is a separate untracked boundary: its
        // literals fall through `fallback: None` and are dropped.
        label: "host-provider producer",
        path: HOST_PROVIDER_CANONICAL_FILE,
        mixed: true,
        force: None,
        fallback: None,
        allow: &[],
        exclude: &[],
    },
    SiteSpec {
        label: "browser gate",
        path: "packages/plec-browser/src/index.ts",
        mixed: true,
        force: None,
        // The file hosts both the bootstrap wrapper gate and the
        // host-provider manifest gate; unrecognised literals stay bootstrap
        // because the bootstrap gate is this site's original contract.
        fallback: Some(Kind::Bootstrap),
        allow: &[],
        exclude: &[],
    },
    SiteSpec {
        label: "host-provider tests",
        path: "packages/plec-browser/src/index.test.ts",
        mixed: true,
        force: None,
        fallback: None,
        allow: &[],
        exclude: &[],
    },
    SiteSpec {
        label: "browser fixtures",
        path: "packages/plec-browser/src/bootstrap.test.ts",
        mixed: true,
        force: None,
        fallback: Some(Kind::Snapshot),
        allow: &[],
        exclude: &[],
    },
    SiteSpec {
        label: "e2e tests",
        path: "packages/plec-e2e/tests",
        mixed: true,
        force: None,
        fallback: Some(Kind::Snapshot),
        allow: &[(999, "version gate fixture")],
        exclude: &["/bench/", "/bench"],
    },
    SiteSpec {
        label: "docs",
        path: "docs/ssr-architecture.md",
        mixed: true,
        force: None,
        fallback: Some(Kind::Snapshot),
        allow: &[],
        exclude: &[],
    },
];

/// Scan the repo for protocol-version disagreements.
pub fn scan(repo: &Repo) -> Result<Report, String> {
    // Canonical snapshot version: compiled into this CLI through the
    // plec-ir dependency. Cross-check against the source text so a stale
    // CLI build cannot silently bless old constants.
    let compiled_snapshot = plec_ir::SSR_SNAPSHOT_VERSION;
    let source_snapshot = scan_definition(repo, CANONICAL_FILE, "pub const SSR_SNAPSHOT_VERSION")?;
    let manifest_canonical = scan_manifest_version(repo)?;
    // Bootstrap canonical: whatever the server producer currently emits.
    let bootstrap_canonical = first_bootstrap_version(repo)?;
    // Host-provider canonical: the named constant in the plec-build producer.
    let host_provider_canonical =
        scan_definition(repo, HOST_PROVIDER_CANONICAL_FILE, HOST_PROVIDER_DEFINITION)?
            .map(|(value, _)| value);

    let mut snapshot_rows = vec![snapshot_definition_row(compiled_snapshot, source_snapshot)];
    let mut bootstrap_rows = Vec::new();
    let mut host_provider_rows = Vec::new();
    let mut manifest_rows = Vec::new();

    // Rows repeated within one line (a line can mention `version` twice) are
    // noise; identical findings collapse to one entry.
    fn dedupe(rows: &mut Vec<Row>) {
        rows.sort_by(|a, b| {
            (&a.file, a.line, a.value, &a.status).cmp(&(&b.file, b.line, b.value, &b.status))
        });
        rows.dedup_by(|a, b| {
            a.file == b.file && a.line == b.line && a.value == b.value && a.status == b.status
        });
    }

    for site in SITES {
        for (file, lines) in iter_site_files(repo, site)? {
            for (index, text) in lines.iter().enumerate() {
                let matches = version_matches(text);

                let kind_of = |hint: Option<Kind>| {
                    classify(
                        &window(&lines, index),
                        &next_lines(&lines, index, 2),
                        hint,
                        site,
                    )
                };

                for (value, hint) in &matches {
                    let value = *value;
                    // `None` means the literal sits in a context this
                    // scanner does not track on this site; dropping it is
                    // the contract, not a missed detection.
                    let Some(kind) = kind_of(*hint) else {
                        continue;
                    };
                    let status = row_status(
                        value,
                        kind,
                        site,
                        compiled_snapshot,
                        manifest_canonical,
                        bootstrap_canonical,
                        host_provider_canonical,
                    );

                    let row = Row {
                        site: site.label.to_string(),
                        file: file.clone(),
                        line: index + 1,
                        kind,
                        value: Some(value),
                        status: status.to_string(),
                        text: text.trim().to_string(),
                    };

                    match kind {
                        Kind::Snapshot => snapshot_rows.push(row),
                        Kind::Bootstrap => bootstrap_rows.push(row),
                        Kind::HostProvider => host_provider_rows.push(row),
                        Kind::Manifest => manifest_rows.push(row),
                    }
                }

                // Constant-reference rows: the site deliberately names the
                // canonical constant instead of hard-coding a literal. Lines
                // that carry a literal were already reported above.
                if matches.is_empty() {
                    if text.contains("SSR_SNAPSHOT_VERSION")
                        && !text.contains("pub const SSR_SNAPSHOT_VERSION")
                    {
                        snapshot_rows.push(Row {
                            site: site.label.to_string(),
                            file: file.clone(),
                            line: index + 1,
                            kind: Kind::Snapshot,
                            value: None,
                            status: "constant ref".to_string(),
                            text: text.trim().to_string(),
                        });
                    }
                    // The provider producer names `PROVIDER_MANIFEST_VERSION`
                    // at the emission site; a literal there would be a
                    // regression the canonical constant exists to prevent.
                    if text.contains(HOST_PROVIDER_DEFINITION_NAME)
                        && !text.contains(HOST_PROVIDER_DEFINITION)
                    {
                        host_provider_rows.push(Row {
                            site: site.label.to_string(),
                            file: file.clone(),
                            line: index + 1,
                            kind: Kind::HostProvider,
                            value: None,
                            status: "constant ref".to_string(),
                            text: text.trim().to_string(),
                        });
                    }
                }
            }
        }
    }

    dedupe(&mut snapshot_rows);
    dedupe(&mut bootstrap_rows);
    dedupe(&mut manifest_rows);

    Ok(Report {
        snapshot: Section {
            canonical: Some(compiled_snapshot),
            canonical_source: format!(
                "{CANONICAL_FILE} (compiled into plec-cli: {compiled_snapshot})"
            ),
            rows: snapshot_rows,
        },
        bootstrap: Section {
            canonical: bootstrap_canonical,
            canonical_source: bootstrap_canonical
                .map(|value| {
                    format!("crates/plec-server/src/ssr/snapshot.rs (producer emits {value})")
                })
                .unwrap_or_else(|| "producer not found".into()),
            rows: bootstrap_rows,
        },
        host_provider: Section {
            canonical: host_provider_canonical,
            canonical_source: host_provider_canonical
                .map(|value| {
                    format!("{HOST_PROVIDER_CANONICAL_FILE} ({HOST_PROVIDER_DEFINITION}: {value})")
                })
                .unwrap_or_else(|| "producer constant not found".into()),
            rows: host_provider_rows,
        },
        manifest: Section {
            canonical: manifest_canonical,
            canonical_source: manifest_canonical
                .map(|value| format!("{CANONICAL_FILE} (validate gate: {value})"))
                .unwrap_or_else(|| "validate gate not found".into()),
            rows: manifest_rows,
        },
    })
}

fn snapshot_definition_row(compiled: u32, source: Option<(u32, usize)>) -> Row {
    let (status, value) = match source {
        Some((source_value, line)) if source_value == compiled => (
            format!("definition ✓ (source line {})", line),
            Some(source_value),
        ),
        Some((source_value, line)) => (
            format!(
                "CONFLICT: CLI was compiled with snapshot {compiled} but the source now says \
                 {source_value} (line {line}) — rebuild plec-cli"
            ),
            Some(source_value),
        ),
        None => (
            "CONFLICT: SSR_SNAPSHOT_VERSION definition not found in source".into(),
            None,
        ),
    };

    Row {
        site: "definition".into(),
        file: CANONICAL_FILE.into(),
        line: source.map(|(_, line)| line).unwrap_or(0),
        kind: Kind::Snapshot,
        value,
        status,
        text: "pub const SSR_SNAPSHOT_VERSION".into(),
    }
}

fn row_status(
    value: u32,
    kind: Kind,
    site: &SiteSpec,
    snapshot_canonical: u32,
    manifest_canonical: Option<u32>,
    bootstrap_canonical: Option<u32>,
    host_provider_canonical: Option<u32>,
) -> String {
    let canonical = match kind {
        Kind::Snapshot => Some(snapshot_canonical),
        Kind::Manifest => manifest_canonical,
        Kind::Bootstrap => bootstrap_canonical,
        Kind::HostProvider => host_provider_canonical,
    };

    let Some(canonical) = canonical else {
        return "unverifiable (no canonical)".into();
    };

    if value == canonical {
        return "✓".into();
    }

    if let Some((_, note)) = site.allow.iter().find(|(allowed, _)| *allowed == value) {
        return format!("~ {note}");
    }

    format!("CONFLICT: found {value}, expected {canonical}")
}

/// Kind resolution for a matched line:
/// 1. forced by the site spec (single-purpose sites),
/// 2. manifest context (`rootGraphId` / `RouteManifest` nearby),
/// 3. bootstrap wrapper emission (`snapshot: {` within the next two lines)
///    or "bootstrap" in the surrounding window,
/// 4. host-provider manifest context (`provider` in the surrounding
///    window),
/// 5. the line's own hint,
/// 6. the site's fallback, defaulting to snapshot.
///
/// `None` drops the match: the literal belongs to a manifest this scanner
/// does not track on that site (for example the server host manifest
/// emitted alongside the provider manifest in `host.rs`).
fn classify(window: &str, next: &str, hint: Option<Kind>, site: &SiteSpec) -> Option<Kind> {
    if let Some(force) = site.force {
        return Some(force);
    }

    if site.mixed {
        if window.contains("rootGraphId") || window.contains("RouteManifest") {
            return Some(Kind::Manifest);
        }
        if next.contains("snapshot: {") || window.to_ascii_lowercase().contains("bootstrap") {
            return Some(Kind::Bootstrap);
        }
        if window.to_ascii_lowercase().contains("provider") {
            return Some(Kind::HostProvider);
        }
        return hint.or(site.fallback);
    }

    Some(hint.unwrap_or(Kind::Snapshot))
}

fn window(lines: &[String], index: usize) -> String {
    let start = index.saturating_sub(5);
    let end = (index + 6).min(lines.len());
    lines[start..end].join("\n")
}

fn next_lines(lines: &[String], index: usize, count: usize) -> String {
    let start = (index + 1).min(lines.len());
    let end = (index + 1 + count).min(lines.len());
    lines[start..end].join("\n")
}

/// All `version: N` style literals in one line, with a kind hint from the
/// immediate line shape. Handles JSON (`"version": 2`), TypeScript
/// (`version: 2`, `parsed.version === 2`), and Rust fixture mutation
/// (`snapshot["version"] = serde_json::json!(999)`).
fn version_matches(line: &str) -> Vec<(u32, Option<Kind>)> {
    let lower = line.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut search_from = 0usize;

    while let Some(offset) = lower[search_from..].find("version") {
        let after = search_from + offset + "version".len();
        search_from = after;

        let rest = &line[after.min(line.len())..];
        let Some((value, hint)) = parse_version_value(rest) else {
            continue;
        };

        out.push((value, hint));
    }

    out
}

fn parse_version_value(rest: &str) -> Option<(u32, Option<Kind>)> {
    let trimmed = rest.trim_start_matches(|c: char| {
        c.is_whitespace() || matches!(c, '"' | '\'' | ':' | '=' | ']' | '!' | '(')
    });

    if trimmed.starts_with("number") || trimmed.starts_with('?') || trimmed.is_empty() {
        return None;
    }

    let stripped = trimmed
        .strip_prefix("serde_json::json!(")
        .or_else(|| trimmed.strip_prefix("json!("))
        .unwrap_or(trimmed)
        .trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '('));

    let digit_end = stripped
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(stripped.len());
    let digits = &stripped[..digit_end];

    let value: u32 = digits.parse().ok()?;

    // A digit run followed by '.' is a semantic-version fragment
    // (`"version": "0.10"`), not a protocol literal.
    if stripped[digit_end..].starts_with('.') {
        return None;
    }

    // `version: 2,` directly before a `snapshot: {` is the bootstrap wrapper.
    let hint = if stripped.contains("snapshot") {
        Some(Kind::Snapshot)
    } else {
        None
    };

    Some((value, hint))
}

fn iter_site_files(repo: &Repo, site: &SiteSpec) -> Result<Vec<(String, Vec<String>)>, String> {
    let absolute = repo.root.join(site.path);

    if absolute.is_file() {
        let content = fs::read_to_string(&absolute)
            .map_err(|error| format!("cannot read {}: {error}", absolute.display()))?;
        return Ok(vec![(
            site.path.to_string(),
            content.lines().map(String::from).collect(),
        )]);
    }

    if absolute.is_dir() {
        let mut found = Vec::new();
        collect_dir_files(&absolute, &mut found);
        found.sort();

        let mut files = Vec::new();
        for file in found {
            let relative = file
                .strip_prefix(&repo.root)
                .unwrap_or(&file)
                .to_string_lossy()
                .to_string();
            if site
                .exclude
                .iter()
                .any(|excluded| relative.contains(excluded))
            {
                continue;
            }
            let Ok(content) = fs::read_to_string(&file) else {
                continue;
            };
            files.push((relative, content.lines().map(String::from).collect()));
        }
        return Ok(files);
    }

    // A missing site is not an error: the contract report shows canonical
    // rows even when a package has not been checked out or built yet.
    Ok(Vec::new())
}

fn collect_dir_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_dir_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "ts") {
            out.push(path);
        }
    }
}

/// A `const NAME: u32 = N;` initializer as written in the given source file —
/// guards against a stale constant disagreeing with a consumer elsewhere.
fn scan_definition(repo: &Repo, file: &str, needle: &str) -> Result<Option<(u32, usize)>, String> {
    let content = read_repo_file(repo, file)?;

    for (index, line) in content.lines().enumerate() {
        if line.contains(needle) {
            let after = line
                .split('=')
                .nth(1)
                .and_then(|rest| rest.trim().trim_end_matches(';').trim().parse::<u32>().ok());
            if let Some(value) = after {
                return Ok(Some((value, index + 1)));
            }
        }
    }

    Ok(None)
}

/// `RouteManifest::validate` freezes the manifest version as a literal.
fn scan_manifest_version(repo: &Repo) -> Result<Option<u32>, String> {
    let content = read_repo_file(repo, CANONICAL_FILE)?;

    for line in content.lines() {
        if line.contains("self.version != ") {
            let digits: String = line
                .split("!=")
                .nth(1)
                .map(|rest| {
                    rest.trim()
                        .chars()
                        .take_while(|c| c.is_ascii_digit())
                        .collect::<String>()
                })
                .unwrap_or_default();
            if let Ok(value) = digits.parse::<u32>() {
                return Ok(Some(value));
            }
        }
    }

    Ok(None)
}

/// The first bootstrap-wrapper version the server emits.
fn first_bootstrap_version(repo: &Repo) -> Result<Option<u32>, String> {
    let content = read_repo_file(repo, "crates/plec-server/src/ssr/snapshot.rs")?;

    for line in content.lines() {
        if !line.contains("BOOTSTRAP_WRAPPER_VERSION") {
            continue;
        }
        if line.contains("SSR_SNAPSHOT_VERSION") {
            return Ok(Some(plec_ir::SSR_SNAPSHOT_VERSION));
        }
        for (value, _) in version_matches(line) {
            return Ok(Some(value));
        }
    }

    Ok(None)
}

fn read_repo_file(repo: &Repo, relative: &str) -> Result<String, String> {
    fs::read_to_string(repo.root.join(relative))
        .map_err(|error| format!("cannot read {relative}: {error}"))
}

pub fn print_report(report: &Report) -> bool {
    println!("SSR protocol contract\n");

    let show = |value: Option<u32>| {
        value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "?".into())
    };

    println!("Snapshot");
    println!(
        "  canonical version       {}  {}",
        show(report.snapshot.canonical),
        report.snapshot.canonical_source
    );
    for row in &report.snapshot.rows {
        print_row(row);
    }

    println!("\nBootstrap");
    println!(
        "  canonical version       {}  {}",
        show(report.bootstrap.canonical),
        report.bootstrap.canonical_source
    );
    for row in &report.bootstrap.rows {
        print_row(row);
    }

    println!("\nHost provider manifest");
    println!(
        "  canonical version       {}  {}",
        show(report.host_provider.canonical),
        report.host_provider.canonical_source
    );
    for row in &report.host_provider.rows {
        print_row(row);
    }

    println!("\nManifest");
    println!(
        "  canonical version       {}  {}",
        show(report.manifest.canonical),
        report.manifest.canonical_source
    );
    for row in &report.manifest.rows {
        print_row(row);
    }

    let conflicts = report.conflicts();
    println!();
    if conflicts.is_empty() {
        println!("✓ no stale hard-coded protocol versions");
        true
    } else {
        println!("✗ {} conflict(s)", conflicts.len());
        for row in &conflicts {
            println!("  {}:{} — {}", row.file, row.line, row.status);
        }
        false
    }
}

fn print_row(row: &Row) {
    let value = row
        .value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "—".into());
    println!(
        "  {:22} {:>4}  {}:{}  {}",
        row.site, value, row.file, row.line, row.status
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_json_literals() {
        let values = version_matches(r#"        "version": 2,"#);
        assert_eq!(values, vec![(2, None)]);
    }

    #[test]
    fn extracts_ts_gate_comparisons() {
        let values = version_matches("    parsed.version === 2 &&");
        assert_eq!(values, vec![(2, None)]);
    }

    #[test]
    fn extracts_rust_fixture_mutations() {
        let values = version_matches(r#"    snapshot["version"] = serde_json::json!(999);"#);
        assert_eq!(values, vec![(999, None)]);
    }

    #[test]
    fn definition_lines_yield_no_literal_value() {
        // The definition is reported through `scan_definition`, which parses
        // the `= N` initializer — `version_matches` finds no literal because
        // the type annotation precedes the value.
        let values = version_matches("pub const SSR_SNAPSHOT_VERSION: u32 = 2;");
        assert!(values.is_empty());
    }

    #[test]
    fn parses_definition_initializer() {
        let line = "pub const SSR_SNAPSHOT_VERSION: u32 = 2;";
        let after = line.split('=').nth(1).unwrap().trim().trim_end_matches(';');
        assert_eq!(after.parse::<u32>().unwrap(), 2);
    }

    #[test]
    fn skips_type_annotations_and_prose() {
        assert!(
            version_matches("mutate: (payload: { version: number; snapshot: any }) => void,")
                .is_empty()
        );
        assert!(version_matches("the snapshot has its own version constant").is_empty());
    }

    #[test]
    fn classifies_manifest_context() {
        let site = SiteSpec {
            label: "fixtures",
            path: "x",
            mixed: true,
            force: None,
            fallback: Some(Kind::Snapshot),
            allow: &[],
            exclude: &[],
        };
        let line = r#"        "version": 3,"#;
        let context = r#"        "rootGraphId": "app#Root","#;

        assert_eq!(classify(context, "", None, &site), Some(Kind::Manifest));
        // A `snapshot: {` opener within the next two lines marks the
        // bootstrap wrapper, not the snapshot payload itself.
        assert_eq!(
            classify(line, "snapshot: {", None, &site),
            Some(Kind::Bootstrap)
        );
        assert_eq!(classify(line, "", None, &site), Some(Kind::Snapshot));
    }

    #[test]
    fn classifies_host_provider_context_and_fallbacks() {
        let browser_gate = SiteSpec {
            label: "browser gate",
            path: "packages/plec-browser/src/index.ts",
            mixed: true,
            force: None,
            fallback: Some(Kind::Bootstrap),
            allow: &[],
            exclude: &[],
        };
        // The host-provider gate sits in the same file as the bootstrap
        // gate; the window decides which boundary a literal belongs to.
        assert_eq!(
            classify("type HostProviderManifest = {", "", None, &browser_gate),
            Some(Kind::HostProvider)
        );
        assert_eq!(
            classify(
                "const manifest = value as Partial<HostProviderManifest>;",
                "",
                None,
                &browser_gate
            ),
            Some(Kind::HostProvider)
        );
        // The bootstrap wrapper gate itself: no boundary keyword in the
        // window, so the site fallback applies.
        assert_eq!(
            classify("parsed.version === 2 &&", "", None, &browser_gate),
            Some(Kind::Bootstrap)
        );

        // The producer file also emits the server host manifest; those
        // literals belong to an untracked boundary and must be dropped.
        let producer = SiteSpec {
            label: "host-provider producer",
            path: HOST_PROVIDER_CANONICAL_FILE,
            mixed: true,
            force: None,
            fallback: None,
            allow: &[],
            exclude: &[],
        };
        assert_eq!(
            classify("let manifest = ProviderManifest {", "", None, &producer),
            Some(Kind::HostProvider)
        );
        assert_eq!(
            classify("let manifest = ServerManifest {", "", None, &producer),
            None
        );
    }

    #[test]
    fn row_status_respects_allowlist() {
        let site = SiteSpec {
            label: "fixtures",
            path: "x",
            mixed: true,
            force: None,
            fallback: Some(Kind::Snapshot),
            allow: &[(999, "version gate fixture")],
            exclude: &[],
        };
        assert_eq!(
            row_status(999, Kind::Snapshot, &site, 2, None, None, Some(1)),
            "~ version gate fixture"
        );
        assert_eq!(
            row_status(1, Kind::Snapshot, &site, 2, None, None, Some(1)),
            "CONFLICT: found 1, expected 2"
        );
        assert_eq!(
            row_status(2, Kind::Snapshot, &site, 2, None, None, Some(1)),
            "✓"
        );
        // Host-provider literals compare against the provider canonical.
        assert_eq!(
            row_status(1, Kind::HostProvider, &site, 2, None, None, Some(1)),
            "✓"
        );
        assert_eq!(
            row_status(2, Kind::HostProvider, &site, 2, None, None, Some(1)),
            "CONFLICT: found 2, expected 1"
        );
    }
}
