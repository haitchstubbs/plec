//! Launch the first-party Vite development coordinator.

use std::{
    path::PathBuf,
    process::{Command, Stdio},
};

pub struct DevOptions {
    pub source: PathBuf,
    pub out_dir: PathBuf,
    pub client_entry: PathBuf,
    pub server_entry: PathBuf,
    pub host: Option<String>,
    pub port: Option<u16>,
}

pub fn run(options: DevOptions) -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::current_dir()?;
    const RESOLVE: &str = "import { createRequire } from 'node:module'; const core = import.meta.resolve('@plec/core'); console.log(createRequire(core).resolve('@plec/vite/dev'));";
    let node = std::env::var_os("NODE").unwrap_or_else(|| "node".into());
    let resolution = Command::new(&node)
        .args(["--input-type=module", "--eval", RESOLVE])
        .current_dir(&root)
        .output()?;
    if !resolution.status.success() {
        return Err(format!(
            "plec dev could not resolve the @plec/vite coordinator from installed @plec/core: {}",
            String::from_utf8_lossy(&resolution.stderr).trim()
        )
        .into());
    }
    let module = String::from_utf8(resolution.stdout)?.trim().to_owned();
    let port = options
        .port
        .or_else(|| std::env::var("PORT").ok()?.parse().ok())
        .unwrap_or(3000);
    let mut child = Command::new(node)
        .arg(module)
        .arg(options.source)
        .arg(options.out_dir)
        .arg(options.host.unwrap_or_else(|| "127.0.0.1".into()))
        .arg(port.to_string())
        .arg(options.client_entry)
        .arg(options.server_entry)
        .env("PLEC_CLI_BINARY", std::env::current_exe()?)
        .current_dir(root)
        .stdin(Stdio::piped())
        .spawn()?;
    // Keeping the pipe open lets the coordinator detect abrupt termination of
    // the Rust CLI and close Vite plus its native-host child cleanly.
    let _coordinator_lifetime = child.stdin.take();
    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("Vite development session exited with {status}").into())
    }
}
