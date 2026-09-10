//! The Node sidecar protocol, mirrored by `packages/plec-node-runtime`.
//!
//! Both sides speak plain HTTP over a private socket (Unix domain socket, or
//! loopback TCP + token on platforms without them). These constants are the
//! contract; keep them in lockstep with the runtime script.

/// The supervisor authenticates every internal request with this header
/// before the sidecar reads or dispatches anything.
pub(crate) const INTERNAL_TOKEN_HEADER: &str = "x-plec-internal-token";

/// The header the sidecar itself injects — and only when the application's
/// `handleRequest` resolved `undefined` — to mark "no handler accepted".
/// Application responses never carry it: the sidecar strips it from
/// application headers, and the host strips it from inbound public requests
/// before forwarding, so only the sidecar can originate it.
pub(crate) const UNHANDLED_HEADER: &str = "x-plec-runtime-result";
pub(crate) const UNHANDLED_VALUE: &str = "unhandled";

/// Structured sidecar stdout lines. Application modules may log freely
/// during import, so readiness is a recognizable protocol line, never "the
/// first stdout output".
pub(crate) const READY_PREFIX: &str = "➠︎           Plec Ready ";
pub(crate) const ERROR_PREFIX: &str = "➠︎           Plec Runtime Error ";

/// The sidecar protocol version carried in the READY payload.
pub(crate) const SIDECAR_PROTOCOL_VERSION: u32 = 1;

/// A reserved internal path the sidecar answers itself; applications never
/// see requests for it.
pub(crate) const HEALTH_PATH: &str = "/_plec-runtime/health";
