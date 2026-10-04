//! Consumer development loop: build into isolation, then serve the last good
//! output while polling application sources for changes.

use crate::diagnostic::DevelopmentDiagnostic;
use plec_build::{build, modules::build::Stage, BuildError, BuildOptions, RuntimeSource};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::{Duration, SystemTime};
use tokio::sync::oneshot;

pub struct DevOptions {
    pub source: PathBuf,
    pub out_dir: PathBuf,
    pub client_entry: PathBuf,
    pub server_entry: PathBuf,
    pub host: Option<String>,
    pub port: Option<u16>,
}

pub fn run(options: DevOptions) -> Result<(), Box<dyn std::error::Error>> {
    let mut shutdown = ShutdownSignals::start()?;
    let source = fs::canonicalize(&options.source).map_err(|error| {
        DevelopmentDiagnostic::build(BuildError::with_source(
            Stage::Compile,
            format!(
                "cannot resolve application entry {}",
                options.source.display()
            ),
            error,
        ))
    })?;
    let app_dir = source
        .parent()
        .and_then(Path::parent)
        .ok_or("could not determine application directory")?
        .to_path_buf();
    let out_dir = absolute(&options.out_dir)?;
    let mut live = build_options(&options, source.clone());
    live.out_dir = temporary_output(&out_dir);

    println!("Plec dev: building {}", source.display());
    let initial_candidate = CandidateOutput::new(live.out_dir.clone());
    build(live.clone()).map_err(DevelopmentDiagnostic::build)?;
    if shutdown.requested.load(Ordering::SeqCst) {
        drop(initial_candidate);
        shutdown.stop();
        return Ok(());
    }
    replace_output(&live.out_dir, &out_dir)?;
    drop(initial_candidate);

    let state_path = out_dir.with_file_name(format!(".plec-dev-state-{}.json", std::process::id()));
    let session = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_nanos()
    );
    let mut generation = 0u64;
    let mut session_guard = DevSessionGuard::new(state_path.clone(), out_dir.clone());
    write_generation(&state_path, &session, generation)?;
    if shutdown.requested.load(Ordering::SeqCst) {
        shutdown.stop();
        return Ok(());
    }
    let host_name = options.host.as_deref().unwrap_or("127.0.0.1");
    let port = options
        .port
        .or_else(|| std::env::var("PORT").ok()?.parse().ok())
        .unwrap_or(3000);
    // Establish the watcher baseline before the application becomes reachable;
    // otherwise an edit immediately after readiness can be absorbed into the
    // initial snapshot and lost.
    let mut previous = source_snapshot(&app_dir, &out_dir)?;
    session_guard.host = Some(spawn_host(
        &out_dir,
        options.host.as_deref(),
        options.port,
        &state_path,
    )?);
    wait_ready(host_name, port, session_guard.host.as_mut().unwrap())?;
    println!("Plec dev: initial build complete; watching for changes");
    println!(
        "Plec dev: watching {} inputs under {}",
        previous.len(),
        app_dir.display()
    );

    loop {
        if shutdown.requested.load(Ordering::SeqCst) {
            println!("Plec dev: shutting down");
            break;
        }
        thread::sleep(Duration::from_millis(250));
        if session_guard.host.as_mut().unwrap().try_wait()?.is_some() {
            session_guard.host = None;
            return Err("plec serve exited unexpectedly".into());
        }

        let current = source_snapshot(&app_dir, &out_dir)?;
        if current == previous {
            continue;
        }
        println!("Plec dev: change detected; waiting for filesystem activity to settle");
        // Quiet-window debounce: extend the deadline whenever another relevant
        // input changes. This coalesces editor temp-file/rename save patterns.
        let mut settled = current;
        loop {
            if shutdown.requested.load(Ordering::SeqCst) {
                break;
            }
            thread::sleep(Duration::from_millis(100));
            let observed = source_snapshot(&app_dir, &out_dir)?;
            if observed == settled {
                break;
            }
            settled = observed;
        }
        if shutdown.requested.load(Ordering::SeqCst) {
            break;
        }
        // This is the exact state the upcoming attempt is intended to build.
        // Keep it as the comparison point so writes during compilation survive.
        previous = settled;

        let mut next = build_options(&options, source.clone());
        next.out_dir = temporary_output(&out_dir);
        let candidate = CandidateOutput::new(next.out_dir.clone());
        println!("Plec dev: rebuilding...");
        match build(next.clone()) {
            Ok(_) => {
                if shutdown.requested.load(Ordering::SeqCst) {
                    break;
                }
                let restart = changed_server_artifacts(&out_dir, &next.out_dir)?;
                if restart {
                    stop_host(session_guard.host.as_mut().unwrap())?;
                    session_guard.host = None;
                }
                if restart {
                    let backup = backup_output(&out_dir);
                    if let Err(error) = promote_output(&next.out_dir, &out_dir, &backup) {
                        eprintln!("Plec dev: output promotion failed: {error}");
                        session_guard.host = Some(spawn_host(
                            &out_dir,
                            options.host.as_deref(),
                            options.port,
                            &state_path,
                        )?);
                        wait_ready(host_name, port, session_guard.host.as_mut().unwrap())?;
                        continue;
                    }
                    let replacement =
                        spawn_host(&out_dir, options.host.as_deref(), options.port, &state_path);
                    let startup = match replacement {
                        Ok(mut replacement) => {
                            match wait_ready(host_name, port, &mut replacement) {
                                Ok(()) => {
                                    session_guard.host = Some(replacement);
                                    Ok(())
                                }
                                Err(error) => {
                                    let _ = stop_host(&mut replacement);
                                    Err(error)
                                }
                            }
                        }
                        Err(error) => Err(error),
                    };
                    if let Err(error) = startup {
                        eprintln!("Plec dev: replacement host failed readiness: {error}");
                        rollback_output(&backup, &out_dir)?;
                        session_guard.host = Some(spawn_host(
                            &out_dir,
                            options.host.as_deref(),
                            options.port,
                            &state_path,
                        )?);
                        wait_ready(host_name, port, session_guard.host.as_mut().unwrap())?;
                        continue;
                    }
                    remove_dir(&backup);
                } else {
                    replace_output(&next.out_dir, &out_dir)?;
                }
                if shutdown.requested.load(Ordering::SeqCst) {
                    continue;
                }
                generation = generation.saturating_add(1);
                write_generation(&state_path, &session, generation)?;
                // Capture only after the attempt. Changes made during compile
                // remain different from this snapshot and schedule one more
                // quiet-window rebuild on the next iteration.
                println!(
                    "Plec dev: rebuild complete{}",
                    if restart { "; host restarted" } else { "" }
                );
            }
            Err(error) => {
                remove_dir(&next.out_dir);
                eprintln!(
                    "Plec dev: build failed: {}",
                    DevelopmentDiagnostic::build(error)
                );
                eprintln!("Plec dev: serving previous successful build");
            }
        }
        drop(candidate);
    }
    shutdown.stop();
    session_guard.shutdown();
    Ok(())
}

struct CandidateOutput(PathBuf);

impl CandidateOutput {
    fn new(path: PathBuf) -> Self {
        Self(path)
    }
}

impl Drop for CandidateOutput {
    fn drop(&mut self) {
        remove_dir(&self.0);
    }
}

struct DevSessionGuard {
    host: Option<Child>,
    state_path: PathBuf,
    out_dir: PathBuf,
}

impl DevSessionGuard {
    fn new(state_path: PathBuf, out_dir: PathBuf) -> Self {
        Self {
            host: None,
            state_path,
            out_dir,
        }
    }

    fn shutdown(&mut self) {
        if let Some(mut host) = self.host.take() {
            if let Err(error) = stop_host(&mut host) {
                eprintln!("Plec dev: warning: failed to stop host cleanly: {error}");
            }
        }
        if let Err(error) = fs::remove_file(&self.state_path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                eprintln!("Plec dev: warning: failed to remove dev state: {error}");
            }
        }
        let _ = fs::remove_file(self.state_path.with_extension("tmp"));
        remove_dir(&self.out_dir.with_file_name(format!(
            "{}.next",
            self.out_dir.file_name().unwrap_or_default().to_string_lossy()
        )));
        remove_dir(&backup_output(&self.out_dir));
    }
}

impl Drop for DevSessionGuard {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct ShutdownSignals {
    requested: Arc<AtomicBool>,
    cancel: Option<oneshot::Sender<()>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl ShutdownSignals {
    fn start() -> Result<Self, Box<dyn std::error::Error>> {
        let requested = Arc::new(AtomicBool::new(false));
        let signal_requested = requested.clone();
        let (cancel, cancelled) = oneshot::channel();
        let thread = thread::spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            runtime.block_on(async move {
                #[cfg(unix)]
                {
                    use tokio::signal::unix::{signal, SignalKind};
                    let Ok(mut interrupt) = signal(SignalKind::interrupt()) else {
                        return;
                    };
                    let Ok(mut terminate) = signal(SignalKind::terminate()) else {
                        return;
                    };
                    tokio::select! {
                        _ = interrupt.recv() => signal_requested.store(true, Ordering::SeqCst),
                        _ = terminate.recv() => signal_requested.store(true, Ordering::SeqCst),
                        _ = cancelled => {},
                    }
                }
                #[cfg(windows)]
                {
                    let Ok(mut ctrl_break) = tokio::signal::windows::ctrl_break() else {
                        return;
                    };
                    tokio::select! {
                        result = tokio::signal::ctrl_c() => {
                            if result.is_ok() { signal_requested.store(true, Ordering::SeqCst); }
                        }
                        _ = ctrl_break.recv() => signal_requested.store(true, Ordering::SeqCst),
                        _ = cancelled => {},
                    }
                }
            });
        });
        Ok(Self {
            requested,
            cancel: Some(cancel),
            thread: Some(thread),
        })
    }

    fn stop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for ShutdownSignals {
    fn drop(&mut self) {
        self.stop();
    }
}

fn build_options(options: &DevOptions, source: PathBuf) -> BuildOptions {
    BuildOptions {
        source,
        client_entry: options.client_entry.clone(),
        server_entry: options.server_entry.clone(),
        out_dir: options.out_dir.clone(),
        optimize: false,
        title: String::new(),
        description: None,
        styles_href: None,
        preloads: Vec::new(),
        runtime_source: RuntimeSource::Auto,
    }
}

fn spawn_host(
    out_dir: &Path,
    host: Option<&str>,
    port: Option<u16>,
    state_path: &Path,
) -> Result<Child, Box<dyn std::error::Error>> {
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg("serve")
        .arg(out_dir)
        .arg("--development")
        .arg("--dev-state")
        .arg(state_path);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0000_0200); // CREATE_NEW_PROCESS_GROUP
    }
    if let Some(host) = host {
        command.args(["--host", host]);
    }
    if let Some(port) = port {
        command.args(["--port", &port.to_string()]);
    }
    Ok(command.spawn()?)
}

fn write_generation(
    path: &Path,
    session: &str,
    generation: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let temporary = path.with_extension("tmp");
    fs::write(
        &temporary,
        serde_json::json!({"session":session,"generation":generation}).to_string(),
    )?;
    #[cfg(windows)]
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(temporary, path)?;
    Ok(())
}

fn wait_ready(host: &str, port: u16, child: &mut Child) -> Result<(), Box<dyn std::error::Error>> {
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        if child.try_wait()?.is_some() {
            return Err("plec serve exited before becoming ready".into());
        }
        if let Ok(mut stream) = TcpStream::connect((host, port)) {
            stream.set_read_timeout(Some(Duration::from_secs(2)))?;
            stream.set_write_timeout(Some(Duration::from_secs(2)))?;
            stream.write_all(b"GET /__plec/dev/events HTTP/1.1\r\nHost: localhost\r\nAccept: text/event-stream\r\nConnection: close\r\n\r\n")?;
            let mut response = Vec::new();
            let mut chunk = [0; 512];
            while !response.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let read = stream.read(&mut chunk)?;
                if read == 0 {
                    break;
                }
                response.extend_from_slice(&chunk[..read]);
            }
            if response.starts_with(b"HTTP/1.1 200") || response.starts_with(b"HTTP/1.0 200") {
                return Ok(());
            }
        }
        if std::time::Instant::now() >= deadline {
            return Err(format!("timed out waiting for http://{host}:{port}").into());
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn stop_host(host: &mut Child) -> Result<(), Box<dyn std::error::Error>> {
    if host.try_wait()?.is_none() {
        #[cfg(unix)]
        {
            let status = Command::new("kill")
                .args(["-TERM", &host.id().to_string()])
                .status();
            if !matches!(status, Ok(status) if status.success()) {
                host.kill()?;
            }
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while std::time::Instant::now() < deadline {
                if host.try_wait()?.is_some() {
                    return Ok(());
                }
                thread::sleep(Duration::from_millis(50));
            }
            host.kill()?;
        }
        #[cfg(windows)]
        {
            const CTRL_BREAK_EVENT: u32 = 1;
            #[link(name = "Kernel32")]
            unsafe extern "system" {
                fn GenerateConsoleCtrlEvent(event: u32, process_group_id: u32) -> i32;
            }
            let sent = unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, host.id()) } != 0;
            if !sent {
                host.kill()?;
            } else {
                let deadline = std::time::Instant::now() + Duration::from_secs(5);
                while std::time::Instant::now() < deadline {
                    if host.try_wait()?.is_some() {
                        return Ok(());
                    }
                    thread::sleep(Duration::from_millis(50));
                }
                host.kill()?;
            }
        }
    }
    host.wait()?;
    Ok(())
}

fn changed_server_artifacts(old: &Path, next: &Path) -> Result<bool, Box<dyn std::error::Error>> {
    Ok(
        !same_file(&old.join("server/app.mjs"), &next.join("server/app.mjs"))?
            || !same_file(
                &old.join("server/runtime.mjs"),
                &next.join("server/runtime.mjs"),
            )?
            || !same_file(
                &old.join("plec-server.json"),
                &next.join("plec-server.json"),
            )?,
    )
}

fn same_file(left: &Path, right: &Path) -> Result<bool, Box<dyn std::error::Error>> {
    match (fs::read(left), fs::read(right)) {
        (Ok(left), Ok(right)) => Ok(left == right),
        (Err(left), Err(_)) if left.kind() == std::io::ErrorKind::NotFound => Ok(true),
        _ => Ok(false),
    }
}

fn replace_output(next: &Path, live: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let backup = backup_output(live);
    promote_output(next, live, &backup)?;
    remove_dir(&backup);
    Ok(())
}

fn backup_output(live: &Path) -> PathBuf {
    live.with_file_name(format!(
        "{}.previous-{}",
        live.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id()
    ))
}

fn promote_output(
    next: &Path,
    live: &Path,
    backup: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let prepared = live.with_file_name(format!(
        "{}.next",
        live.file_name().unwrap_or_default().to_string_lossy()
    ));
    remove_dir(&prepared);
    remove_dir(&backup);
    // Prepare a complete sibling directory before touching the live output.
    fs::rename(next, &prepared)?;
    if live.exists() {
        fs::rename(live, backup)?;
    }
    if let Err(error) = fs::rename(&prepared, live) {
        if backup.exists() {
            let _ = fs::rename(backup, live);
        }
        return Err(error.into());
    }
    Ok(())
}

fn rollback_output(backup: &Path, live: &Path) -> Result<(), Box<dyn std::error::Error>> {
    remove_dir(live);
    fs::rename(backup, live)?;
    Ok(())
}

fn temporary_output(live: &Path) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    live.with_file_name(format!(".plec-dev-{}-{stamp}", std::process::id()))
}

fn absolute(path: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    })
}

fn remove_dir(path: &Path) {
    let _ = fs::remove_dir_all(path);
}

fn source_snapshot(
    root: &Path,
    out_dir: &Path,
) -> Result<BTreeMap<PathBuf, [u8; 32]>, Box<dyn std::error::Error>> {
    let mut snapshot = BTreeMap::new();
    visit(root, root, &mut snapshot)?;
    // The compiler emits the actual source dependency paths and URLs. Retain
    // those inputs even when an asset is outside the application directory.
    if let Ok(manifest) = fs::read(out_dir.join("plec-assets.json")) {
        let dependencies: Vec<serde_json::Value> = serde_json::from_slice(&manifest)?;
        for dependency in dependencies {
            if let Some(source) = dependency.get("source").and_then(|value| value.as_str()) {
                let path = PathBuf::from(source);
                let path = if path.is_absolute() {
                    path
                } else {
                    root.join(path)
                };
                if let Ok(metadata) = fs::metadata(&path) {
                    if metadata.is_file() {
                        snapshot.insert(path.clone(), digest_file(&path)?);
                    }
                }
            }
        }
    }
    Ok(snapshot)
}

fn visit(
    root: &Path,
    directory: &Path,
    snapshot: &mut BTreeMap<PathBuf, [u8; 32]>,
) -> Result<(), Box<dyn std::error::Error>> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let relative = path.strip_prefix(root)?;
        if relative.components().any(|component| {
            matches!(
                component.as_os_str().to_str(),
                Some("node_modules" | ".git" | "dist")
            ) || component
                .as_os_str()
                .to_string_lossy()
                .starts_with(".plec-dev-")
        }) {
            continue;
        }
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            visit(root, &path, snapshot)?;
        } else if metadata.is_file() {
            snapshot.insert(relative.to_path_buf(), digest_file(&path)?);
        }
    }
    Ok(())
}

fn digest_file(path: &Path) -> Result<[u8; 32], Box<dyn std::error::Error>> {
    Ok(Sha256::digest(fs::read(path)?).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_ignores_build_and_dependency_directories() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::create_dir_all(dir.path().join("api")).unwrap();
        fs::create_dir_all(dir.path().join("node_modules/pkg")).unwrap();
        fs::create_dir_all(dir.path().join("dist")).unwrap();
        fs::write(dir.path().join("src/router.tsx"), "route").unwrap();
        fs::write(dir.path().join("api/todos.ts"), "route").unwrap();
        fs::write(dir.path().join("node_modules/pkg/index.js"), "ignored").unwrap();
        fs::write(dir.path().join("dist/index.html"), "ignored").unwrap();
        let snapshot = source_snapshot(dir.path(), &dir.path().join("dist")).unwrap();
        assert_eq!(snapshot.len(), 2);
        assert!(snapshot.contains_key(Path::new("src/router.tsx")));
        assert!(snapshot.contains_key(Path::new("api/todos.ts")));
    }

    #[test]
    fn snapshot_includes_compiler_asset_dependencies_outside_app_root() {
        let app = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        let asset = external.path().join("logo.svg");
        fs::write(&asset, "logo").unwrap();
        fs::create_dir_all(app.path().join("dist")).unwrap();
        fs::write(
            app.path().join("dist/plec-assets.json"),
            serde_json::json!([{"source": asset.to_string_lossy(), "url": "/assets/logo.svg"}])
                .to_string(),
        )
        .unwrap();
        let snapshot = source_snapshot(app.path(), &app.path().join("dist")).unwrap();
        assert!(snapshot.contains_key(&asset));
    }

    #[test]
    fn server_change_requires_restart() {
        let old = tempfile::tempdir().unwrap();
        let next = tempfile::tempdir().unwrap();
        fs::create_dir_all(old.path().join("server")).unwrap();
        fs::create_dir_all(next.path().join("server")).unwrap();
        fs::write(old.path().join("server/app.mjs"), "old").unwrap();
        fs::write(next.path().join("server/app.mjs"), "new").unwrap();
        assert!(changed_server_artifacts(old.path(), next.path()).unwrap());
    }

    #[test]
    fn promotion_replaces_the_tree_only_after_candidate_is_complete() {
        let parent = tempfile::tempdir().unwrap();
        let live = parent.path().join("dist");
        let candidate = parent.path().join(".plec-dev-candidate");
        fs::create_dir_all(&live).unwrap();
        fs::create_dir_all(&candidate).unwrap();
        fs::write(live.join("old.txt"), "old").unwrap();
        fs::write(candidate.join("new.txt"), "new").unwrap();
        replace_output(&candidate, &live).unwrap();
        assert!(!live.join("old.txt").exists());
        assert_eq!(fs::read_to_string(live.join("new.txt")).unwrap(), "new");
        assert!(!candidate.exists());
    }
}
