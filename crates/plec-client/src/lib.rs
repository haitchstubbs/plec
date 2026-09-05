//! Client-side execution of compiled Plec application semantics: the typed
//! graph runtime (state, events, cookies, fetch actions, keyed reorder, the
//! action VM) plus the typed binding sinks.
//!
//! Ownership note: this crate owns `RuntimeState`, the shared instance
//! forest consumed by `plec-router` and the `plec-runtime` wasm facade.
//! Items here are `pub` because those crates reach into them; they are not
//! a public API contract.
//!
//! The shared-import prelude mirrors the pre- split
//! `plec-runtime::runtime::lifecycle` prelude so the extraction stays
//! reviewable; prune it opportunistically.

pub mod bindings;
pub mod cookie;
pub mod events;
#[cfg(feature = "fetch")]
pub mod fetch;
pub mod prelude;
pub mod reorder;
pub mod route;
pub mod runtime;
pub mod state;
pub mod vm;
