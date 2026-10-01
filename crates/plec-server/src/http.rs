use std::collections::HashMap;

use axum::{
    body::Body,
    extract::State,
    http::{Method, Request, Response, StatusCode},
};
use serde_json::json;

use crate::{
    DocumentMetadata, PlecServerOptions, ServerError, ServerState,
    artifact::{self, Manifest, Route},
    assets, loader,
    request::RequestContext,
    runtime::HostRenderRequest,
    ssr,
};

pub(crate) struct RouteMatch<'a> {
    pub route: &'a Route,
    pub params: HashMap<String, String>,
}

pub(crate) struct RouteExecution<'a> {
    pub route_match: RouteMatch<'a>,
    pub loader: Option<plec_ir::SsrLoaderOutcome>,
    /// The instance rendered its not-found boundary (a loader in its subtree
    /// produced a not-found outcome and this route owns the boundary).
    pub not_found: bool,
}

pub(crate) async fn dispatch(
    State(state): State<ServerState>,
    request: Request<Body>,
) -> Response<Body> {
    match dispatch_inner(&state, request).await {
        Ok(response) => response,
        Err(error) => {
            eprintln!("Plec server request failed: {error}");
            json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &json!({ "error": "Internal Server Error" }),
            )
        }
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

    if context.pathname.starts_with("/_plec/actions/") {
        return handle_server_action(state, request, context).await;
    }

    if is_document_request(&context.pathname) {
        return render_document(state, &mut context).await;
    }

    Ok(assets::serve(state, request).await)
}

async fn handle_server_action(
    state: &ServerState,
    request: Request<Body>,
    context: RequestContext,
) -> Result<Response<Body>, ServerError> {
    let fail = |status, message: &str| json_response(status, &json!({"error": message}));
    if request.method() != Method::POST {
        return Ok(fail(StatusCode::METHOD_NOT_ALLOWED, "method not allowed"));
    }
    // Credential-bearing action POSTs are same-origin only. Browsers send
    // Origin for fetch POST; compare its authority against the request host.
    let origin = context
        .headers
        .get(axum::http::header::ORIGIN)
        .and_then(|v| v.to_str().ok());
    let expected = context.url.split('/').take(3).collect::<Vec<_>>().join("/");
    if origin != Some(expected.as_str()) {
        return Ok(fail(
            StatusCode::FORBIDDEN,
            "same-origin action POST required",
        ));
    }
    let Some(id) = context
        .pathname
        .strip_prefix("/_plec/actions/")
        .filter(|id| {
            !id.is_empty()
                && !id.contains('/')
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        })
    else {
        return Ok(fail(StatusCode::NOT_FOUND, "unknown server action"));
    };
    let (parts, body) = request.into_parts();
    let bytes = match crate::request::read_bounded_body(&parts.headers, body).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return Ok(fail(
                StatusCode::PAYLOAD_TOO_LARGE,
                "server action request exceeds limit",
            ));
        }
    };
    let arguments: Vec<plec_schema::RuntimeValue> = match serde_json::from_slice(&bytes) {
        Ok(arguments) => arguments,
        Err(_) => {
            return Ok(fail(
                StatusCode::BAD_REQUEST,
                "invalid server action arguments",
            ));
        }
    };
    if arguments.len() > plec_ir::limits::MAX_COMPONENT_COLLECTION_LEN {
        return Ok(fail(
            StatusCode::BAD_REQUEST,
            "server action argument count exceeds limit",
        ));
    }
    if arguments
        .iter()
        .any(|argument| argument.check_limits().is_err())
    {
        return Ok(fail(
            StatusCode::BAD_REQUEST,
            "server action arguments exceed value limits",
        ));
    }
    let Some(runtime) = state.options.application_runtime.as_deref() else {
        return Ok(fail(
            StatusCode::SERVICE_UNAVAILABLE,
            "server actions unavailable",
        ));
    };
    match runtime
        .invoke_action(crate::runtime::ServerActionRequest {
            id: id.to_owned(),
            arguments,
            context,
        })
        .await
    {
        Ok(value) => {
            if value.check_limits().is_err() {
                return Ok(fail(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "server action result exceeds value limits",
                ));
            }
            let value = serde_json::to_value(value).unwrap_or(serde_json::Value::Null);
            Ok(json_response(StatusCode::OK, &value))
        }
        Err(error) => {
            if matches!(error, ServerError::UnknownServerAction) {
                return Ok(fail(
                    StatusCode::NOT_FOUND,
                    "unknown or stale server action",
                ));
            }
            eprintln!("Plec server action failed: {error}");
            Ok(fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "server action failed",
            ))
        }
    }
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
            eprintln!("Plec SSR render failed: {error}");
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
                    &json!({ "error": "Internal Server Error" }),
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
    // Loader redirects answer 307 so the browser URL, the re-requested
    // document, and the snapshot location stay one consistent destination.
    // The hops are followed internally only to bound the chain: loops fail
    // with a diagnostic instead of bouncing the browser forever.
    let mut hops = 0usize;
    let mut destination = String::new();
    loop {
        let matched = match_route(&bundle.manifest, &context.pathname);
        let mut executions = Vec::new();
        let mut outcome = LoaderDocumentOutcome::Render;
        if let Some(matched) = matched {
            'chain: for route_match in matched {
                let params: Vec<(String, String)> = route_match
                    .params
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect();
                context.params = route_match.params.clone();
                let execution = loader::execute_route_loader(
                    &bundle,
                    route_match.route,
                    context,
                    &params,
                    &state.http,
                )
                .await?;
                // A not-found origin must still join the execution list so
                // boundary resolution can see it; redirects never commit.
                let (loader, terminal) = match execution {
                    loader::LoaderExecution::None => (None, None),
                    loader::LoaderExecution::Snapshot(outcome) => (Some(outcome), None),
                    loader::LoaderExecution::Redirect { location, .. } => {
                        (None, Some(LoaderDocumentOutcome::Redirect(location)))
                    }
                    loader::LoaderExecution::NotFound => (
                        Some(plec_ir::SsrLoaderOutcome {
                            graph_id: route_match.route.graph_id.clone(),
                            action: route_match.route.loader_action.unwrap_or_default(),
                            state: plec_ir::SsrLoaderState::NotFound,
                        }),
                        Some(LoaderDocumentOutcome::NotFound),
                    ),
                };
                executions.push(RouteExecution {
                    route_match,
                    loader,
                    not_found: false,
                });
                if let Some(terminal) = terminal {
                    outcome = terminal;
                    break 'chain;
                }
            }
        }
        match outcome {
            LoaderDocumentOutcome::Render => {
                if hops > 0 {
                    return redirect_response(&destination);
                }
                return render_executions(state, context, &bundle, executions, false).await;
            }
            LoaderDocumentOutcome::Redirect(location) => {
                hops += 1;
                if hops > plec_ir::limits::MAX_REDIRECT_HOPS {
                    return Err(ServerError::message(format!(
                        "route loader redirect loop exceeded {} hops at {location}",
                        plec_ir::limits::MAX_REDIRECT_HOPS
                    )));
                }
                destination = apply_redirect_location(context, &location)?;
            }
            LoaderDocumentOutcome::NotFound => {
                if hops > 0 {
                    // The redirect target resolves as not found; answer the
                    // redirect so the destination URL owns the 404 response.
                    return redirect_response(&destination);
                }
                return render_not_found_document(state, context, &bundle, executions).await;
            }
        }
    }
}

/// One 307 hop to a loader redirect destination. Documents are GET requests,
/// so 307 preserves semantics while keeping the browser URL authoritative.
fn redirect_response(location: &str) -> Result<Response<Body>, ServerError> {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::TEMPORARY_REDIRECT;
    let headers = response.headers_mut();
    let value = axum::http::HeaderValue::from_str(location).map_err(|_| {
        ServerError::message("route loader redirect target is not a valid header value")
    })?;
    headers.insert(axum::http::header::LOCATION, value);
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}

enum LoaderDocumentOutcome {
    Render,
    Redirect(String),
    NotFound,
}

/// Applies a validated redirect target to the request context so the next hop
/// matches, seeds loaders, and snapshots against the destination URL. Returns
/// the canonical destination path (the 307 `Location` value).
fn apply_redirect_location(
    context: &mut RequestContext,
    location: &str,
) -> Result<String, ServerError> {
    let uri = location
        .parse::<axum::http::Uri>()
        .map_err(|_| ServerError::message("route loader redirect target is malformed"))?;
    if uri.scheme().is_some() || uri.authority().is_some() || location.contains(['#', '\\']) {
        return Err(ServerError::message(
            "route loader redirects must use an absolute application path without a fragment",
        ));
    }
    let path = uri.path();
    if !path.starts_with('/') || path.starts_with("//") {
        return Err(ServerError::message(
            "route loader redirects must use an absolute application path",
        ));
    }
    let query = uri.query();
    context.query = crate::request::parse_query(query.unwrap_or_default())?;
    context.pathname = path.to_owned();
    // Rebuild the absolute URL against the original host.
    let (scheme, host) = context
        .url
        .split_once("://")
        .map(|(scheme, rest)| (scheme, rest.split('/').next().unwrap_or("localhost")))
        .unwrap_or(("http", "localhost"));
    context.url = match query {
        Some(query) => format!("{scheme}://{host}{path}?{query}"),
        None => format!("{scheme}://{host}{path}"),
    };
    Ok(match query {
        Some(query) => format!("{path}?{query}"),
        None => path.to_owned(),
    })
}

/// Selects the not-found boundary owner for a not-found outcome raised
/// somewhere in `executions` (the chain executed up to the origin). The owner
/// is the deepest matched route declaring `notFoundComponent`, else the root
/// boundary. The chain truncates at the owner: descendants never commit.
fn resolve_not_found_boundary(
    bundle: &artifact::Manifest,
    executions: &mut Vec<RouteExecution<'_>>,
) -> bool {
    let owner = executions
        .iter()
        .rposition(|execution| execution.route_match.route.not_found_graph_id.is_some());
    match owner {
        Some(index) => {
            executions.truncate(index + 1);
            executions[index].not_found = true;
            true
        }
        // Root boundary: the persistent root renders the boundary and no
        // child route instance commits.
        None => {
            executions.clear();
            bundle.root_not_found_graph_id.is_some()
        }
    }
}

async fn render_not_found_document(
    state: &ServerState,
    context: &mut RequestContext,
    bundle: &artifact::ArtifactBundle,
    mut executions: Vec<RouteExecution<'_>>,
) -> Result<Response<Body>, ServerError> {
    let root_not_found = resolve_not_found_boundary(&bundle.manifest, &mut executions);
    if executions.is_empty() && !root_not_found {
        // No notFoundComponent anywhere: the built-in minimal 404 body.
        return Ok(json_response(
            StatusCode::NOT_FOUND,
            &json!({ "error": "not found" }),
        ));
    }
    let response = render_executions(state, context, bundle, executions, root_not_found).await?;
    let (mut parts, body) = response.into_parts();
    parts.status = StatusCode::NOT_FOUND;
    Ok(Response::from_parts(parts, body))
}

async fn render_executions(
    state: &ServerState,
    context: &mut RequestContext,
    bundle: &artifact::ArtifactBundle,
    executions: Vec<RouteExecution<'_>>,
    root_not_found: bool,
) -> Result<Response<Body>, ServerError> {
    let tag_policy = plec_ir::sink::TagPolicy {
        custom_elements: state.options.custom_elements.iter().cloned().collect(),
    };
    let rendered = ssr::render_application(
        bundle,
        &executions,
        context,
        &tag_policy,
        state.options.development,
        root_not_found,
    )?;
    let body = resolve_host_renders(
        &rendered.body,
        &rendered.host_renders,
        state.options.application_runtime.as_deref(),
    )
    .await;
    let payload = ssr::bootstrap_payload(bundle, &executions, context, &rendered, root_not_found);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::QueryValue;

    #[test]
    fn internal_redirect_preserves_https_and_non_default_port() {
        let mut context = RequestContext {
            url: "https://app.example.test:8443/before?old=1".into(),
            pathname: "/before".into(),
            method: Method::GET,
            headers: Default::default(),
            cookies: HashMap::new(),
            params: HashMap::new(),
            query: HashMap::from([("old".into(), QueryValue::One("1".into()))]),
        };

        let location = apply_redirect_location(&mut context, "/after?tab=2").unwrap();

        assert_eq!(location, "/after?tab=2");
        assert_eq!(context.url, "https://app.example.test:8443/after?tab=2");
        assert_eq!(context.pathname, "/after");
    }

    #[test]
    fn rejects_malformed_or_non_local_redirect_targets() {
        let context = || RequestContext {
            url: "https://app.example.test/before".into(),
            pathname: "/before".into(),
            method: Method::GET,
            headers: Default::default(),
            cookies: HashMap::new(),
            params: HashMap::new(),
            query: HashMap::new(),
        };

        for target in [
            "",
            "relative/path",
            "//outside.example/path",
            "https://outside.example/path",
            "/bad path",
            "/ok?bad=%",
            "/ok#fragment",
        ] {
            assert!(
                apply_redirect_location(&mut context(), target).is_err(),
                "accepted invalid redirect target {target:?}"
            );
        }
    }
}
