use std::collections::HashMap;

use axum::{
    body::Body,
    extract::State,
    http::{Method, Request, Response, StatusCode},
};
use serde_json::json;

use crate::{
    artifact::{self, Manifest, Route},
    assets, loader,
    request::RequestContext,
    runtime::HostRenderRequest,
    ssr, DocumentMetadata, PlecServerOptions, ServerError, ServerState,
};

pub(crate) struct RouteMatch<'a> {
    pub route: &'a Route,
    pub params: HashMap<String, String>,
}

pub(crate) struct RouteExecution<'a> {
    pub route_match: RouteMatch<'a>,
    pub loader: Option<plec_ir::SsrLoaderOutcome>,
}

pub(crate) async fn dispatch(
    State(state): State<ServerState>,
    request: Request<Body>,
) -> Response<Body> {
    match dispatch_inner(&state, request).await {
        Ok(response) => response,
        Err(error) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &json!({ "error": format!("Plec server failed: {error}") }),
        ),
    }
}

async fn dispatch_inner(
    state: &ServerState,
    request: Request<Body>,
) -> Result<Response<Body>, ServerError> {
    let (parts, body) = request.into_parts();
    // Inbound request bytes are untrusted: every non-GET/HEAD body is read
    // under the documented ceiling before any application dispatch, so
    // oversized or malformed bodies fail before app handlers or SSR run.
    let request = if parts.method == Method::GET || parts.method == Method::HEAD {
        Request::from_parts(parts, body)
    } else {
        let bytes = match crate::request::read_bounded_body(&parts.headers, body).await {
            Ok(bytes) => bytes,
            // Oversized or malformed request bodies fail before any app
            // dispatch.
            Err(error) => {
                return Ok(json_response(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    &json!({ "error": error.to_string() }),
                ));
            }
        };
        Request::from_parts(parts, Body::from(bytes))
    };
    let mut context =
        RequestContext::from_parts(request.method().clone(), request.uri(), request.headers())?;

    if context.pathname.starts_with("/api/") {
        return handle_api(state, request, context).await;
    }

    if is_document_request(&context.pathname) {
        return render_document(state, &mut context).await;
    }

    Ok(assets::serve(state, request).await)
}

async fn handle_api(
    state: &ServerState,
    request: Request<Body>,
    context: RequestContext,
) -> Result<Response<Body>, ServerError> {
    let not_found = || {
        json_response(
            StatusCode::NOT_FOUND,
            &json!({ "error": "endpoint not found" }),
        )
    };
    let Some(runtime) = state.options.application_runtime.as_deref() else {
        return Ok(not_found());
    };
    match runtime.dispatch(request, context).await? {
        Some(response) => Ok(response),
        None => Ok(not_found()),
    }
}

async fn render_document(
    state: &ServerState,
    context: &mut RequestContext,
) -> Result<Response<Body>, ServerError> {
    match render_document_inner(state, context).await {
        Ok(response) => Ok(response),
        Err(error) => {
            // A fixture without compiler artifacts remains useful for
            // HTTP-host tests. Real Plec builds always supply the artifact
            // and therefore take the SSR path above.
            let shell = tokio::fs::read(state.options.public_dir.join("index.html")).await;
            match shell {
                Ok(shell) => {
                    let mut response = Response::new(Body::from(shell));
                    let headers = response.headers_mut();
                    headers.insert(
                        axum::http::header::CONTENT_TYPE,
                        axum::http::HeaderValue::from_static("text/html; charset=utf-8"),
                    );
                    headers.insert(
                        axum::http::header::CACHE_CONTROL,
                        axum::http::HeaderValue::from_static("no-cache"),
                    );
                    if state.options.development {
                        if let Ok(value) = axum::http::HeaderValue::from_str(&error.to_string()) {
                            headers.insert("x-plec-ssr-fallback", value);
                        }
                    }
                    Ok(response)
                }
                Err(_) => Ok(json_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &json!({ "error": format!("Plec SSR failed: {error}") }),
                )),
            }
        }
    }
}

async fn render_document_inner(
    state: &ServerState,
    context: &mut RequestContext,
) -> Result<Response<Body>, ServerError> {
    let bundle = artifact::read_bounded(&state.options.artifact_path).await?;
    let matched = match_route(&bundle.manifest, &context.pathname);
    // SSR markup and app handlers must observe the same matched params the
    // browser snapshot carries, so the request context stops dropping them.
    let mut executions = Vec::new();
    if let Some(matched) = matched {
        for route_match in matched {
            context.params = route_match.params.clone();
            let loader =
                loader::execute_route_loader(&bundle, route_match.route, context, &state.http)
                    .await?;
            executions.push(RouteExecution {
                route_match,
                loader,
            });
        }
    }
    let tag_policy = plec_ir::sink::TagPolicy {
        custom_elements: state.options.custom_elements.iter().cloned().collect(),
    };
    let rendered = ssr::render_application(
        &bundle,
        &executions,
        context,
        &tag_policy,
        state.options.development,
    )?;
    let body = resolve_host_renders(
        &rendered.body,
        &rendered.host_renders,
        state.options.application_runtime.as_deref(),
    )
    .await;
    let payload = ssr::bootstrap_payload(&bundle, &executions, context, &rendered);
    // No bootstrap means nothing to resume: the browser mounts fresh.
    let bootstrap = payload.map(|payload| {
        serde_json::to_string(&payload)
            .expect("snapshot serialization cannot fail")
            .replace('<', "\\u003c")
    });
    let document = executions
        .last()
        .and_then(|execution| execution.route_match.route.meta.as_ref())
        .map(|meta| DocumentMetadata {
            title: meta.title.clone(),
            description: meta.description.clone(),
        })
        .unwrap_or_else(|| state.options.document.clone());
    let gating = if state.options.development {
        rendered.gating.clone()
    } else {
        Vec::new()
    };
    Ok(send_html(
        &state.options,
        &document,
        &body,
        bootstrap.as_deref(),
        &gating,
    ))
}

/// Resolves provider fragments only through the private application runtime.
/// A missing/inert provider — or any sidecar failure — leaves an empty owned
/// boundary, preserving the prior CSR mount contract instead of failing the
/// whole document. Replacements run in reverse marker order so one trusted
/// provider fragment cannot contain a later placeholder that is accidentally
/// substituted as another provider's result.
async fn resolve_host_renders(
    body: &str,
    renders: &[ssr::HostRender],
    runtime: Option<&dyn crate::ApplicationRuntime>,
) -> String {
    let mut fragments = Vec::with_capacity(renders.len());
    for render in renders {
        let fragment = match runtime {
            Some(runtime) => runtime
                .render_host(HostRenderRequest {
                    provider: render.provider.clone(),
                    component: render.component.clone(),
                    props: render.props.clone(),
                })
                .await
                .ok()
                .flatten(),
            None => None,
        };
        fragments.push(fragment);
    }
    let mut body = body.to_owned();
    for (render, fragment) in renders.iter().zip(fragments).rev() {
        let marker = format!("<!--{}-->", render.placeholder);
        body = body.replacen(&marker, fragment.as_deref().unwrap_or(""), 1);
    }
    body
}

/// Application route matching is not HTTP routing: it resolves the compiled
/// manifest's `$param` segments against the request path, preferring static
/// segments and treating a catch-all as a fallback, never a competing match
/// for the index route.
pub(crate) fn match_route<'a>(
    manifest: &'a Manifest,
    pathname: &str,
) -> Option<Vec<RouteMatch<'a>>> {
    let routing_manifest = manifest.routing_manifest();
    plec_schema::routing::match_route_chain(&routing_manifest, pathname).and_then(|chain| {
        chain
            .into_iter()
            .map(|matched| {
                manifest
                    .routes
                    .iter()
                    .find(|route| route.id == matched.route.id)
                    .map(|route| RouteMatch {
                        route,
                        params: matched.params,
                    })
            })
            .collect()
    })
}

fn is_document_request(pathname: &str) -> bool {
    pathname == "/" || std::path::Path::new(pathname).extension().is_none()
}

pub(crate) fn json_response(status: StatusCode, value: &serde_json::Value) -> Response<Body> {
    let body = serde_json::to_string(value).unwrap_or_else(|_| "{}".to_owned());
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/json; charset=utf-8"),
    );
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    response
}

fn send_html(
    options: &PlecServerOptions,
    metadata: &DocumentMetadata,
    body: &str,
    bootstrap: Option<&str>,
    gating: &[String],
) -> Response<Body> {
    let title = ssr::escape_html(metadata.title.as_deref().unwrap_or("Plec application"));
    let description = ssr::escape_html(metadata.description.as_deref().unwrap_or(""));
    let preloads: String = options
        .preloads
        .iter()
        .map(|href| {
            format!(
                "<link rel=\"preload\" as=\"font\" type=\"font/woff2\" crossorigin href=\"{}\">",
                ssr::escape_attribute(href)
            )
        })
        .collect();
    let styles = options
        .styles_href
        .as_deref()
        .map(|href| {
            format!(
                "<link rel=\"stylesheet\" href=\"{}\">",
                ssr::escape_attribute(href)
            )
        })
        .unwrap_or_default();
    let script = options
        .client_script
        .as_deref()
        .map(|src| {
            format!(
                "<script type=\"module\" src=\"{}\"></script>",
                ssr::escape_attribute(src)
            )
        })
        .unwrap_or_default();
    let bootstrap_script = bootstrap
        .map(|payload| {
            format!("<script id=\"plec-bootstrap\" type=\"application/json\">{payload}</script>")
        })
        .unwrap_or_default();
    let description_meta = if description.is_empty() {
        String::new()
    } else {
        format!(
            "<meta name=\"description\" content=\"{}\">",
            ssr::escape_attribute(&description)
        )
    };
    let html = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" \
         content=\"width=device-width, initial-scale=1\"><title>{title}</title>{description_meta}\
         {preloads}{styles}</head><body><div id=\"app\">{body}</div>{bootstrap_script}{script}\
         </body></html>",
    );
    let mut response = Response::new(Body::from(html));
    let headers = response.headers_mut();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("text/html; charset=utf-8"),
    );
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-cache"),
    );
    // x-plec-ssr-fallback-style observability: development hosts learn which
    // server-only host loads were gated out of the public artifacts.
    if !gating.is_empty() {
        if let Ok(value) = axum::http::HeaderValue::from_str(&gating.join(",")) {
            headers.insert("x-plec-ssr-gating", value);
        }
    }
    response
}
