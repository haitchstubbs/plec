use std::sync::{Arc, Mutex};

use futures_util::stream;
use napi::{
    bindgen_prelude::{BufferSlice, Function, Promise, ReadableStream},
    threadsafe_function::ThreadsafeFunction,
    Env, Status,
};
use napi_derive::napi;
use plec_server_engine::runtime::{ApplicationCapabilities, HostRenderFuture, HostRenderRequest};
use plec_server_engine::{
    artifact::ArtifactBundle, request::RequestContext, DocumentMetadata, DocumentOptions,
};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

#[napi(object)]
#[derive(Clone)]
pub struct NativeApplicationOptions {
    pub artifact_path: String,
    pub client_script: Option<String>,
    pub client_styles: Option<Vec<String>>,
    pub styles_href: Option<String>,
    pub preloads: Option<Vec<String>>,
    pub custom_elements: Option<Vec<String>>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub development: Option<bool>,
}

#[napi(object)]
#[derive(Clone)]
pub struct NativeHeader {
    pub name: String,
    pub value: String,
}

#[napi(object)]
pub struct NativeRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<NativeHeader>,
}

type HostRenderCallback = ThreadsafeFunction<String, Promise<String>, String, Status, false>;

struct CallbackManager {
    render_host: Arc<HostRenderCallback>,
}

#[napi]
pub struct NativeCallbacks {
    inner: Mutex<Option<CallbackManager>>,
}

#[napi]
pub fn create_callbacks(
    render_host: Function<'_, String, Promise<String>>,
) -> napi::Result<NativeCallbacks> {
    let callback = render_host
        .build_threadsafe_function()
        .callee_handled::<false>()
        .build()?;
    Ok(NativeCallbacks {
        inner: Mutex::new(Some(CallbackManager {
            render_host: Arc::new(callback),
        })),
    })
}

impl ApplicationCapabilities for CallbackManager {
    fn render_host<'a>(&'a self, request: HostRenderRequest) -> HostRenderFuture<'a> {
        Box::pin(async move {
            let value = serde_json::json!({
                "provider": request.provider,
                "component": request.component,
                "props": request.props,
            });
            let promise = self
                .render_host
                .call_async_catch(value.to_string())
                .await
                .map_err(|error| plec_server_engine::ServerError::message(error.to_string()))?;
            let html = promise
                .await
                .map_err(|error| plec_server_engine::ServerError::message(error.to_string()))?;
            if html.len() > 1024 * 1024 {
                return Err(plec_server_engine::ServerError::message(
                    "host render exceeds limit",
                ));
            }
            Ok(Some(html))
        })
    }
}

struct Snapshot {
    options: DocumentOptions,
    artifact: ArtifactBundle,
    client: reqwest::Client,
    callbacks: CallbackManager,
}

struct Lifecycle {
    closing: bool,
    closed: bool,
    active: usize,
    snapshot: Option<Arc<Snapshot>>,
}

struct Inner {
    lifecycle: Mutex<Lifecycle>,
    drained: Notify,
    cancellation: CancellationToken,
}

#[napi]
pub struct PlecApplication {
    inner: Arc<Inner>,
}

#[napi]
impl NativeCallbacks {
    #[napi]
    pub async fn load_application(
        &self,
        options: NativeApplicationOptions,
    ) -> napi::Result<PlecApplication> {
        let callbacks = self
            .inner
            .lock()
            .map_err(|_| napi::Error::from_reason("callback manager poisoned"))?
            .take()
            .ok_or_else(|| napi::Error::from_reason("callback manager already consumed"))?;
        let path = std::fs::canonicalize(&options.artifact_path).map_err(binding_error)?;
        let artifact = plec_server_engine::document::load_artifact(&path)
            .await
            .map_err(binding_error)?;
        let snapshot = Snapshot {
            options: DocumentOptions {
                artifact_path: path,
                client_script: options.client_script,
                client_styles: options.client_styles.unwrap_or_default(),
                styles_href: options.styles_href,
                preloads: options.preloads.unwrap_or_default(),
                custom_elements: options.custom_elements.unwrap_or_default(),
                document: DocumentMetadata {
                    title: options.title,
                    description: options.description,
                },
                development: options.development.unwrap_or(false),
            },
            artifact,
            client: reqwest::Client::new(),
            callbacks,
        };
        Ok(PlecApplication {
            inner: Arc::new(Inner {
                lifecycle: Mutex::new(Lifecycle {
                    closing: false,
                    closed: false,
                    active: 0,
                    snapshot: Some(Arc::new(snapshot)),
                }),
                drained: Notify::new(),
                cancellation: CancellationToken::new(),
            }),
        })
    }
}

#[napi]
impl PlecApplication {
    #[napi]
    pub async fn handle_document(
        &self,
        request: NativeRequest,
    ) -> napi::Result<NativeDocumentResponse> {
        let (snapshot, cancellation) = {
            let mut lifecycle = self
                .inner
                .lifecycle
                .lock()
                .map_err(|_| napi::Error::from_reason("application lifecycle poisoned"))?;
            if lifecycle.closing || lifecycle.closed {
                return Err(napi::Error::from_reason("PLEC_APPLICATION_CLOSED"));
            }
            lifecycle.active += 1;
            let snapshot = lifecycle
                .snapshot
                .as_ref()
                .cloned()
                .ok_or_else(|| napi::Error::from_reason("PLEC_APPLICATION_CLOSED"))?;
            (snapshot, self.inner.cancellation.child_token())
        };
        let active = ActiveOperation {
            inner: Arc::clone(&self.inner),
        };
        let result = tokio::select! {
            _ = cancellation.cancelled() => Err(napi::Error::from_reason("PLEC_APPLICATION_CLOSED")),
            result = execute(snapshot, request) => result,
        };
        drop(active);
        result
    }

    #[napi]
    pub async fn close(&self) -> napi::Result<()> {
        {
            let mut lifecycle = self
                .inner
                .lifecycle
                .lock()
                .map_err(|_| napi::Error::from_reason("application lifecycle poisoned"))?;
            if lifecycle.closed {
                return Ok(());
            }
            lifecycle.closing = true;
        }
        self.inner.cancellation.cancel();
        loop {
            let notified = self.inner.drained.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let done = self
                .inner
                .lifecycle
                .lock()
                .map_err(|_| napi::Error::from_reason("application lifecycle poisoned"))?
                .active
                == 0;
            if done {
                break;
            }
            notified.await;
        }
        let mut lifecycle = self
            .inner
            .lifecycle
            .lock()
            .map_err(|_| napi::Error::from_reason("application lifecycle poisoned"))?;
        lifecycle.snapshot.take();
        lifecycle.closed = true;
        Ok(())
    }
}

struct ActiveOperation {
    inner: Arc<Inner>,
}

impl Drop for ActiveOperation {
    fn drop(&mut self) {
        if let Ok(mut lifecycle) = self.inner.lifecycle.lock() {
            lifecycle.active = lifecycle.active.saturating_sub(1);
            if lifecycle.active == 0 {
                self.inner.drained.notify_waiters();
            }
        }
    }
}

#[napi]
pub struct NativeDocumentResponse {
    status: u32,
    headers: Vec<NativeHeader>,
    html: Option<String>,
}

#[napi]
impl NativeDocumentResponse {
    #[napi(getter)]
    pub fn status(&self) -> u32 {
        self.status
    }

    #[napi(getter)]
    pub fn headers(&self) -> Vec<NativeHeader> {
        self.headers.clone()
    }

    #[napi]
    pub fn body(
        &mut self,
        env: Env,
    ) -> napi::Result<ReadableStream<'static, BufferSlice<'static>>> {
        let bytes = self.html.take().unwrap_or_default().into_bytes();
        ReadableStream::create_with_stream_bytes(&env, stream::iter(vec![Ok(bytes)]))
    }
}

async fn execute(
    snapshot: Arc<Snapshot>,
    request: NativeRequest,
) -> napi::Result<NativeDocumentResponse> {
    let method = http::Method::from_bytes(request.method.as_bytes()).map_err(binding_error)?;
    let uri = request.url.parse::<http::Uri>().map_err(binding_error)?;
    let mut headers = http::HeaderMap::new();
    for entry in request.headers {
        let name =
            http::header::HeaderName::from_bytes(entry.name.as_bytes()).map_err(binding_error)?;
        let value = http::header::HeaderValue::from_str(&entry.value).map_err(binding_error)?;
        headers.append(name, value);
    }
    if !headers.contains_key(http::header::HOST) {
        if let Some(authority) = uri.authority() {
            let value =
                http::header::HeaderValue::from_str(authority.as_str()).map_err(binding_error)?;
            headers.insert(http::header::HOST, value);
        }
    }
    if !headers.contains_key("x-forwarded-proto") {
        if let Some(scheme) = uri.scheme_str() {
            if matches!(scheme, "http" | "https") {
                headers.insert(
                    "x-forwarded-proto",
                    http::header::HeaderValue::from_str(scheme).map_err(binding_error)?,
                );
            }
        }
    }
    let mut context = RequestContext::from_parts(method, &uri, &headers).map_err(binding_error)?;
    let outcome = plec_server_engine::document::execute_document(
        &snapshot.options,
        &snapshot.artifact,
        &mut context,
        &snapshot.client,
        Some(&snapshot.callbacks),
    )
    .await
    .map_err(binding_error)?;
    let (status, headers, html) = match outcome {
        plec_server_engine::DocumentOutcome::Rendered {
            status,
            headers,
            html,
        } => (status, headers, html),
        plec_server_engine::DocumentOutcome::Redirect {
            status, headers, ..
        } => (status, headers, String::new()),
        plec_server_engine::DocumentOutcome::NotFound { headers, html } => (404, headers, html),
    };
    Ok(NativeDocumentResponse {
        status: status as u32,
        headers: headers
            .iter()
            .map(|(name, value)| NativeHeader {
                name: name.as_str().to_owned(),
                value: value.to_str().unwrap_or_default().to_owned(),
            })
            .collect(),
        html: Some(html),
    })
}

fn binding_error(error: impl std::fmt::Display) -> napi::Error {
    napi::Error::from_reason(error.to_string())
}
