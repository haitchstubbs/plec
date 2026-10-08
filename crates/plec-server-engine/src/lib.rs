//! Host-independent execution of compiled Plec server semantics.
//!
//! This crate deliberately has no dependency on HTTP host frameworks or N-API.
//! The Node host adapts transport into these semantic inputs and outputs.

use std::path::PathBuf;

pub mod action;
pub mod artifact;
pub mod document;
pub mod loader;
pub mod request;
pub mod ssr;

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("request body exceeds byte limit")]
    RequestBodyTooLarge,
    #[error("application artifact exceeds byte limit")]
    ArtifactTooLarge,
    #[error("unknown or stale server action")]
    UnknownServerAction,
    #[error("application callback capacity is exhausted")]
    CallbackCapacity,
    #[error("{0}")]
    Other(String),
}

impl ServerError {
    pub fn message(message: impl Into<String>) -> Self {
        Self::Other(message.into())
    }
}

impl From<std::io::Error> for ServerError {
    fn from(error: std::io::Error) -> Self {
        Self::Other(error.to_string())
    }
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct DocumentMetadata {
    pub title: Option<String>,
    pub description: Option<String>,
}

/// Configuration needed by compiled document execution. Static-file and
/// public-listener settings remain owned by each host.
#[derive(Debug, Clone)]
pub struct DocumentOptions {
    pub artifact_path: PathBuf,
    pub client_script: Option<String>,
    pub client_styles: Vec<String>,
    pub styles_href: Option<String>,
    pub preloads: Vec<String>,
    pub custom_elements: Vec<String>,
    pub document: DocumentMetadata,
    pub development: bool,
}

pub mod http {
    use std::collections::HashMap;

    use crate::artifact::Route;

    pub struct RouteMatch<'a> {
        pub route: &'a Route,
        pub params: HashMap<String, String>,
    }

    pub struct RouteExecution<'a> {
        pub route_match: RouteMatch<'a>,
        pub loader: Option<plec_ir::SsrLoaderOutcome>,
        pub not_found: bool,
    }
}

pub mod runtime {
    #[derive(Debug, Clone)]
    pub struct HostRenderRequest {
        pub provider: String,
        pub component: String,
        pub props: serde_json::Value,
    }

    pub type HostRenderFuture<'a> = std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<Option<String>, crate::ServerError>>
                + Send
                + 'a,
        >,
    >;

    pub trait ApplicationCapabilities: Send + Sync {
        fn render_host<'a>(&'a self, _request: HostRenderRequest) -> HostRenderFuture<'a> {
            Box::pin(async { Ok(None) })
        }
    }

    pub type ActionFuture<'a> = std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<plec_schema::RuntimeValue, crate::ServerError>>
                + Send
                + 'a,
        >,
    >;

    pub trait ActionCapabilities: Send + Sync {
        fn invoke_action<'a>(
            &'a self,
            request: crate::action::ServerActionRequest,
        ) -> ActionFuture<'a>;
    }
}

/// Semantic document response. HTTP-specific response construction belongs
/// to the host adapter; the current renderer still produces complete HTML.
#[derive(Debug, Clone)]
pub enum DocumentOutcome {
    Rendered {
        status: u16,
        headers: ::http::HeaderMap,
        html: String,
    },
    Redirect {
        status: u16,
        location: String,
        headers: ::http::HeaderMap,
    },
    NotFound {
        headers: ::http::HeaderMap,
        html: String,
    },
}
