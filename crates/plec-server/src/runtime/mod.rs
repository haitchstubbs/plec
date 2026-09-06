//! The application runtime boundary: how `/api/*` requests reach executable
//! server code.
//!
//! Plec's own semantics (route loaders, SSR expressions, snapshots) never
//! cross this boundary — those execute in Rust. This trait carries only the
//! requests that genuinely require the application's own JavaScript runtime.

use std::{future::Future, pin::Pin, sync::Arc};

use axum::{
    body::Body,
    http::{Request, Response},
};

use crate::{request::RequestContext, ServerError};

pub(crate) mod internal;
pub mod node;
pub(crate) mod protocol;

pub use node::{NodeApplicationRuntime, NodeRuntimeOptions};

/// A server-side execution capability for application code. The production
/// implementation is [`NodeApplicationRuntime`] (a Node.js sidecar over a
/// private socket); tests and embedders can supply anything else.
///
/// Resolving `None` means no handler accepted the request; the host turns
/// that into its canonical JSON 404. A runtime error is a `ServerError` and
/// becomes the dispatcher's structured 500.
/// The boxed future every runtime dispatch resolves into.
pub type ApplicationDispatch<'a> =
    Pin<Box<dyn Future<Output = Result<Option<Response<Body>>, ServerError>> + Send + 'a>>;

pub trait ApplicationRuntime: Send + Sync + 'static {
    fn dispatch<'a>(
        &'a self,
        request: Request<Body>,
        context: RequestContext,
    ) -> ApplicationDispatch<'a>;
}

/// The closure shape for in-process application handlers (tests, embedders
/// wiring a Rust-native runtime). Closures of this shape implement
/// [`ApplicationRuntime`] directly.
pub type AppRequestHandler = Arc<
    dyn Fn(
            Request<Body>,
            RequestContext,
        ) -> Pin<Box<dyn Future<Output = Option<Response<Body>>> + Send>>
        + Send
        + Sync,
>;

impl<F> ApplicationRuntime for F
where
    F: Fn(
            Request<Body>,
            RequestContext,
        ) -> Pin<Box<dyn Future<Output = Option<Response<Body>>> + Send>>
        + Send
        + Sync
        + 'static,
{
    fn dispatch<'a>(
        &'a self,
        request: Request<Body>,
        context: RequestContext,
    ) -> ApplicationDispatch<'a> {
        Box::pin(async move { Ok(self(request, context).await) })
    }
}

/// The handler-alias form: `Arc<dyn Fn …>` is itself a concrete type, so it
/// implements the runtime trait directly. Wrap once with `Arc::new(handler)`
/// to store it as `Arc<dyn ApplicationRuntime>`.
impl ApplicationRuntime for AppRequestHandler {
    fn dispatch<'a>(
        &'a self,
        request: Request<Body>,
        context: RequestContext,
    ) -> ApplicationDispatch<'a> {
        Box::pin(async move { Ok(self(request, context).await) })
    }
}
