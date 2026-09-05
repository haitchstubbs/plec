//! Compile the Plec-owned runtime WASM artifact.
//!
//! The WASM runtime is a Plec asset, not a consumer asset: applications stage
//! whatever `packages/plec-runtime/dist/runtime` contains and never compile it
//! themselves. The build pipeline itself stays in `scripts/build-wasm.mjs`
//! (wasm-pack → wasm-tools strip → protocol stamp → brotli sidecars →
//! provenance); this command is its CLI entry point so the workflow does not
//! depend on remembering a yarn incantation. It stays behind the dev frontend
//! because shipping a release app must not require the WASM toolchain — the
//! app build (`plec build`) stages the prebuilt artifact and fails loudly when
//! it is missing. Producer and verifier remain separate layers:
//! `plec workspace artifact stale` audits whatever this command produced.

use serde::Serialize;
use std::process::Command;

use super::repo::Repo;
use super::wasm_section::read_custom_section;
use crate::dev::artifact::{ProtocolSection, PROTOCOL_SECTION};

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum CompileProfile {
    Full,
    Core,
    Router,
    Fetch,
}

impl CompileProfile {
    fn as_str(self) -> &'static str {
        match self {
            CompileProfile::Full => "full",
            CompileProfile::Core => "core",
            CompileProfile::Router => "router",
            CompileProfile::Fetch => "fetch",
        }
    }
}

pub struct CompileOptions {
    pub profile: CompileProfile,
    pub features: Option<String>,
    pub no_optimize: bool,
}

#[derive(Debug, Serialize)]
pub struct CompileReport {
    pub out_dir: String,
    pub profile: String,
    pub wasm_bytes: u64,
    pub wasm_brotli_bytes: Option<u64>,
    pub js_bytes: u64,
    pub js_brotli_bytes: Option<u64>,
    /// SSR snapshot protocol the freshly built binary actually implements,
    /// read back from its `plec-protocol` custom section.
    pub protocol: Option<u32>,
    pub wasm_sha256: Option<String>,
}

/// Builds the `node scripts/build-wasm.mjs` argument list. Split out for a
/// unit test: the script's CLI is positional plus flags, and a wrong order
/// would silently build the wrong thing.
fn build_script_args(options: &CompileOptions) -> Vec<String> {
    let mut args = vec![
        "scripts/build-wasm.mjs".to_string(),
        "crates/plec-runtime".to_string(),
        "packages/plec-runtime/dist/runtime".to_string(),
        "--profile".to_string(),
        options.profile.as_str().to_string(),
    ];
    if let Some(features) = &options.features {
        args.push("--features".to_string());
        args.push(features.clone());
    }
    if options.no_optimize {
        args.push("--no-optimize".to_string());
    }
    args
}

pub fn compile(repo: &Repo, options: &CompileOptions) -> Result<CompileReport, String> {
    let args = build_script_args(options);
    let mut child = Command::new("node")
        .args(&args)
        .current_dir(&repo.root)
        .spawn()
        .map_err(|error| format!("cannot run node ({error}) — is Node.js on PATH?"))?;

    let status = child
        .wait()
        .map_err(|error| format!("node exited abnormally: {error}"))?;
    if !status.success() {
        return Err(format!(
            "runtime WASM build failed (exit {status}) — see the wasm-pack output above"
        ));
    }

    report_artifact(repo, options)
}

/// Reads back what the build produced: artifact sizes, brotli sidecars, and
/// the protocol section the fresh binary carries. Reading the section again
/// here keeps the command self-verifying — a build that stamped nothing would
/// report `protocol: none` instead of silently shipping.
fn report_artifact(repo: &Repo, options: &CompileOptions) -> Result<CompileReport, String> {
    let out_dir = repo.runtime_dist_dir();
    let wasm_path = out_dir.join("runtime_bg.wasm");
    let js_path = out_dir.join("runtime.js");

    let file_len = |path: &std::path::Path| {
        std::fs::metadata(path)
            .map(|meta| meta.len())
            .map_err(|error| format!("cannot read {}: {error}", path.display()))
    };

    let wasm_bytes = file_len(&wasm_path)?;
    let js_bytes = file_len(&js_path)?;
    let wasm_brotli_bytes = file_len(&wasm_path.with_extension("wasm.br")).ok();
    let js_brotli_bytes = file_len(&js_path.with_extension("js.br")).ok();

    let wasm = std::fs::read(&wasm_path)
        .map_err(|error| format!("cannot read {}: {error}", wasm_path.display()))?;
    let protocol = read_custom_section(&wasm, PROTOCOL_SECTION)?
        .map(|payload| serde_json::from_slice::<ProtocolSection>(&payload))
        .transpose()
        .map_err(|error| format!("cannot parse {PROTOCOL_SECTION} section: {error}"))?
        .map(|section| section.ssrSnapshot);

    // Field names mirror the camelCase keys written by build-wasm.mjs.
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Provenance {
        wasm_sha256: Option<String>,
    }
    let provenance: Provenance = std::fs::read(out_dir.join("provenance.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or(Provenance { wasm_sha256: None });

    Ok(CompileReport {
        out_dir: out_dir
            .strip_prefix(&repo.root)
            .unwrap_or(&out_dir)
            .display()
            .to_string(),
        profile: options.profile.as_str().to_string(),
        wasm_bytes,
        wasm_brotli_bytes,
        js_bytes,
        js_brotli_bytes,
        protocol,
        wasm_sha256: provenance.wasm_sha256,
    })
}

pub fn print_report(report: &CompileReport) {
    println!("runtime WASM compiled");
    println!(
        "  wasm       {} bytes (brotli: {})",
        report.wasm_bytes,
        report
            .wasm_brotli_bytes
            .map(|bytes| bytes.to_string())
            .unwrap_or_else(|| "none".into()),
    );
    println!(
        "  glue js    {} bytes (brotli: {})",
        report.js_bytes,
        report
            .js_brotli_bytes
            .map(|bytes| bytes.to_string())
            .unwrap_or_else(|| "none".into()),
    );
    println!(
        "  protocol   ssrSnapshot {}",
        report
            .protocol
            .map(|version| version.to_string())
            .unwrap_or_else(|| "MISSING".into()),
    );
    if let Some(sha) = &report.wasm_sha256 {
        println!("  sha256     {sha}");
    }
    println!("  output     {}", report.out_dir);
    println!(
        "next: plec build stages this artifact; plec workspace artifact stale verifies the pipeline"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(
        profile: CompileProfile,
        features: Option<&str>,
        no_optimize: bool,
    ) -> CompileOptions {
        CompileOptions {
            profile,
            features: features.map(str::to_owned),
            no_optimize,
        }
    }

    #[test]
    fn forwards_profile_features_and_optimize_flag_in_script_order() {
        assert_eq!(
            build_script_args(&options(CompileProfile::Full, None, false)),
            vec![
                "scripts/build-wasm.mjs",
                "crates/plec-runtime",
                "packages/plec-runtime/dist/runtime",
                "--profile",
                "full",
            ]
        );
        assert_eq!(
            build_script_args(&options(CompileProfile::Fetch, Some("fetch"), true)),
            vec![
                "scripts/build-wasm.mjs",
                "crates/plec-runtime",
                "packages/plec-runtime/dist/runtime",
                "--profile",
                "fetch",
                "--features",
                "fetch",
                "--no-optimize",
            ]
        );
    }
}
