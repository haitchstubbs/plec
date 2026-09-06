use std::collections::HashMap;

use axum::{
    body::Body,
    http::{header, HeaderMap, Method, Uri},
};

use crate::ServerError;

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
    pub(crate) fn from_parts(
        method: Method,
        uri: &Uri,
        headers: &HeaderMap,
    ) -> Result<Self, ServerError> {
        let host = headers
            .get(header::HOST)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("localhost");
        // The TS host resolved request URLs against `http://{host}`; the
        // native host terminates TLS at a proxy in the same deployments, so
        // the scheme stays an explicit host detail.
        let path_and_query = uri
            .path_and_query()
            .map(|value| value.as_str().to_owned())
            .unwrap_or_else(|| "/".to_owned());
        let url = format!("http://{host}{path_and_query}");
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

/// Reads and bounds an inbound body before any application dispatch. The
/// declared `content-length` fails fast exactly like the streaming check in
/// the TS host; the buffered ceiling catches lying or absent declarations.
pub(crate) async fn read_bounded_body(
    headers: &HeaderMap,
    body: Body,
) -> Result<Vec<u8>, ServerError> {
    use plec_ir::limits::MAX_REQUEST_BODY_BYTES;
    if let Some(declared) = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
    {
        if declared > MAX_REQUEST_BODY_BYTES as u64 {
            return Err(ServerError::RequestBodyTooLarge);
        }
    }
    let bytes = axum::body::to_bytes(body, MAX_REQUEST_BODY_BYTES)
        .await
        .map_err(|_| ServerError::RequestBodyTooLarge)?;
    Ok(bytes.to_vec())
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

fn parse_query(query: &str) -> Result<HashMap<String, QueryValue>, ServerError> {
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
