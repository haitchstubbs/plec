//! Compiled document execution: route matching, loaders, SSR, and snapshots.

use http::{header, HeaderMap, HeaderValue, StatusCode, Uri};

use crate::{
    artifact::{self, ArtifactBundle, Manifest},
    http::{RouteExecution, RouteMatch},
    loader::{self, LoaderExecution},
    request::RequestContext,
    runtime::{ApplicationCapabilities, HostRenderRequest},
    ssr, DocumentOptions, ServerError,
};

/// Loads the immutable compiled artifact once for an application instance.
pub async fn load_artifact(path: &std::path::Path) -> Result<ArtifactBundle, ServerError> {
    artifact::read_bounded(path).await
}

/// Execute a document request without constructing an HTTP-host response.
pub async fn execute_document(
    options: &DocumentOptions,
    bundle: &ArtifactBundle,
    context: &mut RequestContext,
    client: &reqwest::Client,
    capabilities: Option<&dyn ApplicationCapabilities>,
) -> Result<crate::DocumentOutcome, ServerError> {
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
                    bundle,
                    route_match.route,
                    context,
                    &params,
                    client,
                )
                .await?;
                let (loader, terminal) = match execution {
                    LoaderExecution::None => (None, None),
                    LoaderExecution::Snapshot(outcome) => (Some(outcome), None),
                    LoaderExecution::Redirect { location, .. } => {
                        (None, Some(LoaderDocumentOutcome::Redirect(location)))
                    }
                    LoaderExecution::NotFound => (
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
                    return redirect_outcome(&destination);
                }
                return render_executions(
                    options,
                    bundle,
                    context,
                    executions,
                    false,
                    capabilities,
                )
                .await;
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
                    return redirect_outcome(&destination);
                }
                let root_not_found = resolve_not_found_boundary(&bundle.manifest, &mut executions);
                if executions.is_empty() && !root_not_found {
                    return Ok(crate::DocumentOutcome::NotFound {
                        headers: json_headers(),
                        html: r#"{"error":"not found"}"#.to_owned(),
                    });
                }
                return render_executions(
                    options,
                    bundle,
                    context,
                    executions,
                    root_not_found,
                    capabilities,
                )
                .await
                .map(|response| match response {
                    crate::DocumentOutcome::Rendered {
                        status: _,
                        headers,
                        html,
                    } => crate::DocumentOutcome::NotFound { headers, html },
                    outcome => outcome,
                });
            }
        }
    }
}

async fn render_executions(
    options: &DocumentOptions,
    bundle: &ArtifactBundle,
    context: &RequestContext,
    executions: Vec<RouteExecution<'_>>,
    root_not_found: bool,
    capabilities: Option<&dyn ApplicationCapabilities>,
) -> Result<crate::DocumentOutcome, ServerError> {
    let tag_policy = plec_ir::sink::TagPolicy {
        custom_elements: options.custom_elements.iter().cloned().collect(),
    };
    let rendered = ssr::render_application(
        bundle,
        &executions,
        context,
        &tag_policy,
        options.development,
        root_not_found,
    )?;
    let body = resolve_host_renders(&rendered.body, &rendered.host_renders, capabilities).await;
    let payload = ssr::bootstrap_payload(bundle, &executions, context, &rendered, root_not_found);
    let bootstrap = payload.map(|payload| {
        serde_json::to_string(&payload)
            .expect("snapshot serialization cannot fail")
            .replace('<', "\\u003c")
    });
    let document = executions
        .last()
        .and_then(|execution| execution.route_match.route.meta.as_ref())
        .map(|meta| crate::DocumentMetadata {
            title: meta.title.clone(),
            description: meta.description.clone(),
        })
        .unwrap_or_else(|| options.document.clone());
    let gating = if options.development {
        &rendered.gating[..]
    } else {
        &[]
    };
    let mut headers = html_headers();
    if !gating.is_empty() {
        if let Ok(value) = HeaderValue::from_str(&gating.join(",")) {
            headers.insert("x-plec-ssr-gating", value);
        }
    }
    Ok(crate::DocumentOutcome::Rendered {
        status: StatusCode::OK.as_u16(),
        headers,
        html: send_html(options, &document, &body, bootstrap.as_deref(), gating),
    })
}

async fn resolve_host_renders(
    body: &str,
    renders: &[ssr::HostRender],
    capabilities: Option<&dyn ApplicationCapabilities>,
) -> String {
    let mut fragments = Vec::with_capacity(renders.len());
    for render in renders {
        let fragment = match capabilities {
            Some(capabilities) => capabilities
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
        body = body.replacen(
            &format!("<!--{}-->", render.placeholder),
            fragment.as_deref().unwrap_or(""),
            1,
        );
    }
    body
}

fn match_route<'a>(manifest: &'a Manifest, pathname: &str) -> Option<Vec<RouteMatch<'a>>> {
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

fn resolve_not_found_boundary(bundle: &Manifest, executions: &mut Vec<RouteExecution<'_>>) -> bool {
    let owner = executions
        .iter()
        .rposition(|execution| execution.route_match.route.not_found_graph_id.is_some());
    match owner {
        Some(index) => {
            executions.truncate(index + 1);
            executions[index].not_found = true;
            true
        }
        None => {
            executions.clear();
            bundle.root_not_found_graph_id.is_some()
        }
    }
}

enum LoaderDocumentOutcome {
    Render,
    Redirect(String),
    NotFound,
}

fn apply_redirect_location(
    context: &mut RequestContext,
    location: &str,
) -> Result<String, ServerError> {
    let uri = location
        .parse::<Uri>()
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

fn redirect_outcome(location: &str) -> Result<crate::DocumentOutcome, ServerError> {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::LOCATION,
        HeaderValue::from_str(location).map_err(|_| {
            ServerError::message("route loader redirect target is not a valid header value")
        })?,
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(crate::DocumentOutcome::Redirect {
        status: StatusCode::TEMPORARY_REDIRECT.as_u16(),
        location: location.to_owned(),
        headers,
    })
}

fn html_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    headers
}

fn json_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers
}

fn send_html(
    options: &DocumentOptions,
    metadata: &crate::DocumentMetadata,
    body: &str,
    bootstrap: Option<&str>,
    _gating: &[String],
) -> String {
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
    let mut styles: String = options
        .client_styles
        .iter()
        .map(|href| {
            format!(
                "<link rel=\"stylesheet\" href=\"{}\">",
                ssr::escape_attribute(href)
            )
        })
        .collect();
    if let Some(href) = options.styles_href.as_deref() {
        styles.push_str(&format!(
            "<link rel=\"stylesheet\" href=\"{}\">",
            ssr::escape_attribute(href)
        ));
    }
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
    let bootstrap = bootstrap
        .map(|payload| {
            format!("<script id=\"plec-bootstrap\" type=\"application/json\">{payload}</script>")
        })
        .unwrap_or_default();
    let description = if description.is_empty() {
        String::new()
    } else {
        format!(
            "<meta name=\"description\" content=\"{}\">",
            ssr::escape_attribute(&description)
        )
    };
    format!("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{title}</title>{description}{preloads}{styles}</head><body><div id=\"app\">{body}</div>{bootstrap}{script}</body></html>")
}
