//! Shared imports for the runtime crates. This replicates the pre-split
//! `lifecycle` prelude so the crate extraction stays mechanically
//! reviewable.

pub use serde_json::Value;
pub use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};
pub use wasm_bindgen::{closure::Closure, prelude::*, JsCast};
#[cfg(feature = "fetch")]
pub use wasm_bindgen_futures::{spawn_local, JsFuture};
#[cfg(feature = "fetch")]
pub use web_sys::{AbortController, RequestInit, Response};
pub use web_sys::{
    Document, Element, Event, EventTarget, HtmlInputElement, KeyboardEvent, MouseEvent, Node,
};

pub use plec_schema::{
    delta::{coalesce_deltas, runtime_from_json, Delta, MountMetrics, RuntimeValue, UpdateMetrics},
    routing::RouteManifest,
    typed::{
        TypedActionInstruction, TypedApplication, TypedCollection, TypedComponentApplication,
        TypedEventField, TypedExpressionInstruction, TypedHostComponentTarget, TypedNode,
    },
};

pub use crate::state::{
    error, graph_instance_id, ReconcileBudget, RegionSlot, RegionTracker, RouterListener,
    RuntimeState,
};
