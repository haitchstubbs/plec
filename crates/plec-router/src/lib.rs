//! Client-side routing for compiled Plec applications: URL matching against
//! the route manifest, browser listener wiring, and navigation/adoption
//! orchestration over `plec-client`'s runtime state.
//!
//! Items here are `pub` because `plec-runtime` (the wasm facade) consumes
//! them; they are not a public API contract.

pub mod listeners;
pub mod navigation;
