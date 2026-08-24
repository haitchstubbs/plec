use std::collections::HashMap;

use plec_hir::{BindingId, ComponentId, HirComponent, HirExpr};
use plec_ir::{ExecutableApplication, Value};

use crate::LoweringError;

pub(crate) type ComponentTargets = HashMap<ComponentId, (usize, Vec<(String, bool)>, bool)>;

pub(crate) struct Ctx<'a> {
    pub(crate) component: &'a HirComponent,
    pub(crate) app: ExecutableApplication,
    pub(crate) strings: HashMap<String, usize>,
    pub(crate) constants: HashMap<String, usize>,
    pub(crate) states: HashMap<BindingId, usize>,
    pub(crate) inputs: HashMap<BindingId, usize>,
    pub(crate) callables: HashMap<BindingId, usize>,
    pub(crate) props: HashMap<BindingId, usize>,
    pub(crate) callback_props: HashMap<BindingId, usize>,
    pub(crate) action_parameters: HashMap<BindingId, usize>,
    pub(crate) async_slots: HashMap<BindingId, usize>,
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
            inputs: HashMap::new(),
            callables: HashMap::new(),
            props: HashMap::new(),
            callback_props: HashMap::new(),
            action_parameters: HashMap::new(),
            async_slots: HashMap::new(),
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
    pub(crate) fn expr(&self, id: plec_hir::ExprId) -> Result<&HirExpr, LoweringError> {
        self.component
            .expressions
            .get(id.0 as usize)
            .map(|e| &e.expression)
            .ok_or_else(|| self.err("expression handle missing"))
    }
}
