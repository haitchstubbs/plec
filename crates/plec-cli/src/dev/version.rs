//! Product SemVer management for the Plec workspace.
//!
//! One canonical release version lives in the Cargo workspace
//! (`[workspace.package] version` in the root `Cargo.toml`), is inherited by
//! Rust crates, and is mirrored by every JavaScript workspace package.
//!
//! The product version is deliberately blind to every protocol/schema
//! version (IR/component 0.10, route manifest, SSR snapshot, sidecar
//! protocol, DOM markers). Those follow release policy decisions; changing
//! one never dictates a product bump here, and a product bump here never
//! touches them.

use super::repo::Repo;
use semver::Version;
use std::fs;
use std::path::{Path, PathBuf};

/// Authoritative Cargo and JavaScript product-version declarations.
pub struct VersionFiles {
    pub cargo_toml: PathBuf,
    pub package_jsons: Vec<PathBuf>,
}

impl VersionFiles {
    pub fn in_repo(repo: &Repo) -> Result<VersionFiles, String> {
        let mut package_jsons = vec![repo.root.join("package.json")];
        for directory in ["apps", "packages", "scripts"] {
            collect_package_jsons(&repo.root.join(directory), &mut package_jsons)?;
        }
        package_jsons.sort();
        Ok(VersionFiles {
            cargo_toml: repo.root.join("Cargo.toml"),
            package_jsons,
        })
    }
}

pub fn run(repo: &Repo, set: Option<String>, check: bool) -> Result<(), String> {
    if check && set.is_some() {
        return Err("--check and --set cannot be used together".into());
    }

    let files = VersionFiles::in_repo(repo)?;

    if let Some(requested) = set {
        let version = Version::parse(&requested)
            .map_err(|error| format!("invalid SemVer {requested:?}: {error}"))?;
        set_version(&files, &version)?;
        println!("Plec product version set to {version}");
        println!("  updated {}", files.cargo_toml.display());
        for path in &files.package_jsons {
            println!("  updated {}", path.display());
        }
        println!("  Cargo.lock refreshes on the next cargo build");
        return Ok(());
    }

    let cargo = workspace_version(&read_source(&files.cargo_toml)?)?;
    println!("Plec product version");
    println!("  Cargo workspace [workspace.package]  {cargo}");
    let mut mismatches = Vec::new();
    for path in &files.package_jsons {
        let package = package_version(&read_source(path)?)?;
        println!(
            "  {}  {package}",
            path.strip_prefix(&repo.root).unwrap_or(path).display()
        );
        if package != cargo {
            mismatches.push(format!(
                "{} declares {package}",
                path.strip_prefix(&repo.root).unwrap_or(path).display()
            ));
        }
    }
    if !mismatches.is_empty() {
        return Err(format!(
            "product version mismatch: Cargo workspace declares {cargo}; {} — realign with `plec workspace version --set <version>`",
            mismatches.join(", ")
        ));
    }

    Ok(())
}

fn set_version(files: &VersionFiles, version: &Version) -> Result<(), String> {
    // Validate and stage every replacement before writing anything.
    let cargo_source = read_source(&files.cargo_toml)?;
    let cargo_updated = replace_workspace_version(&cargo_source, version)?;
    let mut replacements = vec![(files.cargo_toml.clone(), cargo_source, cargo_updated)];
    for path in &files.package_jsons {
        let source = read_source(path)?;
        let updated = replace_package_version(&source, version)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        replacements.push((path.clone(), source, updated));
    }

    let mut written = Vec::new();
    for (path, original, updated) in &replacements {
        if let Err(error) = write_source(path, updated) {
            for (written_path, written_original) in written.into_iter().rev() {
                let _ = fs::write(written_path, written_original);
            }
            return Err(error);
        }
        written.push((path, original));
    }

    Ok(())
}

fn read_source(path: &PathBuf) -> Result<String, String> {
    fs::read_to_string(path).map_err(|error| format!("cannot read {}: {error}", path.display()))
}

fn write_source(path: &PathBuf, contents: &str) -> Result<(), String> {
    fs::write(path, contents).map_err(|error| format!("cannot write {}: {error}", path.display()))
}

fn collect_package_jsons(directory: &Path, output: &mut Vec<PathBuf>) -> Result<(), String> {
    if !directory.exists() {
        return Ok(());
    }
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("cannot read {}: {error}", directory.display()))?;
    for entry in entries {
        let path = entry
            .map_err(|error| format!("cannot read directory entry: {error}"))?
            .path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| {
                matches!(
                    name.to_str(),
                    Some("node_modules" | "dist" | ".git" | "target")
                )
            }) {
                continue;
            }
            collect_package_jsons(&path, output)?;
        } else if path.file_name().is_some_and(|name| name == "package.json") {
            output.push(path);
        }
    }
    Ok(())
}

/// The `version` value inside `[workspace.package]`.
fn workspace_version(source: &str) -> Result<String, String> {
    let (line, _) = find_workspace_version_line(source)?;
    let (start, end) = quoted_value_range(line, "version")
        .ok_or_else(|| "malformed version line in [workspace.package]".to_string())?;
    Ok(line[start..end].to_owned())
}

/// Returns the source with the `[workspace.package]` version replaced.
fn replace_workspace_version(source: &str, version: &Version) -> Result<String, String> {
    let (_, index) = find_workspace_version_line(source)?;
    let line = source.lines().nth(index).expect("index came from lines()");
    let (start, end) = quoted_value_range(line, "version")
        .ok_or_else(|| "malformed version line in [workspace.package]".to_string())?;

    let mut updated_lines: Vec<String> = source.lines().map(str::to_owned).collect();
    updated_lines[index] = format!("{}{}{}", &line[..start], version, &line[end..]);
    join_lines(source, updated_lines)
}

fn find_workspace_version_line(source: &str) -> Result<(&str, usize), String> {
    let mut in_section = false;
    for (index, line) in source.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            in_section = trimmed.trim() == "[workspace.package]";
            continue;
        }
        if !in_section {
            continue;
        }
        if quoted_value_range(line, "version").is_some() {
            return Ok((line, index));
        }
    }
    Err("cannot find a version declaration in [workspace.package]".into())
}

/// The top-level `version` field of a workspace package manifest.
fn package_version(source: &str) -> Result<String, String> {
    let value: serde_json::Value =
        serde_json::from_str(source).map_err(|error| format!("invalid package.json: {error}"))?;
    value
        .get("version")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| "package.json has no top-level version".into())
}

/// Returns the source with a top-level package `version` set or added.
fn replace_package_version(source: &str, version: &Version) -> Result<String, String> {
    let mut updated_lines: Vec<String> = Vec::new();
    let mut replaced = false;
    for line in source.lines() {
        if !replaced {
            if line.starts_with("  ") && !line.starts_with("   ") {
                if let Some((start, end)) = quoted_value_range(line, "\"version\"") {
                    updated_lines.push(format!("{}{}{}", &line[..start], version, &line[end..]));
                    replaced = true;
                    continue;
                }
            }
        }
        updated_lines.push(line.to_owned());
    }
    if !replaced {
        let name_index = updated_lines.iter().position(|line| {
            line.starts_with("  \"name\"") && quoted_value_range(line, "\"name\"").is_some()
        });
        let insertion = name_index.map_or(1, |index| index + 1);
        updated_lines.insert(insertion, format!("  \"version\": \"{version}\","));
    }

    let updated = join_lines(source, updated_lines)?;
    if package_version(&updated)? != version.to_string() {
        return Err("package.json version replacement did not land on the top-level field".into());
    }
    Ok(updated)
}

/// Byte range of the quoted value in a `key = "value"` (TOML) or
/// `"key": "value"` (JSON) line, resolved against the whole line.
fn quoted_value_range(line: &str, key: &str) -> Option<(usize, usize)> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with(key) {
        return None;
    }
    let rest = &trimmed[key.len()..];
    let separator = rest.find(['=', ':'])?;
    let after = &rest[separator + 1..];
    let start = after.find('"')?;
    let end = after[start + 1..].find('"')? + start + 1;
    // `after` is a suffix slice of `line`, so its offset inside the line is
    // recovered from the length difference.
    let after_offset = line.len() - after.len();
    Some((after_offset + start + 1, after_offset + end))
}

fn join_lines(source: &str, lines: Vec<String>) -> Result<String, String> {
    let mut updated = lines.join("\n");
    if source.ends_with('\n') {
        updated.push('\n');
    }
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct Fixture {
        dir: tempfile::TempDir,
    }

    impl Fixture {
        fn new(cargo_version: &str, package_version: &str) -> Fixture {
            let dir = tempfile::tempdir().expect("tempdir");
            fs::write(
                dir.path().join("Cargo.toml"),
                format!(
                    "[workspace]\nresolver = \"2\"\nmembers = [\"crates/plec-cli\"]\n\n\
                     [workspace.package]\nversion = \"{cargo_version}\"\nedition = \"2021\"\n\n\
                     [workspace.dependencies]\ntoml = \"0.8\"\n"
                ),
            )
            .expect("write Cargo.toml");
            fs::write(
                dir.path().join("package.json"),
                format!("{{\n  \"name\": \"workspace\",\n  \"version\": \"{package_version}\",\n  \"private\": true\n}}\n"),
            )
            .expect("write root package.json");
            fs::create_dir_all(dir.path().join("packages/plec")).expect("mkdir packages/plec");
            fs::write(
                dir.path().join("packages/plec/package.json"),
                format!(
                    "{{\n  \"name\": \"plec\",\n  \"version\": \"{package_version}\",\n  \
                     \"private\": true\n}}\n"
                ),
            )
            .expect("write package.json");
            Fixture { dir }
        }

        fn repo(&self) -> Repo {
            Repo {
                root: self.dir.path().to_path_buf(),
            }
        }
    }

    fn cargo_version(repo: &Repo) -> String {
        workspace_version(&read_source(&VersionFiles::in_repo(repo).unwrap().cargo_toml).unwrap())
            .unwrap()
    }

    fn package_version_of(repo: &Repo) -> String {
        package_version(&read_source(&repo.root.join("packages/plec/package.json")).unwrap())
            .unwrap()
    }

    #[test]
    fn set_updates_workspace_and_all_package_declarations() {
        let fixture = Fixture::new("0.1.0", "0.0.0");
        let repo = fixture.repo();
        fs::create_dir_all(repo.root.join("apps/example")).unwrap();
        fs::write(
            repo.root.join("apps/example/package.json"),
            "{\n  \"name\": \"example\",\n  \"private\": true\n}\n",
        )
        .unwrap();
        run(&repo, Some("1.2.3-rc.1+001".into()), false).expect("set should succeed");
        assert_eq!(cargo_version(&repo), "1.2.3-rc.1+001");
        assert_eq!(package_version_of(&repo), "1.2.3-rc.1+001");
        assert_eq!(
            package_version(&read_source(&repo.root.join("package.json")).unwrap()).unwrap(),
            "1.2.3-rc.1+001"
        );
        let package = read_source(&repo.root.join("packages/plec/package.json")).unwrap();
        assert!(
            package.contains("\"private\": true"),
            "visibility must remain unchanged"
        );
        let new_package = read_source(&repo.root.join("apps/example/package.json")).unwrap();
        assert_eq!(package_version(&new_package).unwrap(), "1.2.3-rc.1+001");
        assert!(new_package.contains("\"private\": true"));
    }

    #[test]
    fn set_leaves_dependency_versions_and_protocol_constants_alone() {
        let fixture = Fixture::new("0.1.0", "0.1.0");
        let repo = fixture.repo();

        let ir_source = "pub const VERSION: &str = \"0.10\";\n\
                         pub const COMPONENT_VERSION: &str = \"0.10\";\n\
                         pub const SSR_SNAPSHOT_VERSION: u32 = 2;\n";
        let ir_dir = fixture.dir.path().join("crates/plec-ir/src");
        fs::create_dir_all(&ir_dir).expect("mkdir plec-ir");
        let ir_path = ir_dir.join("lib.rs");
        fs::write(&ir_path, ir_source).expect("write plec-ir lib.rs");

        run(&repo, Some("0.2.0".into()), false).expect("set should succeed");

        let cargo_source = read_source(&VersionFiles::in_repo(&repo).unwrap().cargo_toml).unwrap();
        assert!(
            cargo_source.contains("toml = \"0.8\""),
            "dependency pin must not move"
        );
        assert_eq!(
            fs::read_to_string(&ir_path).unwrap(),
            ir_source,
            "protocol constants must be untouched by a product bump"
        );
    }

    #[test]
    fn invalid_semver_is_rejected_without_modification() {
        for invalid in ["1.0", "1.0.0.0", "banana", ""] {
            let fixture = Fixture::new("0.1.0", "0.1.0");
            let repo = fixture.repo();
            let result = run(&repo, Some(invalid.into()), false);
            assert!(result.is_err(), "{invalid:?} must be rejected");
            assert_eq!(cargo_version(&repo), "0.1.0");
            assert_eq!(package_version_of(&repo), "0.1.0");
        }
    }

    #[test]
    fn check_reports_a_mismatch_with_both_versions() {
        let fixture = Fixture::new("0.1.0", "0.2.0");
        let result = run(&fixture.repo(), None, true);
        let error = result.expect_err("mismatch must fail");
        assert!(
            error.contains("0.1.0"),
            "must show the Cargo version: {error}"
        );
        assert!(
            error.contains("0.2.0"),
            "must show the package version: {error}"
        );
    }

    #[test]
    fn check_passes_when_versions_agree() {
        let fixture = Fixture::new("1.0.0", "1.0.0");
        run(&fixture.repo(), None, true).expect("matching versions must pass");
    }

    #[test]
    fn check_and_set_are_mutually_exclusive() {
        let fixture = Fixture::new("0.1.0", "0.1.0");
        let result = run(&fixture.repo(), Some("0.2.0".into()), true);
        assert!(result.is_err());
    }
}
