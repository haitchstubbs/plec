use std::process::Command;

/// `plec --version` must print exactly the product version compiled from
/// the workspace package metadata — the same SemVer the release artifact
/// carries in `packages/plec/package.json`.
#[test]
fn version_flag_prints_product_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_plec"))
        .arg("--version")
        .output()
        .expect("plec binary should launch");

    assert!(
        output.status.success(),
        "--version must exit 0: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout)
            .expect("utf8 stdout")
            .trim(),
        format!("plec {}", env!("CARGO_PKG_VERSION")),
        "--version must print the canonical product version"
    );
}
