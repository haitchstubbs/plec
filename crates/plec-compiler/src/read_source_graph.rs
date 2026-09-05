use plec_parser::{module_dependencies, parse_module, ParsedModule};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fs,
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
/// Workspace/package resolution is deliberately isolated behind
/// `resolve_module` and can be added without changing graph traversal.
pub fn read_source_graph(
    entry: impl AsRef<Path>,
    root_dir: impl AsRef<Path>,
    repo_root_dir: impl AsRef<Path>,
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
    let workspace = WorkspaceIndex::load(&repo_root_dir)?;

    visit_module(
        entry,
        &root_dir,
        &repo_root_dir,
        &workspace,
        None,
        &mut seen,
        &mut modules,
        &mut resolved_imports,
        0,
    )?;

    Ok(SourceGraph {
        modules,
        resolved_imports,
    })
}

fn visit_module(
    file_path: &Path,
    root_dir: &Path,
    repo_root_dir: &Path,
    workspace: &WorkspaceIndex,
    canonical_id: Option<String>,
    seen: &mut HashSet<PathBuf>,
    modules: &mut Vec<ParsedModule>,
    resolved_imports: &mut HashMap<(String, String), String>,
    depth: usize,
) -> Result<(), String> {
    use plec_ir::limits::{MAX_IMPORT_DEPTH, MAX_MODULE_COUNT, MAX_SOURCE_FILE_BYTES};

    if depth > MAX_IMPORT_DEPTH {
        return Err(format!(
            "Import graph exceeds the maximum depth of {MAX_IMPORT_DEPTH} at {}",
            file_path.display()
        ));
    }

    let absolute = fs::canonicalize(file_path)
        .map_err(|error| format!("Failed to resolve {}: {error}", file_path.display()))?;

    if !seen.insert(absolute.clone()) {
        return Ok(());
    }

    if modules.len() >= MAX_MODULE_COUNT {
        return Err(format!(
            "Source graph exceeds the maximum module count of {MAX_MODULE_COUNT}"
        ));
    }

    let source = {
        let metadata = fs::metadata(&absolute)
            .map_err(|error| format!("Failed to read {}: {error}", absolute.display()))?;
        if metadata.len() > MAX_SOURCE_FILE_BYTES {
            return Err(format!(
                "Source file {} exceeds the maximum size of {MAX_SOURCE_FILE_BYTES} bytes",
                absolute.display()
            ));
        }
        fs::read_to_string(&absolute)
            .map_err(|error| format!("Failed to read {}: {error}", absolute.display()))?
    };

    let module_id =
        canonical_id.unwrap_or_else(|| module_id_from_path(&absolute, root_dir, repo_root_dir));

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
        let Some(resolved) = resolve_module(&specifier, &absolute, workspace)? else {
            continue;
        };

        let target_absolute = fs::canonicalize(&resolved)
            .map_err(|error| format!("Failed to resolve {}: {error}", resolved.display()))?;
        let target_id = module_id_from_path(&target_absolute, root_dir, repo_root_dir);
        resolved_imports.insert((module_id.clone(), specifier.clone()), target_id);

        visit_module(
            &resolved,
            root_dir,
            repo_root_dir,
            workspace,
            None,
            seen,
            modules,
            resolved_imports,
            depth + 1,
        )?;
    }

    Ok(())
}

fn resolve_module(
    specifier: &str,
    from_file: &Path,
    workspace: &WorkspaceIndex,
) -> Result<Option<PathBuf>, String> {
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

        return Ok(resolve_source_candidate(&base));
    }

    if let Some(workspace_module) = resolve_workspace_module(specifier, workspace)? {
        return Ok(Some(workspace_module));
    }

    resolve_dependency_module(specifier, from_file)
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

/// Resolve an import to another package in the Plec workspace.
///
/// The existing TypeScript implementation understands workspace package
/// exports and prefers authored `src` files over published `dist` files.
///
/// This is intentionally isolated so that package-layout policy does not leak
/// into source graph traversal.
impl WorkspaceIndex {
    fn load(repo_root_dir: &Path) -> Result<Self, String> {
        let mut packages = HashMap::new();
        let root = repo_root_dir.join("packages");
        let Ok(entries) = fs::read_dir(&root) else {
            return Ok(Self { packages });
        };
        for entry in entries {
            let entry =
                entry.map_err(|error| format!("Failed to read workspace package: {error}"))?;
            let manifest = entry.path().join("package.json");
            let Ok(source) = fs::read_to_string(&manifest) else {
                continue;
            };
            let value: Value = serde_json::from_str(&source)
                .map_err(|error| format!("Invalid {}: {error}", manifest.display()))?;
            if let Some(name) = value.get("name").and_then(Value::as_str) {
                packages.insert(name.into(), entry.path());
            }
        }
        Ok(Self { packages })
    }
}

fn resolve_workspace_module(
    specifier: &str,
    workspace: &WorkspaceIndex,
) -> Result<Option<PathBuf>, String> {
    let (name, subpath) = split_package_specifier(specifier);
    let Some(package_dir) = workspace.packages.get(name) else {
        return Ok(None);
    };
    let manifest_path = package_dir.join("package.json");
    let manifest: Value = serde_json::from_str(
        &fs::read_to_string(&manifest_path)
            .map_err(|error| format!("Failed to read {}: {error}", manifest_path.display()))?,
    )
    .map_err(|error| format!("Invalid {}: {error}", manifest_path.display()))?;
    let requested = if subpath.is_empty() {
        ".".to_string()
    } else {
        format!("./{subpath}")
    };
    let target = manifest
        .get("exports")
        .and_then(|exports| resolve_export(exports, &requested));

    if let Some(target) = target {
        return Ok(resolve_workspace_source(package_dir, &target));
    }
    if subpath.is_empty() {
        return Ok(resolve_source_candidate(&package_dir.join("src/index")));
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

fn resolve_workspace_source(package_dir: &Path, target: &str) -> Option<PathBuf> {
    let published = package_dir.join(target);
    let source = target
        .replace("./dist/", "./src/")
        .replace(".d.ts", "")
        .replace(".js", "");
    resolve_source_candidate(&package_dir.join(source))
        .or_else(|| published.is_file().then_some(published))
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
        assert_eq!(
            resolve_workspace_module("@scope/icons/icons/mark", &index).unwrap(),
            Some(repo.path().join("packages/icons/src/icons/mark.tsx"))
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
