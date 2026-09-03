//! Runtime lifecycle and the compatibility execution path.

pub(crate) use serde_json::Value;
pub(crate) use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};
pub(crate) use wasm_bindgen::{closure::Closure, prelude::*, JsCast};
#[cfg(feature = "fetch")]
pub(crate) use wasm_bindgen_futures::{spawn_local, JsFuture};
#[cfg(feature = "fetch")]
pub(crate) use web_sys::{AbortController, Request, RequestInit, Response};
pub(crate) use web_sys::{
    Comment, Document, Element, Event, EventTarget, HtmlInputElement, KeyboardEvent, MouseEvent,
    Node,
};

pub(crate) use crate::schema::{
    app::{
        Application, Binding, Conditional, ContextScope, ElementNode, EventBinding, Expression,
        Loop, PropProgram, TextNode,
    },
    delta::{coalesce_deltas, runtime_from_json, Delta, MountMetrics, RuntimeValue, UpdateMetrics},
    routing::{RouteManifest, RouterState},
    typed::{
        TypedActionInstruction, TypedApplication, TypedBinding, TypedCollection,
        TypedComponentApplication, TypedEventField, TypedExpressionInstruction, TypedNode,
    },
};

use crate::dom::{bindings::*, instantiate::*, listeners::*, platform::*, util::*};
use crate::eval::value::*;
use crate::router::listeners::*;
use crate::runtime::snapshots::SnapshotInput;
use crate::runtime::state::*;
use crate::typed::cookie::CookiePolicy;
use crate::typed::runtime::*;

/** Runtime-owned identity. ComponentGraph ids are compiler artifacts; an
 * instance id is stable for its mount position, outlet, and optional key. */
pub(crate) struct GraphInstance {
    pub(crate) parent_id: Option<String>,
    pub(crate) outlet_id: String,
    // tell rust analyer to ignore

    //
    pub(crate) key: Option<String>,
    pub(crate) graph_id: Option<String>,
    // These resources are deliberately owned by the topology instance rather
    // than the application definition. They are the boundary required for a
    // persistent layout with independently replaced outlet children.
    pub(crate) local_state: HashMap<String, Value>,
    pub(crate) dom_nodes: HashMap<String, Node>,
    pub(crate) host_refs: HashMap<String, Node>,
    pub(crate) rows: HashMap<String, HashMap<String, Row>>,
    pub(crate) root: Option<Node>,
    pub(crate) listeners: Vec<Listener>,
    pub(crate) continuation_epoch: u64,
    pub(crate) next_request_id: u64,
    #[cfg(feature = "fetch")]
    pub(crate) abort_controllers: HashMap<u64, AbortController>,
    pub(crate) active_effects: usize,
    pub(crate) active_listeners: usize,
}

#[wasm_bindgen]
pub struct PlecRuntime {
    /** Immutable compiled definitions. Mount state stays in instances; loading
     * a second graph must not overwrite the definition of the first. */
    pub(crate) registry: Rc<RefCell<HashMap<String, Application>>>,
    pub(crate) instances: Rc<RefCell<HashMap<String, GraphInstance>>>,
    pub(crate) router: Rc<RefCell<Option<RouterState>>>,
    pub(crate) router_listeners: Rc<RefCell<Vec<RouterListener>>>,
    /// Typed graph state is instance-owned so a persistent layout never loses
    /// its DOM, state, listeners, or fetch ownership when a child route moves.
    pub(crate) typed: Rc<RefCell<HashMap<String, TypedGraphInstance>>>,
    pub(crate) typed_root: Rc<RefCell<Option<Element>>>,
    /// Monotonic across typed graph replacements so an old request can never
    /// match a newly-created graph that happens to start at generation one.
    pub(crate) typed_generation: Rc<RefCell<u64>>,
    pub(crate) typed_registry: Rc<RefCell<HashMap<String, TypedApplication>>>,
    /// Independently fetched 0.10 graph closures.  Do not merge these by
    /// component id: the same helper component may legitimately occur in two
    /// live route artifacts.
    pub(crate) typed_component_registry: Rc<RefCell<HashMap<String, TypedComponentApplication>>>,
    pub(crate) typed_manifest: Rc<RefCell<Option<RouteManifest>>>,
    pub(crate) typed_host_inputs: Rc<RefCell<HashMap<String, RuntimeValue>>>,
    /// Host-input keys seeded from an imported SSR snapshot. `abandon_adoption`
    /// purges exactly these so a fallback remount starts from window-derived
    /// state instead of leaked request state.
    pub(crate) typed_ssr_host_inputs: Rc<RefCell<HashSet<String>>>,
    /// Whether the in-flight adoption is backed by an imported snapshot.
    /// Adopted graph instances read this to enable the divergence observation.
    pub(crate) typed_ssr_imported: Rc<RefCell<bool>>,
    /// The route chain the server published in the imported snapshot. The
    /// adoption path cross-validates it against its own URL-derived chain so
    /// the transferred cause, not a browser re-match, defines execution
    /// identity.
    pub(crate) typed_ssr_route_chain: Rc<RefCell<Option<Vec<plec_ir::SsrRouteInstance>>>>,
    /// Loader outcomes the server already executed, keyed by
    /// `loader_ref(graph_id, action)`. Adopted loader routes resume from
    /// these instead of re-running their loaders on first paint.
    pub(crate) typed_ssr_loaders: Rc<RefCell<HashMap<String, plec_ir::SsrLoaderState>>>,
    /// Selected conditional branches per graph instance from the imported
    /// snapshot. The record is the ownership cause: adoption claims the
    /// marked region and registers it with this exact branch selection.
    pub(crate) typed_ssr_branches:
        Rc<RefCell<HashMap<String, HashMap<usize, plec_ir::SsrSelectedBranch>>>>,
    /// Keyed SSR loop rows per graph instance. Values are identity/order only;
    /// row values are recomputed from imported runtime state during claim.
    pub(crate) typed_ssr_loops: Rc<RefCell<HashMap<String, HashMap<usize, Vec<String>>>>>,
    /// Recorded execution state for nested component instances, keyed by the
    /// component's marker path. The adopter hands each instance its subtree's
    /// records so nested branch/loop state is claimed, never inferred.
    pub(crate) typed_ssr_nested: Rc<RefCell<SsrNestedRecords>>,
    /// Immutable component definitions for the currently loaded IR 0.10 application.
    pub(crate) typed_components: Rc<RefCell<Option<TypedComponentApplication>>>,
    pub(crate) snapshot_inputs: Rc<RefCell<HashMap<String, SnapshotInput>>>,
    pub(crate) cookie_policy: Rc<RefCell<Option<HashMap<String, CookiePolicy>>>>,
}

impl Clone for PlecRuntime {
    fn clone(&self) -> Self {
        Self {
            registry: Rc::clone(&self.registry),
            instances: Rc::clone(&self.instances),
            router: Rc::clone(&self.router),
            router_listeners: Rc::clone(&self.router_listeners),
            typed: Rc::clone(&self.typed),
            typed_root: Rc::clone(&self.typed_root),
            typed_generation: Rc::clone(&self.typed_generation),
            typed_registry: Rc::clone(&self.typed_registry),
            typed_component_registry: Rc::clone(&self.typed_component_registry),
            typed_manifest: Rc::clone(&self.typed_manifest),
            typed_host_inputs: Rc::clone(&self.typed_host_inputs),
            typed_ssr_host_inputs: Rc::clone(&self.typed_ssr_host_inputs),
            typed_ssr_imported: Rc::clone(&self.typed_ssr_imported),
            typed_ssr_route_chain: Rc::clone(&self.typed_ssr_route_chain),
            typed_ssr_loaders: Rc::clone(&self.typed_ssr_loaders),
            typed_ssr_branches: Rc::clone(&self.typed_ssr_branches),
            typed_ssr_loops: Rc::clone(&self.typed_ssr_loops),
            typed_ssr_nested: Rc::clone(&self.typed_ssr_nested),
            typed_components: Rc::clone(&self.typed_components),
            snapshot_inputs: Rc::clone(&self.snapshot_inputs),
            cookie_policy: Rc::clone(&self.cookie_policy),
        }
    }
}

#[wasm_bindgen::prelude::wasm_bindgen]
impl PlecRuntime {
    pub fn load_application(&self, ir: JsValue) -> Result<(), JsValue> {
        let value: Value = serde_wasm_bindgen::from_value(ir.clone()).map_err(error)?;
        if value.get("version").and_then(Value::as_str) == Some("0.9") {
            let app: TypedApplication = serde_json::from_value(value).map_err(error)?;
            let mut typed = TypedRuntime::new(app)?;
            typed.set_host_inputs(self.typed_host_inputs.borrow().clone())?;
            typed.graph_generation = self.next_typed_generation();
            self.dispose_typed_instances();
            *self.typed_components.borrow_mut() = None;
            self.typed.borrow_mut().insert(
                graph_instance_id(None, "main", None),
                TypedGraphInstance {
                    parent_id: None,
                    outlet_id: "main".into(),
                    graph_id: "__typed__".into(),
                    route_id: None,
                    match_key: None,
                    route_state: None,
                    loader_runtime: None,
                    component_call: None,
                    component_start: None,
                    runtime: typed,
                },
            );
            return Ok(());
        }
        if value.get("version").and_then(Value::as_str) == Some("0.10") {
            let application: TypedComponentApplication =
                serde_json::from_value(value).map_err(error)?;
            application.validate()?;
            let mut typed =
                TypedRuntime::new(application.components[application.root_component].clone())?;
            typed.set_component_definitions(application.components.clone());
            typed.set_host_inputs(self.typed_host_inputs.borrow().clone())?;
            typed.graph_generation = self.next_typed_generation();
            self.dispose_typed_instances();
            *self.typed_components.borrow_mut() = Some(application);
            self.typed.borrow_mut().insert(
                graph_instance_id(None, "main", None),
                TypedGraphInstance {
                    parent_id: None,
                    outlet_id: "main".into(),
                    graph_id: "__typed_component__".into(),
                    route_id: None,
                    match_key: None,
                    route_state: None,
                    loader_runtime: None,
                    component_call: None,
                    component_start: None,
                    runtime: typed,
                },
            );
            return Ok(());
        }
        if value.get("version").and_then(Value::as_str) != Some("0.8") {
            return Err(JsValue::from_str("unsupported application IR version"));
        }
        let application: Application = serde_wasm_bindgen::from_value(ir).map_err(error)?;
        // The historical single-graph API remains a thin adapter for existing
        // callers. It is a registry entry, never runtime-global execution state.
        self.registry
            .borrow_mut()
            .insert("__legacy__".into(), application);
        Ok(())
    }
}

impl PlecRuntime {
    pub(crate) fn dispose_typed_instances(&self) {
        for (_, mut instance) in self.typed.borrow_mut().drain() {
            instance.runtime.invalidate_fetches();
            instance.runtime.clear_listeners();
            instance.runtime.clear_host_refs();
        }
    }
}

#[wasm_bindgen::prelude::wasm_bindgen]
impl PlecRuntime {
    pub fn set_host_inputs(&self, values: JsValue) -> Result<(), JsValue> {
        let values: HashMap<String, RuntimeValue> =
            serde_wasm_bindgen::from_value(values).map_err(error)?;
        *self.typed_host_inputs.borrow_mut() = values;
        Ok(())
    }
}

impl PlecRuntime {
    pub(crate) fn next_typed_generation(&self) -> u64 {
        let mut generation = self.typed_generation.borrow_mut();
        *generation = generation.saturating_add(1);
        *generation
    }
}

#[wasm_bindgen::prelude::wasm_bindgen]
impl PlecRuntime {
    pub fn register_graph(&self, graph_id: String, ir: JsValue) -> Result<(), JsValue> {
        let value: Value = serde_wasm_bindgen::from_value(ir.clone()).map_err(error)?;
        if value.get("version").and_then(Value::as_str) == Some("0.10") {
            let application: TypedComponentApplication =
                serde_json::from_value(value).map_err(error)?;
            application.validate()?;
            self.typed_component_registry
                .borrow_mut()
                .insert(graph_id, application.clone());
            *self.typed_components.borrow_mut() = Some(application);
            return Ok(());
        }
        if value.get("version").and_then(Value::as_str) == Some("0.9") {
            let app: TypedApplication = serde_json::from_value(value).map_err(error)?;
            app.validate()?;
            self.typed_registry.borrow_mut().insert(graph_id, app);
            return Ok(());
        }
        if value.get("version").and_then(Value::as_str) != Some("0.8") {
            return Err(JsValue::from_str("unsupported application IR version"));
        }
        let application: Application = serde_wasm_bindgen::from_value(ir).map_err(error)?;
        self.registry.borrow_mut().insert(graph_id, application);
        Ok(())
    }
}

#[wasm_bindgen::prelude::wasm_bindgen]
impl PlecRuntime {
    pub fn registered_graph_count(&self) -> u32 {
        self.registry.borrow().len() as u32
    }
}

#[wasm_bindgen::prelude::wasm_bindgen]
impl PlecRuntime {
    pub fn start(&self, root: Element, manifest: JsValue) -> Result<(), JsValue> {
        let manifest_value: Value = serde_wasm_bindgen::from_value(manifest).map_err(error)?;
        let manifest: RouteManifest = if manifest_value.get("version").and_then(Value::as_u64)
            == Some(3)
            && manifest_value
                .get("revision")
                .and_then(Value::as_str)
                .is_some()
        {
            let manifest: plec_ir::RouteManifest =
                serde_json::from_value(manifest_value).map_err(error)?;
            manifest
                .validate()
                .map_err(|message| JsValue::from_str(&message))?;
            RouteManifest {
                version: Some(manifest.version),
                root_graph_id: manifest.root_graph_id,
                routes: manifest
                    .routes
                    .into_iter()
                    .map(|route| crate::schema::routing::RouteManifestEntry {
                        id: route.id,
                        parent_id: route.parent_id,
                        path: route.path,
                        graph_id: route.graph_id,
                        pending_graph_id: route.pending_graph_id,
                        pending_mode: route.pending_mode,
                        error_graph_id: route.error_graph_id,
                        outlet_id: route.outlet_id,
                        loader: None,
                        loader_state_slot_id: None,
                        loader_action: route.loader_action,
                    })
                    .collect(),
            }
        } else {
            serde_json::from_value(manifest_value).map_err(error)?
        };
        if manifest.version == Some(3) {
            self.validate_typed_manifest(&manifest)?;
            *self.typed_manifest.borrow_mut() = Some(manifest);
            *self.typed_root.borrow_mut() = Some(root.clone());
            self.dispose_router_listeners();
            self.install_router_listeners()?;
            let location = window()?.location();
            let href = format!(
                "{}{}{}",
                location.pathname().unwrap_or_else(|_| "/".into()),
                location.search().unwrap_or_default(),
                location.hash().unwrap_or_default(),
            );
            return self.navigate_typed_route(&href, root, true, false);
        }
        self.dispose_router_listeners();
        let root_instance_id = graph_instance_id(None, "main", None);
        if self.instances.borrow().contains_key(&root_instance_id) {
            self.dispose_graph_instance(root_instance_id.clone())?;
        }
        self.create_instance(
            root_instance_id.clone(),
            None,
            "main".into(),
            None,
            manifest.root_graph_id.clone(),
        )?;
        self.mount_instance(&root_instance_id, root, true)?;
        *self.router.borrow_mut() = Some(RouterState {
            manifest,
            root_instance_id,
        });
        self.install_router_listeners()?;
        let pathname = window()?
            .location()
            .pathname()
            .unwrap_or_else(|_| "/".into());
        self.navigate_internal(&pathname, true)
    }

    /// Starts a typed route tree from server-rendered DOM.  This is separate
    /// from `start` so normal client mounts never *read* markers: both paths
    /// emit the same canonical structural addresses, but only adoption
    /// resolves them (see docs/dom-address-protocol.md).
    pub fn start_adopt(&self, root: Element, manifest: JsValue) -> Result<(), JsValue> {
        self.start_adopt_snapshot(root, manifest, JsValue::UNDEFINED)
    }

    /// `start_adopt` with an SSR execution snapshot (the v2 bootstrap
    /// payload). The snapshot is parsed, version/revision gated, and fully
    /// validated against the manifest plus the registered component
    /// application **before** adoption claims DOM; its public state is seeded
    /// into the host inputs so adopted instances evaluate from imported
    /// causes instead of blank initializers. Any snapshot failure is a
    /// fail-closed code; the caller falls back to a normal mount.
    pub fn start_adopt_snapshot(
        &self,
        root: Element,
        manifest: JsValue,
        snapshot: JsValue,
    ) -> Result<(), JsValue> {
        // Adoption lifecycle invariant (one-shot): adoption claims server
        // DOM exactly once, before any client-rendered DOM exists in the
        // scope. A live typed instance forest means the application already
        // materialized DOM (a mount, navigation, or a prior adoption), and
        // claiming into it would blend server- and client-created nodes.
        // This must be an explicit fail-closed precondition, never implied
        // by call order (see docs/dom-address-protocol.md).
        if !self.typed.borrow().is_empty() {
            return Err(JsValue::from_str("invariant:adoption-once"));
        }
        let manifest_value: Value = serde_wasm_bindgen::from_value(manifest).map_err(error)?;
        if manifest_value.get("version").and_then(Value::as_u64) != Some(3) {
            return Err(JsValue::from_str("unsupported:ssr-manifest"));
        }
        let manifest: plec_ir::RouteManifest =
            serde_json::from_value(manifest_value).map_err(error)?;
        manifest
            .validate()
            .map_err(|message| JsValue::from_str(&format!("mismatch:ssr-manifest:{message}")))?;
        self.import_ssr_snapshot(&snapshot, &manifest)?;
        let manifest = RouteManifest {
            version: Some(manifest.version),
            root_graph_id: manifest.root_graph_id,
            routes: manifest
                .routes
                .into_iter()
                .map(|route| crate::schema::routing::RouteManifestEntry {
                    id: route.id,
                    parent_id: route.parent_id,
                    path: route.path,
                    graph_id: route.graph_id,
                    pending_graph_id: route.pending_graph_id,
                    pending_mode: route.pending_mode,
                    error_graph_id: route.error_graph_id,
                    outlet_id: route.outlet_id,
                    loader: None,
                    loader_state_slot_id: None,
                    loader_action: route.loader_action,
                })
                .collect(),
        };
        self.validate_typed_manifest(&manifest)?;
        *self.typed_manifest.borrow_mut() = Some(manifest);
        *self.typed_root.borrow_mut() = Some(root.clone());
        self.dispose_router_listeners();
        self.install_router_listeners()?;
        let location = window()?.location();
        let href = format!(
            "{}{}{}",
            location.pathname().unwrap_or_else(|_| "/".into()),
            location.search().unwrap_or_default(),
            location.hash().unwrap_or_default(),
        );
        self.adopt_typed_route(&href, root)
    }

    /// Validates an optional SSR snapshot and seeds its public state. Kept
    /// separate from `start_adopt_snapshot` so the manifest remains the only
    /// required input; seeding happens before `adopt_typed_route` creates
    /// instances, which is what makes imported state visible to state
    /// initializers and claimed DOM.
    fn import_ssr_snapshot(
        &self,
        snapshot: &JsValue,
        manifest: &plec_ir::RouteManifest,
    ) -> Result<(), JsValue> {
        if snapshot.is_undefined() || snapshot.is_null() {
            *self.typed_ssr_imported.borrow_mut() = false;
            *self.typed_ssr_route_chain.borrow_mut() = None;
            self.typed_ssr_loaders.borrow_mut().clear();
            self.typed_ssr_branches.borrow_mut().clear();
            self.typed_ssr_loops.borrow_mut().clear();
            self.typed_ssr_nested.borrow_mut().clear();
            return Ok(());
        }
        self.reset_ssr_text_divergences();
        let value: Value = serde_wasm_bindgen::from_value(snapshot.clone())
            .map_err(|_| JsValue::from_str("mismatch:ssr-snapshot-payload"))?;
        let parsed: plec_ir::PlecSsrSnapshot = serde_json::from_value(value)
            .map_err(|_| JsValue::from_str("mismatch:ssr-snapshot-payload"))?;
        if parsed.version != plec_ir::SSR_SNAPSHOT_VERSION {
            return Err(JsValue::from_str("unsupported:ssr-snapshot-version"));
        }
        if parsed.revision != manifest.revision {
            return Err(JsValue::from_str("stale-revision"));
        }
        let application = self
            .typed_components
            .borrow()
            .clone()
            .ok_or_else(|| JsValue::from_str("mismatch:ssr-snapshot-graphs"))?;
        let structure = RegisteredStructureApplication {
            primary: application,
            registry: self.typed_component_registry.borrow().clone(),
        };
        parsed
            .validate(&plec_ir::SsrSnapshotReferences {
                manifest,
                application: &structure,
            })
            .map_err(|message| JsValue::from_str(&format!("mismatch:ssr-snapshot:{message}")))?;
        self.seed_ssr_host_inputs(&parsed)?;
        *self.typed_ssr_loaders.borrow_mut() = parsed
            .loaders
            .iter()
            .map(|outcome| {
                (
                    plec_ir::loader_ref(&outcome.graph_id, outcome.action),
                    outcome.state.clone(),
                )
            })
            .collect();
        *self.typed_ssr_route_chain.borrow_mut() = Some(parsed.routes);
        // Branch selections become the adoption ownership map for conditional
        // regions, keyed by the same graph instance ids the adopter uses.
        *self.typed_ssr_branches.borrow_mut() = parsed
            .structure
            .graphs
            .iter()
            .filter(|(_, structure)| !structure.branches.is_empty())
            .map(|(instance, structure)| {
                (
                    instance.clone(),
                    structure
                        .branches
                        .iter()
                        .map(|branch| (branch.node, branch.selected))
                        .collect::<HashMap<_, _>>(),
                )
            })
            .collect();
        *self.typed_ssr_loops.borrow_mut() = parsed
            .structure
            .graphs
            .iter()
            .filter(|(_, structure)| !structure.loops.is_empty())
            .map(|(instance, structure)| {
                (
                    instance.clone(),
                    structure
                        .loops
                        .iter()
                        .map(|loop_rows| (loop_rows.node, loop_rows.keys.clone()))
                        .collect::<HashMap<_, _>>(),
                )
            })
            .collect();
        // Nested component records address component instances below the
        // graph root by marker path; validation has already proven every
        // record against the compiled node tables.
        *self.typed_ssr_nested.borrow_mut() = parsed
            .structure
            .nested
            .iter()
            .map(|(path, structure)| {
                (
                    path.clone(),
                    SsrNestedComponentRecords {
                        branches: structure
                            .branches
                            .iter()
                            .map(|branch| (branch.node, branch.selected))
                            .collect(),
                        loops: structure
                            .loops
                            .iter()
                            .map(|loop_rows| (loop_rows.node, loop_rows.keys.clone()))
                            .collect(),
                    },
                )
            })
            .collect();
        *self.typed_ssr_imported.borrow_mut() = true;
        Ok(())
    }

    /// Applies the snapshot's public state to the shared host inputs. State
    /// initializers run per instance from these inputs, so seeding here is
    /// what makes imported state observable before adoption claims DOM.
    fn seed_ssr_host_inputs(&self, snapshot: &plec_ir::PlecSsrSnapshot) -> Result<(), JsValue> {
        let (pathname, search, hash) = split_ssr_location(&snapshot.public.location);
        let current = window()?
            .location()
            .pathname()
            .unwrap_or_else(|_| "/".into());
        if pathname != current {
            return Err(JsValue::from_str("mismatch:ssr-location"));
        }
        let mut seeded: HashMap<String, RuntimeValue> = HashMap::from([
            ("location.pathname".into(), RuntimeValue::String(pathname)),
            ("location.search".into(), RuntimeValue::String(search)),
            ("location.hash".into(), RuntimeValue::String(hash)),
        ]);
        for (name, export) in &snapshot.public.exports {
            seeded.insert(name.clone(), ssr_snapshot_value(&export.value));
        }
        *self.typed_ssr_host_inputs.borrow_mut() = seeded.keys().cloned().collect();
        self.typed_host_inputs.borrow_mut().extend(seeded);
        Ok(())
    }

    /// Discards only partially reconstructed runtime ownership after an SSR
    /// mismatch. The server DOM is intentionally left intact so the caller
    /// can make the normal mount path the single, observable fallback.
    pub fn abandon_adoption(&self) {
        self.dispose_typed_instances();
        *self.typed_manifest.borrow_mut() = None;
        *self.typed_root.borrow_mut() = None;
        {
            let seeded = self.typed_ssr_host_inputs.borrow().clone();
            let mut host_inputs = self.typed_host_inputs.borrow_mut();
            for key in &seeded {
                host_inputs.remove(key);
            }
        }
        self.typed_ssr_host_inputs.borrow_mut().clear();
        *self.typed_ssr_imported.borrow_mut() = false;
        *self.typed_ssr_route_chain.borrow_mut() = None;
        self.typed_ssr_loaders.borrow_mut().clear();
        self.typed_ssr_branches.borrow_mut().clear();
        self.typed_ssr_loops.borrow_mut().clear();
        self.typed_ssr_nested.borrow_mut().clear();
        self.dispose_router_listeners();
    }
}

/// Snapshot structure validation resolves graph ids against every registered
/// 0.10 application. Lazy route graphs are registered as independent
/// applications, and the snapshot's structure legitimately references graphs
/// (the root layout) from applications other than the most recent one.
struct RegisteredStructureApplication {
    primary: TypedComponentApplication,
    registry: HashMap<String, TypedComponentApplication>,
}

impl plec_ir::SsrStructureApplication for RegisteredStructureApplication {
    fn structure_graph(&self, graph_id: &str) -> Option<&dyn plec_ir::SsrStructureGraph> {
        if let Some(found) = self.primary.structure_graph(graph_id) {
            return Some(found);
        }
        if let Some(found) = self
            .registry
            .get(graph_id)
            .and_then(|application| application.structure_graph(graph_id))
        {
            return Some(found);
        }
        // Nested component records reference compiled component ids, which
        // live inside a registered application's component table rather
        // than being registry entries of their own.
        self.registry
            .values()
            .find_map(|application| application.structure_graph(graph_id))
    }
}

fn split_ssr_location(location: &str) -> (String, String, String) {
    let (path_part, hash) = match location.split_once('#') {
        Some((before, hash)) => (before, format!("#{hash}")),
        None => (location, String::new()),
    };
    match path_part.split_once('?') {
        Some((pathname, search)) => (pathname.into(), format!("?{search}"), hash),
        None => (path_part.into(), String::new(), hash),
    }
}

pub(crate) fn ssr_snapshot_value(value: &plec_ir::SsrSnapshotValue) -> RuntimeValue {
    match value {
        plec_ir::SsrSnapshotValue::Null => RuntimeValue::Null,
        plec_ir::SsrSnapshotValue::Bool(value) => RuntimeValue::Bool(*value),
        plec_ir::SsrSnapshotValue::Number(value) => RuntimeValue::Number(*value),
        plec_ir::SsrSnapshotValue::String(value) => RuntimeValue::String(value.clone()),
        plec_ir::SsrSnapshotValue::Array(values) => {
            RuntimeValue::Array(values.iter().map(ssr_snapshot_value).collect())
        }
        plec_ir::SsrSnapshotValue::Record(values) => RuntimeValue::Record(
            values
                .iter()
                .map(|(key, value)| (key.clone(), ssr_snapshot_value(value)))
                .collect(),
        ),
    }
}

#[wasm_bindgen::prelude::wasm_bindgen]
impl PlecRuntime {
    pub fn navigate(&self, href: String, replace: bool) -> Result<(), JsValue> {
        if self.typed_manifest.borrow().is_some() {
            let root = self
                .typed_root
                .borrow()
                .clone()
                .ok_or_else(|| JsValue::from_str("typed router has not started"))?;
            return self.navigate_typed_route(&href, root, replace, true);
        }
        self.navigate_internal(&href, replace)
    }
}

#[wasm_bindgen::prelude::wasm_bindgen]
impl PlecRuntime {
    pub(crate) fn mount_instance(
        &self,
        instance_id: &str,
        root: Element,
        replace_root: bool,
    ) -> Result<JsValue, JsValue> {
        let app = self.app_for_instance(instance_id)?;
        let doc = document()?;
        let elements = index_elements(&app);
        let texts = index_texts(&app);
        let mut nodes = HashMap::new();
        let contexts = index_contexts(&app);
        let loops = index_loops(&app);
        let conditionals = index_conditionals(&app);
        let environment = context_defaults(&app);
        let root_node = instantiate(
            &doc,
            &app.root_element_id,
            &elements,
            &texts,
            &contexts,
            &loops,
            &conditionals,
            &app.bindings,
            &app.prop_programs,
            &app.expressions,
            &app.events,
            &self.state_scope(instance_id, &app)?,
            &environment,
            &mut nodes,
        )?;
        if replace_root {
            root.set_inner_html("");
        }
        root.append_child(&root_node)?;
        let mut handles = HashMap::new();
        for reference in &app.host_element_refs {
            if let Some(node) = nodes.get(&reference.target_id) {
                for attachment in &reference.attachments {
                    handles.insert(attachment.clone(), node.clone());
                }
                handles.insert(reference.id.clone(), node.clone());
            }
        }
        for binding in &app.bindings {
            if let Some(node) = nodes.get(&binding.target_id) {
                apply_binding_host(
                    node,
                    binding,
                    &app.expressions,
                    &self.state_scope(instance_id, &app)?,
                    &handles,
                )?;
            }
        }
        {
            let mut instances = self.instances.borrow_mut();
            let instance = instances
                .get_mut(instance_id)
                .ok_or_else(|| JsValue::from_str("unknown graph instance"))?;
            instance.dom_nodes = nodes;
            instance.host_refs = handles;
            instance.rows.clear();
            instance.root = Some(root_node);
        }
        self.install_event_listeners(instance_id, &app)?;
        finish(MountMetrics {
            program_revision: app.revision.clone(),
            created_elements: app.elements.len() as u32,
            created_texts: app.texts.len() as u32,
            bindings: app.bindings.len() as u32,
            dom_operations: 1,
            ..Default::default()
        })
    }
}

#[wasm_bindgen::prelude::wasm_bindgen]
impl PlecRuntime {
    pub fn adopt(&self, root: Element) -> Result<JsValue, JsValue> {
        let instance_id = graph_instance_id(None, "main", None);
        if self.instances.borrow().contains_key(&instance_id) {
            self.dispose_graph_instance(instance_id.clone())?;
        }
        self.create_instance(
            instance_id.clone(),
            None,
            "main".into(),
            None,
            "__legacy__".into(),
        )?;
        let app = self.app_for_instance(&instance_id)?;
        let root_selector = format!("[data-runtime-node=\"{}\"]", app.root_element_id);
        let Some(runtime_root) = root.query_selector(&root_selector)? else {
            return serde_wasm_bindgen::to_value(&Option::<MountMetrics>::None).map_err(error);
        };
        let mut nodes = HashMap::new();
        nodes.insert(app.root_element_id.clone(), runtime_root.clone().into());
        let descendants = runtime_root.query_selector_all("[data-runtime-node]")?;
        for index in 0..descendants.length() {
            let Some(node) = descendants.item(index) else {
                continue;
            };
            let Ok(element) = node.clone().dyn_into::<Element>() else {
                continue;
            };
            let Some(id) = element.get_attribute("data-runtime-node") else {
                continue;
            };
            nodes.insert(id, node);
        }
        let mut handles = HashMap::new();
        for reference in &app.host_element_refs {
            if let Some(node) = nodes.get(&reference.target_id) {
                for attachment in &reference.attachments {
                    handles.insert(attachment.clone(), node.clone());
                }
                handles.insert(reference.id.clone(), node.clone());
            }
        }
        for binding in &app.bindings {
            if let Some(node) = nodes.get(&binding.target_id) {
                apply_binding_host(
                    node,
                    binding,
                    &app.expressions,
                    &self.state_scope(&instance_id, &app)?,
                    &handles,
                )?;
            }
        }
        {
            let mut instances = self.instances.borrow_mut();
            let instance = instances
                .get_mut(&instance_id)
                .ok_or_else(|| JsValue::from_str("unknown graph instance"))?;
            instance.dom_nodes = nodes;
            instance.host_refs = handles;
            instance.rows.clear();
            instance.root = Some(runtime_root.into());
        }
        self.install_event_listeners(&instance_id, &app)?;
        serde_wasm_bindgen::to_value(&Some(MountMetrics {
            program_revision: app.revision.clone(),
            dom_operations: 0,
            ..Default::default()
        }))
        .map_err(error)
    }
}

#[wasm_bindgen::prelude::wasm_bindgen]
impl PlecRuntime {
    pub fn dispose(&self) -> Result<(), JsValue> {
        for (_, mut instance) in self.typed.borrow_mut().drain() {
            instance.runtime.invalidate_fetches();
            instance.runtime.clear_listeners();
            instance.runtime.clear_host_refs();
            if let Some(root) = instance.runtime.root {
                root.set_inner_html("");
            }
        }
        if let Some(root) = self.typed_root.borrow_mut().take() {
            root.set_inner_html("");
        }
        self.dispose_router_listeners();
        *self.router.borrow_mut() = None;
        *self.typed_manifest.borrow_mut() = None;
        self.typed_registry.borrow_mut().clear();
        self.typed_component_registry.borrow_mut().clear();
        *self.typed_components.borrow_mut() = None;
        let instance_ids = self.instances.borrow().keys().cloned().collect::<Vec<_>>();
        for instance_id in instance_ids {
            if self.instances.borrow().contains_key(&instance_id) {
                self.dispose_graph_instance(instance_id)?;
            }
        }
        self.registry.borrow_mut().clear();
        Ok(())
    }
}

#[wasm_bindgen::prelude::wasm_bindgen]
impl PlecRuntime {
    pub fn mount_graph_instance(
        &self,
        parent_instance_id: Option<String>,
        outlet_id: String,
        key: Option<String>,
    ) -> Result<String, JsValue> {
        let instance_id =
            graph_instance_id(parent_instance_id.as_deref(), &outlet_id, key.as_deref());
        let mut instances = self.instances.borrow_mut();
        if instances.contains_key(&instance_id) {
            return Err(JsValue::from_str("graph instance mount collision"));
        }
        instances.insert(
            instance_id.clone(),
            GraphInstance {
                parent_id: parent_instance_id,
                outlet_id,
                key,
                graph_id: None,
                local_state: HashMap::new(),
                dom_nodes: HashMap::new(),
                host_refs: HashMap::new(),
                rows: HashMap::new(),
                root: None,
                listeners: Vec::new(),
                continuation_epoch: 0,
                next_request_id: 0,
                #[cfg(feature = "fetch")]
                abort_controllers: HashMap::new(),
                active_effects: 0,
                active_listeners: 0,
            },
        );
        Ok(instance_id)
    }
}

#[wasm_bindgen::prelude::wasm_bindgen]
impl PlecRuntime {
    pub fn dispose_graph_instance(&self, instance_id: String) -> Result<(), JsValue> {
        let mut instances = self.instances.borrow_mut();
        if !instances.contains_key(&instance_id) {
            return Err(JsValue::from_str("unknown graph instance"));
        }
        let descendants = instances
            .iter()
            .filter_map(|(id, instance)| {
                (instance.parent_id.as_deref() == Some(instance_id.as_str())).then(|| id.clone())
            })
            .collect::<Vec<_>>();
        for child in descendants {
            drop(instances);
            self.dispose_graph_instance(child)?;
            instances = self.instances.borrow_mut();
        }
        let mut instance = instances
            .remove(&instance_id)
            .expect("instance checked above");
        drop(instances);
        instance.continuation_epoch = instance.continuation_epoch.saturating_add(1);
        #[cfg(feature = "fetch")]
        for (_, controller) in instance.abort_controllers.drain() {
            controller.abort();
        }
        for listener in instance.listeners {
            let _ = listener.element.remove_event_listener_with_callback(
                &listener.event_type,
                listener.callback.as_ref().unchecked_ref(),
            );
        }
        if let Some(root) = instance.root {
            if let Some(parent) = root.parent_node() {
                parent.remove_child(&root)?;
            }
        }
        Ok(())
    }
}

#[wasm_bindgen::prelude::wasm_bindgen]
impl PlecRuntime {
    pub fn replace_outlet_instance(
        &self,
        parent_instance_id: String,
        outlet_id: String,
        graph_id: String,
    ) -> Result<String, JsValue> {
        if !self.instances.borrow().contains_key(&parent_instance_id) {
            return Err(JsValue::from_str("unknown parent graph instance"));
        }
        let existing = self
            .instances
            .borrow()
            .iter()
            .filter_map(|(id, instance)| {
                (instance.parent_id.as_deref() == Some(parent_instance_id.as_str())
                    && instance.outlet_id == outlet_id)
                    .then(|| id.clone())
            })
            .collect::<Vec<_>>();
        for id in existing {
            self.dispose_graph_instance(id)?;
        }
        if !self.registry.borrow().contains_key(&graph_id) {
            return Err(JsValue::from_str("unregistered graph"));
        }
        let outlet = self.outlet_element(&parent_instance_id, &outlet_id)?;
        let id = graph_instance_id(Some(&parent_instance_id), &outlet_id, None);
        self.create_instance(
            id.clone(),
            Some(parent_instance_id),
            outlet_id,
            None,
            graph_id,
        )?;
        self.mount_instance(&id, outlet, false)?;
        Ok(id)
    }
}

impl PlecRuntime {
    pub(crate) fn legacy_instance_id(&self) -> Result<String, JsValue> {
        let id = graph_instance_id(None, "main", None);
        self.instances
            .borrow()
            .contains_key(&id)
            .then_some(id)
            .ok_or_else(|| JsValue::from_str("mount must be called first"))
    }
}

impl PlecRuntime {
    pub(crate) fn app_for_instance(&self, instance_id: &str) -> Result<Application, JsValue> {
        let graph_id = self
            .instances
            .borrow()
            .get(instance_id)
            .and_then(|instance| instance.graph_id.clone())
            .ok_or_else(|| JsValue::from_str("graph instance has no graph"))?;
        self.registry
            .borrow()
            .get(&graph_id)
            .cloned()
            .ok_or_else(|| JsValue::from_str("graph definition is not registered"))
    }
}

impl PlecRuntime {
    pub(crate) fn create_instance(
        &self,
        id: String,
        parent_id: Option<String>,
        outlet_id: String,
        key: Option<String>,
        graph_id: String,
    ) -> Result<(), JsValue> {
        let app = self
            .registry
            .borrow()
            .get(&graph_id)
            .cloned()
            .ok_or_else(|| JsValue::from_str("unregistered graph"))?;
        let values = app
            .local_states
            .iter()
            .map(|slot| (slot.id.clone(), parse_initial_state(&slot.initial_value)))
            .collect();
        let mut instances = self.instances.borrow_mut();
        if instances.contains_key(&id) {
            return Err(JsValue::from_str("graph instance mount collision"));
        }
        instances.insert(
            id,
            GraphInstance {
                parent_id,
                outlet_id,
                key,
                graph_id: Some(graph_id),
                local_state: values,
                dom_nodes: HashMap::new(),
                host_refs: HashMap::new(),
                rows: HashMap::new(),
                root: None,
                listeners: Vec::new(),
                continuation_epoch: 1,
                next_request_id: 0,
                #[cfg(feature = "fetch")]
                abort_controllers: HashMap::new(),
                active_effects: 0,
                active_listeners: 0,
            },
        );
        Ok(())
    }
}

pub(crate) fn graph_instance_id(parent: Option<&str>, outlet: &str, key: Option<&str>) -> String {
    let segment = |value: &str| value.replace('%', "%25").replace('/', "%2F");
    let parent = parent.map(segment).unwrap_or_else(|| "root".into());
    let key = key
        .map(|value| format!("/key:{}", segment(value)))
        .unwrap_or_default();
    format!("{parent}/outlet:{}{key}", segment(outlet))
}

pub(crate) fn error(error: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&error.to_string())
}

pub(crate) fn finish(metrics: MountMetrics) -> Result<JsValue, JsValue> {
    serde_wasm_bindgen::to_value(&metrics).map_err(error)
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
}
