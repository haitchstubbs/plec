use super::repo::Repo;
use super::wasm_section::read_custom_section;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::process::Command;

/// Provenance for the compiled runtime artifact pipeline:
///
/// ```text
/// crates/plec-runtime (source)
///     → wasm-pack → temporary release directory → packages/plec/dist/runtime
///     → staged copy → apps/fullstack/dist/runtime (what the app serves)
/// ```
///
/// Two staleness shapes are checked:
/// - **build identity** — does the file on disk still match the hash recorded
///   when it was built, and does the staged copy match package dist?
/// - **protocol drift** — which SSR snapshot protocol does the binary
///   actually implement? Read from the `plec-protocol` WASM custom section
///   embedded at compile time, so a stale binary built against snapshot v1
///   is caught even though source now says v2.

pub const PROTOCOL_SECTION: &str = "plec-protocol";

// Field names mirror the camelCase keys written by scripts/build-wasm.mjs.
#[allow(non_snake_case)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolSection {
    #[serde(default)]
    pub ssrSnapshot: u32,
}

#[allow(non_snake_case)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceFile {
    #[serde(default)]
    pub builtAt: String,
    #[serde(default)]
    pub gitFingerprint: String,
    #[serde(default)]
    pub gitDirty: bool,
    #[serde(default)]
    pub wasmSha256: String,
    #[serde(default)]
    pub jsSha256: String,
    #[serde(default)]
    pub protocol: Option<ProtocolSection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layer {
    pub name: String,
    pub path: String,
    pub present: bool,
    pub wasm_sha256: Option<String>,
    /// Protocol implemented by this binary, from its custom section.
    pub protocol: Option<u32>,
    pub problems: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceReport {
    pub source_protocol: u32,
    pub git_fingerprint: Option<String>,
    pub git_dirty: bool,
    pub provenance_file: Option<ProvenanceFile>,
    pub dist: Option<Layer>,
    pub staged: Option<Layer>,
    pub ok: bool,
}

pub fn inspect(repo: &Repo) -> ProvenanceReport {
    let source_protocol = plec_ir::SSR_SNAPSHOT_VERSION;
    let git = git_fingerprint(repo);

    let dist_dir = repo.runtime_dist_dir();
    let staged_dir = repo.staged_runtime_dir();

    let provenance_file = read_provenance(&dist_dir);
    let mut dist = inspect_layer(&dist_dir, "package dist");
    let mut staged = inspect_layer(&staged_dir, "app staged");

    let mut ok = true;

    if let Some(layer) = dist.as_mut() {
        check_layer(layer, source_protocol);
        if let Some(provenance) = &provenance_file {
            if !provenance.wasmSha256.is_empty()
                && layer.wasm_sha256.as_deref() != Some(provenance.wasmSha256.as_str())
            {
                layer.problems.push(
                    "dist wasm hash differs from provenance.json — rebuilt or modified after build"
                        .into(),
                );
            }
        }
    }
    if let Some(layer) = staged.as_mut() {
        check_layer(layer, source_protocol);
        let dist_hash = dist.as_ref().and_then(|layer| layer.wasm_sha256.clone());
        if let (Some(staged_hash), Some(dist_hash)) = (&layer.wasm_sha256, dist_hash) {
            if staged_hash != &dist_hash {
                layer
                    .problems
                    .push("staged wasm hash differs from package dist — run the fullstack build to restage".into());
            }
        }
    }

    ok &= dist
        .as_ref()
        .map(|layer| layer.problems.is_empty())
        .unwrap_or(false);
    ok &= staged
        .as_ref()
        .map(|layer| layer.problems.is_empty())
        .unwrap_or(false);
    ok &= provenance_file.is_some();

    ProvenanceReport {
        source_protocol,
        git_fingerprint: git.as_ref().map(|(hash, _)| hash.clone()),
        git_dirty: git.map(|(_, dirty)| dirty).unwrap_or(false),
        provenance_file,
        dist,
        staged,
        ok,
    }
}

fn check_layer(layer: &mut Layer, source_protocol: u32) {
    if !layer.present {
        layer
            .problems
            .push("missing — artifact has not been built".into());
        return;
    }

    match layer.protocol {
        None => layer
            .problems
            .push("no plec-protocol custom section — rebuild with marker support".into()),
        Some(protocol) if protocol != source_protocol => layer.problems.push(format!(
            "STALE: implements snapshot protocol {protocol}, source is {source_protocol}"
        )),
        Some(_) => {}
    }
}

fn inspect_layer(dir: &std::path::Path, name: &str) -> Option<Layer> {
    let wasm_path = dir.join("runtime_bg.wasm");
    let present = wasm_path.is_file();

    let layer = Layer {
        name: name.into(),
        path: dir.display().to_string(),
        present,
        wasm_sha256: None,
        protocol: None,
        problems: Vec::new(),
    };

    if !present {
        return Some(layer);
    }

    let wasm = fs::read(&wasm_path).ok()?;
    let wasm_sha256 = Some(sha256_hex(&wasm));
    let protocol = read_custom_section(&wasm, PROTOCOL_SECTION)
        .ok()
        .flatten()
        .and_then(|payload| serde_json::from_slice::<ProtocolSection>(&payload).ok())
        .map(|section| section.ssrSnapshot);

    Some(Layer {
        wasm_sha256,
        protocol,
        ..layer
    })
}

fn read_provenance(dir: &std::path::Path) -> Option<ProvenanceFile> {
    let bytes = fs::read(dir.join("provenance.json")).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn git_fingerprint(repo: &Repo) -> Option<(String, bool)> {
    let head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&repo.root)
        .output()
        .ok()?;
    if !head.status.success() {
        return None;
    }
    let hash = String::from_utf8_lossy(&head.stdout).trim().to_string();

    let dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(&repo.root)
        .output()
        .map(|output| !String::from_utf8_lossy(&output.stdout).trim().is_empty())
        .unwrap_or(false);

    Some((hash.chars().take(12).collect(), dirty))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Exported runtime surface, read from the generated `runtime.d.ts`: module
/// functions plus the methods of the runtime class (where the SSR
/// diagnostics like `ssr_text_divergences` live).
fn dist_exports(dist_dir: &std::path::Path) -> Vec<String> {
    let Ok(content) = fs::read_to_string(dist_dir.join("runtime.d.ts")) else {
        return Vec::new();
    };

    let mut exports = Vec::new();
    let mut in_class = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if let Some(rest) = trimmed.strip_prefix("export class ") {
            in_class = true;
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            exports.push(format!("{name} (class)"));
            continue;
        }
        if in_class && trimmed == "}" {
            in_class = false;
            continue;
        }

        if in_class {
            let name: String = trimmed
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            // Method declarations look like `name(args): ret;`
            if !name.is_empty()
                && trimmed.starts_with(&name)
                && trimmed[name.len()..].starts_with('(')
            {
                exports.push(name);
            }
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("export function ") {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                exports.push(name);
            }
        }
    }

    exports
}

pub fn print_provenance(report: &ProvenanceReport, repo: &Repo) -> bool {
    println!("plec-runtime\n");

    println!("source");
    if let Some(fingerprint) = &report.git_fingerprint {
        println!(
            "  git fingerprint       {fingerprint}{}",
            if report.git_dirty {
                " (dirty tree)"
            } else {
                ""
            }
        );
    }
    println!(
        "  snapshot protocol     {} (compiled into plec-cli)",
        report.source_protocol
    );

    if let Some(provenance) = &report.provenance_file {
        println!("\npackage dist provenance");
        println!("  built                 {}", provenance.builtAt);
        if !provenance.gitFingerprint.is_empty() {
            println!(
                "  git fingerprint       {}{}",
                provenance.gitFingerprint,
                if provenance.gitDirty {
                    " (dirty tree)"
                } else {
                    ""
                }
            );
        }
        if !provenance.wasmSha256.is_empty() {
            println!("  wasm sha256           {}", provenance.wasmSha256);
        }
    } else {
        println!("\npackage dist provenance");
        println!("  provenance.json       missing — rebuild (yarn workspace plec build:wasm)");
    }

    for layer in [&report.dist, &report.staged] {
        let Some(layer) = layer else { continue };

        println!("\n{} ({})", layer.name, layer.path);
        if !layer.present {
            println!("  wasm                  ✗ missing");
        } else {
            if let Some(hash) = &layer.wasm_sha256 {
                println!("  wasm sha256           {hash}");
            }
            match layer.protocol {
                Some(protocol) => println!("  snapshot protocol     {protocol}"),
                None => println!("  snapshot protocol     unknown (no marker)"),
            }
            let exports = dist_exports(&repo.runtime_dist_dir());
            if layer.name == "package dist" && !exports.is_empty() {
                const SHOWN: usize = 8;
                let shown: Vec<String> = exports.iter().take(SHOWN).cloned().collect();
                println!("  exports               {}", shown.join(", "));
                if exports.len() > SHOWN {
                    println!(
                        "                          … and {} more",
                        exports.len() - SHOWN
                    );
                }
            }
        }

        for problem in &layer.problems {
            println!("  ✗ {problem}");
        }
    }

    println!();
    if report.ok {
        println!("✓ artifact provenance consistent");
    } else {
        println!("✗ artifact provenance problems found");
    }

    report.ok
}

pub fn print_stale(report: &ProvenanceReport) -> bool {
    for layer in [&report.dist, &report.staged] {
        let Some(layer) = layer else { continue };

        let status = if !layer.present {
            "MISSING".to_string()
        } else if layer.problems.is_empty() {
            "OK".to_string()
        } else if layer
            .problems
            .iter()
            .any(|problem| problem.starts_with("STALE"))
        {
            "STALE".to_string()
        } else {
            "PROBLEM".to_string()
        };

        println!("{:26} {}", layer.name, status);
        for problem in &layer.problems {
            println!("  ✗ {problem}");
        }
    }

    report.ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_is_deterministic() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn parses_provenance_sections() {
        let payload = br#"{"ssrSnapshot":2}"#;
        let section: ProtocolSection = serde_json::from_slice(payload).unwrap();
        assert_eq!(section.ssrSnapshot, 2);
    }
}
