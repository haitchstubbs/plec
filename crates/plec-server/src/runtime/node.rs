//! The production [`ApplicationRuntime`]: a Node.js sidecar that imports the
//! application's server bundle and serves it over a private socket.
//!
//! The sidecar is a capability of the host, never the public server: Rust
//! owns the socket, the lifecycle, and every byte ceiling; Node executes the
//! application code for which Node semantics actually matter.

use std::{
    process::Stdio,
    sync::{Arc, Mutex},
};

use axum::{
    body::Body,
    http::{Method, Request, Response, StatusCode},
};
use tokio::{
    io::AsyncBufReadExt,
    sync::oneshot,
    time::{timeout, Duration},
};

use super::{
    internal::{dispatch_internal, InternalAddress, InternalRequest},
    protocol,
};
use crate::{
    request::{read_bounded_body, RequestContext},
    runtime::{ApplicationDispatch, HostRenderDispatch, HostRenderRequest},
    ApplicationRuntime, PlecServerOptions, ServerError,
};

/// How the sidecar is launched. Paths are resolved by the caller (usually
/// from the generated server manifest), so this struct stays deployment
/// configuration, not application configuration.
#[derive(Debug, Clone)]
pub struct NodeRuntimeOptions {
    /// Node executable; resolved from `PATH` by default.
    pub node: std::path::PathBuf,
    /// The `plec-node-runtime` script.
    pub script: std::path::PathBuf,
    /// The application server bundle (`server/app.mjs`).
    pub bundle: std::path::PathBuf,
}

impl NodeRuntimeOptions {
    pub fn new(
        script: impl Into<std::path::PathBuf>,
        bundle: impl Into<std::path::PathBuf>,
    ) -> Self {
        Self {
            node: "node".into(),
            script: script.into(),
            bundle: bundle.into(),
        }
    }
}

/// How long the sidecar has to import its bundle, bind its socket, and
/// report readiness. A malformed or missing bundle fails long before this.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);

/// A running Node sidecar. Dropping it kills the child and removes the
/// private runtime directory; [`NodeApplicationRuntime::shutdown`] does the
/// same explicitly so the host can shut the listener down first.
#[derive(Clone)]
pub struct NodeApplicationRuntime {
    process: Arc<RuntimeProcess>,
}

impl std::fmt::Debug for NodeApplicationRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeApplicationRuntime")
            .finish_non_exhaustive()
    }
}

struct RuntimeProcess {
    address: InternalAddress,
    token: String,
    runtime_dir: std::path::PathBuf,
    child: tokio::sync::Mutex<tokio::process::Child>,
}

impl NodeApplicationRuntime {
    /// Spawns the sidecar and waits for readiness. Fails — killing the
    /// child — when the script is missing, the bundle fails to import, the
    /// bundle does not export `handleRequest`, or the sidecar never reports
    /// the structured READY line.
    pub async fn spawn(options: NodeRuntimeOptions) -> Result<Self, ServerError> {
        let token = generate_token();
        let runtime_dir = create_runtime_dir(&token)?;

        #[cfg(unix)]
        let address = InternalAddress::UnixSocket(runtime_dir.join("app.sock"));
        #[cfg(not(unix))]
        let address = {
            // Loopback only, ephemeral port; the token is the access
            // boundary, never the bind address.
            let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|error| {
                ServerError::message(format!("cannot reserve sidecar port: {error}"))
            })?;
            let address = listener.local_addr().map_err(|error| {
                ServerError::message(format!("cannot reserve sidecar port: {error}"))
            })?;
            drop(listener);
            InternalAddress::Tcp(address)
        };

        let socket_env: String = match &address {
            #[cfg(unix)]
            InternalAddress::UnixSocket(path) => path.to_string_lossy().into_owned(),
            #[cfg(unix)]
            InternalAddress::Tcp(_) => unreachable!("unix builds never reserve tcp sidecars"),
            #[cfg(not(unix))]
            InternalAddress::UnixSocket(_) => unreachable!("windows builds never use unix sockets"),
            #[cfg(not(unix))]
            InternalAddress::Tcp(address) => format!("tcp:{}", address),
        };

        let mut child = tokio::process::Command::new(&options.node)
            .arg(&options.script)
            .env("PLEC_RUNTIME_SOCKET", &socket_env)
            .env("PLEC_RUNTIME_BUNDLE", &options.bundle)
            .env(
                "PLEC_RUNTIME_PROVIDER_MANIFEST",
                options
                    .bundle
                    .parent()
                    .and_then(std::path::Path::parent)
                    .map(|dir| dir.join("public/host-providers.json"))
                    .unwrap_or_else(|| std::path::PathBuf::from("host-providers.json")),
            )
            .env("PLEC_RUNTIME_TOKEN", &token)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| {
                ServerError::message(format!("cannot spawn node application runtime: {error}"))
            })?;

        // Child output is protocol output on stdout and diagnostics on
        // stderr. Both are drained continuously for the child's whole life:
        // application modules log during import and after readiness, and a
        // full pipe would otherwise block the sidecar mid-request.
        let (ready_tx, ready_rx) = oneshot::channel();
        let diagnostics = Arc::new(Diagnostics::default());
        if let Some(stdout) = child.stdout.take() {
            let mut stdout = tokio::io::BufReader::new(stdout).lines();
            let ready_tx = Arc::new(Mutex::new(Some(ready_tx)));
            let diagnostics = Arc::clone(&diagnostics);
            tokio::spawn(async move {
                while let Ok(Some(line)) = stdout.next_line().await {
                    if let Some(payload) = line.strip_prefix(protocol::READY_PREFIX) {
                        if let Ok(ready) =
                            serde_json::from_str::<super::internal::SidecarReady>(payload)
                        {
                            if let Some(sender) = ready_tx.lock().expect("ready lock").take() {
                                let _ = sender.send(ready);
                            }
                        }
                    } else if let Some(message) = line.strip_prefix(protocol::ERROR_PREFIX) {
                        diagnostics.record_error(message.to_owned());
                    }
                    println!("{line}");
                }
            });
        }
        if let Some(stderr) = child.stderr.take() {
            let mut stderr = tokio::io::BufReader::new(stderr).lines();
            let diagnostics = Arc::clone(&diagnostics);
            tokio::spawn(async move {
                while let Ok(Some(line)) = stderr.next_line().await {
                    diagnostics.record_diagnostic(line.clone());
                    eprintln!("{line}");
                }
            });
        }

        let ready = match timeout(STARTUP_TIMEOUT, ready_rx).await {
            Ok(Ok(ready)) => ready,
            Ok(Err(_)) => {
                return Err(Self::startup_failure(
                    &mut child,
                    &runtime_dir,
                    &diagnostics,
                    "sidecar exited before reporting readiness",
                ));
            }
            Err(_) => {
                return Err(Self::startup_failure(
                    &mut child,
                    &runtime_dir,
                    &diagnostics,
                    "sidecar did not report readiness",
                ));
            }
        };
        if ready._protocol != protocol::SIDECAR_PROTOCOL_VERSION {
            return Err(Self::startup_failure(
                &mut child,
                &runtime_dir,
                &diagnostics,
                &format!(
                    "unsupported sidecar protocol {} (expected {})",
                    ready._protocol,
                    protocol::SIDECAR_PROTOCOL_VERSION
                ),
            ));
        }
        if ready.address != socket_env {
            return Err(Self::startup_failure(
                &mut child,
                &runtime_dir,
                &diagnostics,
                &format!(
                    "sidecar reported socket {} instead of {}",
                    ready.address, socket_env
                ),
            ));
        }

        let process = Arc::new(RuntimeProcess {
            address,
            token,
            runtime_dir,
            child: tokio::sync::Mutex::new(child),
        });
        let runtime = Self { process };

        // READY means the socket is bound; the probe exercises the internal
        // client path once so a broken transport fails at startup instead of
        // on the first request.
        let mut probe_error = None;
        for _ in 0..3 {
            let request = InternalRequest {
                method: axum::http::Method::GET,
                uri: protocol::HEALTH_PATH
                    .parse()
                    .expect("health path is a valid uri"),
                headers: Default::default(),
                body: Default::default(),
            };
            match dispatch_internal(&runtime.process.address, &runtime.process.token, request).await
            {
                Ok(response) if response.status == StatusCode::NO_CONTENT => {
                    probe_error = None;
                    break;
                }
                Ok(response) => {
                    probe_error = Some(format!(
                        "health probe returned {}",
                        response.status.as_u16()
                    ));
                }
                Err(error) => {
                    probe_error = Some(error.to_string());
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        if let Some(error) = probe_error {
            let mut child = runtime.process.child.lock().await;
            return Err(Self::startup_failure(
                &mut child,
                &runtime.process.runtime_dir,
                &diagnostics,
                &format!("sidecar health probe failed: {error}"),
            ));
        }

        Ok(runtime)
    }

    /// Kills the sidecar and removes its private runtime directory. Called
    /// after the public listener has already drained.
    pub async fn shutdown(&self) {
        let mut child = self.process.child.lock().await;
        let _ = child.kill().await;
        drop(child);
        let _ = std::fs::remove_dir_all(&self.process.runtime_dir);
    }

    fn startup_failure(
        child: &mut tokio::process::Child,
        runtime_dir: &std::path::Path,
        diagnostics: &Diagnostics,
        reason: &str,
    ) -> ServerError {
        let _ = child.start_kill();
        let _ = std::fs::remove_dir_all(runtime_dir);
        let tail = diagnostics.tail();
        ServerError::message(if tail.is_empty() {
            format!("node application runtime failed: {reason}")
        } else {
            format!("node application runtime failed: {reason}: {tail}")
        })
    }
}

impl RuntimeProcess {
    fn failed(&self, error: impl std::fmt::Display) -> ServerError {
        ServerError::message(format!("application runtime unavailable: {error}"))
    }
}

impl ApplicationRuntime for NodeApplicationRuntime {
    fn dispatch<'a>(
        &'a self,
        request: Request<Body>,
        _context: RequestContext,
    ) -> ApplicationDispatch<'a> {
        Box::pin(async move {
            // The public edge already bounded this body; the ceiling is
            // re-enforced here so the internal boundary never forwards
            // unbounded bytes regardless of the caller.
            let (parts, body) = request.into_parts();
            let bytes = read_bounded_body(&parts.headers, body).await?;
            let request = InternalRequest {
                method: parts.method,
                uri: parts.uri,
                headers: parts.headers,
                body: bytes.into(),
            };
            let internal = match dispatch_internal(
                &self.process.address,
                &self.process.token,
                request,
            )
            .await
            {
                Ok(response) => response,
                Err(error) => return Err(self.process.failed(error)),
            };

            // Only the sidecar's own sentinel (injected for an `undefined`
            // handler result) maps to the host's canonical 404. Application
            // 404s — and any other status — pass through untouched.
            let unhandled = internal.status == StatusCode::NOT_FOUND
                && internal
                    .headers
                    .get(protocol::UNHANDLED_HEADER)
                    .map(|value| value == protocol::UNHANDLED_VALUE)
                    .unwrap_or(false);
            if unhandled {
                return Ok(None);
            }

            let mut response = Response::builder()
                .status(internal.status)
                .body(Body::from(internal.body))
                .expect("status and body cannot fail");
            *response.headers_mut() = internal.headers;
            Ok(Some(response))
        })
    }

    fn render_host<'a>(&'a self, request: HostRenderRequest) -> HostRenderDispatch<'a> {
        Box::pin(async move {
            let body = serde_json::to_vec(&serde_json::json!({
                "provider": request.provider,
                "component": request.component,
                "props": request.props,
            }))
            .map_err(|error| {
                ServerError::message(format!("host render request serialization failed: {error}"))
            })?;
            let internal = dispatch_internal(
                &self.process.address,
                &self.process.token,
                InternalRequest {
                    method: Method::POST,
                    uri: protocol::HOST_RENDER_PATH
                        .parse()
                        .expect("host render path is a valid URI"),
                    headers: Default::default(),
                    body: body.into(),
                },
            )
            .await
            .map_err(|error| self.process.failed(error))?;
            if internal.status == StatusCode::NO_CONTENT {
                return Ok(None);
            }
            if internal.status != StatusCode::OK {
                return Err(self
                    .process
                    .failed(format!("host render returned {}", internal.status)));
            }
            #[derive(serde::Deserialize)]
            struct HostRenderResponse {
                html: String,
            }
            let response: HostRenderResponse =
                serde_json::from_slice(&internal.body).map_err(|_| {
                    self.process
                        .failed("host render returned an invalid response")
                })?;
            if response.html.len() > plec_ir::limits::MAX_PROVIDER_MANIFEST_JSON_BYTES {
                return Err(self
                    .process
                    .failed("host render response exceeds byte limit"));
            }
            Ok(Some(response.html))
        })
    }
}

/// Rolling sidecar output used to make startup failures diagnosable: the
/// supervisor keeps a bounded tail instead of guessing from a dead pipe.
#[derive(Default)]
struct Diagnostics {
    error: Mutex<Option<String>>,
    tail: Mutex<std::collections::VecDeque<String>>,
}

const TAIL_LINES: usize = 16;

impl Diagnostics {
    fn record_error(&self, message: String) {
        *self.error.lock().expect("error lock") = Some(message);
    }

    fn record_diagnostic(&self, line: String) {
        let mut tail = self.tail.lock().expect("tail lock");
        if tail.len() == TAIL_LINES {
            tail.pop_front();
        }
        tail.push_back(line);
    }

    fn tail(&self) -> String {
        let tail = self.tail.lock().expect("tail lock");
        let joined = tail.iter().cloned().collect::<Vec<_>>().join(" ⏎ ");
        let error = self.error.lock().expect("error lock").clone();
        match error {
            Some(error) => format!("{error} | {joined}"),
            None => joined,
        }
    }
}

/// A fresh high-entropy token per child: the entire access boundary for the
/// loopback transport, and cheap defense in depth on Unix sockets.
fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("os randomness is available");
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn create_runtime_dir(token: &str) -> Result<std::path::PathBuf, ServerError> {
    let dir = std::env::temp_dir().join(format!(
        "plec-runtime-{}-{}",
        std::process::id(),
        &token[..12.min(token.len())]
    ));
    std::fs::create_dir_all(&dir).map_err(|error| {
        ServerError::message(format!("cannot create sidecar runtime dir: {error}"))
    })?;
    // The directory, not the socket file, is the access boundary: the node
    // process creates the socket itself under the host's inherited umask,
    // so a private directory is the enforceable guarantee.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).map_err(
            |error| ServerError::message(format!("cannot restrict sidecar runtime dir: {error}")),
        )?;
    }
    Ok(dir)
}

impl Drop for RuntimeProcess {
    fn drop(&mut self) {
        // kill_on_drop covers the child; the directory removal is
        // best-effort here and explicit in `shutdown`.
        let _ = std::fs::remove_dir_all(&self.runtime_dir);
    }
}

/// Convenience: builds host options whose `/api/*` traffic executes in the
/// spawned sidecar.
pub async fn spawn_with_options(
    options: PlecServerOptions,
    runtime: NodeRuntimeOptions,
) -> Result<(PlecServerOptions, NodeApplicationRuntime), ServerError> {
    let runtime = NodeApplicationRuntime::spawn(runtime).await?;
    let mut options = options;
    options.application_runtime = Some(Arc::new(runtime.clone()));
    Ok((options, runtime))
}
