//! Host-independent server-action validation and execution.

use bytes::Bytes;
use futures_util::Stream;
use http::{header, HeaderMap, Method, Uri};
use plec_schema::RuntimeValue;
use serde_json::json;

use crate::{request::RequestContext, runtime::ActionCapabilities};

#[derive(Debug, Clone)]
pub struct ServerActionRequest {
    pub id: String,
    pub arguments: Vec<RuntimeValue>,
    pub context: RequestContext,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ActionOutcome {
    Success(RuntimeValue),
    MethodNotAllowed,
    SameOriginRequired,
    UnknownAction,
    InvalidArguments(&'static str),
    RequestTooLarge,
    RequestStreamFailed,
    Overloaded,
    ResultTooLarge,
    Failed,
}

impl ActionOutcome {
    pub fn status(&self) -> u16 {
        match self {
            Self::Success(_) => 200,
            Self::MethodNotAllowed => 405,
            Self::SameOriginRequired => 403,
            Self::UnknownAction => 404,
            Self::InvalidArguments(_) | Self::RequestStreamFailed => 400,
            Self::RequestTooLarge => 413,
            Self::Overloaded => 503,
            Self::ResultTooLarge | Self::Failed => 500,
        }
    }

    pub fn body(&self) -> serde_json::Value {
        match self {
            Self::Success(value) => serde_json::to_value(value).unwrap_or(serde_json::Value::Null),
            Self::MethodNotAllowed => json!({"error": "method not allowed"}),
            Self::SameOriginRequired => json!({"error": "same-origin action POST required"}),
            Self::UnknownAction => json!({"error": "unknown server action"}),
            Self::InvalidArguments(message) => json!({"error": message}),
            Self::RequestStreamFailed => json!({"error": "invalid server action arguments"}),
            Self::RequestTooLarge => json!({"error": "server action request exceeds limit"}),
            Self::Overloaded => json!({"error": "service unavailable"}),
            Self::ResultTooLarge => json!({"error": "server action result exceeds value limits"}),
            Self::Failed => json!({"error": "server action failed"}),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BoundedBodyError {
    #[error("request body exceeds byte limit")]
    TooLarge,
    #[error("request body stream failed")]
    Stream,
}

pub async fn read_bounded_stream<S, E>(
    mut stream: S,
    declared_content_length: Option<u64>,
    limit: usize,
) -> Result<Vec<u8>, BoundedBodyError>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
{
    use futures_util::StreamExt;
    if declared_content_length.is_some_and(|length| length > limit as u64) {
        return Err(BoundedBodyError::TooLarge);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| BoundedBodyError::Stream)?;
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err(BoundedBodyError::TooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

pub async fn execute_action<S, E>(
    context: RequestContext,
    declared_content_length: Option<u64>,
    body: S,
    capabilities: &dyn ActionCapabilities,
) -> ActionOutcome
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
{
    if context.method != Method::POST {
        return ActionOutcome::MethodNotAllowed;
    }
    if !same_origin(&context.url, &context.headers) {
        return ActionOutcome::SameOriginRequired;
    }
    let Some(id) = context
        .pathname
        .strip_prefix("/_plec/actions/")
        .filter(|id| {
            !id.is_empty()
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        })
    else {
        return ActionOutcome::UnknownAction;
    };

    let bytes = match read_bounded_stream(
        body,
        declared_content_length,
        plec_ir::limits::MAX_REQUEST_BODY_BYTES,
    )
    .await
    {
        Ok(bytes) => bytes,
        Err(BoundedBodyError::TooLarge) => return ActionOutcome::RequestTooLarge,
        Err(BoundedBodyError::Stream) => return ActionOutcome::RequestStreamFailed,
    };
    let arguments: Vec<RuntimeValue> = match serde_json::from_slice(&bytes) {
        Ok(arguments) => arguments,
        Err(_) => return ActionOutcome::InvalidArguments("invalid server action arguments"),
    };
    if arguments.len() > plec_ir::limits::MAX_COMPONENT_COLLECTION_LEN {
        return ActionOutcome::InvalidArguments("server action argument count exceeds limit");
    }
    if arguments
        .iter()
        .any(|argument| argument.check_limits().is_err())
    {
        return ActionOutcome::InvalidArguments("server action arguments exceed value limits");
    }
    match capabilities
        .invoke_action(ServerActionRequest {
            id: id.to_owned(),
            arguments,
            context,
        })
        .await
    {
        Ok(value) if value.check_limits().is_ok() => ActionOutcome::Success(value),
        Ok(_) => ActionOutcome::ResultTooLarge,
        Err(error) if matches!(error, crate::ServerError::UnknownServerAction) => {
            ActionOutcome::UnknownAction
        }
        Err(error) if matches!(error, crate::ServerError::CallbackCapacity) => {
            ActionOutcome::Overloaded
        }
        Err(_) => ActionOutcome::Failed,
    }
}

fn same_origin(request_url: &str, headers: &HeaderMap) -> bool {
    let mut values = headers.get_all(header::ORIGIN).iter();
    let Some(origin) = values.next().and_then(|value| value.to_str().ok()) else {
        return false;
    };
    if values.next().is_some() {
        return false;
    }
    let Ok(request) = request_url.parse::<Uri>() else {
        return false;
    };
    let Ok(origin) = origin.parse::<Uri>() else {
        return false;
    };
    if origin
        .path_and_query()
        .is_some_and(|path| path.as_str() != "/")
    {
        return false;
    }
    if origin
        .authority()
        .is_some_and(|authority| authority.as_str().contains('@'))
    {
        return false;
    }
    let Some(scheme) = origin
        .scheme_str()
        .filter(|scheme| matches!(*scheme, "http" | "https"))
    else {
        return false;
    };
    let (Some(request_scheme), Some(request_authority), Some(origin_authority)) = (
        request.scheme_str(),
        request.authority(),
        origin.authority(),
    ) else {
        return false;
    };
    let request_host = request_authority.host();
    let origin_host = origin_authority.host();
    let default_port = |scheme: &str| if scheme == "https" { 443 } else { 80 };
    let effective_port =
        |uri: &Uri, scheme: &str| uri.port_u16().unwrap_or_else(|| default_port(scheme));
    request_scheme == scheme
        && request_host.eq_ignore_ascii_case(origin_host)
        && effective_port(&request, request_scheme) == effective_port(&origin, scheme)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    #[tokio::test]
    async fn bounded_reader_stops_polling_after_overflow() {
        let polls = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&polls);
        let body = futures_util::stream::iter(vec![
            Ok::<_, ()>(Bytes::from_static(b"123")),
            Ok(Bytes::from_static(b"456")),
            Ok(Bytes::from_static(b"unread")),
        ])
        .inspect(move |_| {
            observed.fetch_add(1, Ordering::SeqCst);
        });
        let result = read_bounded_stream(body, None, 5).await;
        assert!(matches!(result, Err(BoundedBodyError::TooLarge)));
        assert_eq!(polls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn declared_oversize_rejects_without_polling() {
        let polls = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&polls);
        let body = futures_util::stream::iter(vec![Ok::<_, ()>(Bytes::from_static(b"body"))])
            .inspect(move |_| {
                observed.fetch_add(1, Ordering::SeqCst);
            });
        let result = read_bounded_stream(body, Some(6), 5).await;
        assert!(matches!(result, Err(BoundedBodyError::TooLarge)));
        assert_eq!(polls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn origin_comparison_is_structural_and_rejects_duplicates() {
        let headers = |origins: &[&str]| {
            let mut headers = HeaderMap::new();
            for origin in origins {
                headers.append(header::ORIGIN, (*origin).parse().unwrap());
            }
            headers
        };
        assert!(same_origin(
            "https://[::1]:443/_plec/actions/x",
            &headers(&["https://[::1]"])
        ));
        assert!(!same_origin(
            "https://example.test/_plec/actions/x",
            &headers(&["null"])
        ));
        assert!(!same_origin(
            "https://example.test/_plec/actions/x",
            &headers(&["https://example.test", "https://example.test"])
        ));
        assert!(!same_origin(
            "https://example.test/_plec/actions/x",
            &headers(&["https://user@example.test"])
        ));
    }
}
