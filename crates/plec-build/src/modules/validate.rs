use super::build::{BuildError, Stage};
use serde_json::Value;
use std::{fs, path::Path};

/// Compiler/server-only dependencies that must never reach the browser.
const FORBIDDEN_BROWSER_DEPENDENCIES: &[&str] = &["zod", "typescript", "@swc/core"];

/// Enforce the browser dependency boundary against the browser bundle
/// metafile.
///
/// The build fails closed: every offending package is reported with its
/// import paths, and the build never continues on a violation.
pub fn browser_dependencies(metafile_path: &Path) -> Result<(), BuildError> {
    let stage = Stage::DependencyValidation;

    let source = fs::read_to_string(metafile_path).map_err(|error| {
        BuildError::with_source(
            stage,
            format!(
                "failed to read browser bundle metafile {}",
                metafile_path.display()
            ),
            error,
        )
    })?;

    let metafile: Value = serde_json::from_str(&source).map_err(|error| {
        BuildError::with_source(stage, "browser bundle metafile is not valid JSON", error)
    })?;

    let Some(inputs) = metafile.get("inputs").and_then(Value::as_object) else {
        return Err(BuildError::new(
            stage,
            "browser bundle metafile is missing `inputs`",
        ));
    };

    let mut violations: Vec<(String, Vec<String>)> = Vec::new();

    for input in inputs.keys() {
        for package in FORBIDDEN_BROWSER_DEPENDENCIES {
            if matches_package(input, package) {
                match violations.iter_mut().find(|(name, _)| name == package) {
                    Some((_, paths)) => paths.push(input.clone()),
                    None => violations.push(((*package).to_string(), vec![input.clone()])),
                }
            }
        }
    }

    if violations.is_empty() {
        return Ok(());
    }

    let mut message = String::from("forbidden dependency leaked into browser bundle:\n");

    for (package, paths) in &violations {
        message.push_str(&format!("\n{package}:\n"));

        for path in paths {
            message.push_str(&format!("  - {path}\n"));
        }
    }

    message.push_str(&format!(
        "\nInspect {} to trace the import path.",
        metafile_path.display()
    ));

    Err(BuildError::new(stage, message))
}

/// Mirror of the path-boundary matching used by the original JavaScript
/// build: the package must appear as a path segment directly below a
/// `node_modules/` or workspace `packages/` directory.
fn matches_package(input: &str, package: &str) -> bool {
    let input = input.replace('\\', "/");
    let segments: Vec<&str> = input.split('/').collect();
    let package_segments: Vec<&str> = package.split('/').collect();

    for anchor in ["node_modules", "packages"] {
        for index in 0..segments.len() {
            if segments[index] != anchor {
                continue;
            }

            let remainder = &segments[index + 1..];

            if remainder.len() >= package_segments.len()
                && remainder[..package_segments.len()] == package_segments[..]
            {
                return true;
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::matches_package;

    #[test]
    fn matches_node_modules_and_workspace_paths() {
        assert!(matches_package("node_modules/zod/index.js", "zod"));
        assert!(matches_package("node_modules/zod", "zod"));
        assert!(matches_package(
            "app/node_modules/@swc/core/core.js",
            "@swc/core"
        ));
        assert!(matches_package(
            "packages/plec-server/src/index.ts",
            "plec-server"
        ));
        assert!(matches_package(
            "apps\\demo\\node_modules\\zod\\x.js",
            "zod"
        ));
    }

    #[test]
    fn ignores_unrelated_paths_and_prefixes() {
        assert!(!matches_package("src/zodlike.js", "zod"));
        assert!(!matches_package("node_modules/zod-dom/index.js", "zod"));
        assert!(!matches_package(
            "packages/plec-server-tools/x.ts",
            "plec-server"
        ));
        assert!(!matches_package("src/client.tsx", "typescript"));
    }
}
