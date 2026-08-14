use crate::dom::{bindings::*, platform::*};
use crate::eval::typed_vm::*;
use crate::runtime::lifecycle::*;
use crate::typed::{events::*, fetch::*};

#[derive(Default)]
pub(crate) struct TypedLoopRows {
    pub(crate) order: Vec<String>,
    pub(crate) rows: HashMap<String, TypedRow>,
}

pub(crate) struct TypedRow {
    pub(crate) root: Node,
    pub(crate) values: HashMap<String, RuntimeValue>,
    pub(crate) nodes: HashMap<usize, Node>,
    pub(crate) generation: u64,
}

pub(crate) struct TypedRuntime {
    pub(crate) app: TypedApplication,
    pub(crate) root: Option<Element>,
    pub(crate) nodes: HashMap<usize, Node>,
    pub(crate) states: Vec<RuntimeValue>,
    pub(crate) collections: HashMap<usize, TypedCollection>,
    pub(crate) loops: HashMap<usize, TypedLoopRows>,
    pub(crate) listeners: Vec<TypedListener>,
    pub(crate) next_generation: u64,
    pub(crate) pending_fetches: Vec<TypedPendingFetch>,
}

#[wasm_bindgen::prelude::wasm_bindgen]
impl PlecRuntime {
    #[wasm_bindgen(constructor)]
    pub fn new() -> PlecRuntime {
        PlecRuntime {
            registry: Rc::new(RefCell::new(HashMap::new())),
            instances: Rc::new(RefCell::new(HashMap::new())),
            router: Rc::new(RefCell::new(None)),
            router_listeners: Rc::new(RefCell::new(Vec::new())),
            typed: Rc::new(RefCell::new(None)),
            typed_registry: Rc::new(RefCell::new(HashMap::new())),
            typed_manifest: Rc::new(RefCell::new(None)),
        }
    }
}

#[wasm_bindgen::prelude::wasm_bindgen]
impl PlecRuntime {
    pub fn mount(&self, root: Element) -> Result<JsValue, JsValue> {
        if self.typed.borrow().is_some() {
            return self.mount_typed(root);
        }
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
        self.mount_instance(&instance_id, root, true)
    }
}

#[wasm_bindgen::prelude::wasm_bindgen]
impl PlecRuntime {
    pub fn apply_delta(&self, delta: JsValue) -> Result<JsValue, JsValue> {
        if self.typed.borrow().is_some() {
            return self.apply_typed_delta(delta);
        }
        let instance_id = self.legacy_instance_id()?;
        self.apply_delta_for(&instance_id, delta)
    }
}

impl PlecRuntime {
    pub(crate) fn mount_typed(&self, root: Element) -> Result<JsValue, JsValue> {
        let metrics = {
            let mut typed = self.typed.borrow_mut();
            typed
                .as_mut()
                .ok_or_else(|| JsValue::from_str("typed application missing"))?
                .mount(root)?
        };
        self.install_typed_event_listeners()?;
        serde_wasm_bindgen::to_value(&metrics).map_err(error)
    }
}

impl PlecRuntime {
    pub(crate) fn initialize_typed_input(
        &self,
        input_id: &str,
        rows: JsValue,
    ) -> Result<JsValue, JsValue> {
        let rows: Vec<Value> = serde_wasm_bindgen::from_value(rows).map_err(error)?;
        let metrics = {
            let mut typed = self.typed.borrow_mut();
            let typed = typed
                .as_mut()
                .ok_or_else(|| JsValue::from_str("typed application missing"))?;
            let mut metrics = UpdateMetrics::default();
            typed.reconcile_input(input_id, rows, &mut metrics)?;
            metrics
        };
        self.install_typed_event_listeners()?;
        serde_wasm_bindgen::to_value(&metrics).map_err(error)
    }
}

impl PlecRuntime {
    pub(crate) fn apply_typed_delta(&self, delta: JsValue) -> Result<JsValue, JsValue> {
        let delta: Delta = serde_wasm_bindgen::from_value(delta).map_err(error)?;
        let metrics = {
            let mut typed = self.typed.borrow_mut();
            let typed = typed
                .as_mut()
                .ok_or_else(|| JsValue::from_str("typed application missing"))?;
            let mut metrics = UpdateMetrics::default();
            typed.apply_delta(delta, &mut metrics)?;
            metrics
        };
        self.install_typed_event_listeners()?;
        serde_wasm_bindgen::to_value(&metrics).map_err(error)
    }
}

impl TypedRuntime {
    pub(crate) fn new(app: TypedApplication) -> Result<Self, JsValue> {
        app.validate()?;
        let mut states = Vec::new();
        for slot in &app.state_slots {
            states.push(typed_eval(&app, slot.initial_expression, &[], None, 0)?);
        }
        Ok(Self {
            app,
            root: None,
            nodes: HashMap::new(),
            states,
            loops: HashMap::new(),
            collections: HashMap::new(),
            listeners: Vec::new(),
            next_generation: 1,
            pending_fetches: Vec::new(),
        })
    }
}

impl TypedRuntime {
    pub(crate) fn mount(&mut self, root: Element) -> Result<MountMetrics, JsValue> {
        root.set_inner_html("");
        self.nodes.clear();
        self.loops.clear();
        let doc = document()?;
        let node =
            self.instantiate_node(&doc, self.app.root_node, None, None, 0, &mut HashMap::new())?;
        root.append_child(&node)?;
        self.root = Some(root);
        self.apply_static_bindings()?;
        Ok(MountMetrics {
            created_elements: self
                .app
                .nodes
                .iter()
                .filter(|n| matches!(n, TypedNode::Element { .. }))
                .count() as u32,
            created_texts: self.app.texts.len() as u32,
            bindings: self.app.bindings.len() as u32,
            dom_operations: 1,
            ..Default::default()
        })
    }
}

impl TypedRuntime {
    pub(crate) fn clear_listeners(&mut self) {
        for listener in self.listeners.drain(..) {
            let listener = listener.listener;
            let _ = listener.element.remove_event_listener_with_callback(
                &listener.event_type,
                listener.callback.as_ref().unchecked_ref(),
            );
        }
    }
}

impl TypedRuntime {
    pub(crate) fn dispose_region_listeners(
        &mut self,
        loop_index: usize,
        key: &str,
        generation: u64,
    ) {
        let mut keep = Vec::new();
        for entry in self.listeners.drain(..) {
            if entry.loop_index == Some(loop_index)
                && entry.row_key.as_deref() == Some(key)
                && entry.generation == generation
            {
                let listener = entry.listener;
                let _ = listener.element.remove_event_listener_with_callback(
                    &listener.event_type,
                    listener.callback.as_ref().unchecked_ref(),
                );
            } else {
                keep.push(entry);
            }
        }
        self.listeners = keep;
    }
}

impl TypedRuntime {
    pub(crate) fn instantiate_node(
        &mut self,
        doc: &Document,
        index: usize,
        parent: Option<&Node>,
        row: Option<&HashMap<String, RuntimeValue>>,
        row_index: usize,
        local: &mut HashMap<usize, Node>,
    ) -> Result<Node, JsValue> {
        match self
            .app
            .nodes
            .get(index)
            .ok_or_else(|| JsValue::from_str("node handle out of range"))?
            .clone()
        {
            TypedNode::Element { tag, children, .. } => {
                let element = doc.create_element(
                    self.app
                        .strings
                        .get(tag)
                        .ok_or_else(|| JsValue::from_str("tag handle out of range"))?,
                )?;
                element.set_attribute("data-runtime-node", &index.to_string())?;
                let node: Node = element.into();
                if let Some(parent) = parent {
                    parent.append_child(&node)?;
                }
                for child in children {
                    self.instantiate_node(doc, child, Some(&node), row, row_index, local)?;
                }
                if row.is_some() {
                    local.insert(index, node.clone());
                } else {
                    self.nodes.insert(index, node.clone());
                }
                Ok(node)
            }
            TypedNode::Text { text, .. } => {
                let descriptor = self
                    .app
                    .texts
                    .get(text)
                    .ok_or_else(|| JsValue::from_str("text handle out of range"))?;
                let value = descriptor.value.clone().unwrap_or_default();
                let node: Node = doc.create_text_node(&value).into();
                if let Some(parent) = parent {
                    parent.append_child(&node)?;
                }
                if row.is_some() {
                    local.insert(index, node.clone());
                } else {
                    self.nodes.insert(index, node.clone());
                }
                Ok(node)
            }
            TypedNode::Loop { r#loop, .. } => {
                let marker: Node = doc.create_comment(&format!("plec:loop:{}", r#loop)).into();
                if let Some(parent) = parent {
                    parent.append_child(&marker)?;
                }
                self.render_loop(
                    r#loop,
                    parent.ok_or_else(|| JsValue::from_str("loop parent missing"))?,
                )?;
                Ok(marker)
            }
            TypedNode::Conditional {
                test,
                consequent,
                alternate,
                ..
            } => {
                let selected =
                    if typed_truthy(&typed_eval(&self.app, test, &self.states, row, row_index)?) {
                        consequent
                    } else {
                        alternate.unwrap_or(consequent)
                    };
                self.instantiate_node(doc, selected, parent, row, row_index, local)
            }
        }
    }
}

impl TypedRuntime {
    pub(crate) fn render_loop(&mut self, loop_index: usize, parent: &Node) -> Result<(), JsValue> {
        let loop_def = self
            .app
            .loops
            .get(loop_index)
            .ok_or_else(|| JsValue::from_str("loop handle out of range"))?
            .clone();
        let values = typed_eval(&self.app, loop_def.source_expression, &self.states, None, 0)?;
        let rows = values
            .array()
            .ok_or_else(|| JsValue::from_str("LOOP_SOURCE_NOT_ARRAY"))?;
        let mut projection = Vec::new();
        for (index, value) in rows.into_iter().enumerate() {
            let row = value
                .record()
                .cloned()
                .ok_or_else(|| JsValue::from_str("LOOP_ROW_NOT_OBJECT"))?;
            let key = typed_value_string(&typed_eval(
                &self.app,
                loop_def.key_expression,
                &self.states,
                Some(&row),
                index,
            )?);
            if projection
                .iter()
                .any(|(existing, _): &(String, HashMap<String, RuntimeValue>)| existing == &key)
            {
                return Err(JsValue::from_str("DUPLICATE_LOOP_KEY"));
            }
            projection.push((key, row));
        }
        self.reconcile_loop(
            loop_index,
            parent,
            projection,
            &mut UpdateMetrics::default(),
        )
    }
}

impl TypedRuntime {
    pub(crate) fn reconcile_input(
        &mut self,
        input: &str,
        values: Vec<Value>,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let input_index = self
            .app
            .inputs
            .iter()
            .position(|entry| self.app.strings.get(entry.name).map(String::as_str) == Some(input))
            .ok_or_else(|| JsValue::from_str("unknown input"))?;
        let targets = self
            .app
            .loops
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| (entry.input == Some(input_index)).then_some(index))
            .collect::<Vec<_>>();
        for loop_index in targets {
            let loop_def = self.app.loops[loop_index].clone();
            let parent = self.parent_for_loop(loop_index)?;
            let mut projection = Vec::new();
            for (index, value) in values.iter().enumerate() {
                let row = runtime_from_json(value.clone())?
                    .record()
                    .cloned()
                    .ok_or_else(|| JsValue::from_str("LOOP_ROW_NOT_OBJECT"))?;
                projection.push((
                    typed_value_string(&typed_eval(
                        &self.app,
                        loop_def.key_expression,
                        &self.states,
                        Some(&row),
                        index,
                    )?),
                    row,
                ));
            }
            self.reconcile_loop(loop_index, &parent, projection, metrics)?;
        }
        Ok(())
    }
}

impl TypedRuntime {
    pub(crate) fn parent_for_loop(&self, loop_index: usize) -> Result<Node, JsValue> {
        let node_index = self
            .app
            .nodes
            .iter()
            .position(
                |node| matches!(node, TypedNode::Loop { r#loop, .. } if *r#loop == loop_index),
            )
            .ok_or_else(|| JsValue::from_str("loop node missing"))?;
        let parent = match self.app.nodes.get(node_index) {
            Some(TypedNode::Loop { parent, .. }) => *parent,
            _ => None,
        }
        .ok_or_else(|| JsValue::from_str("loop parent missing"))?;
        self.nodes
            .get(&parent)
            .cloned()
            .ok_or_else(|| JsValue::from_str("loop parent not mounted"))
    }
}

impl TypedRuntime {
    pub(crate) fn reconcile_loop(
        &mut self,
        loop_index: usize,
        parent: &Node,
        projection: Vec<(String, HashMap<String, RuntimeValue>)>,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let desired = projection
            .iter()
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        let stale = self
            .loops
            .get(&loop_index)
            .map(|rows| {
                rows.order
                    .iter()
                    .filter(|key| !desired.contains(key))
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for key in stale {
            if let Some(row) = self
                .loops
                .get_mut(&loop_index)
                .and_then(|rows| rows.rows.remove(&key))
            {
                self.dispose_region_listeners(loop_index, &key, row.generation);
                if let Some(parent) = row.root.parent_node() {
                    parent.remove_child(&row.root)?;
                    metrics.dom_operations += 1;
                }
            }
        }
        for (position, (key, values)) in projection.into_iter().enumerate() {
            let existing = self
                .loops
                .get(&loop_index)
                .and_then(|rows| rows.rows.get(&key))
                .map(|row| row.values.clone());
            if let Some(previous) = existing {
                if previous != values {
                    self.update_typed_row(loop_index, &key, values, metrics)?;
                }
            } else {
                self.insert_typed_row(loop_index, parent, key.clone(), values, position, metrics)?;
            }
        }
        let roots = self
            .loops
            .get(&loop_index)
            .map(|rows| {
                desired
                    .iter()
                    .filter_map(|key| rows.rows.get(key).map(|row| row.root.clone()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for root in roots {
            parent.append_child(&root)?;
        }
        self.loops.entry(loop_index).or_default().order = desired;
        Ok(())
    }
}

impl TypedRuntime {
    pub(crate) fn insert_typed_row(
        &mut self,
        loop_index: usize,
        parent: &Node,
        key: String,
        values: HashMap<String, RuntimeValue>,
        index: usize,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let template = self.app.loops[loop_index].row_template;
        let mut nodes = HashMap::new();
        let doc = document()?;
        let root = self.instantiate_node(&doc, template, None, Some(&values), index, &mut nodes)?;
        if let Ok(element) = root.clone().dyn_into::<Element>() {
            element.set_attribute("data-runtime-row-key", &key)?;
        }
        parent.append_child(&root)?;
        let generation = self.next_generation;
        self.next_generation += 1;
        self.loops.entry(loop_index).or_default().rows.insert(
            key,
            TypedRow {
                root,
                values,
                nodes,
                generation,
            },
        );
        metrics.dom_operations += 1;
        Ok(())
    }
}

impl TypedRuntime {
    pub(crate) fn update_typed_row(
        &mut self,
        loop_index: usize,
        key: &str,
        values: HashMap<String, RuntimeValue>,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let row = self
            .loops
            .get_mut(&loop_index)
            .and_then(|rows| rows.rows.get_mut(key))
            .ok_or_else(|| JsValue::from_str("row missing"))?;
        row.values = values;
        let bindings = self.app.bindings.clone();
        for binding in bindings {
            if let Some(node) = row.nodes.get(&binding.target) {
                typed_apply_binding(
                    &self.app,
                    &binding,
                    node,
                    &self.states,
                    Some(&row.values),
                    0,
                )?;
                metrics.dom_operations += 1;
                metrics.bindings_touched += 1;
            }
        }
        Ok(())
    }
}

impl TypedRuntime {
    pub(crate) fn apply_static_bindings(&mut self) -> Result<(), JsValue> {
        for binding in self.app.bindings.clone() {
            if let Some(node) = self.nodes.get(&binding.target) {
                typed_apply_binding(&self.app, &binding, node, &self.states, None, 0)?;
            }
        }
        Ok(())
    }
}

impl TypedRuntime {
    pub(crate) fn apply_delta(
        &mut self,
        delta: Delta,
        metrics: &mut UpdateMetrics,
    ) -> Result<(), JsValue> {
        let input = match &delta {
            Delta::Update { input_id, .. }
            | Delta::Insert { input_id, .. }
            | Delta::Remove { input_id, .. }
            | Delta::Move { input_id, .. } => input_id,
        };
        let input_index = self
            .app
            .inputs
            .iter()
            .position(|entry| {
                self.app.strings.get(entry.name).map(String::as_str) == Some(input.as_str())
            })
            .ok_or_else(|| JsValue::from_str("unknown input"))?;
        let targets = self
            .app
            .loops
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| (entry.input == Some(input_index)).then_some(index))
            .collect::<Vec<_>>();
        for loop_index in targets {
            let mut keys = self
                .loops
                .get(&loop_index)
                .map(|rows| rows.order.clone())
                .unwrap_or_default();
            let mut values = self
                .loops
                .get(&loop_index)
                .map(|rows| {
                    rows.rows
                        .iter()
                        .map(|(key, row)| (key.clone(), row.values.clone()))
                        .collect::<HashMap<_, _>>()
                })
                .unwrap_or_default();
            match &delta {
                Delta::Update {
                    row_key, changes, ..
                } => {
                    let row = values
                        .get_mut(row_key)
                        .ok_or_else(|| JsValue::from_str("row missing"))?;
                    row.extend(
                        changes
                            .clone()
                            .into_iter()
                            .map(|(key, value)| Ok((key, runtime_from_json(value)?)))
                            .collect::<Result<HashMap<_, _>, JsValue>>()?,
                    );
                }
                Delta::Insert {
                    row_key,
                    row,
                    before_row_key,
                    ..
                } => {
                    values.insert(
                        row_key.clone(),
                        row.clone()
                            .into_iter()
                            .map(|(key, value)| Ok((key, runtime_from_json(value)?)))
                            .collect::<Result<HashMap<_, _>, JsValue>>()?,
                    );
                    keys.retain(|key| key != row_key);
                    let position = before_row_key
                        .as_ref()
                        .and_then(|before| keys.iter().position(|key| key == before))
                        .unwrap_or(keys.len());
                    keys.insert(position, row_key.clone());
                }
                Delta::Remove { row_key, .. } => {
                    values.remove(row_key);
                    keys.retain(|key| key != row_key);
                }
                Delta::Move {
                    row_key,
                    before_row_key,
                    ..
                } => {
                    keys.retain(|key| key != row_key);
                    let position = before_row_key
                        .as_ref()
                        .and_then(|before| keys.iter().position(|key| key == before))
                        .unwrap_or(keys.len());
                    keys.insert(position, row_key.clone());
                }
            }
            let projection = keys
                .into_iter()
                .filter_map(|key| values.remove(&key).map(|value| (key, value)))
                .collect::<Vec<_>>();
            let parent = self.parent_for_loop(loop_index)?;
            self.reconcile_loop(loop_index, &parent, projection, metrics)?;
        }
        Ok(())
    }
}
