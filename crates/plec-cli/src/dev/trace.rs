use super::repo::Repo;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

/// Semantic grep: categorize matches for a symbol or adoption error code by
/// their role in the repo, so an agent can answer "where can this come from,
/// and which tests expect it?" without reconstructing provenance by hand.

const MAX_PER_CATEGORY: usize = 25;
const MAX_FILE_BYTES: u64 = 512 * 1024;

const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".cache",
    ".turbo",
    ".tools",
    "test-results",
    "playwright-report",
    "coverage",
    ".tmp",
    ".yarn",
    ".vscode",
    ".idea",
];

const SEARCHABLE_EXTENSIONS: &[&str] = &[
    "rs",
    "ts",
    "tsx",
    "mts",
    "cts",
    "js",
    "mjs",
    "cjs",
    "md",
    "json",
    "toml",
    "yaml",
    "yml",
    "html",
    "css",
    "sh",
    "playwright.ts",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    DefinedIn,
    ProducedBy,
    AssertedBy,
    DocumentedBy,
    Other,
}

impl Category {
    pub fn heading(&self) -> &'static str {
        match self {
            Category::DefinedIn => "DEFINED IN",
            Category::ProducedBy => "PRODUCED BY",
            Category::AssertedBy => "ASSERTED BY",
            Category::DocumentedBy => "DOCUMENTED BY",
            Category::Other => "OTHER",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TraceMatch {
    pub file: String,
    pub line: usize,
    pub text: String,
    pub category: Category,
}

#[derive(Debug, Clone, Serialize)]
pub struct TraceReport {
    pub query: String,
    pub related_contract: Option<String>,
    pub matches: Vec<TraceMatch>,
    pub truncated: usize,
}

pub fn trace(repo: &Repo, query: &str) -> TraceReport {
    let mut matches: Vec<TraceMatch> = Vec::new();
    let mut truncated = 0usize;

    let mut files = Vec::new();
    collect_files(&repo.root, &mut files);
    files.sort();

    for file in files {
        let relative = file
            .strip_prefix(&repo.root)
            .unwrap_or(&file)
            .to_string_lossy()
            .to_string();

        let Ok(content) = fs::read_to_string(&file) else {
            continue;
        };

        for (index, line) in content.lines().enumerate() {
            if !line.contains(query) {
                continue;
            }

            let category = categorize(&relative, line);
            if matches.iter().filter(|m| m.category == category).count() >= MAX_PER_CATEGORY {
                truncated += 1;
                continue;
            }

            matches.push(TraceMatch {
                file: relative.clone(),
                line: index + 1,
                text: line.trim().to_string(),
                category,
            });
        }
    }

    matches.sort_by(|a, b| {
        a.category
            .cmp(&b.category)
            .then_with(|| a.file.cmp(&b.file))
            .then_with(|| a.line.cmp(&b.line))
    });

    TraceReport {
        query: query.to_string(),
        related_contract: related_contract(query),
        matches,
        truncated,
    }
}

fn categorize(relative: &str, line: &str) -> Category {
    let is_test_path = relative.contains("/tests/")
        || relative.ends_with(".test.ts")
        || relative.ends_with(".test.tsx")
        || relative.contains("packages/plec-e2e/");
    let is_doc_path = relative.ends_with(".md");
    let is_source_path = relative.contains("/src/");

    // A definition line inside source wins over path-based categories so a
    // constant or function declaration is always reported as a definition.
    if is_source_path && is_definition_line(line) {
        return Category::DefinedIn;
    }

    if is_test_path {
        return Category::AssertedBy;
    }
    if is_doc_path {
        return Category::DocumentedBy;
    }
    if is_source_path {
        return Category::ProducedBy;
    }

    Category::Other
}

fn is_definition_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("pub const ")
        || trimmed.starts_with("const ")
        || trimmed.starts_with("pub fn ")
        || trimmed.starts_with("pub(crate) fn ")
        || trimmed.starts_with("fn ")
        || trimmed.starts_with("export function ")
        || trimmed.starts_with("export const ")
        || trimmed.starts_with("function ")
        || trimmed.starts_with("class ")
        || trimmed.starts_with("export class ")
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_dir() {
            let skipped = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| SKIP_DIRS.contains(&name));
            if !skipped {
                collect_files(&path, out);
            }
            continue;
        }

        let searchable = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| SEARCHABLE_EXTENSIONS.contains(&ext));
        if !searchable {
            continue;
        }

        if entry.metadata().map(|m| m.len()).unwrap_or(0) > MAX_FILE_BYTES {
            continue;
        }

        out.push(path);
    }
}

/// Known adoption error codes and symbols map to the contract they belong to.
/// The value is read from the compiled workspace so it stays current.
pub(super) fn related_contract(query: &str) -> Option<String> {
    let snapshot_value = plec_ir::SSR_SNAPSHOT_VERSION;

    let contract = if query == "SSR_SNAPSHOT_VERSION" || query.contains("ssr-snapshot-version") {
        Some(format!(
            "SSR_SNAPSHOT_VERSION = {snapshot_value} ({})",
            super::contract::CANONICAL_FILE
        ))
    } else if query.contains("stale-revision") {
        Some("Snapshot revision gate: snapshot.revision must equal RouteManifest.revision (crates/plec-ir/src/lib.rs)".into())
    } else if query.contains("ssr-marker") || query.contains("duplicate:ssr") {
        Some("Marker uniqueness: every structural address names at most one node (docs/dom-address-protocol.md)".into())
    } else if query.contains("missing:ssr-loop") || query.contains("missing:ssr-branch") {
        Some("Nested component execution state: snapshot v2 structure.nested records (docs/ssr-architecture.md)".into())
    } else if query.contains("ssr-component") {
        Some("Component ownership boundary: <!--plec:component:...--> markers (docs/dom-address-protocol.md)".into())
    } else if query.contains("mismatch:ssr") || query.contains("missing:ssr") {
        Some("Adoption fails closed on structural disagreement (docs/ssr-architecture.md)".into())
    } else if query.contains("invalid:ssr-bootstrap") {
        Some(
            "Bootstrap payload must parse as JSON or adoption fails closed (packages/plec-browser)"
                .into(),
        )
    } else if query.contains("adoption-once") {
        Some("Adoption claims server DOM at most once per application lifetime (docs/dom-address-protocol.md)".into())
    } else if query.contains("detached:ssr-text") {
        Some("Text marker adjacency: the marker must immediately precede its text (docs/dom-address-protocol.md)".into())
    } else {
        None
    };

    contract
}

pub fn print_report(report: &TraceReport) {
    println!("{}\n", report.query);

    if let Some(contract) = &report.related_contract {
        println!("RELATED CONTRACT");
        println!("  {contract}\n");
    }

    if report.matches.is_empty() {
        println!("(no matches)");
        return;
    }

    let mut current: Option<Category> = None;
    for match_item in &report.matches {
        if current != Some(match_item.category) {
            current = Some(match_item.category);
            println!("{}", match_item.category.heading());
        }
        println!(
            "  {}:{}  {}",
            match_item.file, match_item.line, match_item.text
        );
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

    #[test]
    fn test_paths_are_assertions() {
        assert_eq!(
            categorize(
                "crates/plec-runtime/tests/typed_events.rs",
                "\"unsupported:ssr-snapshot-version\""
            ),
            Category::AssertedBy
        );
        assert_eq!(
            categorize("packages/plec-browser/src/bootstrap.test.ts", "version: 2,"),
            Category::AssertedBy
        );
        assert_eq!(
            categorize(
                "packages/plec-e2e/tests/acceptance/adoption.playwright.ts",
                "adopted"
            ),
            Category::AssertedBy
        );
    }

    #[test]
    fn definitions_win_in_source_paths() {
        assert_eq!(
            categorize(
                "crates/plec-ir/src/lib.rs",
                "pub const SSR_SNAPSHOT_VERSION: u32 = 2;"
            ),
            Category::DefinedIn
        );
        assert_eq!(
            categorize(
                "crates/plec-server/src/ssr/snapshot.rs",
                "pub(crate) fn bootstrap_payload("
            ),
            Category::DefinedIn
        );
    }

    #[test]
    fn docs_and_producers() {
        assert_eq!(
            categorize("docs/ssr-architecture.md", "SSR_SNAPSHOT_VERSION = 2"),
            Category::DocumentedBy
        );
        assert_eq!(
            categorize(
                "crates/plec-runtime/src/runtime/lifecycle.rs",
                "if parsed.version != x {"
            ),
            Category::ProducedBy
        );
    }

    #[test]
    fn known_error_codes_have_contracts() {
        let contract = related_contract("unsupported:ssr-snapshot-version").unwrap();
        assert!(contract.contains("SSR_SNAPSHOT_VERSION = 2"));

        assert!(related_contract("missing:ssr-loop:root/outlet:main:6").is_some());
        assert!(related_contract("some_random_symbol").is_none());
    }
}
