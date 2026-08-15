use crate::dom::{bindings::*, instantiate::*, platform::*, properties::*, util::*};
use crate::eval::{dependencies::*, value::*};
use crate::runtime::lifecycle::*;
use crate::runtime::state::*;

#[wasm_bindgen::prelude::wasm_bindgen]
impl PlecRuntime {
    pub fn initialize_input(&self, input_id: String, rows: JsValue) -> Result<JsValue, JsValue> {
        if !self.typed.borrow().is_empty() {
            return self.initialize_typed_input(&input_id, rows);
        }
        let instance_id = self.legacy_instance_id()?;
        self.initialize_input_for(&instance_id, input_id, rows)
    }
}

#[wasm_bindgen::prelude::wasm_bindgen]
impl PlecRuntime {
    pub fn initialize_instance_input(
        &self,
        instance_id: String,
        input_id: String,
        rows: JsValue,
    ) -> Result<JsValue, JsValue> {
        self.initialize_input_for(&instance_id, input_id, rows)
    }
}

#[wasm_bindgen::prelude::wasm_bindgen]
impl PlecRuntime {
    pub(crate) fn initialize_input_for(
        &self,
        instance_id: &str,
        input_id: String,
        rows: JsValue,
    ) -> Result<JsValue, JsValue> {
        let values: Vec<HashMap<String, Value>> =
            serde_wasm_bindgen::from_value(rows).map_err(error)?;
        let app = self.app_for_instance(instance_id)?;
        let loop_node = find_loop(&app, &input_id)?;
        let parent = self
            .instances
            .borrow()
            .get(instance_id)
            .ok_or_else(|| JsValue::from_str("unknown graph instance"))?
            .dom_nodes
            .get(&loop_node.parent_id)
            .cloned()
            .ok_or_else(|| JsValue::from_str("loop parent missing"))?;
        let parent: Element = parent
            .dyn_into()
            .map_err(|_| JsValue::from_str("loop parent"))?;
        let root_id = loop_node
            .row_template_root_element_id
            .ok_or_else(|| JsValue::from_str("row template missing"))?;
        let doc = document()?;
        let elements = index_elements(&app);
        let texts = index_texts(&app);
        let fragment = doc.create_document_fragment();
        let mut collected = HashMap::new();
        let contexts = index_contexts(&app);
        let loops = index_loops(&app);
        let conditionals = index_conditionals(&app);
        let environment = context_defaults(&app);
        for value in values {
            let key = value_string(value.get("id"));
            let scope = row_scope(&loop_node.item_name, &value);
            let mut nodes = HashMap::new();
            let row_root = instantiate(
                &doc,
                &root_id,
                &elements,
                &texts,
                &contexts,
                &loops,
                &conditionals,
                &app.bindings,
                &app.prop_programs,
                &app.expressions,
                &app.events,
                &scope,
                &environment,
                &mut nodes,
            )?;
            if let Ok(element) = row_root.clone().dyn_into::<Element>() {
                element.set_attribute("data-runtime-row-key", &key)?;
            }
            self.install_event_listeners_for_nodes(instance_id, &app, &nodes)?;
            fragment.append_child(&row_root)?;
            collected.insert(
                key,
                Row {
                    root: row_root,
                    values: value,
                    nodes,
                },
            );
        }
        parent.append_child(&fragment)?;
        let count = collected.len() as u32;
        self.instances
            .borrow_mut()
            .get_mut(instance_id)
            .ok_or_else(|| JsValue::from_str("unknown graph instance"))?
            .rows
            .insert(input_id, collected);
        finish(MountMetrics {
            row_count: count,
            dom_operations: 1,
            ..Default::default()
        })
    }
}

impl PlecRuntime {
    pub(crate) fn update(
        &self,
        instance_id: &str,
        input: &str,
        key: &str,
        changes: HashMap<String, Value>,
        m: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let app = self.app_for_instance(instance_id)?;
        let changed = changes.keys().cloned().collect::<HashSet<_>>();
        let expressions = app
            .expressions
            .iter()
            .map(|entry| (entry.id.as_str(), &entry.expression))
            .collect::<HashMap<_, _>>();
        let mut instances = self.instances.borrow_mut();
        let rows = &mut instances
            .get_mut(instance_id)
            .ok_or_else(|| JsValue::from_str("unknown graph instance"))?
            .rows;
        let row = rows
            .get_mut(input)
            .and_then(|items| items.get_mut(key))
            .ok_or_else(|| JsValue::from_str("row missing"))?;
        row.values.extend(changes);
        for binding in &app.bindings {
            if !binding_dependencies(binding, &expressions)
                .iter()
                .any(|field| changed.contains(field))
            {
                continue;
            }
            if let Some(node) = row.nodes.get(&binding.target_id) {
                apply_binding(node, binding, &app.expressions, &row.values)?;
                m.bindings_touched += 1;
                m.nodes_touched += 1;
                m.dom_operations += 1;
            }
        }
        for program in &app.prop_programs {
            if !program_dependencies(program, &expressions)
                .iter()
                .any(|field| changed.contains(field))
            {
                continue;
            }
            if let Some(node) = row.nodes.get(&program.target_id) {
                apply_prop_program(node, program, &app.expressions, &row.values)?;
                m.nodes_touched += 1;
                m.dom_operations += 1;
            }
        }
        Ok(())
    }
}

impl PlecRuntime {
    pub(crate) fn insert(
        &self,
        instance_id: &str,
        input: &str,
        key: String,
        value: HashMap<String, Value>,
        before: Option<String>,
        m: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let app = self.app_for_instance(instance_id)?;
        let loop_node = find_loop(&app, input)?;
        let parent = self
            .instances
            .borrow()
            .get(instance_id)
            .ok_or_else(|| JsValue::from_str("unknown graph instance"))?
            .dom_nodes
            .get(&loop_node.parent_id)
            .cloned()
            .ok_or_else(|| JsValue::from_str("loop parent missing"))?;
        let parent: Element = parent
            .dyn_into()
            .map_err(|_| JsValue::from_str("loop parent"))?;
        let doc = document()?;
        let mut nodes = HashMap::new();
        let elements = index_elements(&app);
        let texts = index_texts(&app);
        let contexts = index_contexts(&app);
        let loops = index_loops(&app);
        let conditionals = index_conditionals(&app);
        let environment = context_defaults(&app);
        let scope = row_scope(&loop_node.item_name, &value);
        let root = instantiate(
            &doc,
            &loop_node
                .row_template_root_element_id
                .ok_or_else(|| JsValue::from_str("row template missing"))?,
            &elements,
            &texts,
            &contexts,
            &loops,
            &conditionals,
            &app.bindings,
            &app.prop_programs,
            &app.expressions,
            &app.events,
            &scope,
            &environment,
            &mut nodes,
        )?;
        if let Ok(element) = root.clone().dyn_into::<Element>() {
            element.set_attribute("data-runtime-row-key", &key)?;
        }
        self.install_event_listeners_for_nodes(instance_id, &app, &nodes)?;
        let anchor = before.and_then(|id| {
            self.instances
                .borrow()
                .get(instance_id)
                .and_then(|instance| instance.rows.get(input))
                .and_then(|rows| rows.get(&id))
                .map(|row| row.root.clone())
        });
        parent.insert_before(&root, anchor.as_ref())?;
        self.instances
            .borrow_mut()
            .get_mut(instance_id)
            .ok_or_else(|| JsValue::from_str("unknown graph instance"))?
            .rows
            .entry(input.into())
            .or_default()
            .insert(
                key,
                Row {
                    root,
                    values: value,
                    nodes,
                },
            );
        m.dom_operations += 1;
        Ok(())
    }
}

impl PlecRuntime {
    pub(crate) fn remove(
        &self,
        instance_id: &str,
        input: &str,
        key: &str,
        m: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let removed = if let Some(row) = self
            .instances
            .borrow_mut()
            .get_mut(instance_id)
            .ok_or_else(|| JsValue::from_str("unknown graph instance"))?
            .rows
            .get_mut(input)
            .and_then(|rows| rows.remove(key))
        {
            row.root
                .parent_node()
                .map(|parent| parent.remove_child(&row.root))
                .transpose()?;
            m.dom_operations += 1;
            true
        } else {
            false
        };
        if removed {
            self.rebuild_event_listeners(instance_id, &self.app_for_instance(instance_id)?)?;
        }
        Ok(())
    }
}

impl PlecRuntime {
    pub(crate) fn move_row(
        &self,
        instance_id: &str,
        input: &str,
        key: &str,
        before: Option<String>,
        m: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let app = self.app_for_instance(instance_id)?;
        let parent: Element = self
            .instances
            .borrow()
            .get(instance_id)
            .ok_or_else(|| JsValue::from_str("unknown graph instance"))?
            .dom_nodes
            .get(&find_loop(&app, input)?.parent_id)
            .cloned()
            .ok_or_else(|| JsValue::from_str("loop parent missing"))?
            .dyn_into()
            .map_err(|_| JsValue::from_str("loop parent"))?;
        let row = self
            .instances
            .borrow()
            .get(instance_id)
            .ok_or_else(|| JsValue::from_str("unknown graph instance"))?
            .rows
            .get(input)
            .and_then(|rows| rows.get(key))
            .ok_or_else(|| JsValue::from_str("row missing"))?
            .root
            .clone();
        let anchor = before.and_then(|id| {
            self.instances
                .borrow()
                .get(instance_id)
                .and_then(|instance| instance.rows.get(input))
                .and_then(|rows| rows.get(&id))
                .map(|row| row.root.clone())
        });
        parent.insert_before(&row, anchor.as_ref())?;
        m.dom_operations += 1;
        Ok(())
    }
}
