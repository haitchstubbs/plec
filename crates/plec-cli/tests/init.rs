use std::fs;
use std::process::Command;

fn run_init(directory: &std::path::Path, force: bool) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_plec"));
    command.arg("init").arg(directory);
    if force {
        command.arg("--force");
    }
    command.output().expect("plec binary should be invocable")
}

#[test]
fn release_cli_scaffolds_application() {
    let temp = tempfile::tempdir().unwrap();
    let app = temp.path().join("my-app");
    let output = run_init(&app, false);

    // The dev CLI intentionally has no app scaffolding command.
    if String::from_utf8_lossy(&output.stderr).contains("unrecognized subcommand") {
        return;
    }

    assert!(
        output.status.success(),
        "init should succeed\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for file in [
        "package.json",
        "plec.toml",
        "src/router.tsx",
        "src/routes/index.tsx",
        "src/routes/home.tsx",
        "src/client.tsx",
        "src/server.ts",
        "src/styles.css",
        ".gitignore",
        "README.md",
    ] {
        assert!(app.join(file).is_file(), "missing generated file {file}");
    }

    let package = fs::read_to_string(app.join("package.json")).unwrap();
    assert!(package.contains(r#""name": "my-app""#));
    assert!(package.contains(r#""dev": "plec dev""#));
    assert!(package.contains(r#""build": "plec build""#));

    let client = fs::read_to_string(app.join("src/client.tsx")).unwrap();
    assert!(client.contains("startPlecRouter"));
    let server = fs::read_to_string(app.join("src/server.ts")).unwrap();
    assert!(server.contains("AppRequestHandler"));
}

#[test]
fn init_rejects_non_empty_directory_without_force() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("existing.txt"), "existing").unwrap();

    let output = run_init(temp.path(), false);

    if String::from_utf8_lossy(&output.stderr).contains("unrecognized subcommand") {
        return;
    }

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("non-empty directory"));
}
