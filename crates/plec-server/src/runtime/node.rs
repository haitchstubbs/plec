//! The production [`ApplicationRuntime`]: a Node.js sidecar that imports the
//! application's server bundle and serves it over a private socket.
//!
//! The sidecar is a capability of the host, never the public server: Rust
//! owns the socket, the lifecycle, and every byte ceiling; Node executes the
//! application code for which Node semantics actually matter.

use std::{
    io::{self, Write},
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
    time::{Duration, timeout},
};

use super::{
    internal::{InternalAddress, InternalRequest, dispatch_internal},
    protocol,
};
use crate::{
    ApplicationRuntime, PlecServerOptions, ServerError,
    request::{RequestContext, read_bounded_body},
    runtime::{
        ApplicationDispatch, HostRenderDispatch, HostRenderRequest, ServerActionDispatch,
        ServerActionRequest,
    },
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
    /// Include captured sidecar detail in host diagnostics.
    pub development: bool,
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
            development: false,
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
        let development = options.development;
        let token = generate_token();
        let runtime_dir = create_runtime_dir(&token).map_err(|error| {
            ServerError::message(format!(
                "[PLEC-SIDECAR-STARTUP] sidecar: cannot prepare private runtime directory: {error}"
            ))
        })?;

        #[cfg(unix)]
        let address = InternalAddress::UnixSocket(runtime_dir.join("app.sock"));
        #[cfg(not(unix))]
        let address = {
            // Loopback only, ephemeral port; the token is the access
            // boundary, never the bind address.
            let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|error| {
                ServerError::message(format!(
                    "[PLEC-SIDECAR-STARTUP] sidecar: cannot reserve private port: {error}"
                ))
            })?;
            let address = listener.local_addr().map_err(|error| {
                ServerError::message(format!(
                    "[PLEC-SIDECAR-STARTUP] sidecar: cannot reserve private port: {error}"
                ))
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
                ServerError::message(format!(
                    "[PLEC-SIDECAR-STARTUP] sidecar: cannot spawn Node process: {error}"
                ))
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
                    // stdout carries ordinary application logs as well as
                    // the READY/error protocol records. Forward it in every
                    // environment; only captured startup detail is
                    // development-gated below.
                    let _ = forward_sidecar_log_line(&mut io::stdout().lock(), &line);
                }
            });
        }
        if let Some(stderr) = child.stderr.take() {
            let mut stderr = tokio::io::BufReader::new(stderr).lines();
            let diagnostics = Arc::clone(&diagnostics);
            tokio::spawn(async move {
                while let Ok(Some(line)) = stderr.next_line().await {
                    diagnostics.record_diagnostic(line.clone());
                    // stderr is the application process log stream. Forward
                    // it unchanged; only the bounded tail included in a
                    // structured startup diagnostic is development-only.
                    let _ = forward_sidecar_log_line(&mut io::stderr().lock(), &line);
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
                    development,
                    "sidecar exited before reporting readiness",
                ));
            }
            Err(_) => {
                return Err(Self::startup_failure(
                    &mut child,
                    &runtime_dir,
                    &diagnostics,
                    development,
                    "sidecar did not report readiness",
                ));
            }
        };
        if ready._protocol != protocol::SIDECAR_PROTOCOL_VERSION {
            return Err(Self::startup_failure(
                &mut child,
                &runtime_dir,
                &diagnostics,
                development,
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
                development,
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
                development,
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
        development: bool,
        reason: &str,
    ) -> ServerError {
        let process_status = child
            .try_wait()
            .ok()
            .flatten()
            .map(|status| format!("; sidecar process exited with {status}"))
            .unwrap_or_default();
        let _ = child.start_kill();
        let _ = std::fs::remove_dir_all(runtime_dir);
        let tail = if development {
            diagnostics.tail()
        } else {
            String::new()
        };
        let (code, phase) =
            if reason.contains("protocol") || reason.contains("sidecar reported socket") {
                ("PLEC-SIDECAR-PROTOCOL", "protocol")
            } else {
                ("PLEC-SIDECAR-STARTUP", "sidecar")
            };
        ServerError::message(if tail.is_empty() {
            format!("[{code}] {phase}: sidecar startup failed: {reason}{process_status}")
        } else {
            format!("[{code}] {phase}: sidecar startup failed: {reason}{process_status}: {tail}")
        })
    }
}

impl RuntimeProcess {
    async fn failed(&self, error: impl std::fmt::Display) -> ServerError {
        let process_status = self
            .child
            .lock()
            .await
            .try_wait()
            .ok()
            .flatten()
            .map(|status| format!("; sidecar process exited with {status}"))
            .unwrap_or_default();
        ServerError::message(format!(
            "[PLEC-SIDECAR-REQUEST] sidecar: application runtime unavailable: {error}{process_status}"
        ))
    }
}

fn sidecar_protocol_error(message: impl std::fmt::Display) -> ServerError {
    ServerError::message(format!(
        "[PLEC-SIDECAR-PROTOCOL] protocol: invalid sidecar response: {message}"
    ))
}

fn forward_sidecar_log_line(writer: &mut impl Write, line: &str) -> io::Result<()> {
    writeln!(writer, "{line}")
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
                Err(error) => return Err(self.process.failed(error).await),
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
            let internal = match dispatch_internal(
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
            {
                Ok(response) => response,
                Err(error) => return Err(self.process.failed(error).await),
            };
            if internal.status == StatusCode::NO_CONTENT {
                return Ok(None);
            }
            if internal.status != StatusCode::OK {
                return Err(ServerError::message(format!(
                    "[PLEC-PROVIDER-RENDER] provider: server render failed (sidecar returned {})",
                    internal.status
                )));
            }
            #[derive(serde::Deserialize)]
            struct HostRenderResponse {
                html: String,
            }
            let response: HostRenderResponse = serde_json::from_slice(&internal.body)
                .map_err(|_| sidecar_protocol_error("host render returned invalid JSON"))?;
            if response.html.len() > plec_ir::limits::MAX_PROVIDER_MANIFEST_JSON_BYTES {
                return Err(sidecar_protocol_error(
                    "host render response exceeds byte limit",
                ));
            }
            Ok(Some(response.html))
        })
    }

    fn invoke_action<'a>(&'a self, request: ServerActionRequest) -> ServerActionDispatch<'a> {
        Box::pin(async move {
            let body = serde_json::to_vec(&serde_json::json!({
                "id": request.id,
                "arguments": request.arguments,
                "context": request_context_json(&request.context),
            }))
            .map_err(|error| {
                ServerError::message(format!(
                    "server action request serialization failed: {error}"
                ))
            })?;
            let internal = match dispatch_internal(
                &self.process.address,
                &self.process.token,
                InternalRequest {
                    method: Method::POST,
                    uri: protocol::ACTION_PATH.parse().expect("action path is valid"),
                    headers: Default::default(),
                    body: body.into(),
                },
            )
            .await
            {
                Ok(response) => response,
                Err(error) => return Err(self.process.failed(error).await),
            };
            if internal.status != StatusCode::OK {
                if internal.status == StatusCode::NOT_FOUND {
                    return Err(ServerError::UnknownServerAction);
                }
                return Err(ServerError::message(format!(
                    "[PLEC-SERVER-ACTION] action: application action failed (sidecar returned {})",
                    internal.status
                )));
            }
            #[derive(serde::Deserialize)]
            struct ActionResponse {
                ok: bool,
                value: Option<serde_json::Value>,
            }
            let response: ActionResponse = serde_json::from_slice(&internal.body)
                .map_err(|_| sidecar_protocol_error("server action returned invalid JSON"))?;
            if !response.ok {
                return Err(ServerError::message("server action failed"));
            }
            let value: plec_schema::RuntimeValue =
                serde_json::from_value(response.value.ok_or_else(|| {
                    sidecar_protocol_error("server action response omitted value")
                })?)
                .map_err(|_| {
                    sidecar_protocol_error("server action returned an unsupported value")
                })?;
            value
                .check_limits()
                .map_err(|_| sidecar_protocol_error("server action result exceeds value limits"))?;
            Ok(value)
        })
    }
}

fn request_context_json(context: &RequestContext) -> serde_json::Value {
    // The public TS contract is a string record, so duplicate field values are
    // intentionally flattened to the first value in HeaderMap's stored order.
    // Use an ordered map for deterministic serialization and never expose the
    // sidecar's private authentication credential through application context.
    let headers = context
        .headers
        .keys()
        .filter(|name| name.as_str() != super::protocol::INTERNAL_TOKEN_HEADER)
        .filter_map(|name| {
            context
                .headers
                .get_all(name)
                .iter()
                .next()
                .and_then(|value| value.to_str().ok())
                .map(|value| {
                    (
                        name.as_str().to_owned(),
                        serde_json::Value::String(value.to_owned()),
                    )
                })
        })
        .collect::<serde_json::Map<_, _>>();
    let query = context
        .query
        .iter()
        .map(|(name, value)| {
            let value = match value {
                crate::request::QueryValue::One(value) => serde_json::Value::String(value.clone()),
                crate::request::QueryValue::Many(values) => serde_json::json!(values),
            };
            (name.clone(), value)
        })
        .collect::<serde_json::Map<_, _>>();
    serde_json::json!({
        "url": context.url,
        "pathname": context.pathname,
        "method": context.method.as_str(),
        "headers": headers,
        "cookies": context.cookies,
        "params": context.params,
        "query": query,
    })
}

#[cfg(test)]
mod request_context_tests {
    use super::*;

    #[test]
    fn request_context_hides_sidecar_token_and_flattens_duplicate_headers() {
        let mut headers = axum::http::HeaderMap::new();
        headers.append("authorization", "Bearer first".parse().unwrap());
        headers.append("authorization", "Bearer second".parse().unwrap());
        headers.insert(
            super::protocol::INTERNAL_TOKEN_HEADER,
            "private-token".parse().unwrap(),
        );
        let context = RequestContext {
            url: "https://example.test/".into(),
            pathname: "/".into(),
            method: Method::GET,
            headers,
            cookies: Default::default(),
            params: Default::default(),
            query: Default::default(),
        };
        let json = request_context_json(&context);
        assert!(!json.to_string().contains("private-token"));
        assert!(
            json["headers"]
                .get(super::protocol::INTERNAL_TOKEN_HEADER)
                .is_none()
        );
        assert_eq!(json["headers"]["authorization"], "Bearer first");
    }
}

#[cfg(test)]
mod sidecar_log_forwarding_tests {
    use super::forward_sidecar_log_line;

    #[test]
    fn ordinary_sidecar_output_is_forwarded_without_a_development_gate() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        forward_sidecar_log_line(&mut stdout, "app console.log").unwrap();
        forward_sidecar_log_line(&mut stderr, "app console.error").unwrap();
        assert_eq!(stdout, b"app console.log\n");
        assert_eq!(stderr, b"app console.error\n");
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
