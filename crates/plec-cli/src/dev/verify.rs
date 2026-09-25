use super::artifact;
use super::repo::Repo;
use super::wasmtest;
use serde::Serialize;
use std::process::{Command, Stdio};
use std::time::Instant;

#[derive(Debug, Clone, Serialize)]
pub struct StageReport {
    pub name: String,
    pub ok: bool,
    pub duration_ms: u128,
    pub passed: Option<u32>,
    pub failed: Option<u32>,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VerifyReport {
    pub ok: bool,
    pub stages: Vec<StageReport>,
}

pub fn run(repo: &Repo) -> VerifyReport {
    let mut stages = Vec::new();

    stages.push(run_command(
        repo,
        "Rust compile",
        "cargo",
        &["check", "-p", "plec-runtime", "--tests"],
    ));
    stages.push(run_command(
        repo,
        "runtime unit",
        "cargo",
        &["test", "-p", "plec-runtime", "--lib"],
    ));

    stages.push(run_wasm(repo));
    stages.push(run_command(
        repo,
        "browser unit",
        yarn_command(),
        &["workspace", "@plec/browser", "test"],
    ));
    stages.push(run_command(
        repo,
        "typecheck",
        yarn_command(),
        &["workspace", "@plec/browser", "typecheck"],
    ));
    stages.push(run_command(
        repo,
        "cross adoption E2E",
        yarn_command(),
        &[
            "workspace",
            "@plec/e2e",
            "exec",
            "playwright",
            "test",
            "--config",
            "playwright.acceptance.config.ts",
            "tests/acceptance/adoption.playwright.ts",
            "tests/acceptance/adoption-mismatch.playwright.ts",
        ],
    ));

    let artifact = artifact::inspect(repo);
    let mut details = Vec::new();
    if !artifact.ok {
        for layer in [&artifact.dist, &artifact.staged] {
            if let Some(layer) = layer {
                details.extend(layer.problems.iter().cloned());
            }
        }
    }
    stages.push(StageReport {
        name: "artifact stale".into(),
        ok: artifact.ok,
        duration_ms: 0,
        passed: None,
        failed: None,
        details,
    });

    let ok = stages.iter().all(|stage| stage.ok);
    VerifyReport { ok, stages }
}

fn yarn_command() -> &'static str {
    if cfg!(windows) {
        "yarn.cmd"
    } else {
        "yarn"
    }
}

fn run_wasm(repo: &Repo) -> StageReport {
    let started = Instant::now();
    match wasmtest::run_wasm_tests(repo, &[]) {
        Ok(report) => StageReport {
            name: "WASM browser".into(),
            ok: report.ok(),
            duration_ms: started.elapsed().as_millis(),
            passed: Some(report.passed),
            failed: Some(report.failed),
            details: report
                .failures
                .iter()
                .map(|failure| {
                    format!(
                        "{}: {}",
                        failure.name,
                        failure.message.lines().next().unwrap_or("")
                    )
                })
                .collect(),
        },
        Err(error) => StageReport {
            name: "WASM browser".into(),
            ok: false,
            duration_ms: started.elapsed().as_millis(),
            passed: None,
            failed: None,
            details: vec![error],
        },
    }
}

fn run_command(repo: &Repo, name: &str, program: &str, args: &[&str]) -> StageReport {
    let started = Instant::now();
    let output = Command::new(program)
        .args(args)
        .current_dir(&repo.root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();

    let (ok, output) = match output {
        Ok(output) => (
            output.status.success(),
            combined_output(&output.stdout, &output.stderr),
        ),
        Err(error) => (false, format!("failed to spawn {program}: {error}")),
    };
    let (passed, failed) = test_counts(&output);
    let details = if ok { Vec::new() } else { diagnostics(&output) };

    StageReport {
        name: name.into(),
        ok,
        duration_ms: started.elapsed().as_millis(),
        passed,
        failed,
        details,
    }
}

fn combined_output(stdout: &[u8], stderr: &[u8]) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(stdout),
        String::from_utf8_lossy(stderr)
    )
}

fn test_counts(output: &str) -> (Option<u32>, Option<u32>) {
    let mut passed = 0;
    let mut failed = 0;
    let mut found = false;

    for line in output.lines() {
        let tokens: Vec<_> = line.split_whitespace().collect();
        for index in 1..tokens.len() {
            let label =
                tokens[index].trim_matches(|character: char| !character.is_ascii_alphabetic());
            if label == "passed" {
                if let Ok(value) = tokens[index - 1].parse::<u32>() {
                    passed += value;
                    found = true;
                }
            } else if label == "failed" {
                if let Ok(value) = tokens[index - 1].parse::<u32>() {
                    failed += value;
                    found = true;
                }
            }
        }
    }

    if found {
        (Some(passed), Some(failed))
    } else {
        (None, None)
    }
}

fn diagnostics(output: &str) -> Vec<String> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| {
            line.contains("error")
                || line.contains("FAILED")
                || line.contains("missing:")
                || line.contains("mismatch:")
                || line.contains("unsupported:")
                || line.contains("STALE:")
        })
        .take(8)
        .map(str::to_owned)
        .collect()
}

fn print_stage(stage: &StageReport) {
    let mark = if stage.ok { "✓" } else { "✗" };
    let counts = match (stage.passed, stage.failed) {
        (Some(passed), Some(failed)) => format!(" ({passed} passed, {failed} failed)"),
        _ => String::new(),
    };
    println!("{mark} {}{} ({} ms)", stage.name, counts, stage.duration_ms);
    for detail in &stage.details {
        println!("  {detail}");
    }
}

pub fn print_report(report: &VerifyReport) {
    for stage in &report.stages {
        print_stage(stage);
    }
    if report.ok {
        println!("✓ adoption verification passed");
    } else {
        println!("✗ adoption verification failed");
        println!("hint: plec workspace doctor adoption");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregates_rust_and_playwright_counts() {
        assert_eq!(
            test_counts("test result: ok. 29 passed; 0 failed\n6 passed"),
            (Some(35), Some(0))
        );
    }

    #[test]
    fn extracts_adoption_diagnostics() {
        let details = diagnostics("noise\nerror: mismatch:ssr-loop:key\nmore noise");
        assert_eq!(details, vec!["error: mismatch:ssr-loop:key"]);
    }
}
