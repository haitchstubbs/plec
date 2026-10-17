//! Production artifact size inventory and comparison.

use super::repo::Repo;
use object::read::{Object as _, ObjectSymbol as _};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Inventory {
    schema_version: u32,
    build: BuildInfo,
    revision: String,
    artifacts: Vec<Artifact>,
    functions: Vec<Function>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BuildInfo {
    wasm_profile: String,
    server_profile: String,
    target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Artifact {
    kind: String,
    path: String,
    bytes: u64,
    brotli_bytes: Option<u64>,
    sha256: String,
    #[serde(default)]
    sections: Vec<SectionSize>,
    contributions: Vec<Contribution>,
    function_body_bytes: Option<u64>,
    other_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SectionSize {
    name: String,
    bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Contribution {
    name: String,
    bytes: u64,
    source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Function {
    artifact: String,
    identity: String,
    index: Option<u32>,
    crate_name: Option<String>,
    module: Option<String>,
    rust_signature: Option<String>,
    source_location: Option<String>,
    parameters: Vec<String>,
    results: Vec<String>,
    code_bytes: u64,
    retained_bytes: Option<u64>,
    callers: Vec<String>,
    callees: Vec<String>,
    exported: bool,
    indirect_calls: u32,
    attribution: String,
}

pub fn inspect(repo: &Repo, json_path: &Path, html_path: &Path) -> Result<(), String> {
    let output_root = repo.root.join(".cache/plec/wasm-size-build");
    fs::create_dir_all(&output_root)
        .map_err(|e| format!("create {}: {e}", output_root.display()))?;

    run(
        &repo.root,
        "node",
        &[
            "packages/plec/scripts/build-runtime.mjs",
            "--profile",
            "full",
        ],
    )?;
    run_with_env(
        &repo.root,
        "node",
        &[
            "scripts/build-wasm.mjs",
            "crates/plec-runtime",
            ".cache/plec/wasm-size-analysis-build",
            "--profile",
            "full",
        ],
        "PLEC_WASM_SIZE_ANALYSIS",
        "1",
    )?;

    let target_dir = repo.root.join(".cache/plec/wasm-size-target");
    run(
        &repo.root,
        "cargo",
        &[
            "build",
            "-p",
            "plec-node-bindings",
            "--profile",
            "node-release",
            "--lib",
            "--target-dir",
            target_dir.to_str().ok_or("non-UTF8 target path")?,
        ],
    )?;
    let native = native_library(&target_dir)?;

    if !repo.root.join("packages/plec/dist/browser.js").is_file()
        || !repo
            .root
            .join("packages/plec-browser/dist/index.js")
            .is_file()
    {
        run(
            &repo.root,
            "node",
            &["packages/plec/scripts/build-workspace.mjs"],
        )?;
    }

    let app_out = output_root.join("fullstack");
    plec_build::build(plec_build::BuildOptions {
        source: repo.root.join("apps/fullstack/src/router.tsx"),
        client_entry: PathBuf::from("src/client.tsx"),
        server_entry: PathBuf::from("src/server.ts"),
        out_dir: app_out.clone(),
        optimize: true,
        title: String::new(),
        description: None,
        styles_href: None,
        preloads: Vec::new(),
        runtime_source: plec_build::RuntimeSource::Auto,
        server_metafile: Some(
            repo.root
                .join(".cache/plec/wasm-size-build/server-app.meta.json"),
        ),
    })
    .map_err(|e| format!("build fullstack server bundle: {e}"))?;

    let wasm_path = repo.runtime_dist_dir().join("runtime_bg.wasm");
    let wasm_analysis_path = repo.root.join(".cache/plec/runtime-analysis.wasm");
    let bundle_path = app_out.join("server/app.mjs");
    let mut artifacts = vec![
        artifact(
            "wasm",
            &wasm_path,
            Some(wasm_path.with_extension("wasm.br")),
        )?,
        artifact("native", &native, None)?,
        artifact("server-bundle", &bundle_path, None)?,
    ];
    let production_functions = analyze_wasm(&wasm_path)?;
    let mut functions = analyze_wasm(&wasm_analysis_path)?;
    if production_functions.len() != functions.len()
        || production_functions
            .iter()
            .zip(&functions)
            .any(|(production, analysis)| production.index != analysis.index)
    {
        return Err("named analysis WASM does not match production function indices; refusing to attribute names to different production functions".into());
    }
    // The named analysis build may encode different bodies due to Rust's debug
    // compilation mode. Keep identities from it, but take exact body lengths
    // from the stripped production-equivalent artifact at the matching index.
    for (production, analysis) in production_functions.iter().zip(&mut functions) {
        analysis.code_bytes = production.code_bytes;
        analysis.attribution = if analysis.identity.starts_with("wasm-function[") {
            "unattributed; size from production artifact index mapping".into()
        } else {
            "name-section; size from production artifact index mapping".into()
        };
    }
    recompute_retained_sizes(&mut functions);
    functions.extend(analyze_native(&native)?);
    add_contributions(&mut artifacts, &functions);
    add_bundle_contributions(
        &mut artifacts,
        &repo
            .root
            .join(".cache/plec/wasm-size-build/server-app.meta.json"),
    )?;
    // esbuild does not provide reliable per-minified-function attribution;
    // preserve the bundle as an artifact and its size as the exact measurement.
    let revision = git(&repo.root, &["rev-parse", "HEAD"]).unwrap_or_else(|_| "unknown".into());
    let inventory = Inventory {
        schema_version: 1,
        build: BuildInfo {
            wasm_profile: "release (wasm-opt -Oz)".into(),
            server_profile: "node-release".into(),
            target: format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS),
        },
        revision,
        artifacts,
        functions,
    };
    let json_path = absolute_output(repo, json_path);
    let html_path = absolute_output(repo, html_path);
    if let Some(parent) = json_path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if let Some(parent) = html_path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(&inventory).map_err(|e| e.to_string())?;
    let chart_copy = repo.root.join(".cache/plec/wasm-size-chart.js");
    let chart_path = if chart_copy.is_file() {
        chart_copy
    } else {
        repo.root.join("node_modules/chart.js/dist/chart.umd.js")
    };
    let chart_library = fs::read_to_string(&chart_path)
        .map_err(|e| format!("read bundled Chart.js {}: {e}", chart_path.display()))?;
    fs::write(&json_path, &json).map_err(|e| format!("write {}: {e}", json_path.display()))?;
    fs::write(&html_path, html_report(&inventory, &chart_library))
        .map_err(|e| format!("write {}: {e}", html_path.display()))?;
    println!(
        "Size inventory: {}\nHTML report: {}",
        json_path.display(),
        html_path.display()
    );
    for a in &inventory.artifacts {
        println!(
            "  {}: {} bytes{}",
            a.kind,
            a.bytes,
            a.brotli_bytes
                .map(|n| format!(" (Brotli {n})"))
                .unwrap_or_default()
        );
    }
    Ok(())
}

pub fn top(
    repo: &Repo,
    input: &Path,
    crate_filter: Option<&str>,
    artifact_filter: Option<&str>,
    limit: usize,
) -> Result<(), String> {
    let path = absolute_output(repo, input);
    let inventory: Inventory = serde_json::from_slice(
        &fs::read(&path).map_err(|e| format!("read {}: {e}", path.display()))?,
    )
    .map_err(|e| format!("parse inventory: {e}"))?;
    if artifact_filter.is_some_and(|a| !["wasm", "native", "server-bundle"].contains(&a)) {
        return Err("--artifact must be wasm, native, or server-bundle".into());
    }
    let mut rows = inventory
        .functions
        .iter()
        .filter(|f| crate_filter.is_none_or(|c| f.crate_name.as_deref() == Some(c)))
        .filter(|f| artifact_filter.is_none_or(|a| f.artifact == a))
        .collect::<Vec<_>>();
    rows.sort_by_key(|f| std::cmp::Reverse(f.code_bytes));
    for f in rows.into_iter().take(limit) {
        println!(
            "{:>10}  {:<14} {:<24} {}",
            f.code_bytes,
            f.artifact,
            f.crate_name.as_deref().unwrap_or("unattributed"),
            f.identity
        );
    }
    Ok(())
}

pub fn diff(repo: &Repo, base: &str, target: &str, all: bool, limit: usize) -> Result<(), String> {
    // JSON paths compare previously captured inventories without rebuilding.
    let base_path = Path::new(base);
    let target_path = Path::new(target);
    if base_path.is_file() && target_path.is_file() {
        let before: Inventory =
            serde_json::from_slice(&fs::read(base_path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        let after: Inventory =
            serde_json::from_slice(&fs::read(target_path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        return print_diff(&before, &after, all, limit);
    }
    diff_revisions(repo, base, target, all, limit)
}

fn diff_revisions(
    repo: &Repo,
    base: &str,
    target: &str,
    all: bool,
    limit: usize,
) -> Result<(), String> {
    let work_root = repo.cache_dir().join("wasm-size-revisions");
    fs::create_dir_all(&work_root).map_err(|e| e.to_string())?;
    let nonce = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let base_dir = work_root.join(format!("{nonce}-base"));
    let target_dir = work_root.join(format!("{nonce}-target"));
    let base_json = base_dir.join(".cache/plec/wasm-size.json");
    let target_json = target_dir.join(".cache/plec/wasm-size.json");
    let result = (|| {
        for (revision, path) in [(base, &base_dir), (target, &target_dir)] {
            run(
                &repo.root,
                "git",
                &[
                    "worktree",
                    "add",
                    "--detach",
                    path.to_str().ok_or("non-UTF8 worktree path")?,
                    revision,
                ],
            )?;
            install_worktree_dependencies(&repo.root, path)?;
            copy_size_build_tools(&repo.root, path)?;
        }
        let executable = std::env::current_exe().map_err(|e| e.to_string())?;
        for path in [&base_dir, &target_dir] {
            let json = path.join(".cache/plec/wasm-size.json");
            let html = path.join(".cache/plec/wasm-size.html");
            let status = Command::new(&executable)
                .args([
                    "workspace",
                    "wasm-size",
                    "inspect",
                    "--json",
                    json.to_str().ok_or("non-UTF8 report path")?,
                    "--html",
                    html.to_str().ok_or("non-UTF8 report path")?,
                ])
                .current_dir(path)
                .status()
                .map_err(|e| format!("run size inspection for {}: {e}", path.display()))?;
            if !status.success() {
                return Err(format!(
                    "size inspection failed for {} ({status})",
                    path.display()
                ));
            }
        }
        let before: Inventory =
            serde_json::from_slice(&fs::read(&base_json).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        let after: Inventory =
            serde_json::from_slice(&fs::read(&target_json).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        print_diff(&before, &after, all, limit)
    })();
    for path in [&base_dir, &target_dir] {
        let _ = Command::new("git")
            .args(["worktree", "remove", "--force", path.to_str().unwrap_or("")])
            .current_dir(&repo.root)
            .status();
        let _ = fs::remove_dir_all(path);
    }
    result
}

fn print_diff(
    before: &Inventory,
    after: &Inventory,
    all: bool,
    limit: usize,
) -> Result<(), String> {
    let mut old = BTreeMap::new();
    for f in &before.functions {
        old.insert((f.artifact.clone(), f.identity.clone()), f.code_bytes);
    }
    let mut rows = after
        .functions
        .iter()
        .map(|f| {
            let previous = old
                .remove(&(f.artifact.clone(), f.identity.clone()))
                .unwrap_or(0);
            (f.identity.clone(), previous, f.code_bytes)
        })
        .collect::<Vec<_>>();
    rows.extend(
        old.into_iter()
            .map(|((_, name), previous)| (name, previous, 0)),
    );
    if !all {
        rows.retain(|(_, old, new)| old != new);
    }
    rows.sort_by_key(|(_, old, new)| std::cmp::Reverse(old.abs_diff(*new)));
    println!("Function                                      Before      After      Delta");
    let row_count = rows.len();
    for (name, old, new) in rows.into_iter().take(if all { usize::MAX } else { limit }) {
        println!(
            "{name:<44} {old:>9} {new:>9} {:+9}",
            new as i64 - old as i64
        );
    }
    if row_count == 0 {
        println!("No function-size changes.");
    } else if !all && row_count > limit {
        println!(
            "... {} additional function rows (use --limit or --all)",
            row_count - limit
        );
    }
    let old_artifacts = before
        .artifacts
        .iter()
        .map(|a| (a.kind.as_str(), a.bytes))
        .collect::<BTreeMap<_, _>>();
    println!("\nArtifact                                       Before      After      Delta");
    for artifact in &after.artifacts {
        let old = old_artifacts
            .get(artifact.kind.as_str())
            .copied()
            .unwrap_or(0);
        println!(
            "{:<44} {old:>9} {:>9} {:+9}",
            artifact.kind,
            artifact.bytes,
            artifact.bytes as i64 - old as i64
        );
    }
    Ok(())
}

fn analyze_wasm(path: &Path) -> Result<Vec<Function>, String> {
    let bytes = fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    analyze_wasm_bytes_with_names(&bytes, &BTreeMap::new())
}

#[cfg(test)]
fn analyze_wasm_bytes(bytes: &[u8]) -> Result<Vec<Function>, String> {
    analyze_wasm_bytes_with_names(bytes, &BTreeMap::new())
}

fn analyze_wasm_bytes_with_names(
    bytes: &[u8],
    overrides: &BTreeMap<u32, String>,
) -> Result<Vec<Function>, String> {
    let mut names = BTreeMap::<u32, String>::new();
    let mut types = Vec::<(Vec<String>, Vec<String>)>::new();
    let mut function_types = Vec::<u32>::new();
    let mut bodies = Vec::<(u64, Vec<u32>, u32)>::new();
    let mut exports = Vec::<u32>::new();
    let mut imports = 0u32;
    for payload in wasmparser::Parser::new(0).parse_all(bytes) {
        match payload.map_err(|e| format!("parse WASM: {e}"))? {
            wasmparser::Payload::TypeSection(reader) => {
                for ty in reader.into_iter_err_on_gc_types() {
                    let ty = ty.map_err(|e| e.to_string())?;
                    types.push((
                        ty.params().iter().map(|t| val_type(*t)).collect(),
                        ty.results().iter().map(|t| val_type(*t)).collect(),
                    ));
                }
            }
            wasmparser::Payload::ImportSection(reader) => {
                for imp in reader {
                    let imp = imp.map_err(|e| e.to_string())?;
                    if let wasmparser::TypeRef::Func(index) = imp.ty {
                        function_types.push(index);
                        imports += 1;
                    }
                }
            }
            wasmparser::Payload::FunctionSection(reader) => {
                for ty in reader {
                    function_types.push(ty.map_err(|e| e.to_string())?);
                }
            }
            wasmparser::Payload::ExportSection(reader) => {
                for export in reader {
                    let export = export.map_err(|e| e.to_string())?;
                    if export.kind == wasmparser::ExternalKind::Func {
                        exports.push(export.index);
                    }
                }
            }
            wasmparser::Payload::CodeSectionEntry(body) => {
                let mut callees = Vec::new();
                let mut indirect_calls = 0;
                let mut ops = body.get_operators_reader().map_err(|e| e.to_string())?;
                while !ops.eof() {
                    match ops.read().map_err(|e| e.to_string())? {
                        wasmparser::Operator::Call { function_index }
                        | wasmparser::Operator::ReturnCall { function_index } => {
                            callees.push(function_index)
                        }
                        wasmparser::Operator::CallIndirect { .. }
                        | wasmparser::Operator::ReturnCallIndirect { .. } => indirect_calls += 1,
                        _ => {}
                    }
                }
                bodies.push((body.as_bytes().len() as u64, callees, indirect_calls));
            }
            wasmparser::Payload::CustomSection(section) if section.name() == "name" => {
                let wasmparser::KnownCustom::Name(reader) = section.as_known() else {
                    continue;
                };
                for subsection in reader {
                    if let wasmparser::Name::Function(map) =
                        subsection.map_err(|e| e.to_string())?
                    {
                        for naming in map {
                            let naming = naming.map_err(|e| e.to_string())?;
                            names.insert(naming.index, normalize_wasm_name(naming.name));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let mut result = Vec::new();
    let call_edges = bodies
        .iter()
        .enumerate()
        .map(|(local, (_, callees, _))| (imports + local as u32, callees.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut reverse = BTreeMap::<u32, Vec<String>>::new();
    for (local, (_, callees, _)) in bodies.iter().enumerate() {
        let idx = imports + local as u32;
        let name = names
            .get(&idx)
            .cloned()
            .unwrap_or_else(|| format!("wasm-function[{idx}]"));
        for callee in callees {
            reverse.entry(*callee).or_default().push(name.clone());
        }
    }
    for (local, (size, callees, indirect_calls)) in bodies.into_iter().enumerate() {
        let index = imports + local as u32;
        let identity = names
            .get(&index)
            .cloned()
            .or_else(|| overrides.get(&index).cloned())
            .unwrap_or_else(|| format!("wasm-function[{index}]"));
        let type_index = function_types.get(index as usize).copied();
        let (parameters, results) = type_index
            .and_then(|i| types.get(i as usize).cloned())
            .unwrap_or_default();
        let crate_name = crate_from_symbol(&identity);
        result.push(Function {
            artifact: "wasm".into(),
            identity: identity.clone(),
            index: Some(index),
            crate_name,
            module: None,
            rust_signature: None,
            source_location: None,
            parameters,
            results,
            code_bytes: size,
            retained_bytes: None,
            callers: reverse.remove(&index).unwrap_or_default(),
            callees: callees
                .into_iter()
                .map(|i| {
                    names
                        .get(&i)
                        .cloned()
                        .unwrap_or_else(|| format!("wasm-function[{i}]"))
                })
                .collect(),
            exported: exports.contains(&index),
            indirect_calls,
            attribution: if names.contains_key(&index) {
                "name-section"
            } else if overrides.contains_key(&index) {
                "DWARF function range"
            } else {
                "unattributed"
            }
            .into(),
        });
    }
    let sizes = result
        .iter()
        .filter_map(|f| f.index.map(|i| (i, f.code_bytes)))
        .collect::<BTreeMap<_, _>>();
    let indices = result.iter().filter_map(|f| f.index).collect::<Vec<_>>();
    for function_index in indices {
        let mut pending = vec![function_index];
        let mut visited = std::collections::BTreeSet::new();
        let mut retained = 0u64;
        while let Some(index) = pending.pop() {
            if !visited.insert(index) {
                continue;
            }
            retained += sizes.get(&index).copied().unwrap_or(0);
            if let Some(callees) = call_edges.get(&index) {
                pending.extend(callees.iter().copied());
            }
        }
        if let Some(function) = result.iter_mut().find(|f| f.index == Some(function_index)) {
            function.retained_bytes = Some(retained);
        }
    }
    Ok(result)
}

fn wasm_section_sizes(bytes: &[u8]) -> Result<Vec<SectionSize>, String> {
    if bytes.get(..8) != Some(b"\0asm\x01\0\0\0") {
        return Err("invalid WASM header".into());
    }
    let mut totals = BTreeMap::<String, u64>::from([("wasm-header".into(), 8)]);
    let mut cursor = 8usize;
    while cursor < bytes.len() {
        let start = cursor;
        let id = *bytes.get(cursor).ok_or("truncated WASM section id")?;
        cursor += 1;
        let payload_len = read_leb_u32(bytes, &mut cursor)? as usize;
        let payload_start = cursor;
        let end = payload_start
            .checked_add(payload_len)
            .filter(|end| *end <= bytes.len())
            .ok_or("WASM section extends beyond artifact")?;
        let name = if id == 0 {
            let mut name_cursor = payload_start;
            let name_len = read_leb_u32(bytes, &mut name_cursor)? as usize;
            let name_end = name_cursor
                .checked_add(name_len)
                .filter(|name_end| *name_end <= end)
                .ok_or("invalid custom WASM section name")?;
            format!(
                "custom:{}",
                String::from_utf8_lossy(&bytes[name_cursor..name_end])
            )
        } else {
            match id {
                1 => "type",
                2 => "import",
                3 => "function",
                4 => "table",
                5 => "memory",
                6 => "global",
                7 => "export",
                8 => "start",
                9 => "element",
                10 => "code",
                11 => "data",
                12 => "data-count",
                13 => "tag",
                _ => "unknown",
            }
            .to_owned()
        };
        *totals.entry(name).or_default() += (end - start) as u64;
        cursor = end;
    }
    Ok(totals
        .into_iter()
        .map(|(name, bytes)| SectionSize { name, bytes })
        .collect())
}

fn read_leb_u32(bytes: &[u8], cursor: &mut usize) -> Result<u32, String> {
    let mut value = 0u32;
    for shift in (0..35).step_by(7) {
        let byte = *bytes.get(*cursor).ok_or("truncated WASM LEB128")?;
        *cursor += 1;
        if shift == 28 && byte & 0xf0 != 0 {
            return Err("WASM LEB128 overflows u32".into());
        }
        value |= ((byte & 0x7f) as u32) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err("invalid WASM LEB128".into())
}

fn analyze_native(path: &Path) -> Result<Vec<Function>, String> {
    let data = fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let file =
        object::File::parse(data.as_slice()).map_err(|e| format!("parse native binary: {e}"))?;
    let mut rows = Vec::new();
    for symbol in file
        .symbols()
        .filter(|s| s.is_definition() && s.kind() == object::SymbolKind::Text)
    {
        let Ok(name) = symbol.name() else { continue };
        let size = symbol.size();
        if size == 0 {
            continue;
        }
        let identity = rustc_demangle::try_demangle(name)
            .map(|symbol| format!("{symbol:#}"))
            .unwrap_or_else(|_| name.to_owned());
        let crate_name = crate_from_symbol(&identity);
        rows.push(Function {
            artifact: "native".into(),
            identity,
            index: None,
            crate_name,
            module: None,
            rust_signature: None,
            source_location: None,
            parameters: Vec::new(),
            results: Vec::new(),
            code_bytes: size,
            retained_bytes: None,
            callers: Vec::new(),
            callees: Vec::new(),
            exported: symbol.is_global(),
            indirect_calls: 0,
            attribution: "native-symbol; call graph unavailable".into(),
        });
    }
    Ok(rows)
}

fn val_type(ty: wasmparser::ValType) -> String {
    format!("{ty:?}").to_lowercase()
}

fn crate_from_symbol(identity: &str) -> Option<String> {
    // wasm-bindgen names exported runtime entry points and their ABI shims
    // from the exported JS name rather than a Rust path.
    if identity.starts_with("plecruntime_") {
        return Some("plec-runtime".into());
    }
    if !identity.contains("::") {
        return None;
    }
    if identity.contains(" as core::") || identity.contains(" as std::") {
        return Some("core".into());
    }
    if identity.contains(" as alloc::") {
        return Some("alloc".into());
    }
    let namespace = identity
        .trim_start_matches('<')
        .split("::")
        .next()?
        .split('[')
        .next()?;
    if namespace.ends_with('>') || namespace.starts_with('&') {
        return Some("core".into());
    }
    if namespace.is_empty()
        || !namespace
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return None;
    }
    Some(namespace.trim_start_matches('_').replace('_', "-"))
}

fn normalize_wasm_name(name: &str) -> String {
    let mut output = String::with_capacity(name.len());
    let mut rest = name;
    while let Some(open) = rest.find('[') {
        output.push_str(&rest[..open]);
        let after_open = &rest[open + 1..];
        let Some(close) = after_open.find(']') else {
            output.push_str(&rest[open..]);
            return output;
        };
        let hash = &after_open[..close];
        if hash.len() == 16 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            rest = &after_open[close + 1..];
        } else {
            output.push('[');
            rest = after_open;
        }
    }
    output.push_str(rest);
    output
}

fn recompute_retained_sizes(functions: &mut [Function]) {
    let by_name = functions
        .iter()
        .filter_map(|f| f.index.map(|i| (f.identity.clone(), i)))
        .collect::<BTreeMap<_, _>>();
    let sizes = functions
        .iter()
        .filter_map(|f| f.index.map(|i| (i, f.code_bytes)))
        .collect::<BTreeMap<_, _>>();
    let edges = functions
        .iter()
        .filter_map(|f| {
            f.index.map(|i| {
                (
                    i,
                    f.callees
                        .iter()
                        .filter_map(|name| by_name.get(name).copied())
                        .collect::<Vec<_>>(),
                )
            })
        })
        .collect::<BTreeMap<_, _>>();
    for function in functions {
        let Some(start) = function.index else {
            continue;
        };
        let mut pending = vec![start];
        let mut visited = std::collections::BTreeSet::new();
        let mut retained = 0u64;
        while let Some(index) = pending.pop() {
            if !visited.insert(index) {
                continue;
            }
            retained += sizes.get(&index).copied().unwrap_or(0);
            if let Some(callees) = edges.get(&index) {
                pending.extend(callees.iter().copied());
            }
        }
        function.retained_bytes = Some(retained);
    }
}

fn artifact(kind: &str, path: &Path, brotli: Option<PathBuf>) -> Result<Artifact, String> {
    let bytes = fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let compressed = brotli.and_then(|p| fs::metadata(p).ok().map(|m| m.len()));
    use sha2::Digest;
    let sha256 = format!("{:x}", sha2::Sha256::digest(&bytes));
    let sections = if kind == "wasm" {
        wasm_section_sizes(&bytes)?
    } else {
        Vec::new()
    };
    Ok(Artifact {
        kind: kind.into(),
        path: path.display().to_string(),
        bytes: bytes.len() as u64,
        brotli_bytes: compressed,
        sha256,
        sections,
        contributions: Vec::new(),
        function_body_bytes: None,
        other_bytes: None,
    })
}

fn add_contributions(artifacts: &mut [Artifact], functions: &[Function]) {
    for artifact in artifacts {
        let mut totals = BTreeMap::<String, u64>::new();
        for function in functions.iter().filter(|f| f.artifact == artifact.kind) {
            *totals
                .entry(
                    function
                        .crate_name
                        .clone()
                        .unwrap_or_else(|| "unattributed".into()),
                )
                .or_default() += function.code_bytes;
        }
        artifact.contributions = totals
            .into_iter()
            .map(|(name, bytes)| Contribution {
                name,
                bytes,
                source: "sum of shallow function body/symbol sizes; excludes non-function bytes"
                    .into(),
            })
            .collect();
        let function_bytes = functions
            .iter()
            .filter(|f| f.artifact == artifact.kind)
            .map(|f| f.code_bytes)
            .sum::<u64>();
        artifact.function_body_bytes = (artifact.kind != "server-bundle").then_some(function_bytes);
        artifact.other_bytes = artifact
            .function_body_bytes
            .map(|bytes| artifact.bytes.saturating_sub(bytes));
    }
}

fn add_bundle_contributions(artifacts: &mut [Artifact], metafile: &Path) -> Result<(), String> {
    let bytes = fs::read(metafile)
        .map_err(|e| format!("read esbuild metafile {}: {e}", metafile.display()))?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("parse esbuild metafile: {e}"))?;
    let mut contributions = Vec::new();
    if let Some(inputs) = value
        .get("outputs")
        .and_then(|v| v.as_object())
        .and_then(|o| o.values().next())
        .and_then(|v| v.get("inputs"))
        .and_then(|v| v.as_object())
    {
        for (name, input) in inputs {
            let bytes = input
                .get("bytesInOutput")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let package = name.split("node_modules/").nth(1).map(|s| {
                s.split('/')
                    .take(if s.starts_with('@') { 2 } else { 1 })
                    .collect::<Vec<_>>()
                    .join("/")
            });
            contributions.push(Contribution {
                name: package.unwrap_or_else(|| name.to_owned()),
                bytes,
                source: name.to_owned(),
            });
        }
    }
    contributions.sort_by_key(|c| std::cmp::Reverse(c.bytes));
    if let Some(artifact) = artifacts.iter_mut().find(|a| a.kind == "server-bundle") {
        artifact.contributions = contributions;
    }
    Ok(())
}

fn native_library(target: &Path) -> Result<PathBuf, String> {
    let dir = target.join("node-release");
    for name in [
        "libplec_node_bindings.so",
        "libplec_node_bindings.dylib",
        "plec_node_bindings.dll",
    ] {
        let path = dir.join(name);
        if path.is_file() {
            return Ok(path);
        }
    }
    Err(format!("native addon not found under {}", dir.display()))
}

fn install_worktree_dependencies(repo_root: &Path, worktree: &Path) -> Result<(), String> {
    let status = Command::new("yarn")
        .args(["install", "--immutable"])
        .current_dir(worktree)
        .status()
        .map_err(|e| format!("run yarn install in {}: {e}", worktree.display()))?;
    if !status.success() {
        return Err(format!(
            "yarn install --immutable failed in {} ({status})",
            worktree.display()
        ));
    }
    let tools = repo_root.join(".tools");
    if tools.is_dir() {
        let destination = worktree.join(".tools");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&tools, &destination)
            .map_err(|e| format!("link workspace tools: {e}"))?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&tools, &destination)
            .map_err(|e| format!("link workspace tools: {e}"))?;
    }
    Ok(())
}

fn copy_size_build_tools(repo_root: &Path, worktree: &Path) -> Result<(), String> {
    // Use one build-tool version for both revisions while compiling each
    // revision's Rust/application sources. This also retains WASM names in the
    // analysis copy without changing the production artifact.
    for relative in [
        "packages/plec/scripts/build-runtime.mjs",
        "scripts/build-wasm.mjs",
        "scripts/toolchain.mjs",
        "scripts/utils.js",
    ] {
        fs::copy(repo_root.join(relative), worktree.join(relative))
            .map_err(|e| format!("stage size build tool {relative}: {e}"))?;
    }
    let chart_copy = worktree.join(".cache/plec/wasm-size-chart.js");
    fs::create_dir_all(chart_copy.parent().expect("chart copy has parent"))
        .map_err(|e| e.to_string())?;
    fs::copy(
        repo_root.join("node_modules/chart.js/dist/chart.umd.js"),
        &chart_copy,
    )
    .map_err(|e| format!("stage offline Chart.js into revision worktree: {e}"))?;
    Ok(())
}

fn run(root: &Path, program: &str, args: &[&str]) -> Result<(), String> {
    let status = Command::new(program)
        .args(args)
        .current_dir(root)
        .status()
        .map_err(|e| format!("run {program}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} {} failed ({status})", args.join(" ")))
    }
}

fn run_with_env(
    root: &Path,
    program: &str,
    args: &[&str],
    key: &str,
    value: &str,
) -> Result<(), String> {
    let status = Command::new(program)
        .args(args)
        .current_dir(root)
        .env(key, value)
        .status()
        .map_err(|e| format!("run {program}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} {} failed ({status})", args.join(" ")))
    }
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn absolute_output(repo: &Repo, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        repo.root.join(path)
    }
}

fn html_report(inventory: &Inventory, chart_library: &str) -> String {
    let artifact_rows = inventory
        .artifacts
        .iter()
        .map(|a| {
            format!(
                "<tr><th>{}</th><td>{}</td><td>{}</td></tr>",
                escape(&a.kind),
                a.bytes,
                a.brotli_bytes
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "—".into())
            )
        })
        .collect::<String>();
    let section_rows = inventory
        .artifacts
        .iter()
        .filter(|a| a.kind == "wasm")
        .flat_map(|a| a.sections.iter())
        .map(|s| format!("<tr><th>{}</th><td>{}</td></tr>", escape(&s.name), s.bytes))
        .collect::<String>();
    let maximum = inventory
        .functions
        .iter()
        .map(|f| f.code_bytes)
        .max()
        .unwrap_or(1);
    let contributions = inventory
        .artifacts
        .iter()
        .flat_map(|a| a.contributions.iter().map(move |c| (a.kind.as_str(), c)))
        .collect::<Vec<_>>();
    let contribution_max = contributions
        .iter()
        .map(|(_, c)| c.bytes)
        .max()
        .unwrap_or(1);
    let contribution_bars = contributions.iter().map(|(kind, c)| format!(
        "<li title=\"{} · {}\"><span>{}: {}</span><i class=\"contribution\" style=\"width:{}%\"></i><b>{} B</b></li>",
        escape(kind), escape(&c.source), escape(kind), escape(&c.name), c.bytes * 100 / contribution_max, c.bytes
    )).collect::<String>();
    let bars = inventory
        .functions
        .iter()
        .filter(|f| f.code_bytes > 0)
        .take(150)
        .map(|f| {
            format!(
                "<li title=\"{}\"><span>{}</span><i style=\"width:{}%\"></i><b>{} B</b></li>",
                escape(&f.identity),
                escape(&f.identity),
                f.code_bytes * 100 / maximum,
                f.code_bytes
            )
        })
        .collect::<String>();
    let report_json = serde_json::to_string(inventory)
        .expect("size inventory serialization should not fail")
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e");
    let chart_library = chart_library.replace("</script", "<\\/script");
    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Plec size report</title>
<style>
body{{font:14px system-ui,sans-serif;margin:2rem;max-width:1200px;color:#20252b}}table{{border-collapse:collapse}}td,th{{padding:.5rem 1rem;border-bottom:1px solid #ddd;text-align:left}}ul{{list-style:none;padding:0}}li{{display:grid;grid-template-columns:minmax(18rem,2fr) 3fr 5rem;gap:1rem;align-items:center;margin:.35rem 0}}i{{display:block;background:#4386d1;height:1rem}}.contribution{{background:#4c9b75}}.row-name{{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}}.tabs{{display:flex;gap:.5rem;border-bottom:1px solid #bbb;margin:1.5rem 0}}.tabs button{{border:0;background:transparent;padding:.7rem 1rem;font:inherit;cursor:pointer}}.tabs button[aria-selected=true]{{border-bottom:3px solid #2868a8;font-weight:700}}.controls{{display:flex;flex-wrap:wrap;gap:1rem;align-items:end;margin:1rem 0}}label{{display:grid;gap:.3rem}}select{{font:inherit;padding:.4rem}}.chart-wrap{{height: min(72vh, 780px);min-height:420px;position:relative}}.note{{color:#555}}[hidden]{{display:none!important}}
</style></head><body>
<h1>Plec compiled size report</h1><p>Revision {}</p>
<div class="tabs" role="tablist" aria-label="Report views"><button id="tab-overview" role="tab" aria-controls="panel-overview" aria-selected="true">Overview</button><button id="tab-charts" role="tab" aria-controls="panel-charts" aria-selected="false">Charts</button></div>
<section id="panel-overview" role="tabpanel" aria-labelledby="tab-overview">
<h2>Artifacts</h2><table><thead><tr><th>Artifact</th><th>Bytes</th><th>Brotli bytes</th></tr></thead><tbody>{}</tbody></table><h3>WASM section bytes (including section framing)</h3><table><thead><tr><th>Section</th><th>Bytes</th></tr></thead><tbody>{}</tbody></table>
<h2>Code/package contributions</h2><ul>{}</ul><h2>Largest functions (body/symbol size)</h2><ul>{}</ul>
<p class="note">Function sizes are not per-function Brotli contributions. Native symbol sizes depend on available symbols; server bundle inputs report esbuild bytesInOutput.</p></section>
<section id="panel-charts" role="tabpanel" aria-labelledby="tab-charts" hidden>
<h2>Size comparison</h2><div class="controls">
<label>Artifact<select id="chart-artifact"><option value="all">All artifacts</option><option value="wasm">WASM</option><option value="native">Native server</option><option value="server-bundle">Server bundle</option></select></label>
<label>Chart data<select id="chart-view"><option value="functions">Functions</option><option value="contributions">Crate/package contributions</option></select></label>
<label>Function size<select id="chart-metric"><option value="codeBytes">Shallow bytes</option><option value="retainedBytes">Retained (approx.) bytes</option></select></label>
<label>Top rows<select id="chart-limit"><option>20</option><option selected>50</option><option>100</option></select></label></div>
<p class="note" id="chart-note">Bars are sorted by size. Retained size is approximate; whole-artifact Brotli size is reported in Overview.</p>
<div class="chart-wrap"><canvas id="size-chart" role="img" aria-label="Horizontal bar chart of code sizes"></canvas></div></section>
<script id="report-data" type="application/json">{}</script>
<script>{}</script>
<script>
const report=JSON.parse(document.getElementById('report-data').textContent);
const canvas=document.getElementById('size-chart');let chart;
function drawChart(){{
 const artifact=document.getElementById('chart-artifact').value;
 const view=document.getElementById('chart-view').value;
 const metric=document.getElementById('chart-metric').value;
 const limit=Number(document.getElementById('chart-limit').value);
 document.getElementById('chart-metric').disabled=view!=='functions';
 let rows;
 if(view==='functions'){{
  rows=report.functions.filter(f=>(artifact==='all'||f.artifact===artifact)&&Number.isFinite(f[metric]));
  rows.sort((a,b)=>b[metric]-a[metric]);rows=rows.slice(0,limit);
  rows=rows.map(f=>({{label:(artifact==='all'?f.artifact+' · ':'')+f.identity,value:f[metric],source:f.artifact,detail:f.attribution}}));
 }}else{{
  rows=report.artifacts.filter(a=>artifact==='all'||a.kind===artifact).flatMap(a=>a.contributions.map(c=>({{label:(artifact==='all'?a.kind+' · ':'')+c.name,value:c.bytes,source:a.kind,detail:c.source}})));
  rows.sort((a,b)=>b.value-a.value);rows=rows.slice(0,limit);
 }}
 document.getElementById('chart-note').textContent=rows.length?`${{rows.length}} rows · sizes are exact for compiled bodies/symbols; retained attribution is approximate.`:'No rows match this selection.';
 const data={{labels:rows.map(r=>r.label),datasets:[{{label:view==='functions'?(metric==='retainedBytes'?'Approx. retained bytes':'Function body/symbol bytes'):'Crate/package bytes',data:rows.map(r=>r.value),backgroundColor:rows.map(r=>r.source==='wasm'?'#4386d1':r.source==='native'?'#e19a3e':'#4c9b75'),borderRadius:3}}]}};
 const options={{indexAxis:'y',responsive:true,maintainAspectRatio:false,animation:false,plugins:{{legend:{{display:false}},tooltip:{{callbacks:{{afterLabel:ctx=>rows[ctx.dataIndex]?.detail||''}}}}}},scales:{{x:{{beginAtZero:true,title:{{display:true,text:'Size (bytes)'}},ticks:{{callback:value=>Number(value).toLocaleString()}}}},y:{{title:{{display:true,text:view==='functions'?'Compiled function':'Crate / package'}},ticks:{{autoSkip:false}}}}}}}};
 if(chart){{chart.data=data;chart.options=options;chart.update();}}else{{chart=new Chart(canvas,{{type:'bar',data,options}});}}
}}
for(const [button,panel] of [['tab-overview','panel-overview'],['tab-charts','panel-charts']]){{document.getElementById(button).addEventListener('click',()=>{{for(const id of ['tab-overview','tab-charts'])document.getElementById(id).setAttribute('aria-selected',String(id===button));for(const id of ['panel-overview','panel-charts'])document.getElementById(id).hidden=id!==panel;if(panel==='panel-charts'){{drawChart();chart?.resize();}}}});}}
for(const id of ['chart-artifact','chart-view','chart-metric','chart-limit'])document.getElementById(id).addEventListener('change',drawChart);
</script></body></html>"#,
        escape(&inventory.revision),
        artifact_rows,
        section_rows,
        contribution_bars,
        bars,
        report_json,
        chart_library
    )
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(id: u8, payload: &[u8], out: &mut Vec<u8>) {
        out.push(id);
        out.push(payload.len() as u8);
        out.extend_from_slice(payload);
    }

    #[test]
    fn wasm_inventory_reads_signature_body_calls_and_exports() {
        let mut wasm = b"\0asm\x01\0\0\0".to_vec();
        section(1, &[1, 0x60, 2, 0x7f, 0x7f, 1, 0x7f], &mut wasm);
        section(3, &[1, 0], &mut wasm);
        section(7, &[1, 1, b'f', 0, 0], &mut wasm);
        section(10, &[1, 4, 0, 0x10, 0, 0x0b], &mut wasm);
        section(
            0,
            &[
                4, b'n', b'a', b'm', b'e', 1, 7, 1, 0, 4, b't', b'e', b's', b't',
            ],
            &mut wasm,
        );
        let rows = analyze_wasm_bytes(&wasm).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].parameters, ["i32", "i32"]);
        assert_eq!(rows[0].results, ["i32"]);
        assert_eq!(rows[0].code_bytes, 4);
        assert_eq!(rows[0].identity, "test");
        assert_eq!(rows[0].retained_bytes, Some(4));
        assert!(rows[0].exported);
        assert_eq!(rows[0].callees, ["test"]);
    }

    #[test]
    fn strips_rust_crate_hashes_and_attributes_the_crate() {
        let symbol = "plec_schema[0123456789abcdef]::typed::drive::<BrowserHost>";
        let normalized = normalize_wasm_name(symbol);
        assert_eq!(normalized, "plec_schema::typed::drive::<BrowserHost>");
        assert_eq!(
            crate_from_symbol(&normalized).as_deref(),
            Some("plec-schema")
        );
        assert_eq!(crate_from_symbol("wasm-function[42]"), None);
        assert_eq!(
            crate_from_symbol("<str>::replace::<char>"),
            Some("core".into())
        );
        assert_eq!(
            crate_from_symbol("<u64 as core::fmt::Display>::fmt"),
            Some("core".into())
        );
        assert_eq!(
            crate_from_symbol("plec_dom[d154810741d7084]::platform::document"),
            Some("plec-dom".into())
        );
        assert_eq!(
            crate_from_symbol("plecruntime_start_adopt_snapshot externref shim multivalue shim"),
            Some("plec-runtime".into())
        );
    }

    #[test]
    fn wasm_section_inventory_accounts_for_the_complete_module() {
        let mut wasm = b"\0asm\x01\0\0\0".to_vec();
        section(1, &[1, 0x60, 0, 0], &mut wasm);
        section(10, &[1, 2, 0, 0x0b], &mut wasm);
        let sections = wasm_section_sizes(&wasm).unwrap();
        assert_eq!(
            sections.iter().map(|s| s.bytes).sum::<u64>(),
            wasm.len() as u64
        );
        assert!(
            sections
                .iter()
                .any(|s| s.name == "wasm-header" && s.bytes == 8)
        );
        assert!(sections.iter().any(|s| s.name == "type"));
        assert!(sections.iter().any(|s| s.name == "code"));
    }

    #[test]
    fn html_report_has_offline_chart_tab_and_named_axes() {
        let inventory = Inventory {
            schema_version: 1,
            build: BuildInfo {
                wasm_profile: "release".into(),
                server_profile: "node-release".into(),
                target: "test-target".into(),
            },
            revision: "test-revision".into(),
            artifacts: vec![Artifact {
                kind: "wasm".into(),
                path: "runtime_bg.wasm".into(),
                bytes: 12,
                brotli_bytes: Some(8),
                sha256: "hash".into(),
                sections: vec![],
                contributions: vec![Contribution {
                    name: "plec-schema".into(),
                    bytes: 12,
                    source: "function sum".into(),
                }],
                function_body_bytes: Some(12),
                other_bytes: Some(0),
            }],
            functions: vec![Function {
                artifact: "wasm".into(),
                identity: "plec_schema::typed::decode".into(),
                index: Some(1),
                crate_name: Some("plec-schema".into()),
                module: None,
                rust_signature: None,
                source_location: None,
                parameters: vec!["i32".into()],
                results: vec![],
                code_bytes: 12,
                retained_bytes: Some(12),
                callers: vec![],
                callees: vec![],
                exported: false,
                indirect_calls: 0,
                attribution: "name-section".into(),
            }],
        };
        let html = html_report(&inventory, "window.Chart = {};");
        assert!(html.contains("id=\"tab-charts\""));
        assert!(html.contains("<canvas id=\"size-chart\""));
        assert!(html.contains("Size (bytes)"));
        assert!(html.contains("Crate / package"));
        assert!(html.contains("window.Chart = {}"));
        assert!(!html.contains("https://cdn"));
    }
}
