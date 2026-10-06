use std::sync::{Arc, Mutex};

use futures_util::stream;
use napi::{
    bindgen_prelude::{AsyncTask, Buffer, BufferSlice, Function, Promise, ReadableStream, Reader},
    threadsafe_function::ThreadsafeFunction,
    Env, Status, Task,
};
use napi_derive::napi;
use plec_server_engine::runtime::{
    ActionCapabilities, ActionFuture, ApplicationCapabilities, HostRenderFuture, HostRenderRequest,
};
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
#[derive(Clone)]
pub struct NativeRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<NativeHeader>,
}

type HostRenderCallback = ThreadsafeFunction<String, Promise<String>, String, Status, false>;
type ActionCallback = ThreadsafeFunction<String, Promise<String>, String, Status, false>;

struct CallbackManager {
    render_host: Arc<HostRenderCallback>,
    invoke_action: Arc<ActionCallback>,
    action_permits: Arc<tokio::sync::Semaphore>,
    runtime: Option<tokio::runtime::Handle>,
}

#[napi]
pub struct NativeCallbacks {
    inner: Mutex<Option<CallbackManager>>,
}

#[napi]
pub fn create_callbacks(
    render_host: Function<'_, String, Promise<String>>,
    invoke_action: Function<'_, String, Promise<String>>,
) -> napi::Result<NativeCallbacks> {
    let callback = render_host
        .build_threadsafe_function()
        .callee_handled::<false>()
        .build()?;
    let action_callback = invoke_action
        .build_threadsafe_function()
        .callee_handled::<false>()
        .build()?;
    Ok(NativeCallbacks {
        inner: Mutex::new(Some(CallbackManager {
            render_host: Arc::new(callback),
            invoke_action: Arc::new(action_callback),
            action_permits: Arc::new(tokio::sync::Semaphore::new(64)),
            runtime: None,
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

impl ActionCapabilities for CallbackManager {
    fn invoke_action<'a>(
        &'a self,
        request: plec_server_engine::action::ServerActionRequest,
    ) -> ActionFuture<'a> {
        Box::pin(async move {
            let permit = Arc::clone(&self.action_permits)
                .try_acquire_owned()
                .map_err(|_| plec_server_engine::ServerError::CallbackCapacity)?;
            let context = &request.context;
            let mut headers = serde_json::Map::new();
            for name in context.headers.keys() {
                if let Some(value) = context
                    .headers
                    .get(name)
                    .and_then(|value| value.to_str().ok())
                {
                    headers.insert(
                        name.as_str().to_owned(),
                        serde_json::Value::String(value.to_owned()),
                    );
                }
            }
            let payload = serde_json::json!({
                "id": request.id,
                "arguments": request.arguments,
                "context": {
                    "url": context.url,
                    "pathname": context.pathname,
                    "method": context.method.as_str(),
                    "headers": headers,
                    "cookies": context.cookies,
                    "params": context.params,
                    "query": context.query.iter().map(|(key, value)| (key.clone(), match value {
                        plec_server_engine::request::QueryValue::One(value) => serde_json::Value::String(value.clone()),
                        plec_server_engine::request::QueryValue::Many(values) => serde_json::json!(values),
                    })).collect::<serde_json::Map<_, _>>(),
                }
            }).to_string();
            let callback = Arc::clone(&self.invoke_action);
            let promise = callback
                .call_async_catch(payload)
                .await
                .map_err(|error| plec_server_engine::ServerError::message(error.to_string()))?;
            let (settled, result) = tokio::sync::oneshot::channel();
            self.runtime
                .as_ref()
                .expect("runtime initialized during load")
                .spawn(async move {
                    let result = promise.await;
                    drop(permit);
                    let _ = settled.send(result);
                });
            let result = result
                .await
                .map_err(|_| plec_server_engine::ServerError::message("server action failed"))?
                .map_err(|_| plec_server_engine::ServerError::message("server action failed"))?;
            let value: serde_json::Value = serde_json::from_str(&result).map_err(|_| {
                plec_server_engine::ServerError::message("invalid server action callback result")
            })?;
            if value.get("found").and_then(serde_json::Value::as_bool) == Some(false) {
                return Err(plec_server_engine::ServerError::UnknownServerAction);
            }
            serde_json::from_value(
                value
                    .get("value")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
            )
            .map_err(|_| plec_server_engine::ServerError::message("invalid server action result"))
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
        let mut callbacks = self
            .inner
            .lock()
            .map_err(|_| napi::Error::from_reason("callback manager poisoned"))?
            .take()
            .ok_or_else(|| napi::Error::from_reason("callback manager already consumed"))?;
        callbacks.runtime = Some(tokio::runtime::Handle::current());
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
            let snapshot = lifecycle
                .snapshot
                .as_ref()
                .cloned()
                .ok_or_else(|| napi::Error::from_reason("PLEC_APPLICATION_CLOSED"))?;
            lifecycle.active += 1;
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

    #[napi(ts_return_type = "Promise<NativeActionResponse>")]
    pub fn handle_action(
        &self,
        request: NativeRequest,
        body: ReadableStream<'_, Buffer>,
        cancel_body: Function<'_, String, Promise<()>>,
    ) -> napi::Result<AsyncTask<ActionTask>> {
        let (snapshot, cancellation) = {
            let mut lifecycle = self
                .inner
                .lifecycle
                .lock()
                .map_err(|_| napi::Error::from_reason("application lifecycle poisoned"))?;
            if lifecycle.closing || lifecycle.closed {
                return Err(napi::Error::from_reason("PLEC_APPLICATION_CLOSED"));
            }
            let snapshot = lifecycle
                .snapshot
                .as_ref()
                .cloned()
                .ok_or_else(|| napi::Error::from_reason("PLEC_APPLICATION_CLOSED"))?;
            lifecycle.active += 1;
            (snapshot, self.inner.cancellation.child_token())
        };
        let active = ActiveOperation {
            inner: Arc::clone(&self.inner),
        };
        let cancel = cancel_body
            .build_threadsafe_function()
            .callee_handled::<false>()
            .build()?;
        let reader = body.read()?;
        Ok(AsyncTask::new(ActionTask {
            snapshot,
            request,
            reader: Some(reader),
            cancel_body: Arc::new(cancel),
            cancellation,
            active: Some(active),
        }))
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

#[napi]
pub struct NativeActionResponse {
    status: u32,
    headers: Vec<NativeHeader>,
    body: Option<String>,
}

#[napi]
impl NativeActionResponse {
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
        let bytes = self.body.take().unwrap_or_default().into_bytes();
        ReadableStream::create_with_stream_bytes(&env, stream::iter(vec![Ok(bytes)]))
    }
}

pub struct ActionTask {
    snapshot: Arc<Snapshot>,
    request: NativeRequest,
    reader: Option<Reader<Buffer>>,
    cancel_body: Arc<ThreadsafeFunction<String, Promise<()>, String, Status, false>>,
    cancellation: CancellationToken,
    active: Option<ActiveOperation>,
}

impl Task for ActionTask {
    type Output = NativeActionResponse;
    type JsValue = NativeActionResponse;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        let outcome = futures_executor::block_on(async {
            tokio::select! {
                _ = self.cancellation.cancelled() => return Err(napi::Error::from_reason("PLEC_APPLICATION_CLOSED")),
                outcome = execute_action(Arc::clone(&self.snapshot), self.request.clone(), self.reader.take().expect("action task runs once")) => Ok(outcome),
            }
        })?;
        if matches!(
            outcome,
            plec_server_engine::action::ActionOutcome::RequestTooLarge
                | plec_server_engine::action::ActionOutcome::RequestStreamFailed
        ) {
            if let Ok(promise) = futures_executor::block_on(
                self.cancel_body
                    .call_async_catch("action request body limit exceeded".to_owned()),
            ) {
                let _ = futures_executor::block_on(promise);
            }
        }
        self.active.take();
        Ok(NativeActionResponse {
            status: outcome.status() as u32,
            headers: vec![
                NativeHeader {
                    name: "content-type".into(),
                    value: "application/json; charset=utf-8".into(),
                },
                NativeHeader {
                    name: "cache-control".into(),
                    value: "no-store".into(),
                },
            ],
            body: Some(outcome.body().to_string()),
        })
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(output)
    }
}

async fn execute_action(
    snapshot: Arc<Snapshot>,
    request: NativeRequest,
    reader: Reader<Buffer>,
) -> plec_server_engine::action::ActionOutcome {
    use futures_util::StreamExt;
    let method = match http::Method::from_bytes(request.method.as_bytes()) {
        Ok(method) => method,
        Err(_) => return plec_server_engine::action::ActionOutcome::MethodNotAllowed,
    };
    let uri = match request.url.parse::<http::Uri>() {
        Ok(uri) => uri,
        Err(_) => return plec_server_engine::action::ActionOutcome::SameOriginRequired,
    };
    let mut headers = http::HeaderMap::new();
    for entry in request.headers {
        let (Ok(name), Ok(value)) = (
            http::header::HeaderName::from_bytes(entry.name.as_bytes()),
            http::header::HeaderValue::from_str(&entry.value),
        ) else {
            return plec_server_engine::action::ActionOutcome::SameOriginRequired;
        };
        headers.append(name, value);
    }
    if !headers.contains_key(http::header::HOST) {
        if let Some(authority) = uri.authority() {
            if let Ok(value) = http::header::HeaderValue::from_str(authority.as_str()) {
                headers.insert(http::header::HOST, value);
            }
        }
    }
    if !headers.contains_key("x-forwarded-proto") {
        if let Some(scheme) = uri
            .scheme_str()
            .filter(|scheme| matches!(*scheme, "http" | "https"))
        {
            if let Ok(value) = http::header::HeaderValue::from_str(scheme) {
                headers.insert("x-forwarded-proto", value);
            }
        }
    }
    let context = match RequestContext::from_parts(method, &uri, &headers) {
        Ok(context) => context,
        Err(_) => return plec_server_engine::action::ActionOutcome::SameOriginRequired,
    };
    let length = headers
        .get(http::header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    let chunks = reader.map(|chunk| chunk.map(|buffer| bytes::Bytes::copy_from_slice(&buffer)));
    plec_server_engine::action::execute_action(context, length, chunks, &snapshot.callbacks).await
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
