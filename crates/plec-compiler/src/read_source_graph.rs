use plec_parser::{module_dependencies, parse_module, ParsedModule};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct SourceGraph {
    pub modules: Vec<ParsedModule>,
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

    visit_module(
        entry,
        &root_dir,
        &repo_root_dir,
        None,
        &mut seen,
        &mut modules,
    )?;

    Ok(SourceGraph { modules })
}

fn visit_module(
    file_path: &Path,
    root_dir: &Path,
    repo_root_dir: &Path,
    canonical_id: Option<String>,
    seen: &mut HashSet<PathBuf>,
    modules: &mut Vec<ParsedModule>,
) -> Result<(), String> {
    let absolute = fs::canonicalize(file_path)
        .map_err(|error| format!("Failed to resolve {}: {error}", file_path.display()))?;

    if !seen.insert(absolute.clone()) {
        return Ok(());
    }

    let source = fs::read_to_string(&absolute)
        .map_err(|error| format!("Failed to read {}: {error}", absolute.display()))?;

    let module_id = canonical_id.unwrap_or_else(|| module_id_from_path(&absolute, root_dir));

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
        let Some(resolved) = resolve_module(&specifier, &absolute, repo_root_dir)? else {
            continue;
        };

        visit_module(&resolved, root_dir, repo_root_dir, None, seen, modules)?;
    }

    Ok(())
}

fn resolve_module(
    specifier: &str,
    from_file: &Path,
    repo_root_dir: &Path,
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

    if let Some(workspace_module) = resolve_workspace_module(specifier, repo_root_dir)? {
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

fn module_id_from_path(path: &Path, root_dir: &Path) -> String {
    path.strip_prefix(root_dir)
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
fn resolve_workspace_module(
    _specifier: &str,
    _repo_root_dir: &Path,
) -> Result<Option<PathBuf>, String> {
    // TODO:
    //
    // Port:
    // - resolveWorkspaceModule
    // - resolveWorkspaceSource
    // - resolveExportTarget
    //
    // from the TypeScript node-entry compiler.
    Ok(None)
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
