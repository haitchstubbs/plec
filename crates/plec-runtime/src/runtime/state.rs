use crate::dom::util::*;
use crate::runtime::lifecycle::*;

pub(crate) struct Row {
    pub(crate) root: Node,
    pub(crate) values: HashMap<String, Value>,
    pub(crate) nodes: HashMap<String, Node>,
}

impl PlecRuntime {
    pub(crate) fn state_scope(
        &self,
        instance_id: &str,
        app: &Application,
    ) -> Result<HashMap<String, Value>, JsValue> {
        let instances = self.instances.borrow();
        let values = &instances
            .get(instance_id)
            .ok_or_else(|| JsValue::from_str("unknown graph instance"))?
            .local_state;
        Ok(app
            .local_states
            .iter()
            .filter_map(|slot| {
                values
                    .get(&slot.id)
                    .map(|value| (slot.name.clone(), value.clone()))
            })
            .collect())
    }
}

impl PlecRuntime {
    pub(crate) fn row_scope_for_event(
        &self,
        instance_id: &str,
        app: &Application,
        loop_id: &str,
        key: Option<&Value>,
    ) -> Option<HashMap<String, Value>> {
        let loop_node = app.loops.iter().find(|entry| entry.id == loop_id)?;
        let input_id = loop_node.input_id.as_ref()?;
        let key = key?.as_str()?;
        let instances = self.instances.borrow();
        let row = instances.get(instance_id)?.rows.get(input_id)?.get(key)?;
        Some(row_scope(&loop_node.item_name, &row.values))
    }
}
