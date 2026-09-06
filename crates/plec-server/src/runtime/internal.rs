//! Internal HTTP client for the Node sidecar.
//!
//! The boundary is literal HTTP so streamed bodies stay a forward-compatible
//! path; today both sides buffer under the host's documented ceilings.

use axum::{
    body::Bytes,
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode},
};
use hyper::body::Incoming;
use hyper_util::rt::TokioIo;
use serde::Deserialize;

use super::protocol;
use crate::ServerError;

/// Where the sidecar listens. Unix domain sockets are the primary transport;
/// the loopback + token shape keeps the security boundary conceptually
/// equivalent on platforms without them.
#[derive(Debug, Clone)]
pub(crate) enum InternalAddress {
    UnixSocket(std::path::PathBuf),
    // Constructed only on platforms without Unix domain sockets; kept
    // compiled everywhere so the transport contract stays visible.
    #[cfg_attr(unix, allow(dead_code))]
    Tcp(std::net::SocketAddr),
}

/// A buffered internal request. Bodies are already ceiling-checked at the
/// public edge before dispatch, so this layer never re-reads unbounded data.
pub(crate) struct InternalRequest {
    pub method: Method,
    pub uri: axum::http::Uri,
    pub headers: HeaderMap,
    pub body: Bytes,
}

pub(crate) struct InternalResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Bytes,
}

/// Headers that never cross the internal boundary: hop-by-hop machinery the
/// internal connection re-derives itself, plus the unhandled sentinel, which
/// only the sidecar may originate.
fn is_forwardable_header(name: &HeaderName) -> bool {
    !matches!(
        name.as_str(),
        "connection"
            | "content-length"
            | "transfer-encoding"
            | "keep-alive"
            | "upgrade"
            | "te"
            | "trailer"
            | "proxy-connection"
    ) && name != protocol::UNHANDLED_HEADER
}

fn is_forwardable_response_header(name: &HeaderName) -> bool {
    !matches!(
        name.as_str(),
        "connection" | "content-length" | "transfer-encoding" | "keep-alive"
    )
}

/// The structured READY payload (`PLEC_RUNTIME_READY {"protocol":1,...}`).
#[derive(Debug, Deserialize)]
pub(crate) struct SidecarReady {
    #[serde(rename = "protocol")]
    pub _protocol: u32,
    pub address: String,
}

pub(crate) async fn dispatch_internal(
    address: &InternalAddress,
    token: &str,
    request: InternalRequest,
) -> Result<InternalResponse, ServerError> {
    let path = request
        .uri
        .path_and_query()
        .map(|value| value.as_str().to_owned())
        .unwrap_or_else(|| "/".to_owned());
    // The sidecar speaks origin-form (plain path), like every normal HTTP
    // client; the original Host header is forwarded so application handlers
    // observe the public request URL, with a synthetic fallback when the
    // edge saw none.
    let target: hyper::Uri = path
        .parse()
        .map_err(|error| ServerError::message(format!("invalid internal uri: {error}")))?;

    let mut builder = hyper::Request::builder().method(request.method).uri(target);
    {
        let headers = builder
            .headers_mut()
            .expect("request builder has no headers yet");
        for (name, value) in &request.headers {
            if is_forwardable_header(name) {
                headers.append(name.clone(), value.clone());
            }
        }
        if headers.get(axum::http::header::HOST).is_none() {
            headers.insert(
                axum::http::header::HOST,
                HeaderValue::from_static("plec.internal"),
            );
        }
        headers.insert(
            protocol::INTERNAL_TOKEN_HEADER,
            HeaderValue::from_str(token)
                .map_err(|_| ServerError::message("internal token is not a valid header value"))?,
        );
    }
    let request = builder
        .body(http_body_util::Full::new(request.body))
        .map_err(|error| ServerError::message(format!("internal request build failed: {error}")))?;

    let io = connect(address).await?;
    let (mut sender, connection) =
        hyper::client::conn::http1::handshake(io)
            .await
            .map_err(|error| {
                ServerError::message(format!("application runtime unavailable: {error}"))
            })?;
    // One connection per request keeps the client trivially correct; local
    // socket setup is microseconds. Pooling is a later optimization.
    tokio::spawn(async move {
        let _ = connection.await;
    });

    let response = sender.send_request(request).await.map_err(|error| {
        ServerError::message(format!("application runtime unavailable: {error}"))
    })?;

    let status = response.status();
    let mut headers = HeaderMap::new();
    for (name, value) in response.headers() {
        if is_forwardable_response_header(name) {
            headers.append(name.clone(), value.clone());
        }
    }
    // The response body crosses a local trust boundary (the sidecar we
    // spawned), so it is bounded by the process's own memory discipline
    // rather than an untrusted-input ceiling.
    let body = read_body(response.into_body()).await?;
    Ok(InternalResponse {
        status,
        headers,
        body,
    })
}

async fn connect(address: &InternalAddress) -> Result<TokioIo<InternalStream>, ServerError> {
    match address {
        InternalAddress::UnixSocket(path) => {
            let stream = tokio::net::UnixStream::connect(path)
                .await
                .map_err(|error| {
                    ServerError::message(format!("application runtime unavailable: {error}"))
                })?;
            Ok(TokioIo::new(InternalStream::Unix(stream)))
        }
        InternalAddress::Tcp(address) => {
            let stream = tokio::net::TcpStream::connect(address)
                .await
                .map_err(|error| {
                    ServerError::message(format!("application runtime unavailable: {error}"))
                })?;
            Ok(TokioIo::new(InternalStream::Tcp(stream)))
        }
    }
}

/// Reads an incoming body to completion. `Incoming` errors (an aborted
/// connection) surface as runtime-unavailable failures.
async fn read_body(body: Incoming) -> Result<Bytes, ServerError> {
    use http_body_util::BodyExt;
    let collected = body.collect().await.map_err(|error| {
        ServerError::message(format!("application runtime unavailable: {error}"))
    })?;
    Ok(collected.to_bytes())
}

enum InternalStream {
    Unix(tokio::net::UnixStream),
    Tcp(tokio::net::TcpStream),
}

impl tokio::io::AsyncRead for InternalStream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match &mut *self {
            InternalStream::Unix(stream) => std::pin::Pin::new(stream).poll_read(cx, buf),
            InternalStream::Tcp(stream) => std::pin::Pin::new(stream).poll_read(cx, buf),
        }
    }
}

impl tokio::io::AsyncWrite for InternalStream {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match &mut *self {
            InternalStream::Unix(stream) => std::pin::Pin::new(stream).poll_write(cx, buf),
            InternalStream::Tcp(stream) => std::pin::Pin::new(stream).poll_write(cx, buf),
        }
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match &mut *self {
            InternalStream::Unix(stream) => std::pin::Pin::new(stream).poll_flush(cx),
            InternalStream::Tcp(stream) => std::pin::Pin::new(stream).poll_flush(cx),
        }
    }
    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match &mut *self {
            InternalStream::Unix(stream) => std::pin::Pin::new(stream).poll_shutdown(cx),
            InternalStream::Tcp(stream) => std::pin::Pin::new(stream).poll_shutdown(cx),
        }
    }
}
