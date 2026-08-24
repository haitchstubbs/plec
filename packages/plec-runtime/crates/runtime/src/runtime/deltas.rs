use crate::dom::{bindings::*, instantiate::*, platform::*, properties::*, util::*};
use crate::eval::{expression::*, value::*};
use crate::runtime::lifecycle::*;

#[wasm_bindgen::prelude::wasm_bindgen]
impl PlecRuntime {
    pub fn apply_instance_delta(
        &self,
        instance_id: String,
        delta: JsValue,
    ) -> Result<JsValue, JsValue> {
        self.apply_delta_for(&instance_id, delta)
    }
}

#[wasm_bindgen::prelude::wasm_bindgen]
impl PlecRuntime {
    pub(crate) fn apply_delta_for(
        &self,
        instance_id: &str,
        delta: JsValue,
    ) -> Result<JsValue, JsValue> {
        let start = now();
        let delta: Delta = serde_wasm_bindgen::from_value(delta).map_err(error)?;
        let mut metrics = UpdateMetrics::default();
        match delta {
            Delta::Update {
                input_id,
                row_key,
                changes,
                ..
            } => self.update(instance_id, &input_id, &row_key, changes, &mut metrics)?,
            Delta::Insert {
                input_id,
                row_key,
                row,
                before_row_key,
                ..
            } => self.insert(
                instance_id,
                &input_id,
                row_key,
                row,
                before_row_key,
                &mut metrics,
            )?,
            Delta::Remove {
                input_id, row_key, ..
            } => self.remove(instance_id, &input_id, &row_key, &mut metrics)?,
            Delta::Move {
                input_id,
                row_key,
                before_row_key,
                ..
            } => self.move_row(
                instance_id,
                &input_id,
                &row_key,
                before_row_key,
                &mut metrics,
            )?,
        };
        metrics.wasm_dom_us = (now() - start) * 1000.0;
        serde_wasm_bindgen::to_value(&metrics).map_err(error)
    }
}

#[wasm_bindgen::prelude::wasm_bindgen]
impl PlecRuntime {
    pub fn apply_deltas(&self, deltas: JsValue) -> Result<JsValue, JsValue> {
        let values: Vec<Value> = serde_wasm_bindgen::from_value(deltas).map_err(error)?;
        let mut total = UpdateMetrics::default();
        for value in values {
            let result: UpdateMetrics = serde_wasm_bindgen::from_value(
                self.apply_delta(serde_wasm_bindgen::to_value(&value).map_err(error)?)?,
            )
            .map_err(error)?;
            total.dom_operations += result.dom_operations;
            total.nodes_touched += result.nodes_touched;
            total.bindings_touched += result.bindings_touched;
            total.wasm_dom_us += result.wasm_dom_us;
        }
        serde_wasm_bindgen::to_value(&total).map_err(error)
    }
}

impl PlecRuntime {
    pub(crate) fn refresh_state_bindings(
        &self,
        instance_id: &str,
        app: &Application,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        self.sync_todo_state_loops(instance_id, app, metrics)?;
        self.refresh_row_conditionals_and_bindings(instance_id, app, metrics)?;
        let scope = self.state_scope(instance_id, app)?;
        let instances = self.instances.borrow();
        let instance = instances
            .get(instance_id)
            .ok_or_else(|| JsValue::from_str("unknown graph instance"))?;
        let nodes = &instance.dom_nodes;
        let handles = &instance.host_refs;
        for binding in &app.bindings {
            let Some(node) = nodes.get(&binding.target_id) else {
                continue;
            };
            apply_binding_host(node, binding, &app.expressions, &scope, &handles)?;
            metrics.dom_operations += 1;
            metrics.nodes_touched += 1;
            metrics.bindings_touched += 1;
        }
        for program in &app.prop_programs {
            let Some(node) = nodes.get(&program.target_id) else {
                continue;
            };
            apply_prop_program(node, program, &app.expressions, &scope)?;
            metrics.dom_operations += 1;
            metrics.nodes_touched += 1;
        }
        Ok(())
    }
}

impl PlecRuntime {
    pub(crate) fn refresh_row_conditionals_and_bindings(
        &self,
        instance_id: &str,
        app: &Application,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let loops = app
            .loops
            .iter()
            .filter(|entry| entry.input_id.is_some())
            .cloned()
            .collect::<Vec<_>>();
        let mut changed = false;
        for loop_node in &loops {
            let Some(input_id) = loop_node.input_id.as_deref() else {
                continue;
            };
            let keys = self
                .instances
                .borrow()
                .get(instance_id)
                .and_then(|instance| instance.rows.get(input_id))
                .map(|rows| rows.keys().cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            for key in keys {
                if self.refresh_row_conditionals(
                    instance_id,
                    app,
                    loop_node,
                    input_id,
                    &key,
                    metrics,
                )? {
                    changed = true;
                }
            }
        }
        if changed {
            self.rebuild_event_listeners(instance_id, app)?;
        }
        for loop_node in &loops {
            let Some(input_id) = loop_node.input_id.as_deref() else {
                continue;
            };
            let rows = self
                .instances
                .borrow()
                .get(instance_id)
                .and_then(|instance| instance.rows.get(input_id))
                .map(|rows| {
                    rows.values()
                        .map(|row| (row.values.clone(), row.nodes.clone()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            for (values, nodes) in rows {
                let mut scope = self.state_scope(instance_id, app)?;
                scope.extend(row_scope(&loop_node.item_name, &values));
                for binding in &app.bindings {
                    if let Some(node) = nodes.get(&binding.target_id) {
                        apply_binding(node, binding, &app.expressions, &scope)?;
                        metrics.dom_operations += 1;
                        metrics.nodes_touched += 1;
                        metrics.bindings_touched += 1;
                    }
                }
                for program in &app.prop_programs {
                    if let Some(node) = nodes.get(&program.target_id) {
                        apply_prop_program(node, program, &app.expressions, &scope)?;
                        metrics.dom_operations += 1;
                        metrics.nodes_touched += 1;
                    }
                }
            }
        }
        Ok(())
    }
}

impl PlecRuntime {
    pub(crate) fn refresh_row_conditionals(
        &self,
        instance_id: &str,
        app: &Application,
        loop_node: &Loop,
        input_id: &str,
        key: &str,
        metrics: &mut UpdateMetrics,
    ) -> Result<bool, JsValue> {
        let snapshot = self
            .instances
            .borrow()
            .get(instance_id)
            .and_then(|instance| instance.rows.get(input_id))
            .and_then(|rows| rows.get(key))
            .map(|row| (row.values.clone(), row.nodes.clone()));
        let Some((values, nodes)) = snapshot else {
            return Ok(false);
        };
        let mut scope = self.state_scope(instance_id, app)?;
        scope.extend(row_scope(&loop_node.item_name, &values));
        let elements = index_elements(app);
        let texts = index_texts(app);
        let contexts = index_contexts(app);
        let loops = index_loops(app);
        let conditionals = index_conditionals(app);
        let environment = context_defaults(app);
        let mut changed = false;
        for conditional in &app.conditionals {
            let Some(start) = nodes.get(&conditional.id).cloned() else {
                continue;
            };
            let Ok(comment) = start.clone().dyn_into::<Comment>() else {
                continue;
            };
            let desired = app
                .expressions
                .iter()
                .find(|entry| entry.id == conditional.expression_id)
                .map(|entry| {
                    truthy(&evaluate_with_context(
                        &entry.expression,
                        &scope,
                        &environment,
                    ))
                })
                .unwrap_or(false);
            let marker = format!(
                "plec:conditional:{}:{}",
                conditional.id,
                if desired { 1 } else { 0 }
            );
            if comment.data() == marker {
                continue;
            }
            let end_marker = format!("plec:conditional-end:{}", conditional.id);
            let mut cursor = start.next_sibling();
            let mut end = None;
            while let Some(node) = cursor {
                if node
                    .clone()
                    .dyn_into::<Comment>()
                    .ok()
                    .map(|entry| entry.data() == end_marker)
                    .unwrap_or(false)
                {
                    end = Some(node);
                    break;
                }
                cursor = node.next_sibling();
            }
            let end = end.ok_or_else(|| JsValue::from_str("conditional end marker missing"))?;
            while let Some(node) = start.next_sibling() {
                if node.is_same_node(Some(&end)) {
                    break;
                }
                node.parent_node()
                    .ok_or_else(|| JsValue::from_str("conditional parent missing"))?
                    .remove_child(&node)?;
                metrics.dom_operations += 1;
            }
            let stale = conditional_branch_node_ids(app, conditional);
            let mut fresh = HashMap::new();
            let selected = if desired {
                &conditional.consequent
            } else {
                &conditional.alternate
            };
            for child_id in selected {
                let child = instantiate(
                    &document()?,
                    child_id,
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
                    &mut fresh,
                )?;
                end.parent_node()
                    .ok_or_else(|| JsValue::from_str("conditional parent missing"))?
                    .insert_before(&child, Some(&end))?;
                metrics.dom_operations += 1;
            }
            comment.set_data(&marker);
            let mut instances = self.instances.borrow_mut();
            let row = instances
                .get_mut(instance_id)
                .and_then(|instance| instance.rows.get_mut(input_id))
                .and_then(|rows| rows.get_mut(key))
                .ok_or_else(|| JsValue::from_str("row missing"))?;
            for id in &stale {
                row.nodes.remove(id);
            }
            row.nodes.extend(fresh);
            changed = true;
        }
        Ok(changed)
    }
}

impl PlecRuntime {
    pub(crate) fn sync_todo_state_loops(
        &self,
        instance_id: &str,
        app: &Application,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let Some(slot) = app.local_states.iter().find(|slot| slot.name == "todos") else {
            return Ok(());
        };
        let values = self
            .instances
            .borrow()
            .get(instance_id)
            .and_then(|instance| instance.local_state.get(&slot.id).cloned())
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default();
        let search_slot = app
            .local_states
            .iter()
            .find(|slot| slot.name == "search")
            .map(|slot| slot.id.clone());
        let search = search_slot
            .as_deref()
            .and_then(|slot| {
                self.instances
                    .borrow()
                    .get(instance_id)
                    .and_then(|instance| instance.local_state.get(slot))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_default()
            .trim()
            .to_lowercase();
        let input_ids = app
            .loops
            .iter()
            .filter_map(|loop_node| loop_node.input_id.clone())
            .collect::<HashSet<_>>();
        for input_id in input_ids {
            let next = values
                .iter()
                .filter_map(|value| {
                    let row = value
                        .as_object()?
                        .clone()
                        .into_iter()
                        .collect::<HashMap<_, _>>();
                    if !search.is_empty()
                        && !row
                            .get("title")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_lowercase()
                            .contains(&search)
                    {
                        return None;
                    }
                    let key = row.get("id")?.as_str()?.to_string();
                    Some((key, row))
                })
                .collect::<Vec<_>>();
            let existing = self
                .instances
                .borrow()
                .get(instance_id)
                .and_then(|instance| instance.rows.get(&input_id))
                .map(|rows| {
                    rows.iter()
                        .map(|(key, row)| (key.clone(), row.values.clone()))
                        .collect::<HashMap<_, _>>()
                })
                .unwrap_or_default();
            let next_keys = next
                .iter()
                .map(|(key, _)| key.clone())
                .collect::<HashSet<_>>();
            for key in existing
                .keys()
                .filter(|key| !next_keys.contains(*key))
                .cloned()
                .collect::<Vec<_>>()
            {
                self.remove(instance_id, &input_id, &key, metrics)?;
            }
            for (key, value) in next {
                if let Some(previous) = existing.get(&key) {
                    let changes = value
                        .iter()
                        .filter_map(|(name, value)| {
                            (previous.get(name) != Some(value))
                                .then(|| (name.clone(), value.clone()))
                        })
                        .collect::<HashMap<_, _>>();
                    if !changes.is_empty() {
                        self.update(instance_id, &input_id, &key, changes, metrics)?;
                    }
                } else {
                    self.insert(instance_id, &input_id, key, value, None, metrics)?;
                }
            }
        }
        Ok(())
    }
}
