use axum::body::Body;
use axum::http::{Request, Response, StatusCode};
use tower::ServiceExt;
use tower_http::services::ServeDir;

use crate::{http, ServerState};

/// The precompressed-sidecar model: a `<file>.br` / `<file>.gz` sibling is
/// served negotiated by `Accept-Encoding`; its presence is the opt-in, and
/// the producer that emits the asset must also regenerate its sidecar.
/// Assets without a sidecar fall back to identity bytes. Path-traversal
/// containment is the service's own contract.
pub(crate) fn service(public_dir: &std::path::Path) -> ServeDir {
    ServeDir::new(public_dir)
        .precompressed_br()
        .precompressed_gzip()
}

pub(crate) async fn serve(state: &ServerState, request: Request<Body>) -> Response<Body> {
    match state.assets.clone().oneshot(request).await {
        Ok(mut response) => {
            let status = response.status();
            if status == StatusCode::NOT_FOUND {
                return http::json_response(
                    StatusCode::NOT_FOUND,
                    &serde_json::json!({ "error": "asset not found" }),
                );
            }
            if status.is_success() {
                response.headers_mut().insert(
                    axum::http::header::CACHE_CONTROL,
                    axum::http::HeaderValue::from_static("no-cache"),
                );
            }
            response.map(axum::body::Body::new)
        }
        // The service itself failed (unresolvable path): fail closed.
        Err(_) => http::json_response(
            StatusCode::BAD_REQUEST,
            &serde_json::json!({ "error": "invalid asset path" }),
        ),
    }
}
