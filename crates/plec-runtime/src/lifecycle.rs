//! Runtime lifecycle.

//! Supported contract: IR 0.10 component applications + route manifest v3 +
//! the SSR v2 execution snapshot (see docs/dom-address-protocol.md).

use plec_client::prelude::*;
use plec_client::route::ssr_snapshot_value;
use plec_client::runtime::{SsrNestedComponentRecords, TypedGraphInstance, TypedRuntime};
use plec_client::state::{graph_instance_id, RuntimeState};
use plec_dom::platform::window;
use plec_router::listeners::{dispose_router_listeners, install_router_listeners};
use plec_router::navigation::{adopt_typed_route, navigate_typed_route, validate_typed_manifest};

use super::snapshots::SnapshotInput;

#[derive(Clone)]
#[wasm_bindgen]
pub struct PlecRuntime {
    pub(crate) state: Rc<RuntimeState>,
    pub(crate) snapshot_inputs: Rc<RefCell<HashMap<String, SnapshotInput>>>,
}

#[wasm_bindgen::prelude::wasm_bindgen]
impl PlecRuntime {
    pub fn load_application(&self, ir: JsValue) -> Result<(), JsValue> {
        let value: Value = serde_wasm_bindgen::from_value(ir.clone()).map_err(error)?;

        if value.get("version").and_then(Value::as_str) == Some("0.10") {
            let application: TypedComponentApplication =
                serde_json::from_value(value).map_err(error)?;
            application.validate()?;
            let mut typed =
                TypedRuntime::new(application.components[application.root_component].clone())?;
            typed.set_component_definitions(application.components.clone());
            typed.set_host_inputs(self.state.typed_host_inputs.borrow().clone())?;
            typed.graph_generation = self.state.next_typed_generation();
            self.dispose_typed_instances();
            *self.state.typed_components.borrow_mut() = Some(application);
            self.state.typed.borrow_mut().insert(
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
        // The IR 0.8 string-id graph scheme was removed (wasm-runtime-ixk.7):
        // no compiler emits it and no in-repo consumer reads it.
        Err(JsValue::from_str("unsupported application IR version"))
    }
}

impl PlecRuntime {
    pub(crate) fn dispose_typed_instances(&self) {
        for (_, mut instance) in self.state.typed.borrow_mut().drain() {
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
        *self.state.typed_host_inputs.borrow_mut() = values;
        Ok(())
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
            self.state
                .typed_component_registry
                .borrow_mut()
                .insert(graph_id, application.clone());
            *self.state.typed_components.borrow_mut() = Some(application);
            return Ok(());
        }
        // The IR 0.8 string-id graph scheme was removed (wasm-runtime-ixk.7):
        // no compiler emits it and no in-repo consumer reads it.
        Err(JsValue::from_str("unsupported application IR version"))
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
            typed_route_manifest(manifest)
        } else {
            // Lenient internal parse for v3 manifests without a revision (the
            // route-less test/minimal shape). Anything below v3 was the
            // removed legacy router (wasm-runtime-ixk.7) and fails here.
            serde_json::from_value(manifest_value).map_err(error)?
        };
        if manifest.version != Some(3) {
            return Err(JsValue::from_str("unsupported route manifest version"));
        }
        validate_typed_manifest(&self.state, &manifest)?;
        *self.state.typed_manifest.borrow_mut() = Some(manifest);
        *self.state.typed_root.borrow_mut() = Some(root.clone());
        dispose_router_listeners(&self.state);
        install_router_listeners(&self.state)?;
        let location = window()?.location();
        let href = format!(
            "{}{}{}",
            location.pathname().unwrap_or_else(|_| "/".into()),
            location.search().unwrap_or_default(),
            location.hash().unwrap_or_default(),
        );
        navigate_typed_route(&self.state, &href, root, true, false)
    }

    /// Starts a typed route tree from server-rendered DOM, importing the SSR
    /// execution snapshot (the v2 bootstrap payload). This is separate from
    /// `start` so normal client mounts never *read* markers: both paths
    /// emit the same canonical structural addresses, but only adoption
    /// resolves them (see docs/dom-address-protocol.md). The snapshot is
    /// parsed, version/revision gated, and fully validated against the
    /// manifest plus the registered component application **before** adoption
    /// claims DOM; its public state is seeded into the host inputs so adopted
    /// instances evaluate from imported causes instead of blank
    /// initializers. Any snapshot failure is a fail-closed code; the caller
    /// falls back to a normal mount.
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
        if !self.state.typed.borrow().is_empty() {
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
        let manifest = typed_route_manifest(manifest);
        validate_typed_manifest(&self.state, &manifest)?;
        *self.state.typed_manifest.borrow_mut() = Some(manifest);
        *self.state.typed_root.borrow_mut() = Some(root.clone());
        dispose_router_listeners(&self.state);
        install_router_listeners(&self.state)?;
        let location = window()?.location();
        let href = format!(
            "{}{}{}",
            location.pathname().unwrap_or_else(|_| "/".into()),
            location.search().unwrap_or_default(),
            location.hash().unwrap_or_default(),
        );
        adopt_typed_route(&self.state, &href, root)
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
            *self.state.typed_ssr_imported.borrow_mut() = false;
            *self.state.typed_ssr_route_chain.borrow_mut() = None;
            self.state.typed_ssr_loaders.borrow_mut().clear();
            self.state.typed_ssr_branches.borrow_mut().clear();
            self.state.typed_ssr_loops.borrow_mut().clear();
            self.state.typed_ssr_nested.borrow_mut().clear();
            return Ok(());
        }
        self.state.reset_ssr_text_divergences();
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
            .state
            .typed_components
            .borrow()
            .clone()
            .ok_or_else(|| JsValue::from_str("mismatch:ssr-snapshot-graphs"))?;
        let structure = RegisteredStructureApplication {
            primary: application,
            registry: self.state.typed_component_registry.borrow().clone(),
        };
        parsed
            .validate(&plec_ir::SsrSnapshotReferences {
                manifest,
                application: &structure,
            })
            .map_err(|message| JsValue::from_str(&format!("mismatch:ssr-snapshot:{message}")))?;
        self.seed_ssr_host_inputs(&parsed)?;
        *self.state.typed_ssr_loaders.borrow_mut() = parsed
            .loaders
            .iter()
            .map(|outcome| {
                (
                    plec_ir::loader_ref(&outcome.graph_id, outcome.action),
                    outcome.state.clone(),
                )
            })
            .collect();
        *self.state.typed_ssr_route_chain.borrow_mut() = Some(parsed.routes);
        // Branch selections become the adoption ownership map for conditional
        // regions, keyed by the same graph instance ids the adopter uses.
        *self.state.typed_ssr_branches.borrow_mut() = parsed
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
        *self.state.typed_ssr_loops.borrow_mut() = parsed
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
        *self.state.typed_ssr_nested.borrow_mut() = parsed
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
        *self.state.typed_ssr_imported.borrow_mut() = true;
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
        *self.state.typed_ssr_host_inputs.borrow_mut() = seeded.keys().cloned().collect();
        self.state.typed_host_inputs.borrow_mut().extend(seeded);
        Ok(())
    }

    /// Discards only partially reconstructed runtime ownership after an SSR
    /// mismatch. The server DOM is intentionally left intact so the caller
    /// can make the normal mount path the single, observable fallback.
    pub fn abandon_adoption(&self) {
        self.dispose_typed_instances();
        *self.state.typed_manifest.borrow_mut() = None;
        *self.state.typed_root.borrow_mut() = None;
        {
            let seeded = self.state.typed_ssr_host_inputs.borrow().clone();
            let mut host_inputs = self.state.typed_host_inputs.borrow_mut();
            for key in &seeded {
                host_inputs.remove(key);
            }
        }
        self.state.typed_ssr_host_inputs.borrow_mut().clear();
        *self.state.typed_ssr_imported.borrow_mut() = false;
        *self.state.typed_ssr_route_chain.borrow_mut() = None;
        self.state.typed_ssr_loaders.borrow_mut().clear();
        self.state.typed_ssr_branches.borrow_mut().clear();
        self.state.typed_ssr_loops.borrow_mut().clear();
        self.state.typed_ssr_nested.borrow_mut().clear();
        dispose_router_listeners(&self.state);
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

#[wasm_bindgen::prelude::wasm_bindgen]
impl PlecRuntime {
    pub fn navigate(&self, href: String, replace: bool) -> Result<(), JsValue> {
        let root = self
            .state
            .typed_root
            .borrow()
            .clone()
            .ok_or_else(|| JsValue::from_str("typed router has not started"))?;
        navigate_typed_route(&self.state, &href, root, replace, true)
    }
}

/// Converts the validated `plec-ir` route manifest into the runtime's
/// execution manifest. The `loader`/`loader_state_slot_id` slots of the
/// removed legacy router have no typed equivalent and stay unset.
pub(crate) fn typed_route_manifest(manifest: plec_ir::RouteManifest) -> RouteManifest {
    RouteManifest {
        version: Some(manifest.version),
        root_graph_id: manifest.root_graph_id,
        routes: manifest
            .routes
            .into_iter()
            .map(|route| plec_schema::routing::RouteManifestEntry {
                id: route.id,
                parent_id: route.parent_id,
                path: route.path,
                graph_id: route.graph_id,
                pending_graph_id: route.pending_graph_id,
                pending_mode: route.pending_mode,
                error_graph_id: route.error_graph_id,
                outlet_id: route.outlet_id,
                loader_action: route.loader_action,
            })
            .collect(),
    }
}

#[wasm_bindgen::prelude::wasm_bindgen]
impl PlecRuntime {
    pub fn dispose(&self) -> Result<(), JsValue> {
        for (_, mut instance) in self.state.typed.borrow_mut().drain() {
            instance.runtime.invalidate_fetches();
            instance.runtime.clear_listeners();
            instance.runtime.clear_host_refs();
            if let Some(root) = instance.runtime.root {
                root.set_inner_html("");
            }
        }
        if let Some(root) = self.state.typed_root.borrow_mut().take() {
            root.set_inner_html("");
        }
        dispose_router_listeners(&self.state);
        *self.state.typed_manifest.borrow_mut() = None;
        self.state.typed_component_registry.borrow_mut().clear();
        *self.state.typed_components.borrow_mut() = None;
        Ok(())
    }
}

pub(crate) fn error(error: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&error.to_string())
}

#[wasm_bindgen]
impl PlecRuntime {
    #[wasm_bindgen(constructor)]
    pub fn new() -> PlecRuntime {
        PlecRuntime {
            state: Rc::new(RuntimeState::new()),
            snapshot_inputs: Rc::new(RefCell::new(HashMap::new())),
        }
    }
}

/// Facade delegates. The executable logic lives on `RuntimeState`
/// (plec-client) and in `plec-router`; these bindings preserve the exact
/// JS-facing API the pre-split cdylib exported.
#[wasm_bindgen]
impl PlecRuntime {
    pub fn mount(&self, root: Element) -> Result<JsValue, JsValue> {
        self.state.mount(root)
    }

    pub fn apply_delta(&self, delta: JsValue) -> Result<JsValue, JsValue> {
        self.state.apply_delta(delta)
    }

    pub fn apply_deltas(&self, deltas: JsValue) -> Result<JsValue, JsValue> {
        self.state.apply_deltas(deltas)
    }

    pub fn initialize_input(&self, input_id: String, rows: JsValue) -> Result<JsValue, JsValue> {
        self.state.initialize_input(input_id, rows)
    }

    pub fn list_input_instances(&self, input_id: String) -> Result<JsValue, JsValue> {
        self.state.list_input_instances(input_id)
    }

    pub fn adoption_index_walks(&self) -> u32 {
        self.state.adoption_index_walks()
    }

    pub fn reset_adoption_index_walks(&self) {
        self.state.reset_adoption_index_walks()
    }

    pub fn ssr_text_divergences(&self) -> u32 {
        self.state.ssr_text_divergences()
    }

    pub fn reset_ssr_text_divergences(&self) {
        self.state.reset_ssr_text_divergences()
    }

    pub fn set_cookie_policy(&self, policy: JsValue) -> Result<(), JsValue> {
        self.state.set_cookie_policy(policy)
    }
}
