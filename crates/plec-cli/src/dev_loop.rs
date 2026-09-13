//! Consumer development loop: build into isolation, then serve the last good
//! output while polling application sources for changes.

use plec_build::{build, BuildOptions, RuntimeSource};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, SystemTime};

pub struct DevOptions {
    pub source: PathBuf,
    pub out_dir: PathBuf,
    pub client_entry: PathBuf,
    pub server_entry: PathBuf,
    pub host: Option<String>,
    pub port: Option<u16>,
}

pub fn run(options: DevOptions) -> Result<(), Box<dyn std::error::Error>> {
    let source = fs::canonicalize(&options.source)?;
    let app_dir = source
        .parent()
        .and_then(Path::parent)
        .ok_or("could not determine application directory")?
        .to_path_buf();
    let out_dir = absolute(&options.out_dir)?;
    let mut live = build_options(&options, source.clone());
    live.out_dir = temporary_output(&out_dir);

    println!("Plec dev: building {}", source.display());
    build_and_promote(&live, &out_dir)?;

    let mut host = spawn_host(&out_dir, options.host.as_deref(), options.port)?;
    let mut previous = source_snapshot(&app_dir)?;

    loop {
        thread::sleep(Duration::from_millis(250));
        if host.try_wait()?.is_some() {
            return Err("plec serve exited unexpectedly".into());
        }

        let current = source_snapshot(&app_dir)?;
        if current == previous {
            continue;
        }
        // Collapse editor save bursts into one build.
        thread::sleep(Duration::from_millis(100));
        previous = source_snapshot(&app_dir)?;

        let mut next = build_options(&options, source.clone());
        next.out_dir = temporary_output(&out_dir);
        println!("Plec dev: rebuilding...");
        match build(next.clone()) {
            Ok(_) => {
                let restart = changed_server_artifacts(&out_dir, &next.out_dir)?;
                if restart {
                    stop_host(&mut host)?;
                }
                replace_output(&next.out_dir, &out_dir)?;
                if restart {
                    host = spawn_host(&out_dir, options.host.as_deref(), options.port)?;
                }
                println!(
                    "Plec dev: rebuild complete{}",
                    if restart { "; host restarted" } else { "" }
                );
            }
            Err(error) => {
                remove_dir(&next.out_dir);
                eprintln!("Plec dev: build failed: {error}");
                eprintln!("Plec dev: serving previous successful build");
            }
        }
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

fn build_and_promote(
    options: &BuildOptions,
    out_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    build(options.clone())?;
    replace_output(&options.out_dir, out_dir)
}

fn spawn_host(
    out_dir: &Path,
    host: Option<&str>,
    port: Option<u16>,
) -> Result<Child, Box<dyn std::error::Error>> {
    let mut command = Command::new(std::env::current_exe()?);
    command.arg("serve").arg(out_dir).arg("--development");
    if let Some(host) = host {
        command.args(["--host", host]);
    }
    if let Some(port) = port {
        command.args(["--port", &port.to_string()]);
    }
    Ok(command.spawn()?)
}

fn stop_host(host: &mut Child) -> Result<(), Box<dyn std::error::Error>> {
    if host.try_wait()?.is_none() {
        host.kill()?;
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
    if live.exists() {
        for entry in fs::read_dir(live)? {
            let path = entry?.path();
            if path.is_dir() {
                fs::remove_dir_all(path)?;
            } else {
                fs::remove_file(path)?;
            }
        }
    } else {
        fs::create_dir_all(live)?;
    }
    copy_dir(next, live)?;
    fs::remove_dir_all(next)?;
    Ok(())
}

fn copy_dir(source: &Path, destination: &Path) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&source_path, &destination_path)?;
        } else {
            fs::copy(source_path, destination_path)?;
        }
    }
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
) -> Result<BTreeMap<PathBuf, (u64, SystemTime)>, Box<dyn std::error::Error>> {
    let mut snapshot = BTreeMap::new();
    visit(root, root, &mut snapshot)?;
    Ok(snapshot)
}

fn visit(
    root: &Path,
    directory: &Path,
    snapshot: &mut BTreeMap<PathBuf, (u64, SystemTime)>,
) -> Result<(), Box<dyn std::error::Error>> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let relative = path.strip_prefix(root)?;
        if relative.components().any(|component| {
            matches!(
                component.as_os_str().to_str(),
                Some("node_modules" | ".git" | "dist")
            )
        }) {
            continue;
        }
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            visit(root, &path, snapshot)?;
        } else if metadata.is_file() {
            snapshot.insert(
                relative.to_path_buf(),
                (metadata.len(), metadata.modified()?),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_ignores_build_and_dependency_directories() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::create_dir_all(dir.path().join("node_modules/pkg")).unwrap();
        fs::create_dir_all(dir.path().join("dist")).unwrap();
        fs::write(dir.path().join("src/router.tsx"), "route").unwrap();
        fs::write(dir.path().join("node_modules/pkg/index.js"), "ignored").unwrap();
        fs::write(dir.path().join("dist/index.html"), "ignored").unwrap();
        let snapshot = source_snapshot(dir.path()).unwrap();
        assert_eq!(snapshot.len(), 1);
        assert!(snapshot.contains_key(Path::new("src/router.tsx")));
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
}
