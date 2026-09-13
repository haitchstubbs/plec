use super::repo::Repo;
use serde::Serialize;
use std::fs;

const GENERATED_PATH: &str = "packages/plec-browser/src/limits.generated.ts";

#[derive(Debug, Serialize)]
struct Report {
    path: &'static str,
    status: &'static str,
    expected_bytes: usize,
    actual_bytes: Option<usize>,
}

pub fn run(repo: &Repo, check: bool, write: bool, json: bool) -> Result<(), String> {
    if check && write {
        return Err("--check and --write cannot be used together".into());
    }
    let expected = emit_ts();
    let path = repo.root.join(GENERATED_PATH);
    let actual = fs::read_to_string(&path).ok();
    let matches = actual.as_deref() == Some(expected.as_str());

    if write {
        fs::write(&path, &expected)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    }

    let status = if write {
        "written"
    } else if matches {
        "current"
    } else {
        "drifted"
    };
    let report = Report {
        path: GENERATED_PATH,
        status,
        expected_bytes: expected.len(),
        actual_bytes: actual.as_ref().map(String::len),
    };

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report)
                .map_err(|error| format!("serialize limits report: {error}"))?
        );
    } else {
        println!("TypeScript resource-limit contract");
        println!("  path    {}", report.path);
        println!("  status  {}", report.status);
        if !write && !matches {
            println!("  fix     plec workspace contract limits --write");
        }
    }

    if !write && !matches {
        Err("generated TypeScript limits are stale (--write to regenerate)".into())
    } else {
        Ok(())
    }
}

pub fn emit_ts() -> String {
    format!(
        "// @generated from crates/plec-ir/src/limits.rs; do not edit.\n// Regenerate with: plec workspace contract limits --write\n\nexport const MAX_ARTIFACT_JSON_BYTES = {};\nexport const MAX_SNAPSHOT_JSON_BYTES = {};\nexport const MAX_RUNTIME_JS_BYTES = {};\nexport const MAX_PROVIDER_MANIFEST_JSON_BYTES = {};\n",
        plec_ir::limits::MAX_ARTIFACT_JSON_BYTES,
        plec_ir::limits::MAX_SNAPSHOT_JSON_BYTES,
        plec_ir::limits::MAX_RUNTIME_JS_BYTES,
        plec_ir::limits::MAX_PROVIDER_MANIFEST_JSON_BYTES,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn checked_in_module_matches_rust_limits() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../packages/plec-browser/src/limits.generated.ts");
        let actual = fs::read_to_string(path).expect("generated limits module must exist");
        assert_eq!(
            actual,
            emit_ts(),
            "regenerate with `plec workspace contract limits --write`"
        );
    }
}
