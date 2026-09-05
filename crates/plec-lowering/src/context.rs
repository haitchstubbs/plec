use std::collections::HashMap;

use plec_hir::{BindingId, ComponentId, HirComponent, HirExpr};
use plec_ir::{CookieCapability, ExecutableApplication, HostSlot, Value};

use crate::LoweringError;

/// component index, declared props (name, callable, component), children slot,
/// direct props-bag parameter.
pub(crate) type ComponentTargets =
    HashMap<ComponentId, (usize, Vec<(String, bool, bool)>, bool, bool)>;

pub(crate) struct Ctx<'a> {
    pub(crate) component: &'a HirComponent,
    pub(crate) app: ExecutableApplication,
    pub(crate) strings: HashMap<String, usize>,
    pub(crate) constants: HashMap<String, usize>,
    pub(crate) states: HashMap<BindingId, usize>,
    pub(crate) refs: HashMap<BindingId, usize>,
    pub(crate) host_refs: HashMap<BindingId, usize>,
    pub(crate) inputs: HashMap<BindingId, usize>,
    pub(crate) hosts: HashMap<BindingId, usize>,
    pub(crate) callables: HashMap<BindingId, usize>,
    pub(crate) props: HashMap<BindingId, usize>,
    pub(crate) callback_props: HashMap<BindingId, usize>,
    pub(crate) action_parameters: HashMap<BindingId, usize>,
    pub(crate) async_slots: HashMap<BindingId, usize>,
    pub(crate) try_failure_relays: Vec<(usize, usize, usize, usize)>,
    pub(crate) targets: Option<&'a ComponentTargets>,
    pub(crate) locals: HashMap<BindingId, plec_hir::ExprId>,
    pub(crate) active_locals: Vec<BindingId>,
    pub(crate) active_loop: Option<usize>,
}
impl<'a> Ctx<'a> {
    pub(crate) fn new(component: &'a HirComponent, targets: Option<&'a ComponentTargets>) -> Self {
        Self {
            component,
            app: ExecutableApplication::default(),
            strings: HashMap::new(),
            constants: HashMap::new(),
            states: HashMap::new(),
            refs: HashMap::new(),
            host_refs: HashMap::new(),
            inputs: HashMap::new(),
            hosts: HashMap::new(),
            callables: HashMap::new(),
            props: HashMap::new(),
            callback_props: HashMap::new(),
            action_parameters: HashMap::new(),
            async_slots: HashMap::new(),
            try_failure_relays: vec![],
            targets,
            locals: component
                .locals
                .iter()
                .map(|v| (v.binding, v.initializer))
                .collect(),
            active_locals: vec![],
            active_loop: None,
        }
    }
    pub(crate) fn err(&self, message: &str) -> LoweringError {
        LoweringError(message.into())
    }
    pub(crate) fn string(&mut self, value: &str) -> usize {
        if let Some(id) = self.strings.get(value) {
            return *id;
        }
        let id = self.app.strings.len();
        self.app.strings.push(value.into());
        self.strings.insert(value.into(), id);
        id
    }
    pub(crate) fn constant(&mut self, value: Value) -> usize {
        let key = format!("{value:?}");
        if let Some(id) = self.constants.get(&key) {
            return *id;
        }
        let id = self.app.constants.len();
        self.app.constants.push(value);
        self.constants.insert(key, id);
        id
    }
    pub(crate) fn host(
        &mut self,
        kind: &'static str,
        name: Option<&str>,
    ) -> Result<usize, LoweringError> {
        if kind == "cookie" {
            let name = name.expect("cookie host slots require a static name");
            self.cookie_capability("getSync", name, "/", None, None, None)?;
        }
        let name = name.map(|name| self.string(name));
        if let Some((index, _)) = self
            .app
            .host_slots
            .iter()
            .enumerate()
            .find(|(_, slot)| slot.kind == kind && slot.name == name && slot.query.is_none())
        {
            return Ok(index);
        }
        let index = self.app.host_slots.len();
        self.app.host_slots.push(HostSlot {
            kind,
            query: None,
            name,
        });
        Ok(index)
    }
    pub(crate) fn cookie_capability(
        &mut self,
        operation: &'static str,
        name: &str,
        path: &str,
        same_site: Option<&str>,
        secure: Option<bool>,
        max_age: Option<i64>,
    ) -> Result<(), LoweringError> {
        let expiry = if max_age.is_some() {
            "maxAge"
        } else {
            "session"
        };
        if let Some(capability) = self
            .app
            .capabilities
            .iter_mut()
            .find(|capability| capability.name == name)
        {
            if capability.path != path
                || capability.same_site.as_deref() != same_site
                || capability.secure != secure
            {
                return Err(self.err("cookie capability options conflict for the same name"));
            }
            if !capability.operations.contains(&operation) {
                capability.operations.push(operation);
            }
            if !capability.expiry_modes.contains(&expiry) {
                capability.expiry_modes.push(expiry);
            }
            return Ok(());
        }
        self.app.capabilities.push(CookieCapability {
            kind: "cookie",
            name: name.into(),
            operations: vec![operation],
            path: path.into(),
            same_site: same_site.map(str::to_owned),
            secure,
            expiry_modes: vec![expiry],
        });
        Ok(())
    }
    pub(crate) fn expr(&self, id: plec_hir::ExprId) -> Result<&HirExpr, LoweringError> {
        self.component
            .expressions
            .get(id.0 as usize)
            .map(|e| &e.expression)
            .ok_or_else(|| self.err("expression handle missing"))
    }
}
