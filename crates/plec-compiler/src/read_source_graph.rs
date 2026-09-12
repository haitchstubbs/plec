use crate::CompilerOptions;
use plec_parser::{module_dependencies, parse_module, ParsedModule};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Debug)]
struct WorkspaceIndex {
    packages: HashMap<String, PathBuf>,
}

#[derive(Debug)]
pub struct SourceGraph {
    pub modules: Vec<ParsedModule>,
    /// Authored import specifier -> canonical source-graph module identity.
    pub resolved_imports: HashMap<(String, String), String>,
}

/// Read and parse the complete source graph reachable from an entry module.
///
/// Responsibilities:
///
/// - Read source files from disk.
/// - Parse each module exactly once.
/// - Discover dependencies from the parsed SWC AST.
/// - Resolve relative source imports.
/// - Preserve deterministic parent-before-dependency module ordering.
/// - Prevent cycles and duplicate modules.
///
/// Containment model:
///
/// - Application modules (relative imports) may only reach sources beneath
///   the application root (`root_dir`), so one application can never read
///   another application's sources through the shared repository root.
/// - Workspace imports may only reach sources beneath the resolved package's
///   own directory.
///
/// Workspace/package resolution is deliberately isolated behind
/// `resolve_module` and can be added without changing graph traversal.
pub fn read_source_graph(
    entry: impl AsRef<Path>,
    root_dir: impl AsRef<Path>,
    repo_root_dir: impl AsRef<Path>,
) -> Result<SourceGraph, String> {
    read_source_graph_with_options(entry, root_dir, repo_root_dir, &CompilerOptions::default())
}

pub fn read_source_graph_with_options(
    entry: impl AsRef<Path>,
    root_dir: impl AsRef<Path>,
    repo_root_dir: impl AsRef<Path>,
    options: &CompilerOptions,
) -> Result<SourceGraph, String> {
    let entry = entry.as_ref();

    let root_dir = fs::canonicalize(root_dir.as_ref()).map_err(|error| {
        format!(
            "Failed to resolve root directory {}: {error}",
            root_dir.as_ref().display()
        )
    })?;

    let repo_root_dir = fs::canonicalize(repo_root_dir.as_ref()).map_err(|error| {
        format!(
            "Failed to resolve repository root {}: {error}",
            repo_root_dir.as_ref().display()
        )
    })?;

    let mut seen = HashSet::new();
    let mut modules = Vec::new();
    let mut resolved_imports = HashMap::new();
    let mut total_source_bytes = 0u64;
    let workspace = WorkspaceIndex::load(&repo_root_dir)?;

    visit_module(
        entry,
        &root_dir,
        &root_dir,
        &repo_root_dir,
        &workspace,
        options,
        &mut seen,
        &mut modules,
        &mut resolved_imports,
        &mut total_source_bytes,
        0,
    )?;

    Ok(SourceGraph {
        modules,
        resolved_imports,
    })
}

#[allow(clippy::too_many_arguments)]
fn visit_module(
    file_path: &Path,
    scope_root: &Path,
    root_dir: &Path,
    repo_root_dir: &Path,
    workspace: &WorkspaceIndex,
    options: &CompilerOptions,
    seen: &mut HashSet<PathBuf>,
    modules: &mut Vec<ParsedModule>,
    resolved_imports: &mut HashMap<(String, String), String>,
    total_source_bytes: &mut u64,
    depth: usize,
) -> Result<(), String> {
    use plec_ir::limits::{MAX_IMPORT_DEPTH, MAX_MODULE_COUNT, MAX_TOTAL_SOURCE_BYTES};

    if depth > MAX_IMPORT_DEPTH {
        return Err(format!(
            "Import graph exceeds the maximum depth of {MAX_IMPORT_DEPTH} at {}",
            file_path.display()
        ));
    }

    let absolute = fs::canonicalize(file_path)
        .map_err(|error| format!("Failed to resolve {}: {error}", file_path.display()))?;

    ensure_within_scope(&absolute, scope_root)?;

    if !seen.insert(absolute.clone()) {
        return Ok(());
    }

    if modules.len() >= MAX_MODULE_COUNT {
        return Err(format!(
            "Source graph exceeds the maximum module count of {MAX_MODULE_COUNT}"
        ));
    }

    let source = read_bounded_source(file_path, &absolute)?;
    let source_bytes = source.len() as u64;
    let next_total = total_source_bytes
        .checked_add(source_bytes)
        .ok_or_else(|| "Source graph byte accounting overflowed".to_string())?;
    if next_total > MAX_TOTAL_SOURCE_BYTES {
        return Err(format!(
            "Source graph exceeds the maximum total source size of {MAX_TOTAL_SOURCE_BYTES} bytes"
        ));
    }
    *total_source_bytes = next_total;

    let module_id = module_id_from_path(&absolute, root_dir, repo_root_dir);

    let parsed = parse_module(module_id.clone(), source)
        .map_err(|error| format!("Failed to parse {module_id}: {error}"))?;

    let dependencies = module_dependencies(&parsed.ast);

    // Preserve the existing TypeScript compiler's ordering:
    //
    // current module
    //   -> dependency
    //      -> dependency
    //
    // This also guarantees that modules[0] is the entry module.
    modules.push(parsed);

    for specifier in dependencies {
        if let Some(provider) = options.host_imports.get(&specifier) {
            resolved_imports.insert(
                (module_id.clone(), specifier.clone()),
                format!("host:{provider}"),
            );
            continue;
        }
        let Some((resolved, target_scope)) = resolve_module(&specifier, &absolute, scope_root, workspace)?
        else {
            continue;
        };

        let target_absolute = fs::canonicalize(&resolved)
            .map_err(|error| format!("Failed to resolve {}: {error}", resolved.display()))?;
        ensure_within_scope(&target_absolute, &target_scope)?;
        let target_id = module_id_from_path(&target_absolute, root_dir, repo_root_dir);
        resolved_imports.insert((module_id.clone(), specifier.clone()), target_id);

        visit_module(
            &resolved,
            &target_scope,
            root_dir,
            repo_root_dir,
            workspace,
            options,
            seen,
            modules,
            resolved_imports,
            total_source_bytes,
            depth + 1,
        )?;
    }

    Ok(())
}

/// Resolve one authored import specifier to its source file and the
/// containment scope that file must stay inside.
///
/// - Relative specifiers inherit the importing module's scope (the
///   application root, or the package directory for workspace modules).
/// - Workspace bare specifiers resolve to a target scoped to the owning
///   package directory.
fn resolve_module(
    specifier: &str,
    from_file: &Path,
    from_scope: &Path,
    workspace: &WorkspaceIndex,
) -> Result<Option<(PathBuf, PathBuf)>, String> {
    if specifier.starts_with("node:") {
        return Ok(None);
    }

    if specifier.starts_with('.') {
        let parent = from_file.parent().ok_or_else(|| {
            format!(
                "Cannot resolve {specifier} from {}: source has no parent directory",
                from_file.display()
            )
        })?;

        let base = parent.join(specifier);

        return Ok(resolve_source_candidate(&base)
            .map(|path| (path, from_scope.to_path_buf())));
    }

    if let Some((path, package_dir)) = resolve_workspace_module(specifier, workspace)? {
        return Ok(Some((path, package_dir)));
    }

    // External dependency sources are not part of the source graph today;
    // when this hook gains a real resolver it must also decide which
    // containment scope those sources belong to.
    resolve_dependency_module(specifier, from_file).map(|resolved| {
        resolved.map(|path| (path, from_scope.to_path_buf()))
    })
}

/// Resolve authored source files using the same general preference as the
/// existing TypeScript compiler.
///
/// For example:
///
/// ./foo       -> ./foo.tsx, ./foo.ts, ...
/// ./foo.js    -> ./foo.js, ./foo.tsx, ./foo.ts, ...
/// ./directory -> ./directory/index.tsx, ...
fn resolve_source_candidate(base: &Path) -> Option<PathBuf> {
    let mut candidates = Vec::new();

    if base.extension().is_some() {
        candidates.push(base.to_path_buf());

        // `./home.route` is an extensionless authored module name with a
        // meaningful suffix, not a request for `./home.tsx`.
        if base.extension().and_then(|extension| extension.to_str()) == Some("route") {
            for extension in ["tsx", "ts", "jsx", "mjs", "js"] {
                candidates.push(base.with_extension(format!("route.{extension}")));
            }
        }

        let without_extension = base.with_extension("");
        add_source_candidates(&mut candidates, &without_extension);
    } else {
        add_source_candidates(&mut candidates, base);
    }

    candidates.into_iter().find(|candidate| candidate.is_file())
}

fn add_source_candidates(candidates: &mut Vec<PathBuf>, base: &Path) {
    for extension in ["tsx", "ts", "jsx", "mjs", "js"] {
        candidates.push(base.with_extension(extension));
    }

    for filename in [
        "index.tsx",
        "index.ts",
        "index.jsx",
        "index.mjs",
        "index.js",
    ] {
        candidates.push(base.join(filename));
    }
}

fn module_id_from_path(path: &Path, root_dir: &Path, repo_root_dir: &Path) -> String {
    path.strip_prefix(root_dir)
        .or_else(|_| path.strip_prefix(repo_root_dir))
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Reject modules that resolve outside the scope root that owns them.
///
/// Application modules are scoped to the application root and workspace
/// modules are scoped to their owning package directory. The scope root must
/// be canonical so `starts_with` cannot be fooled by `..` segments or
/// symlinks; callers canonicalize before invoking this check.
fn ensure_within_scope(path: &Path, scope_root: &Path) -> Result<(), String> {
    if path.starts_with(scope_root) {
        return Ok(());
    }

    Err(format!(
        "Source module {} resolves outside the approved source root {}",
        path.display(),
        scope_root.display()
    ))
}

/// Read one source module with resistance to concurrent path replacement.
///
/// The caller canonicalizes and containment-checks the path first; this
/// helper then opens that canonical path and reads through the open file
/// handle (fstat + read on the handle, not the pathname), so the bytes and
/// the size accounting describe the same file even if the directory entry is
/// swapped mid-read. Afterwards the original path must still canonicalize to
/// the opened file — a swap in between (for example a hostile process
/// replacing a shared-build symlink) fails the compile instead of silently
/// admitting foreign bytes.
///
/// This narrows the race window dramatically but cannot eliminate it against
/// a hostile writer with arbitrary filesystem access; production builds must
/// compile from an isolated, immutable workspace.
fn read_bounded_source(resolved: &Path, canonical: &Path) -> Result<String, String> {
    use plec_ir::limits::MAX_SOURCE_FILE_BYTES;

    let mut file = fs::File::open(canonical)
        .map_err(|error| format!("Failed to read {}: {error}", canonical.display()))?;

    // fstat on the open handle: the metadata describes exactly the file the
    // subsequent read consumes, not whatever the pathname points at later.
    let metadata = file
        .metadata()
        .map_err(|error| format!("Failed to read {}: {error}", canonical.display()))?;
    if !metadata.is_file() {
        return Err(format!(
            "Source module {} is not a regular file",
            canonical.display()
        ));
    }
    if metadata.len() > MAX_SOURCE_FILE_BYTES {
        return Err(format!(
            "Source file {} exceeds the maximum size of {MAX_SOURCE_FILE_BYTES} bytes",
            canonical.display()
        ));
    }

    let mut source = String::with_capacity(metadata.len() as usize);
    file.read_to_string(&mut source)
        .map_err(|error| format!("Failed to read {}: {error}", canonical.display()))?;

    let current = fs::canonicalize(resolved).map_err(|error| {
        format!(
            "Source file {} changed while the source graph was being read: {error}",
            resolved.display()
        )
    })?;
    if current != canonical {
        return Err(format!(
            "Source file {} changed while the source graph was being read; compile from an isolated, immutable workspace",
            resolved.display()
        ));
    }

    Ok(source)
}

/// Resolve an import to another package in the Plec workspace.
///
/// The existing TypeScript implementation understands workspace package
/// exports and prefers authored `src` files over published `dist` files.
///
/// This is intentionally isolated so that package-layout policy does not leak
/// into source graph traversal.
impl WorkspaceIndex {
    /// Index the workspace packages beneath `<repo-root>/packages`.
    ///
    /// Package directories are canonicalized and must stay inside the
    /// repository root; the number of scanned manifests and each manifest's
    /// byte size are bounded so a hostile workspace cannot exhaust the
    /// compiler before source containment is even applied.
    fn load(repo_root_dir: &Path) -> Result<Self, String> {
        use plec_ir::limits::MAX_WORKSPACE_PACKAGE_COUNT;

        let mut packages = HashMap::new();
        let root = repo_root_dir.join("packages");
        let Ok(entries) = fs::read_dir(&root) else {
            return Ok(Self { packages });
        };
        let mut scanned = 0usize;
        for entry in entries {
            let entry =
                entry.map_err(|error| format!("Failed to read workspace package: {error}"))?;
            let manifest = entry.path().join("package.json");
            if !manifest.is_file() {
                continue;
            }
            scanned += 1;
            if scanned > MAX_WORKSPACE_PACKAGE_COUNT {
                return Err(format!(
                    "Workspace package index exceeds the maximum package count of {MAX_WORKSPACE_PACKAGE_COUNT}"
                ));
            }
            let value = read_workspace_manifest(&manifest)?;
            let package_dir = fs::canonicalize(entry.path()).map_err(|error| {
                format!("Failed to resolve {}: {error}", entry.path().display())
            })?;
            if !package_dir.starts_with(repo_root_dir) {
                return Err(format!(
                    "Workspace package {} resolves outside the repository root {}",
                    package_dir.display(),
                    repo_root_dir.display()
                ));
            }
            if let Some(name) = value.get("name").and_then(Value::as_str) {
                packages.insert(name.into(), package_dir);
            }
        }
        Ok(Self { packages })
    }
}

/// Read and parse one workspace `package.json` under a strict byte budget.
fn read_workspace_manifest(manifest: &Path) -> Result<Value, String> {
    use plec_ir::limits::MAX_MANIFEST_JSON_BYTES;

    let metadata = fs::metadata(manifest)
        .map_err(|error| format!("Failed to read {}: {error}", manifest.display()))?;
    if metadata.len() > MAX_MANIFEST_JSON_BYTES as u64 {
        return Err(format!(
            "Workspace manifest {} exceeds the maximum manifest size of {MAX_MANIFEST_JSON_BYTES} bytes",
            manifest.display()
        ));
    }
    let source = fs::read_to_string(manifest)
        .map_err(|error| format!("Failed to read {}: {error}", manifest.display()))?;
    serde_json::from_str(&source).map_err(|error| format!("Invalid {}: {error}", manifest.display()))
}

/// Resolve a bare import to a workspace package source file.
///
/// Returns the canonical source file and the owning package directory (the
/// containment scope for the resolved module).
fn resolve_workspace_module(
    specifier: &str,
    workspace: &WorkspaceIndex,
) -> Result<Option<(PathBuf, PathBuf)>, String> {
    let (name, subpath) = split_package_specifier(specifier);
    let Some(package_dir) = workspace.packages.get(name) else {
        return Ok(None);
    };
    let manifest_path = package_dir.join("package.json");
    let manifest = read_workspace_manifest(&manifest_path)?;
    let requested = if subpath.is_empty() {
        ".".to_string()
    } else {
        format!("./{subpath}")
    };
    let target = manifest
        .get("exports")
        .and_then(|exports| resolve_export(exports, &requested));

    if let Some(target) = target {
        return resolve_workspace_source(package_dir, &target)
            .map(|resolved| resolved.map(|path| (path, package_dir.clone())));
    }
    if subpath.is_empty() {
        return Ok(resolve_source_candidate(&package_dir.join("src/index"))
            .map(|path| (path, package_dir.clone())));
    }
    Ok(None)
}

fn split_package_specifier(specifier: &str) -> (&str, &str) {
    if specifier.starts_with('@') {
        let mut parts = specifier.splitn(3, '/');
        let scope = parts.next().unwrap();
        let package = parts.next().unwrap_or("");
        let rest = parts.next().unwrap_or("");
        let length = scope.len() + package.len() + 1;
        (&specifier[..length], rest)
    } else {
        let mut parts = specifier.splitn(2, '/');
        let name = parts.next().unwrap();
        (name, parts.next().unwrap_or(""))
    }
}

fn resolve_export(exports: &Value, requested: &str) -> Option<String> {
    if let Some(value) = exports.get(requested) {
        return resolve_export_target(value);
    }
    let object = exports.as_object()?;
    for (pattern, value) in object {
        let Some(star) = pattern.find('*') else {
            continue;
        };
        let (prefix, suffix) = (&pattern[..star], &pattern[star + 1..]);
        if requested.starts_with(prefix) && requested.ends_with(suffix) {
            let capture = &requested[prefix.len()..requested.len() - suffix.len()];
            return resolve_export_target(value).map(|target| target.replace('*', capture));
        }
    }
    None
}

fn resolve_export_target(value: &Value) -> Option<String> {
    if let Some(value) = value.as_str() {
        return Some(value.into());
    }
    let object = value.as_object()?;
    for condition in ["source", "import", "default", "types"] {
        if let Some(value) = object.get(condition) {
            if let Some(target) = resolve_export_target(value) {
                return Some(target);
            }
        }
    }
    None
}

/// Resolve a workspace export target to an authored source file.
///
/// The resolved target must stay inside the owning package directory.
/// Manifest exports are package-controlled data, so a malicious or mistaken
/// export (`../../secrets`, absolute paths, symlinked targets) must fail with
/// a deterministic diagnostic instead of expanding the readable source graph.
fn resolve_workspace_source(package_dir: &Path, target: &str) -> Result<Option<PathBuf>, String> {
    let published = package_dir.join(target);
    let source = target
        .replace("./dist/", "./src/")
        .replace(".d.ts", "")
        .replace(".js", "");
    let resolved = resolve_source_candidate(&package_dir.join(source))
        .or_else(|| published.is_file().then_some(published));

    let Some(resolved) = resolved else {
        return Ok(None);
    };

    let canonical = fs::canonicalize(&resolved)
        .map_err(|error| format!("Failed to resolve {}: {error}", resolved.display()))?;

    if !canonical.starts_with(package_dir) {
        return Err(format!(
            "Workspace export target {target} in {} resolves outside the package boundary",
            package_dir.display()
        ));
    }

    Ok(Some(canonical))
}

/// Resolve a normal external package import.
///
/// External packages do not necessarily need to become part of Plec's source
/// graph. This hook exists for packages whose authored source must be
/// inspected by the compiler.
fn resolve_dependency_module(
    _specifier: &str,
    _from_file: &Path,
) -> Result<Option<PathBuf>, String> {
    // TODO:
    //
    // Port resolveDependencyModule from node-entry.
    //
    // Rust does not have Node's createRequire(...).resolve(), so this should
    // eventually be implemented as an explicit Node-compatible resolver
    // rather than ad-hoc filesystem traversal.
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::fs;
    use tempfile::tempdir;

    fn write_file(path: &Path, source: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("fixture directory should be created");
        }

        fs::write(path, source).expect("fixture source should be written");
    }

    #[test]
    fn resolves_workspace_wildcard_export_to_authored_svg_source() {
        let repo = tempdir().expect("repo");
        let app = repo.path().join("apps/demo");
        write_file(&app.join("src/App.tsx"), "import { Mark } from '@scope/icons/icons/mark'; export function App(){ return <Mark />; }");
        write_file(
            &repo.path().join("packages/icons/package.json"),
            r#"{"name":"@scope/icons","exports":{"./icons/*":{"default":"./dist/icons/*.js"}}}"#,
        );
        write_file(
            &repo.path().join("packages/icons/src/icons/mark.tsx"),
            "export const Mark = () => <svg><path d=\"M0 0\" /></svg>;",
        );
        let index = WorkspaceIndex::load(repo.path()).unwrap();
        let (resolved, package_scope) =
            resolve_workspace_module("@scope/icons/icons/mark", &index)
                .unwrap()
                .expect("wildcard export should resolve");
        assert_eq!(
            resolved,
            fs::canonicalize(repo.path().join("packages/icons/src/icons/mark.tsx")).unwrap()
        );
        assert_eq!(
            package_scope,
            fs::canonicalize(repo.path().join("packages/icons")).unwrap()
        );
        let graph = read_source_graph(app.join("src/App.tsx"), &app, repo.path())
            .expect("workspace source graph");
        assert!(
            graph
                .modules
                .iter()
                .any(|module| module.id == "packages/icons/src/icons/mark.tsx"),
            "{:?}",
            graph
                .modules
                .iter()
                .map(|module| &module.id)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            graph.resolved_imports.get(&(
                String::from("src/App.tsx"),
                String::from("@scope/icons/icons/mark")
            )),
            Some(&String::from("packages/icons/src/icons/mark.tsx"))
        );
    }

    #[test]
    fn resolves_exact_host_import_bindings_to_provider_ids() {
        let repo = tempdir().expect("repo");
        let app = repo.path().join("apps/demo");
        let entry = app.join("src/App.tsx");
        write_file(
            &entry,
            r#"import { House } from "lucide"; export function App() { return <House />; }"#,
        );
        let options = CompilerOptions {
            host_imports: BTreeMap::from([(String::from("lucide"), String::from("icons"))]),
            custom_elements: Default::default(),
        };

        let graph = read_source_graph_with_options(&entry, &app, repo.path(), &options)
            .expect("host import should resolve");

        assert_eq!(graph.modules.len(), 1);
        assert_eq!(
            graph
                .resolved_imports
                .get(&(String::from("src/App.tsx"), String::from("lucide"),)),
            Some(&String::from("host:icons"))
        );
    }

    #[test]
    fn host_import_bindings_do_not_match_subpaths() {
        let repo = tempdir().expect("repo");
        let app = repo.path().join("apps/demo");
        let entry = app.join("src/App.tsx");
        write_file(
            &entry,
            r#"import { House } from "lucide/internal"; export function App() { return <House />; }"#,
        );
        let options = CompilerOptions {
            host_imports: BTreeMap::from([(String::from("lucide"), String::from("icons"))]),
            custom_elements: Default::default(),
        };

        let graph = read_source_graph_with_options(&entry, &app, repo.path(), &options)
            .expect("unconfigured imports remain outside the host map");

        assert!(!graph
            .resolved_imports
            .values()
            .any(|target| target == "host:icons"));
    }

    #[test]
    fn rejects_relative_parent_traversal_escape() {
        let temp = tempdir().expect("temporary directory should be created");
        let outside = temp.path().join("secret.ts");
        let repo = temp.path().join("repo");
        let app = repo.join("apps/demo");
        let entry = app.join("src/App.tsx");

        write_file(&outside, "export const secret = 1;");
        write_file(
            &entry,
            r#"
                import { secret } from "../../../../secret";

                export function App() {
                    return <div>{secret}</div>;
                }
            "#,
        );

        let error = read_source_graph(&entry, &app, &repo)
            .expect_err("parent traversal escape should be rejected");

        assert!(
            error.contains("outside the approved source root"),
            "{error}"
        );
    }

    #[test]
    fn ignores_absolute_import_specifiers() {
        let temp = tempdir().expect("temporary directory should be created");
        let repo = temp.path().join("repo");
        let app = repo.join("apps/demo");
        let entry = app.join("src/App.tsx");

        write_file(
            &entry,
            r#"
                import { secret } from "/etc/passwd";

                export function App() {
                    return <div />;
                }
            "#,
        );

        let graph = read_source_graph(&entry, &app, &repo)
            .expect("absolute import should not widen the source graph");

        assert_eq!(graph.modules.len(), 1);
        assert!(graph.resolved_imports.is_empty());
    }

    #[test]
    fn rejects_malicious_workspace_export_escape() {
        let temp = tempdir().expect("temporary directory should be created");
        let repo = temp.path().join("repo");
        let app = repo.join("apps/demo");
        let entry = app.join("src/App.tsx");

        write_file(&repo.join("leak.ts"), "export const leak = 1;");
        write_file(
            &entry,
            r#"
                import { leak } from "@scope/evil/escape";

                export function App() {
                    return <div>{leak}</div>;
                }
            "#,
        );
        write_file(
            &repo.join("packages/evil/package.json"),
            r#"{"name":"@scope/evil","exports":{"./escape":{"default":"../../leak.js"}}}"#,
        );

        let error = read_source_graph(&entry, &app, &repo)
            .expect_err("workspace export escape should be rejected");

        assert!(error.contains("outside the package boundary"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_import_escape() {
        let temp = tempdir().expect("temporary directory should be created");
        let outside = temp.path().join("outside/leak.tsx");
        let repo = temp.path().join("repo");
        let app = repo.join("apps/demo");
        let entry = app.join("src/App.tsx");
        let link = app.join("src/escape.tsx");

        write_file(&outside, "export const leak = 1;");
        write_file(
            &entry,
            r#"
                import { leak } from "./escape";

                export function App() {
                    return <div>{leak}</div>;
                }
            "#,
        );
        std::os::unix::fs::symlink(&outside, &link).expect("symlink should be created");

        let error =
            read_source_graph(&entry, &app, &repo).expect_err("symlink escape should be rejected");

        assert!(
            error.contains("outside the approved source root"),
            "{error}"
        );
    }

    #[test]
    fn rejects_relative_import_into_sibling_application() {
        let temp = tempdir().expect("temporary directory should be created");
        let repo = temp.path().join("repo");
        let app = repo.join("apps/demo");
        let sibling = repo.join("apps/other");
        let entry = app.join("src/App.tsx");

        write_file(&sibling.join("src/secret.ts"), "export const secret = 1;");
        write_file(
            &entry,
            r#"
                import { secret } from "../../other/src/secret";

                export function App() {
                    return <div>{secret}</div>;
                }
            "#,
        );

        let error = read_source_graph(&entry, &app, &repo)
            .expect_err("cross-application import should be rejected");

        assert!(
            error.contains("outside the approved source root"),
            "{error}"
        );
    }

    #[test]
    fn rejects_package_relative_escape_from_package_scope() {
        let temp = tempdir().expect("temporary directory should be created");
        let repo = temp.path().join("repo");
        let app = repo.join("apps/demo");
        let entry = app.join("src/App.tsx");

        write_file(&repo.join("leak.ts"), "export const leak = 1;");
        write_file(
            &entry,
            r#"
                import { Thing } from "@scope/pkg";

                export function App() {
                    return <Thing />;
                }
            "#,
        );
        write_file(
            &repo.join("packages/pkg/package.json"),
            r#"{"name":"@scope/pkg","exports":{".":{"default":"./dist/index.js"}}}"#,
        );
        write_file(
            &repo.join("packages/pkg/src/index.tsx"),
            r#"
                import { leak } from "../../../leak";

                export const Thing = () => <div>{leak}</div>;
            "#,
        );

        let error = read_source_graph(&entry, &app, &repo)
            .expect_err("package-relative escape should be rejected");

        assert!(
            error.contains("outside the approved source root"),
            "{error}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn detects_source_replacement_during_read() {
        let temp = tempdir().expect("temporary directory should be created");
        let outside = temp.path().join("outside/evil.tsx");
        let repo = temp.path().join("repo");
        let app = repo.join("apps/demo");
        let real = app.join("src/real.tsx");
        let link = app.join("src/module.tsx");

        write_file(&real, "export const real = 1;");
        write_file(&outside, "export const evil = 1;");
        std::os::unix::fs::symlink(&real, &link).expect("symlink should be created");

        // Simulate visit_module with a symlink swap between canonicalization
        // and the post-open verification: the canonical path still points at
        // the in-bounds file, but the pathname now resolves elsewhere.
        let canonical = fs::canonicalize(&link).expect("canonical path should resolve");
        let app_root = fs::canonicalize(&app).expect("app root should resolve");
        ensure_within_scope(&canonical, &app_root).expect("canonical path should be in scope");

        std::fs::remove_file(&link).expect("link should be removed");
        std::os::unix::fs::symlink(&outside, &link).expect("symlink should be swapped");

        let error = read_bounded_source(&link, &canonical)
            .expect_err("path replacement during read should be detected");

        assert!(
            error.contains("changed while the source graph was being read"),
            "{error}"
        );
    }

    #[test]
    fn rejects_aggregate_source_beyond_total_budget() {
        let temp = tempdir().expect("temporary directory should be created");
        let root = temp.path();
        let src = root.join("src");
        fs::create_dir_all(&src).expect("src directory should be created");

        // MAX_TOTAL_SOURCE_BYTES is 32 MiB and MAX_SOURCE_FILE_BYTES is
        // 2 MiB, so 17 modules of ~1.95 MiB each cross the aggregate budget
        // while every individual file stays within the per-file cap.
        const MODULES: usize = 17;
        const PAD_BYTES: usize = 1950 * 1024;
        for index in 0..MODULES {
            let module = src.join(format!("Module{index}.tsx"));
            let source = if index + 1 < MODULES {
                format!(
                    "import {{ Next }} from \"./Module{}\";\nexport const Next = {{}};\nexport const pad{index} = \"{}\";\n",
                    index + 1,
                    "x".repeat(PAD_BYTES)
                )
            } else {
                format!(
                    "export const Next = {{}};\nexport const pad{index} = \"{}\";\n",
                    "x".repeat(PAD_BYTES)
                )
            };
            fs::write(&module, source).expect("aggregate fixture should be written");
        }

        let entry = src.join("Module0.tsx");
        let error = read_source_graph(&entry, root, root)
            .expect_err("aggregate source exhaustion should be rejected");

        assert!(error.contains("maximum total source size"), "{error}");
    }

    #[test]
    fn rejects_workspace_packages_beyond_count_budget() {
        let temp = tempdir().expect("temporary directory should be created");
        let repo = temp.path().join("repo");
        let app = repo.join("apps/demo");
        let entry = app.join("src/App.tsx");

        write_file(&entry, "export function App() { return <div />; }");
        for index in 0..=plec_ir::limits::MAX_WORKSPACE_PACKAGE_COUNT {
            write_file(
                &repo.join(format!("packages/p{index}/package.json")),
                &format!(r#"{{"name":"pkg{index}"}}"#),
            );
        }

        let error = read_source_graph(&entry, &app, &repo)
            .expect_err("workspace package exhaustion should be rejected");

        assert!(error.contains("maximum package count"), "{error}");
    }

    #[test]
    fn rejects_oversized_workspace_manifest() {
        let temp = tempdir().expect("temporary directory should be created");
        let repo = temp.path().join("repo");
        let app = repo.join("apps/demo");
        let entry = app.join("src/App.tsx");

        write_file(&entry, "export function App() { return <div />; }");
        write_file(
            &repo.join("packages/huge/package.json"),
            &format!(
                r#"{{"name":"@scope/huge","description":"{}"}}"#,
                "x".repeat(plec_ir::limits::MAX_MANIFEST_JSON_BYTES + 1)
            ),
        );

        let error = read_source_graph(&entry, &app, &repo)
            .expect_err("oversized workspace manifest should be rejected");

        assert!(error.contains("maximum manifest size"), "{error}");
    }

    #[test]
    fn reads_single_entry_module() {
        let temp = tempdir().expect("temporary directory should be created");
        let root = temp.path();

        let entry = root.join("src/App.tsx");

        write_file(
            &entry,
            r#"
                export function App() {
                    return <div>Hello</div>;
                }
            "#,
        );

        let graph = read_source_graph(&entry, root, root).expect("source graph should compile");

        assert_eq!(graph.modules.len(), 1);
        assert_eq!(graph.modules[0].id, "src/App.tsx");
        assert!(!graph.modules[0].ast.body.is_empty());
    }

    #[test]
    fn follows_relative_imports() {
        let temp = tempdir().expect("temporary directory should be created");
        let root = temp.path();

        let entry = root.join("src/App.tsx");
        let child = root.join("src/components/Button.tsx");

        write_file(
            &entry,
            r#"
                import { Button } from "./components/Button";

                export function App() {
                    return <Button />;
                }
            "#,
        );

        write_file(
            &child,
            r#"
                export function Button() {
                    return <button>Click me</button>;
                }
            "#,
        );

        let graph = read_source_graph(&entry, root, root).expect("source graph should compile");

        assert_eq!(graph.modules.len(), 2);
        assert_eq!(graph.modules[0].id, "src/App.tsx");
        assert_eq!(graph.modules[1].id, "src/components/Button.tsx");
        assert_eq!(
            graph
                .resolved_imports
                .get(&("src/App.tsx".to_string(), "./components/Button".to_string(),)),
            Some(&"src/components/Button.tsx".to_string()),
        );
    }

    #[test]
    fn preserves_route_file_suffix_when_resolving_imports() {
        let temp = tempdir().expect("temporary directory should be created");
        let root = temp.path();
        let entry = root.join("src/router.tsx");
        write_file(&entry, "import './home.route';");
        write_file(&root.join("src/home.tsx"), "export const page = 1;");
        write_file(&root.join("src/home.route.tsx"), "export const Route = 1;");

        let graph = read_source_graph(&entry, root, root).expect("route import should resolve");
        assert_eq!(graph.modules[1].id, "src/home.route.tsx");
    }

    #[test]
    fn preserves_parent_before_dependency_order() {
        let temp = tempdir().expect("temporary directory should be created");
        let root = temp.path();

        let entry = root.join("src/App.tsx");
        let child = root.join("src/Child.tsx");
        let grandchild = root.join("src/Grandchild.tsx");

        write_file(
            &entry,
            r#"
                import { Child } from "./Child";

                export function App() {
                    return <Child />;
                }
            "#,
        );

        write_file(
            &child,
            r#"
                import { Grandchild } from "./Grandchild";

                export function Child() {
                    return <Grandchild />;
                }
            "#,
        );

        write_file(
            &grandchild,
            r#"
                export function Grandchild() {
                    return <span>Grandchild</span>;
                }
            "#,
        );

        let graph = read_source_graph(&entry, root, root).expect("source graph should compile");

        let ids = graph
            .modules
            .iter()
            .map(|module| module.id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            ids,
            vec!["src/App.tsx", "src/Child.tsx", "src/Grandchild.tsx",]
        );
    }

    #[test]
    fn follows_export_from_dependencies() {
        let temp = tempdir().expect("temporary directory should be created");
        let root = temp.path();

        let entry = root.join("src/index.ts");
        let component = root.join("src/Button.tsx");

        write_file(
            &entry,
            r#"
                export { Button } from "./Button";
            "#,
        );

        write_file(
            &component,
            r#"
                export function Button() {
                    return <button>Button</button>;
                }
            "#,
        );

        let graph = read_source_graph(&entry, root, root).expect("source graph should compile");

        assert_eq!(graph.modules.len(), 2);
        assert_eq!(graph.modules[0].id, "src/index.ts");
        assert_eq!(graph.modules[1].id, "src/Button.tsx");
    }

    #[test]
    fn follows_export_all_dependencies() {
        let temp = tempdir().expect("temporary directory should be created");
        let root = temp.path();

        let entry = root.join("src/index.ts");
        let component = root.join("src/Button.tsx");

        write_file(
            &entry,
            r#"
                export * from "./Button";
            "#,
        );

        write_file(
            &component,
            r#"
                export function Button() {
                    return <button>Button</button>;
                }
            "#,
        );

        let graph = read_source_graph(&entry, root, root).expect("source graph should compile");

        assert_eq!(graph.modules.len(), 2);
        assert_eq!(graph.modules[1].id, "src/Button.tsx");
    }

    #[test]
    fn follows_side_effect_imports() {
        let temp = tempdir().expect("temporary directory should be created");
        let root = temp.path();

        let entry = root.join("src/App.tsx");
        let setup = root.join("src/setup.ts");

        write_file(
            &entry,
            r#"
                import "./setup";

                export function App() {
                    return <div />;
                }
            "#,
        );

        write_file(
            &setup,
            r#"
                const initialized = true;
            "#,
        );

        let graph = read_source_graph(&entry, root, root).expect("source graph should compile");

        assert_eq!(graph.modules.len(), 2);
        assert_eq!(graph.modules[1].id, "src/setup.ts");
    }

    #[test]
    fn resolves_directory_index_modules() {
        let temp = tempdir().expect("temporary directory should be created");
        let root = temp.path();

        let entry = root.join("src/App.tsx");
        let component = root.join("src/components/index.tsx");

        write_file(
            &entry,
            r#"
                import { Button } from "./components";

                export function App() {
                    return <Button />;
                }
            "#,
        );

        write_file(
            &component,
            r#"
                export function Button() {
                    return <button />;
                }
            "#,
        );

        let graph = read_source_graph(&entry, root, root).expect("source graph should compile");

        assert_eq!(graph.modules.len(), 2);
        assert_eq!(graph.modules[0].id, "src/App.tsx");
        assert_eq!(graph.modules[1].id, "src/components/index.tsx");
    }

    #[test]
    fn does_not_duplicate_modules_reached_multiple_times() {
        let temp = tempdir().expect("temporary directory should be created");
        let root = temp.path();

        let entry = root.join("src/App.tsx");
        let left = root.join("src/Left.tsx");
        let right = root.join("src/Right.tsx");
        let shared = root.join("src/Shared.tsx");

        write_file(
            &entry,
            r#"
                import { Left } from "./Left";
                import { Right } from "./Right";

                export function App() {
                    return (
                        <>
                            <Left />
                            <Right />
                        </>
                    );
                }
            "#,
        );

        write_file(
            &left,
            r#"
                import { Shared } from "./Shared";

                export function Left() {
                    return <Shared />;
                }
            "#,
        );

        write_file(
            &right,
            r#"
                import { Shared } from "./Shared";

                export function Right() {
                    return <Shared />;
                }
            "#,
        );

        write_file(
            &shared,
            r#"
                export function Shared() {
                    return <span />;
                }
            "#,
        );

        let graph = read_source_graph(&entry, root, root).expect("source graph should compile");

        let shared_count = graph
            .modules
            .iter()
            .filter(|module| module.id.ends_with("Shared.tsx"))
            .count();

        assert_eq!(shared_count, 1);
        assert_eq!(graph.modules.len(), 4);
    }

    #[test]
    fn handles_circular_imports() {
        let temp = tempdir().expect("temporary directory should be created");
        let root = temp.path();

        let a = root.join("src/A.tsx");
        let b = root.join("src/B.tsx");

        write_file(
            &a,
            r#"
                import { B } from "./B";

                export function A() {
                    return <B />;
                }
            "#,
        );

        write_file(
            &b,
            r#"
                import { A } from "./A";

                export function B() {
                    return <A />;
                }
            "#,
        );

        let graph = read_source_graph(&a, root, root).expect("source graph should compile");

        assert_eq!(graph.modules.len(), 2);
    }

    #[test]
    fn rejects_source_files_beyond_size_limit() {
        let temp = tempdir().expect("temporary directory should be created");
        let root = temp.path();

        let entry = root.join("src/App.tsx");
        let oversized = format!(
            "export function App() {{ return <div>{}</div>; }}",
            "x".repeat(3 * 1024 * 1024)
        );
        std::fs::create_dir_all(entry.parent().unwrap()).expect("src directory should be created");
        std::fs::write(&entry, oversized).expect("oversized source should be written");

        let error = read_source_graph(&entry, root, root)
            .expect_err("oversized source file should be rejected");

        assert!(error.contains("exceeds the maximum size"), "{error}");
    }

    #[test]
    fn rejects_import_chains_beyond_depth_limit() {
        let temp = tempdir().expect("temporary directory should be created");
        let root = temp.path();
        let src = root.join("src");
        std::fs::create_dir_all(&src).expect("src directory should be created");

        const CHAIN_LENGTH: usize = plec_ir::limits::MAX_IMPORT_DEPTH + 8;
        for index in 0..CHAIN_LENGTH {
            let module = src.join(format!("Module{index}.tsx"));
            let source = if index + 1 < CHAIN_LENGTH {
                format!(
                    "import {{ Next }} from \"./Module{}\";\nexport const Next = {{}};\n",
                    index + 1
                )
            } else {
                "export const Next = {};\n".to_string()
            };
            std::fs::write(&module, source).expect("chain module should be written");
        }

        let entry = src.join("Module0.tsx");
        let error = read_source_graph(&entry, root, root)
            .expect_err("over-deep import chain should be rejected");

        assert!(error.contains("maximum depth"), "{error}");
    }

    #[test]
    fn accepts_import_chains_within_depth_limit() {
        let temp = tempdir().expect("temporary directory should be created");
        let root = temp.path();
        let src = root.join("src");
        std::fs::create_dir_all(&src).expect("src directory should be created");

        const CHAIN_LENGTH: usize = plec_ir::limits::MAX_IMPORT_DEPTH - 1;
        for index in 0..CHAIN_LENGTH {
            let module = src.join(format!("Module{index}.tsx"));
            let source = if index + 1 < CHAIN_LENGTH {
                format!(
                    "import {{ Next }} from \"./Module{}\";\nexport const Next = {{}};\n",
                    index + 1
                )
            } else {
                "export const Next = {};\n".to_string()
            };
            std::fs::write(&module, source).expect("chain module should be written");
        }

        let entry = src.join("Module0.tsx");
        let graph = read_source_graph(&entry, root, root)
            .expect("chain within the depth limit should compile");

        assert_eq!(graph.modules.len(), CHAIN_LENGTH);
    }

    #[test]
    fn skips_node_builtin_modules() {
        let temp = tempdir().expect("temporary directory should be created");
        let root = temp.path();

        let entry = root.join("src/App.tsx");

        write_file(
            &entry,
            r#"
                import path from "node:path";

                export function App() {
                    return <div>{path.sep}</div>;
                }
            "#,
        );

        let graph = read_source_graph(&entry, root, root).expect("source graph should compile");

        assert_eq!(graph.modules.len(), 1);
    }

    #[test]
    fn returns_error_for_invalid_entry_source() {
        let temp = tempdir().expect("temporary directory should be created");
        let root = temp.path();

        let entry = root.join("src/App.tsx");

        write_file(
            &entry,
            r#"
                export function App() {
                    return <div>
                }
            "#,
        );

        let error = read_source_graph(&entry, root, root).expect_err("invalid source should fail");

        assert!(error.contains("Failed to parse"));
        assert!(error.contains("src/App.tsx"));
    }

    #[test]
    fn returns_error_when_entry_does_not_exist() {
        let temp = tempdir().expect("temporary directory should be created");
        let root = temp.path();

        let entry = root.join("src/Missing.tsx");

        assert!(read_source_graph(&entry, root, root).is_err());
    }
}
