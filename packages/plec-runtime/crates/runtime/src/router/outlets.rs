use crate::runtime::lifecycle::*;

impl PlecRuntime {
    pub(crate) fn outlet_element(
        &self,
        parent_id: &str,
        outlet_id: &str,
    ) -> Result<Element, JsValue> {
        let app = self.app_for_instance(parent_id)?;
        let element_id = app
            .layout
            .route_outlets
            .iter()
            .find(|outlet| outlet.id == outlet_id)
            .map(|outlet| outlet.element_id.as_str())
            .unwrap_or(outlet_id);
        self.instances
            .borrow()
            .get(parent_id)
            .and_then(|instance| instance.dom_nodes.get(element_id))
            .cloned()
            .ok_or_else(|| JsValue::from_str("route outlet missing"))?
            .dyn_into()
            .map_err(|_| JsValue::from_str("route outlet is not an element"))
    }
}
