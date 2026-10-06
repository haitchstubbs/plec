//! The native HTTP host for compiled Plec applications. It owns HTTP
//! mechanics only; application API handlers are an explicit temporary escape
//! hatch and are not part of Plec's semantic server model. SSR execution,
//! loader execution, and snapshot construction are shared `plec-ir` /
//! renderer concerns, so the native server cannot drift from the runtime the
//! way the TypeScript host it replaces did.

pub use plec_server_engine::artifact;
pub mod assets;
pub mod http;
pub use plec_server_engine::loader;
pub mod manifest;
pub use plec_server_engine::request;
pub mod runtime;
pub use plec_server_engine::ssr;

use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use axum::Router;

pub use runtime::{
    ApplicationRuntime, NodeApplicationRuntime, NodeRuntimeOptions, ServerActionRequest,
};

pub use plec_server_engine::{DocumentMetadata, ServerError};

/// The application-runtime boundary: the production implementation is a
/// Node sidecar ([`NodeApplicationRuntime`]); tests and embedders can wire
/// any [`ApplicationRuntime`], including plain closures of the
/// [`runtime::AppRequestHandler`] shape.
#[derive(Clone)]
pub struct PlecServerOptions {
    pub public_dir: PathBuf,
    /// Plec-generated browser artifacts, exposed only below `/_plec/`.
    pub client_dir: PathBuf,
    /// Rust compiler output written by the application build.
    pub artifact_path: PathBuf,

    pub client_script: Option<String>,
    pub client_styles: Vec<String>,
    pub styles_href: Option<String>,
    /// Font URLs (same origin, under `public_dir`) emitted as `<link
    /// rel="preload" as="font">` before the stylesheet so variable-font woff2
    /// files start downloading during CSS parse instead of after it.
    pub preloads: Vec<String>,

    /// Fallback document metadata used when the matched route declares none.
    pub document: DocumentMetadata,

    /// Trusted custom element tags the SSR serializer may serialize from
    /// executable IR. Sourced from the server manifest (the application
    /// build's `plec.toml`); empty keeps the strict default policy.
    pub custom_elements: Vec<String>,

    pub application_runtime: Option<Arc<dyn ApplicationRuntime>>,

    pub development: bool,
}

#[derive(Clone)]
pub(crate) struct ServerState {
    pub(crate) options: Arc<PlecServerOptions>,
    /// Static asset service; precompressed sidecar support is configured by
    /// `assets::service`.
    pub(crate) assets: tower_http::services::ServeDir,
    pub(crate) client_assets: tower_http::services::ServeDir,
    /// Shared route-loader fetch client.
    pub(crate) http: reqwest::Client,
}

pub fn create_plec_server(options: PlecServerOptions) -> Router {
    let assets = assets::service(&options.public_dir);
    let client_assets = assets::service(&options.client_dir);
    let state = ServerState {
        options: Arc::new(options),
        assets,
        client_assets,
        http: reqwest::Client::new(),
    };

    Router::new().fallback(http::dispatch).with_state(state)
}

pub async fn serve(router: Router, addr: SocketAddr) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;

    axum::serve(listener, router).await
}
