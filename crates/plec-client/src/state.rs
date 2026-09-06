//! Shared runtime state: the live typed instance forest and its ownership
//! handles. The `plec-runtime` wasm facade and `plec-router` operate on this
//! state; all fields are `Rc<RefCell<_>>` so clones share ownership exactly
//! as the pre-split `PlecRuntime` did.

use crate::prelude::*;
use plec_dom::cookie::CookiePolicy;
use plec_schema::routing::RouteManifest;
use plec_schema::typed::TypedComponentApplication;
use std::cell::Cell;

use crate::runtime::{SsrNestedRecords, TypedGraphInstance};

/// Shared live-resource accounting for every graph instance in one runtime.
/// Slots own their reservation and release exactly once when dropped.
pub struct RegionTracker {
    live_regions: Cell<usize>,
    in_flight_fetches: Cell<usize>,
}

impl RegionTracker {
    pub fn new() -> Self {
        Self {
            live_regions: Cell::new(0),
            in_flight_fetches: Cell::new(0),
        }
    }

    pub fn acquire_region(&self) -> Result<(), JsValue> {
        let next = self
            .live_regions
            .get()
            .checked_add(1)
            .ok_or_else(|| JsValue::from_str("mounted region count overflow"))?;
        if next > plec_ir::limits::MAX_MOUNTED_REGIONS {
            return Err(JsValue::from_str("MOUNTED_REGION_LIMIT_EXCEEDED"));
        }
        self.live_regions.set(next);
        Ok(())
    }

    pub fn release_region(&self) {
        self.live_regions
            .set(self.live_regions.get().saturating_sub(1));
    }

    pub fn acquire_fetch(&self) -> Result<(), JsValue> {
        let next = self
            .in_flight_fetches
            .get()
            .checked_add(1)
            .ok_or_else(|| JsValue::from_str("in-flight fetch count overflow"))?;
        if next > plec_ir::limits::MAX_IN_FLIGHT_FETCHES {
            return Err(JsValue::from_str("IN_FLIGHT_FETCH_LIMIT_EXCEEDED"));
        }
        self.in_flight_fetches.set(next);
        Ok(())
    }

    pub fn release_fetch(&self) {
        self.in_flight_fetches
            .set(self.in_flight_fetches.get().saturating_sub(1));
    }

    #[cfg(test)]
    fn live_regions(&self) -> usize {
        self.live_regions.get()
    }
}

/// A region reservation is owned by its lifecycle record. Explicit release is
/// idempotent and `Drop` covers wholesale runtime/instance teardown.
pub struct RegionSlot {
    tracker: Rc<RegionTracker>,
    live_slot_held: bool,
}

impl RegionSlot {
    pub fn acquire(tracker: Rc<RegionTracker>) -> Result<Self, JsValue> {
        tracker.acquire_region()?;
        Ok(Self {
            tracker,
            live_slot_held: true,
        })
    }

    pub fn release(&mut self) {
        if self.live_slot_held {
            self.tracker.release_region();
            self.live_slot_held = false;
        }
    }
}

impl Drop for RegionSlot {
    fn drop(&mut self) {
        self.release();
    }
}

/// One shared reservation survives a top-level reconcile and all component
/// work it queues before the flush reaches quiescence.
pub struct ReconcileBudget {
    remaining_nodes: usize,
    remaining_regions: usize,
    remaining_operations: usize,
}

impl ReconcileBudget {
    pub fn new() -> Self {
        Self {
            remaining_nodes: plec_ir::limits::MAX_MOUNTED_REGIONS,
            remaining_regions: plec_ir::limits::MAX_MOUNTED_REGIONS,
            remaining_operations: plec_ir::limits::MAX_DOM_OPERATIONS_PER_RECONCILE,
        }
    }

    pub fn reserve(
        &mut self,
        nodes: usize,
        regions: usize,
        operations: usize,
    ) -> Result<(), JsValue> {
        if nodes > self.remaining_nodes
            || regions > self.remaining_regions
            || operations > self.remaining_operations
        {
            return Err(JsValue::from_str("RECONCILE_BUDGET_EXCEEDED"));
        }
        self.remaining_nodes -= nodes;
        self.remaining_regions -= regions;
        self.remaining_operations -= operations;
        Ok(())
    }
}

pub struct RouterListener {
    pub target: EventTarget,
    pub event_type: String,
    pub callback: Closure<dyn FnMut(Event)>,
}

#[derive(Clone)]
pub struct RuntimeState {
    pub router_listeners: Rc<RefCell<Vec<RouterListener>>>,
    /// Typed graph state is instance-owned so a persistent layout never loses
    /// its DOM, state, listeners, or fetch ownership when a child route moves.
    pub typed: Rc<RefCell<HashMap<String, TypedGraphInstance>>>,
    pub region_tracker: Rc<RegionTracker>,
    pub reconcile_budget: Rc<RefCell<Option<ReconcileBudget>>>,
    pub typed_root: Rc<RefCell<Option<Element>>>,
    /// Monotonic across typed graph replacements so an old request can never
    /// match a newly-created graph that happens to start at generation one.
    pub typed_generation: Rc<RefCell<u64>>,
    /// Independently fetched 0.10 graph closures.  Do not merge these by
    /// component id: the same helper component may legitimately occur in two
    /// live route artifacts.
    pub typed_component_registry: Rc<RefCell<HashMap<String, TypedComponentApplication>>>,
    pub typed_manifest: Rc<RefCell<Option<RouteManifest>>>,
    pub typed_host_inputs: Rc<RefCell<HashMap<String, RuntimeValue>>>,
    /// Host-input keys seeded from an imported SSR snapshot. `abandon_adoption`
    /// purges exactly these so a fallback remount starts from window-derived
    /// state instead of leaked request state.
    pub typed_ssr_host_inputs: Rc<RefCell<HashSet<String>>>,
    /// Whether the in-flight adoption is backed by an imported snapshot.
    /// Adopted graph instances read this to enable the divergence observation.
    pub typed_ssr_imported: Rc<RefCell<bool>>,
    /// The route chain the server published in the imported snapshot. The
    /// adoption path cross-validates it against its own URL-derived chain so
    /// the transferred cause, not a browser re-match, defines execution
    /// identity.
    pub typed_ssr_route_chain: Rc<RefCell<Option<Vec<plec_ir::SsrRouteInstance>>>>,
    /// Loader outcomes the server already executed, keyed by
    /// `loader_ref(graph_id, action)`. Adopted loader routes resume from
    /// these instead of re-running their loaders on first paint.
    pub typed_ssr_loaders: Rc<RefCell<HashMap<String, plec_ir::SsrLoaderState>>>,
    /// Selected conditional branches per graph instance from the imported
    /// snapshot. The record is the ownership cause: adoption claims the
    /// marked region and registers it with this exact branch selection.
    pub typed_ssr_branches:
        Rc<RefCell<HashMap<String, HashMap<usize, plec_ir::SsrSelectedBranch>>>>,
    /// Keyed SSR loop rows per graph instance. Values are identity/order only;
    /// row values are recomputed from imported runtime state during claim.
    pub typed_ssr_loops: Rc<RefCell<HashMap<String, HashMap<usize, Vec<String>>>>>,
    /// Recorded execution state for nested component instances, keyed by the
    /// component's marker path. The adopter hands each instance its subtree's
    /// records so nested branch/loop state is claimed, never inferred.
    pub typed_ssr_nested: Rc<RefCell<SsrNestedRecords>>,
    /// Immutable component definitions for the currently loaded IR 0.10 application.
    pub typed_components: Rc<RefCell<Option<TypedComponentApplication>>>,
    pub cookie_policy: Rc<RefCell<Option<HashMap<String, CookiePolicy>>>>,
    /// Host-owned fetch grants. `None` denies every artifact fetch; grants
    /// restrict origin, method, headers, and the credentials mode. Stored
    /// even without the `fetch` feature so the host policy surface is
    /// feature-independent.
    pub fetch_policy: Rc<RefCell<Option<Vec<FetchPolicyGrant>>>>,
}

/// One host-owned fetch grant. Authority comes only from grants published
/// through `set_fetch_policy`; artifact-declared fetch capability requests
/// are treated purely as requests and never widen this surface.
#[derive(Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchPolicyGrant {
    /// Exact origin (scheme, host, port) the grant applies to, as a
    /// serialized URL origin (`https://api.example.com`).
    pub origin: String,
    /// Allowed request methods. An empty list grants no methods.
    #[serde(default)]
    pub methods: Vec<String>,
    /// Allowed request header names (case-insensitive match). An empty list
    /// grants no artifact-controlled headers.
    #[serde(default)]
    pub headers: Vec<String>,
    /// Whether requests under this grant may carry credentials. The runtime
    /// forces `include` when true and `omit` when false; the artifact cannot
    /// influence the credentials mode.
    #[serde(default)]
    pub credentials: bool,
}

impl RuntimeState {
    pub fn set_fetch_policy(&self, policy: JsValue) -> Result<(), JsValue> {
        let policy: Option<Vec<FetchPolicyGrant>> =
            serde_wasm_bindgen::from_value(policy).map_err(error)?;
        *self.fetch_policy.borrow_mut() = policy;
        Ok(())
    }
}

impl RuntimeState {
    pub fn new() -> Self {
        Self {
            router_listeners: Rc::new(RefCell::new(Vec::new())),
            typed: Rc::new(RefCell::new(HashMap::new())),
            region_tracker: Rc::new(RegionTracker::new()),
            reconcile_budget: Rc::new(RefCell::new(None)),
            typed_root: Rc::new(RefCell::new(None)),
            typed_generation: Rc::new(RefCell::new(0)),
            typed_component_registry: Rc::new(RefCell::new(HashMap::new())),
            typed_manifest: Rc::new(RefCell::new(None)),
            typed_host_inputs: Rc::new(RefCell::new(HashMap::new())),
            typed_ssr_host_inputs: Rc::new(RefCell::new(HashSet::new())),
            typed_ssr_imported: Rc::new(RefCell::new(false)),
            typed_ssr_route_chain: Rc::new(RefCell::new(None)),
            typed_ssr_loaders: Rc::new(RefCell::new(HashMap::new())),
            typed_ssr_branches: Rc::new(RefCell::new(HashMap::new())),
            typed_ssr_loops: Rc::new(RefCell::new(HashMap::new())),
            typed_ssr_nested: Rc::new(RefCell::new(HashMap::new())),
            typed_components: Rc::new(RefCell::new(None)),
            cookie_policy: Rc::new(RefCell::new(None)),
            fetch_policy: Rc::new(RefCell::new(None)),
        }
    }

    pub fn next_typed_generation(&self) -> u64 {
        let mut generation = self.typed_generation.borrow_mut();
        *generation = generation.saturating_add(1);
        *generation
    }

    /// Deferred child mounts participate in the outer reconcile transaction
    /// when one is active. Fresh root mounts run without a reconcile budget.
    pub fn reserve_deferred_reconcile(
        &self,
        nodes: usize,
        regions: usize,
        operations: usize,
    ) -> Result<(), JsValue> {
        if let Some(budget) = self.reconcile_budget.borrow_mut().as_mut() {
            budget.reserve(nodes, regions, operations)?;
        }
        Ok(())
    }
}

/// Releasing the state must unregister the browser listeners it owns: the
/// registered callbacks otherwise outlive the facade and a later browser
/// event would invoke a dropped wasm closure. Explicit `dispose` drains the
/// same list first, so this is the idempotent backstop for a runtime that
/// was released without a prior dispose.
impl Drop for RuntimeState {
    fn drop(&mut self) {
        for listener in self.router_listeners.borrow_mut().drain(..) {
            let _ = listener.target.remove_event_listener_with_callback(
                &listener.event_type,
                listener.callback.as_ref().unchecked_ref(),
            );
        }
    }
}

pub fn graph_instance_id(parent: Option<&str>, outlet: &str, key: Option<&str>) -> String {
    let segment = |value: &str| value.replace('%', "%25").replace('/', "%2F");
    let parent = parent.map(segment).unwrap_or_else(|| "root".into());
    let key = key
        .map(|value| format!("/key:{}", segment(value)))
        .unwrap_or_default();
    format!("{parent}/outlet:{}{key}", segment(outlet))
}

pub fn error(error: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn graph_instance_identity_comes_from_mount_topology_and_keys() {
        assert_eq!(graph_instance_id(None, "main", None), "root/outlet:main");
        assert_eq!(
            graph_instance_id(Some("root/outlet:main"), "rows", Some("todo/1")),
            "root%2Foutlet:main/outlet:rows/key:todo%2F1"
        );
    }

    #[test]
    fn region_slot_releases_once_when_explicitly_released_then_dropped() {
        let tracker = Rc::new(RegionTracker::new());
        let mut slot = RegionSlot::acquire(tracker.clone()).unwrap();
        assert_eq!(tracker.live_regions(), 1);
        slot.release();
        slot.release();
        assert_eq!(tracker.live_regions(), 0);
        drop(slot);
        assert_eq!(tracker.live_regions(), 0);
    }
}
