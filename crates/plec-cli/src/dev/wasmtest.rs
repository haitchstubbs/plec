use super::repo::Repo;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// The WASM browser suite is run through `scripts/browser-harness.mjs`, which
/// single-sources ChromeDriver/Chrome resolution for wasm-pack.
///
/// Breadcrumb: per the e2e policy in AGENTS.md, Playwright owns long-running
/// browser processes for tests. When wasm-bindgen test orchestration moves
/// under `packages/plec-e2e`, this spawn should be replaced with that runner —
/// the capture/parse layer here is intentionally independent of how the suite
/// is invoked so only this function changes.
const HARNESS_PATH: &str = "scripts/browser-harness.mjs";

const CAPTURE_DIR: &str = "test-wasm";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmFailure {
    pub name: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmTestReport {
    pub started_at_ms: u128,
    pub finished_at_ms: u128,
    pub duration_ms: u128,
    pub filters: Vec<String>,
    pub exit_status: Option<i32>,
    pub passed: u32,
    pub failed: u32,
    pub failures: Vec<WasmFailure>,
}

impl WasmTestReport {
    pub fn ok(&self) -> bool {
        self.failed == 0 && self.exit_status.unwrap_or(1) == 0
    }
}

pub fn run_wasm_tests(repo: &Repo, filters: &[String]) -> Result<WasmTestReport, String> {
    let harness = repo.root.join(HARNESS_PATH);
    if !harness.is_file() {
        return Err(format!("harness not found: {}", harness.display()));
    }

    let started_at_ms = epoch_ms();
    let clock = Instant::now();

    eprintln!(
        "Running WASM suite via {}{}",
        HARNESS_PATH,
        if filters.is_empty() {
            String::new()
        } else {
            format!(" (filters: {})", filters.join(", "))
        }
    );

    let mut child = Command::new("node")
        .arg(&harness)
        .arg("--")
        .args(filters)
        .current_dir(&repo.root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to spawn harness: {error}"))?;

    let captured = Arc::new(Mutex::new(String::new()));

    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");

    let out_capture = Arc::clone(&captured);
    let err_capture = Arc::clone(&captured);

    let out_handle = std::thread::spawn(move || {
        tee_lines(BufReader::new(stdout), out_capture);
    });
    let err_handle = std::thread::spawn(move || {
        tee_lines(BufReader::new(stderr), err_capture);
    });

    out_handle.join().expect("stdout reader");
    err_handle.join().expect("stderr reader");

    let exit_status = child
        .wait()
        .map_err(|error| format!("harness wait: {error}"))?;
    let exit_status = exit_status.code();

    let captured = Arc::try_unwrap(captured)
        .map_err(|_| "capture buffer still shared")?
        .into_inner()
        .expect("capture mutex");

    let mut report = parse_wasm_output(&captured);
    report.started_at_ms = started_at_ms;
    report.finished_at_ms = epoch_ms();
    report.duration_ms = clock.elapsed().as_millis();
    report.filters = filters.to_vec();
    report.exit_status = exit_status;

    let dir = repo.ensure_cache_dir()?;
    let capture_dir = dir.join(CAPTURE_DIR);
    fs::create_dir_all(&capture_dir)
        .map_err(|error| format!("cannot create capture dir: {error}"))?;

    fs::write(capture_dir.join("last.log"), &captured)
        .map_err(|error| format!("cannot write capture log: {error}"))?;
    let json = serde_json::to_string_pretty(&report)
        .map_err(|error| format!("cannot serialize report: {error}"))?;
    fs::write(capture_dir.join("last.json"), json)
        .map_err(|error| format!("cannot write capture report: {error}"))?;

    Ok(report)
}

pub fn load_last_report(repo: &Repo) -> Result<WasmTestReport, String> {
    let path = repo.cache_dir().join(CAPTURE_DIR).join("last.json");
    let raw = fs::read_to_string(&path).map_err(|_| {
        format!(
            "no captured WASM run found at {} — run `plec dev test wasm` first",
            path.display()
        )
    })?;

    serde_json::from_str(&raw).map_err(|error| format!("captured report is unreadable: {error}"))
}

fn tee_lines<R: std::io::Read>(reader: BufReader<R>, capture: Arc<Mutex<String>>) {
    for line in reader.lines().map_while(Result::ok) {
        println!("{line}");
        if let Ok(mut captured) = capture.lock() {
            captured.push_str(&line);
            captured.push('\n');
        }
    }
}

/// Parse wasm-bindgen-test / cargo output into failures and totals.
///
/// Failing tests appear as indented `---- <name> output ----` blocks with a
/// `panicked at <file>:<line>:<col>:` line; each test binary ends with a
/// `test result: ...` summary line. Totals are aggregated across binaries.
fn parse_wasm_output(output: &str) -> WasmTestReport {
    let lines: Vec<&str> = output.lines().collect();
    let mut failures = Vec::new();
    let mut passed = 0u32;
    let mut failed = 0u32;

    let mut index = 0usize;
    while index < lines.len() {
        let line = lines[index];

        if let Some(name) = block_header_name(line) {
            let (end, body) = collect_block(&lines, index + 1);
            if let Some(failure) = parse_failure_block(name, &body) {
                failures.push(failure);
            }
            index = end;
            continue;
        }

        if let Some(result_line) = line.trim().strip_prefix("test result:") {
            let counts = parse_result_counts(result_line);
            passed += counts.0;
            failed += counts.1;
        }

        index += 1;
    }

    WasmTestReport {
        started_at_ms: 0,
        finished_at_ms: 0,
        duration_ms: 0,
        filters: Vec::new(),
        exit_status: None,
        passed,
        failed,
        failures,
    }
}

fn block_header_name(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    trimmed
        .strip_prefix("---- ")
        .and_then(|rest| rest.strip_suffix(" output ----"))
}

/// Collect the indented body of a failing-test block until the next marker.
fn collect_block<'a>(lines: &[&'a str], start: usize) -> (usize, Vec<&'a str>) {
    let mut body = Vec::new();
    let mut index = start;

    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim_start();

        if block_header_name(line).is_some()
            || trimmed.starts_with("Invoking test:")
            || trimmed.starts_with("test result:")
        {
            break;
        }

        body.push(line);
        index += 1;
    }

    (index, body)
}

fn parse_failure_block(name: &str, body: &[&str]) -> Option<WasmFailure> {
    let mut panic_location: Option<(String, u32)> = None;
    let mut message_start: Option<usize> = None;

    for (index, line) in body.iter().enumerate() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("panicked at ") {
            if let Some(parsed) = parse_panic_location(rest) {
                panic_location = Some(parsed);
                message_start = Some(index + 1);
                break;
            }
        }
    }

    let (file, line_number) = panic_location?;

    let mut message_lines = Vec::new();
    if let Some(start) = message_start {
        for line in &body[start..] {
            let trimmed = line.trim_start();
            if trimmed.starts_with("Stack:") {
                break;
            }
            message_lines.push(trimmed);
        }
    }

    let message = message_lines
        .iter()
        .skip_while(|line| line.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();

    Some(WasmFailure {
        name: name.to_string(),
        file: Some(file),
        line: Some(line_number),
        message: if message.is_empty() {
            "(no panic message captured)".into()
        } else {
            message
        },
    })
}

/// `crates/plec-runtime/tests/typed_events.rs:3694:6:`
fn parse_panic_location(rest: &str) -> Option<(String, u32)> {
    let trimmed = rest.trim().trim_end_matches(':');

    // file:line:col — strip the column, then parse the line.
    let col_start = trimmed.rfind(':')?;
    let without_col = &trimmed[..col_start];

    let line_start = without_col.rfind(':')?;
    let line: u32 = without_col[line_start + 1..].parse().ok()?;

    Some((without_col[..line_start].to_string(), line))
}

/// `FAILED. 71 passed; 6 failed; 0 ignored; ...` → (71, 6)
fn parse_result_counts(rest: &str) -> (u32, u32) {
    let mut passed = 0;
    let mut failed = 0;

    for part in rest.split(';') {
        let tokens: Vec<&str> = part.split_whitespace().collect();
        for index in 1..tokens.len() {
            match tokens[index] {
                "passed" => passed = tokens[index - 1].parse().unwrap_or(passed),
                "failed" => failed = tokens[index - 1].parse().unwrap_or(failed),
                _ => {}
            }
        }
    }

    (passed, failed)
}

fn epoch_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

pub fn print_report(report: &WasmTestReport, failures_only: bool) {
    if failures_only {
        print_failures(report);
        return;
    }

    if report.ok() {
        println!(
            "\n✓ {} passed, 0 failed ({} ms)",
            report.passed, report.duration_ms
        );
    } else {
        println!(
            "\n{} / {} failed",
            report.failed,
            report.passed + report.failed
        );
        print_failures(report);
    }
}

pub fn print_failures(report: &WasmTestReport) {
    if report.failures.is_empty() && report.failed == 0 {
        return;
    }

    for failure in &report.failures {
        println!("\n{}", failure.name);
        if let (Some(file), Some(line)) = (&failure.file, failure.line) {
            println!("  {file}:{line}");
        }
        let first = failure
            .message
            .lines()
            .next()
            .unwrap_or("(no message)")
            .trim();
        println!("  {first}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
   Compiling plec-runtime v0.0.0
    Finished test [unoptimized + debuginfo] target(s) in 3.2s

running 1 test
test runtime_loads_application ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

    Invoking test: adopted_duplicate_marker_fails_instead_of_silently_claiming
    Invoking test: nested_component_loop_adopts_recorded_rows_without_remount
        ---- nested_component_loop_adopts_recorded_rows_without_remount output ----
            error output:
                panicked at crates/plec-runtime/tests/typed_events.rs:3793:6:
                called `Result::unwrap()` on an `Err` value: JsValue("mismatch:ssr-snapshot:unknown ssr snapshot route ssr-snapshot.tsx#Home")
                
                Stack:
                
                Error
                    at http://127.0.0.1:39779/wasm-bindgen-test:1336:25
        ---- nested_component_branch_record_contradicting_dom_fails_closed output ----
            error output:
                panicked at crates/plec-runtime/tests/typed_events.rs:3763:5:
                assertion `left == right` failed
test result: FAILED. 71 passed; 6 failed; 0 ignored; 0 filtered out; finished in 0.97s
error: test failed, to rerun pass `--test typed_events`
"#;

    #[test]
    fn parses_failures_and_totals() {
        let report = parse_wasm_output(SAMPLE);

        assert_eq!(report.passed, 72);
        assert_eq!(report.failed, 6);
        assert_eq!(report.failures.len(), 2);

        let first = &report.failures[0];
        assert_eq!(
            first.name,
            "nested_component_loop_adopts_recorded_rows_without_remount"
        );
        assert_eq!(
            first.file.as_deref(),
            Some("crates/plec-runtime/tests/typed_events.rs")
        );
        assert_eq!(first.line, Some(3793));
        assert!(first
            .message
            .starts_with("called `Result::unwrap()` on an `Err` value"));
        // Stack frames are not part of the message.
        assert!(!first.message.contains("wasm-bindgen-test:1336"));

        let second = &report.failures[1];
        assert_eq!(second.line, Some(3763));
        assert_eq!(second.message, "assertion `left == right` failed");
    }

    #[test]
    fn parses_passing_output() {
        let report = parse_wasm_output(
            "test result: ok. 77 passed; 0 failed; 0 ignored; 0 filtered out; finished in 0.93s",
        );
        assert_eq!(report.passed, 77);
        assert_eq!(report.failed, 0);
        assert!(report.failures.is_empty());
    }

    #[test]
    fn parses_panic_location_with_multiple_colons() {
        let (file, line) =
            parse_panic_location("crates/plec-runtime/tests/typed_events.rs:3694:6:").unwrap();
        assert_eq!(file, "crates/plec-runtime/tests/typed_events.rs");
        assert_eq!(line, 3694);
    }
}
