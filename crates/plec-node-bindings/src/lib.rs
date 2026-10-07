mod application;

pub use application::{NativeCallbacks, PlecApplication, create_callbacks};

#[napi_derive::napi]
pub fn max_request_body_bytes() -> u32 {
    plec_ir::limits::MAX_REQUEST_BODY_BYTES as u32
}

#[cfg(feature = "feasibility-gate")]
mod feasibility;
