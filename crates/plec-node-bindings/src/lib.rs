#![cfg(feature = "feasibility-gate")]

use std::sync::atomic::AtomicUsize;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use napi::threadsafe_function::ThreadsafeFunction;
use napi::{
    bindgen_prelude::{AsyncTask, BufferSlice, Function, Promise, ReadableStream, Reader},
    Env, Status, Task,
};
use napi_derive::napi;

type Callback = ThreadsafeFunction<String, Promise<String>, String, Status, false>;
type CancelCallback = ThreadsafeFunction<String, Promise<()>, String, Status, false>;
static OUTBOUND_POLLS: AtomicUsize = AtomicUsize::new(0);

#[napi]
pub struct PlecApplication {
    callback: Arc<Mutex<Option<Arc<Callback>>>>,
    closed: Arc<AtomicBool>,
}

#[napi]
impl PlecApplication {
    #[napi(constructor)]
    pub fn new(callback: Function<'_, String, Promise<String>>) -> napi::Result<Self> {
        let callback = callback
            .build_threadsafe_function()
            .callee_handled::<false>()
            .build()?;
        Ok(Self {
            callback: Arc::new(Mutex::new(Some(Arc::new(callback)))),
            closed: Arc::new(AtomicBool::new(false)),
        })
    }

    #[napi]
    pub async fn invoke(&self, value: String) -> napi::Result<String> {
        if self.closed.load(Ordering::Acquire) {
            return Err(napi::Error::from_reason("PLEC_APPLICATION_CLOSED"));
        }
        let callback = self
            .callback
            .lock()
            .map_err(|_| napi::Error::from_reason("callback manager poisoned"))?
            .as_ref()
            .map(Arc::clone)
            .ok_or_else(|| napi::Error::from_reason("PLEC_APPLICATION_CLOSED"))?;
        let promise = callback.call_async_catch(value).await?;
        promise.await.map_err(|error| {
            napi::Error::from_reason(format!("JavaScript callback failed: {error}"))
        })
    }

    #[napi]
    pub fn close(&self) -> napi::Result<()> {
        self.closed.store(true, Ordering::Release);
        self.callback
            .lock()
            .map_err(|_| napi::Error::from_reason("callback manager poisoned"))?
            .take();
        Ok(())
    }
}

#[napi]
pub fn consume_stream(
    stream: ReadableStream<'_, napi::bindgen_prelude::Buffer>,
    max_bytes: u32,
    cancel_source: Function<'_, String, Promise<()>>,
) -> napi::Result<AsyncTask<ConsumeStream>> {
    Ok(AsyncTask::new(ConsumeStream {
        reader: stream.read()?,
        max_bytes,
        cancel_source: Arc::new(
            cancel_source
                .build_threadsafe_function()
                .callee_handled::<false>()
                .build()?,
        ),
    }))
}

pub struct ConsumeStream {
    reader: Reader<napi::bindgen_prelude::Buffer>,
    max_bytes: u32,
    cancel_source: Arc<CancelCallback>,
}

impl Task for ConsumeStream {
    type Output = u32;
    type JsValue = u32;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        use futures_util::StreamExt;

        let reader = &mut self.reader;
        let max_bytes = self.max_bytes;
        let cancel_source = Arc::clone(&self.cancel_source);
        futures_executor::block_on(async move {
            let mut length = 0u32;
            while let Some(chunk) = reader.next().await {
                length = length
                    .checked_add(chunk?.len() as u32)
                    .ok_or_else(|| napi::Error::from_reason("stream length overflow"))?;
                if length > max_bytes {
                    let _ = cancel_source
                        .call_async_catch("request body exceeds limit".to_owned())
                        .await?
                        .await;
                    return Err(napi::Error::from_reason("request body exceeds limit"));
                }
            }
            Ok(length)
        })
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(output)
    }
}

#[napi]
pub fn produce_stream(env: Env) -> napi::Result<ReadableStream<'static, BufferSlice<'static>>> {
    use futures_util::{stream, StreamExt};

    ReadableStream::create_with_stream_bytes(
        &env,
        stream::iter(vec![
            Ok(vec![b'p', b'l']),
            Ok(vec![b'e', b'c']),
            Ok(vec![b'!']),
        ])
        .inspect(|_| {
            OUTBOUND_POLLS.fetch_add(1, Ordering::Relaxed);
        }),
    )
}

#[napi]
pub fn produce_failing_stream(
    env: Env,
) -> napi::Result<ReadableStream<'static, BufferSlice<'static>>> {
    use futures_util::{stream, StreamExt};

    ReadableStream::create_with_stream_bytes(
        &env,
        stream::iter(vec![
            Ok(vec![b'o', b'k']),
            Err(napi::Error::from_reason("native stream failure")),
        ])
        .inspect(|_| {
            OUTBOUND_POLLS.fetch_add(1, Ordering::Relaxed);
        }),
    )
}

#[napi]
pub fn outbound_poll_count() -> u32 {
    OUTBOUND_POLLS.load(Ordering::Relaxed) as u32
}
