use axum::body::Body;
use axum::http::{Request, Response, StatusCode};
use tower::ServiceExt;
use tower_http::services::ServeDir;

use crate::{ServerState, http};

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
    let client_path = request.uri().path().strip_prefix("/_plec/");
    let service = if let Some(path) = client_path {
        let path_and_query = format!(
            "/{path}{}",
            request
                .uri()
                .query()
                .map(|q| format!("?{q}"))
                .unwrap_or_default()
        );
        let mut request = request;
        match path_and_query.parse() {
            Ok(uri) => *request.uri_mut() = uri,
            Err(_) => {
                return http::json_response(
                    StatusCode::BAD_REQUEST,
                    &serde_json::json!({ "error": "invalid asset path" }),
                );
            }
        }
        state.client_assets.clone().oneshot(request).await
    } else if request.uri().path() == "/_plec" {
        return http::json_response(
            StatusCode::NOT_FOUND,
            &serde_json::json!({ "error": "asset not found" }),
        );
    } else {
        state.assets.clone().oneshot(request).await
    };
    match service {
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
