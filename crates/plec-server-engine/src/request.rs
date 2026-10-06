//! HTTP-independent Plec request context and encoded path semantics.

use std::collections::HashMap;

use http::{header, HeaderMap, Method, Uri};

use crate::ServerError;

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // Consumed by the incoming raw-target adapter in the Node host.
pub(crate) struct CanonicalRequestTarget {
    pub path: String,
    pub query: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteClass {
    Api,
    Action,
    Document,
    Static,
}

/// Extracts the encoded route path without URL normalization or percent
/// decoding. This is the Rust counterpart of `@plec/node`'s ingress contract.
#[allow(dead_code)] // Kept alongside the shared fixtures until Node owns ingress.
pub(crate) fn canonical_request_target(
    target: &str,
) -> Result<CanonicalRequestTarget, ServerError> {
    if target.is_empty()
        || target
            .bytes()
            .any(|byte| byte <= b' ' || byte == 0x7f || byte == b'#' || byte == b'\\')
    {
        return Err(ServerError::message("malformed HTTP request target"));
    }
    let bytes = target.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let valid = bytes
                .get(index + 1..index + 3)
                .is_some_and(|hex| hex.iter().all(u8::is_ascii_hexdigit));
            if !valid {
                return Err(ServerError::message("malformed percent escape"));
            }
            index += 3;
        } else {
            index += 1;
        }
    }

    let path_and_query = if target.starts_with('/') {
        target.to_owned()
    } else {
        let uri = target
            .parse::<Uri>()
            .map_err(|_| ServerError::message("malformed absolute request target"))?;
        let scheme = uri.scheme_str().filter(|scheme| {
            scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
        });
        let authority = uri.authority();
        if scheme.is_none()
            || authority.is_none_or(|value| {
                value.host().is_empty() || !valid_request_authority(value.as_str())
            })
        {
            return Err(ServerError::message("unsupported request-target form"));
        }
        uri.path_and_query()
            .map(|value| value.as_str().to_owned())
            .unwrap_or_else(|| "/".to_owned())
    };
    let (path, query) = path_and_query
        .split_once('?')
        .map(|(path, query)| (path, query))
        .unwrap_or((path_and_query.as_str(), ""));
    let path = if path.is_empty() { "/" } else { path };
    if !path.starts_with('/') {
        return Err(ServerError::message("request path must start with slash"));
    }
    Ok(CanonicalRequestTarget {
        path: path.to_owned(),
        query: query.to_owned(),
    })
}

fn valid_request_authority(authority: &str) -> bool {
    if authority.is_empty() || authority.contains('@') {
        return false;
    }
    let port = if authority.starts_with('[') {
        let Some(end) = authority.find(']') else {
            return false;
        };
        if authority[1..end].parse::<std::net::Ipv6Addr>().is_err() {
            return false;
        }
        let suffix = &authority[end + 1..];
        if suffix.is_empty() {
            None
        } else if let Some(port) = suffix.strip_prefix(':') {
            Some(port)
        } else {
            return false;
        }
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        if host.is_empty() || host.contains(':') {
            return false;
        }
        Some(port)
    } else {
        None
    };
    match port {
        Some(port) => !port.is_empty() && port.parse::<u16>().is_ok(),
        None => true,
    }
}

pub fn classify_plec_path(path: &str) -> RouteClass {
    if path == "/api" || path.starts_with("/api/") {
        return RouteClass::Api;
    }
    if path.starts_with("/_plec/actions/") {
        return RouteClass::Action;
    }
    if path == "/" || !path.rsplit('/').next().unwrap_or_default().contains('.') {
        RouteClass::Document
    } else {
        RouteClass::Static
    }
}

/// The request context both SSR and `/api/*` handlers observe. Headers stay
/// typed (`HeaderMap`) because HTTP semantics, including duplicate headers,
/// are already correct; only the query map converts into Plec values.
#[derive(Debug, Clone)]
pub struct RequestContext {
    pub url: String,
    pub pathname: String,
    pub method: Method,
    pub headers: HeaderMap,
    pub cookies: HashMap<String, String>,
    pub params: HashMap<String, String>,
    pub query: HashMap<String, QueryValue>,
}

#[derive(Debug, Clone)]
pub enum QueryValue {
    One(String),
    Many(Vec<String>),
}

impl RequestContext {
    pub fn from_parts(method: Method, uri: &Uri, headers: &HeaderMap) -> Result<Self, ServerError> {
        let host = headers
            .get(header::HOST)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("localhost");
        // The native host commonly terminates TLS at a proxy. Honor its
        // forwarded scheme when present, while keeping HTTP as the default.
        let scheme = headers
            .get("x-forwarded-proto")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(',').next())
            .map(str::trim)
            .filter(|scheme| matches!(*scheme, "http" | "https"))
            .unwrap_or("http");
        let path_and_query = uri
            .path_and_query()
            .map(|value| value.as_str().to_owned())
            .unwrap_or_else(|| "/".to_owned());
        let url = format!("{scheme}://{host}{path_and_query}");
        let cookies = parse_cookies(
            headers
                .get(header::COOKIE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default(),
        )?;
        let query = parse_query(uri.query().unwrap_or_default())?;
        Ok(Self {
            url,
            pathname: uri.path().to_owned(),
            method,
            headers: headers.clone(),
            cookies,
            params: HashMap::new(),
            query,
        })
    }
}

fn parse_cookies(header: &str) -> Result<HashMap<String, String>, ServerError> {
    let mut cookies = HashMap::new();
    for part in header.split(';') {
        let Some(index) = part.find('=') else {
            continue;
        };
        let name = part[..index].trim();
        let value = decode_uri_component(part[index + 1..].trim())?;
        cookies.insert(name.to_owned(), value);
    }
    Ok(cookies)
}

pub fn parse_query(query: &str) -> Result<HashMap<String, QueryValue>, ServerError> {
    let mut values: Vec<(String, Vec<String>)> = Vec::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (name, value) = match pair.find('=') {
            Some(index) => (&pair[..index], &pair[index + 1..]),
            None => (pair, ""),
        };
        let (name, value) = (decode_form_component(name)?, decode_form_component(value)?);
        match values.iter_mut().find(|(existing, _)| *existing == name) {
            Some((_, existing)) => existing.push(value),
            None => values.push((name, vec![value])),
        }
    }
    // One key observed once decodes to a scalar; repeated keys collect in
    // order, mirroring `URLSearchParams.getAll`.
    Ok(values
        .into_iter()
        .map(|(name, mut values)| {
            let value = if values.len() == 1 {
                QueryValue::One(values.pop().expect("single value"))
            } else {
                QueryValue::Many(values)
            };
            (name, value)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_url_uses_forwarded_https_and_host_port() {
        let uri: Uri = "/before?tab=old".parse().unwrap();
        let headers = HeaderMap::from_iter([
            (header::HOST, "app.example.test:8443".parse().unwrap()),
            (
                "x-forwarded-proto".parse().unwrap(),
                "https".parse().unwrap(),
            ),
        ]);
        let context = RequestContext::from_parts(Method::GET, &uri, &headers).unwrap();

        assert_eq!(context.url, "https://app.example.test:8443/before?tab=old");
    }

    #[test]
    fn query_parser_keeps_repeated_values_ordered_and_form_decodes() {
        let query = parse_query("tag=one&tag=two+words&empty").unwrap();
        assert!(matches!(
            query.get("tag"),
            Some(QueryValue::Many(values)) if values == &["one", "two words"]
        ));
        assert!(matches!(query.get("empty"), Some(QueryValue::One(value)) if value.is_empty()));
        assert!(parse_query("bad=%").is_err());
        assert!(parse_query("bad=%FF").is_err());
    }

    #[test]
    fn canonical_request_targets_match_shared_node_host_fixtures() {
        #[derive(serde::Deserialize)]
        struct Fixture {
            target: String,
            path: Option<String>,
            query: Option<String>,
            class: Option<String>,
            invalid: Option<bool>,
        }

        let fixtures: Vec<Fixture> = serde_json::from_str(include_str!(
            "../../../testdata/node-host/request-targets.json"
        ))
        .unwrap();
        for fixture in fixtures {
            let parsed = canonical_request_target(&fixture.target);
            if fixture.invalid.unwrap_or(false) {
                assert!(parsed.is_err(), "accepted {:?}", fixture.target);
                continue;
            }
            let parsed =
                parsed.unwrap_or_else(|error| panic!("rejected {:?}: {error}", fixture.target));
            assert_eq!(Some(parsed.path.clone()), fixture.path);
            assert_eq!(Some(parsed.query), fixture.query.or(Some(String::new())));
            let class = match classify_plec_path(&parsed.path) {
                RouteClass::Api => "api",
                RouteClass::Action => "action",
                RouteClass::Document => "document",
                RouteClass::Static => "static",
            };
            assert_eq!(
                Some(class),
                fixture.class.as_deref(),
                "{:?}",
                fixture.target
            );
        }
    }

    #[test]
    fn canonical_encoded_path_is_matched_without_separator_or_dot_normalization() {
        let manifest = plec_schema::routing::RouteManifest {
            version: None,
            root_graph_id: "root".into(),
            root_not_found_graph_id: None,
            routes: ["foo/bar", "foo%2Fbar", "b"]
                .into_iter()
                .enumerate()
                .map(|(index, path)| plec_schema::routing::RouteManifestEntry {
                    id: format!("route-{index}"),
                    parent_id: None,
                    path: path.into(),
                    graph_id: format!("graph-{index}"),
                    pending_graph_id: None,
                    error_graph_id: None,
                    not_found_graph_id: None,
                    outlet_id: format!("outlet-{index}"),
                    loader_action: None,
                    pending_mode: "replace".into(),
                })
                .collect(),
        };

        let encoded = canonical_request_target("/foo%2Fbar").unwrap();
        let matched = plec_schema::routing::match_route_chain(&manifest, &encoded.path).unwrap();
        assert_eq!(matched[0].route.id, "route-1");

        let dotted = canonical_request_target("/a/../b").unwrap();
        assert!(plec_schema::routing::match_route_chain(&manifest, &dotted.path).is_none());
    }
}

/// Strict `decodeURIComponent`: `%` escapes must be complete hex pairs and
/// decode to valid UTF-8, matching the TS host's throwing decoder.
pub(crate) fn decode_uri_component(value: &str) -> Result<String, ServerError> {
    decode_component(value, false)
}

/// `URLSearchParams` decoding additionally treats `+` as a space.
fn decode_form_component(value: &str) -> Result<String, ServerError> {
    decode_component(value, true)
}

fn decode_component(value: &str, plus_as_space: bool) -> Result<String, ServerError> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                let hex = bytes
                    .get(index + 1..index + 3)
                    .ok_or_else(|| ServerError::message("malformed percent escape"))?;
                let hex = std::str::from_utf8(hex)
                    .map_err(|_| ServerError::message("malformed percent escape"))?;
                let byte = u8::from_str_radix(hex, 16)
                    .map_err(|_| ServerError::message("malformed percent escape"))?;
                decoded.push(byte);
                index += 3;
            }
            b'+' if plus_as_space => {
                decoded.push(b' ');
                index += 1;
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(decoded).map_err(|_| ServerError::message("request data is not UTF-8"))
}
