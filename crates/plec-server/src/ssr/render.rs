//! SSR execution over the executable component graph. It deliberately has no
//! JSX/VDOM path: every rendered fact comes from the compiled artifact, and
//! every value coercion mirrors the runtime's typed VM so the server
//! instantiates exactly the branch and rows the browser will resume into.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    rc::Rc,
};

use indexmap::IndexMap;
use plec_ir::{limits::MAX_SNAPSHOT_LOOP_KEYS, SsrSelectedBranch};
use serde_json::Value;

use crate::{
    artifact::{
        Component, ComponentApplication, ComponentProp, ExpressionInstruction, HostComponentTarget,
        Node,
    },
    request::RequestContext,
};

use super::{NestedRecord, RenderError, RenderState, RouteRender};

/// A component-valued prop target: which application and component index a
/// `dynamicComponent` resolves to.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ComponentTarget<'a> {
    pub app: &'a ComponentApplication,
    pub component: usize,
}

/// The caller context one component render executes in. Fields are owned so
/// descent clones-and-overrides exactly like the TS host's object spreads.
#[derive(Clone)]
pub(crate) struct Scope<'a> {
    pub request: &'a RequestContext,
    /// The route child graph rendered into this component's route outlet.
    pub outlet: Option<&'a RouteRender<'a>>,
    pub path: String,
    pub props: Vec<Value>,
    pub component_props: HashMap<usize, ComponentTarget<'a>>,
    pub host_component_props: HashMap<usize, HostComponentTarget>,
    pub states: Vec<Value>,
    /// Action frames are execution state the SSR renderer never owns; frame
    /// loads always observe an empty frame.
    pub frame: Vec<Value>,
    pub row: Option<serde_json::Map<String, Value>>,
    pub row_key: Option<String>,
    pub row_root: bool,
    /// The implicit `children` prop content, owned by the caller.
    pub slot: Option<Rc<SlotFrame<'a>>>,
    pub loader_data: Value,
    pub instance: String,
    pub root_component: usize,
    /// Marker path of the nested component instance currently being
    /// rendered; branch/loop records below it address this component's node
    /// table.
    pub nested_key: Option<String>,
    /// Compiled component id of the nested instance at `nested_key`.
    pub nested_graph_id: Option<String>,
}

impl<'a> Scope<'a> {
    /// The bare scope the route loader executes in: request facts only, no
    /// states, frames, or gate.
    pub(crate) fn for_loader(request: &'a RequestContext) -> Self {
        Self {
            request,
            outlet: None,
            path: String::new(),
            props: Vec::new(),
            component_props: HashMap::new(),
            host_component_props: HashMap::new(),
            states: Vec::new(),
            frame: Vec::new(),
            row: None,
            row_key: None,
            row_root: false,
            slot: None,
            loader_data: Value::Null,
            instance: String::new(),
            root_component: 0,
            nested_key: None,
            nested_graph_id: None,
        }
    }
}

pub(crate) struct SlotFrame<'a> {
    pub app: &'a ComponentApplication,
    pub component: usize,
    pub nodes: Vec<usize>,
    pub scope: Box<Scope<'a>>,
}

pub(crate) fn render_component(
    app: &ComponentApplication,
    component_index: usize,
    scope: &Scope<'_>,
    state: &mut RenderState,
) -> Result<String, RenderError> {
    let component = app
        .components
        .get(component_index)
        .ok_or(RenderError::MissingComponent(component_index))?;
    if component_index == scope.root_component && scope.nested_key.is_none() {
        if let Some(outlet) = scope.outlet {
            if !component
                .route_outlets
                .iter()
                .any(|entry| entry.id == outlet.route.outlet_id)
            {
                return Err(RenderError::RouteOutletMissing(
                    outlet.route.id.clone(),
                    outlet.route.outlet_id.clone(),
                ));
            }
        }
    }
    // State initializers observe an empty state table, exactly like the
    // runtime's fresh component mount.
    let initial_scope = Scope {
        states: Vec::new(),
        ..scope.clone()
    };
    let states: Vec<Value> = component
        .state_slots
        .iter()
        .map(|slot| evaluate(component, slot.initial_expression, &initial_scope, state))
        .collect();
    let scope = Scope {
        states,
        ..scope.clone()
    };
    render_node(app, component_index, component.root_node, &scope, state)
}

pub(crate) fn render_node(
    app: &ComponentApplication,
    component_index: usize,
    index: usize,
    scope: &Scope<'_>,
    state: &mut RenderState,
) -> Result<String, RenderError> {
    // Every recursive render path (element children, conditional branches,
    // slot children, loop rows, and component calls) flows back through this
    // wrapper, so a deep — still acyclic, compiler-validated — graph fails
    // with a diagnostic instead of overflowing the native stack.
    if state.render_depth >= plec_ir::limits::MAX_SSR_RENDER_DEPTH {
        return Err(RenderError::RenderDepthExceeded);
    }
    state.render_depth += 1;
    let result = render_node_bounded(app, component_index, index, scope, state);
    state.render_depth -= 1;
    result
}

fn render_node_bounded(
    app: &ComponentApplication,
    component_index: usize,
    index: usize,
    scope: &Scope<'_>,
    state: &mut RenderState,
) -> Result<String, RenderError> {
    let component = app
        .components
        .get(component_index)
        .ok_or(RenderError::MissingComponent(component_index))?;
    let node = component
        .nodes
        .get(index)
        .ok_or(RenderError::MissingNode(component_index, index))?;

    match node {
        Node::Text { text } => {
            let text = component
                .texts
                .get(*text)
                .ok_or(RenderError::MissingText(component_index, *text))?;
            let value = match text.binding {
                None => text.value.clone().unwrap_or_default(),
                Some(binding) => {
                    let expression = component
                        .bindings
                        .get(binding)
                        .map(|binding| binding.expression)
                        .ok_or(RenderError::MissingBinding(component_index, binding))?;
                    dom_string(&evaluate(component, expression, scope, state))
                }
            };
            // An empty value serializes to no text node at all, which would
            // leave the marker ambiguous during adoption. The empty-comment
            // sentinel keeps the position occupied so the runtime can
            // distinguish an empty value from injected markup between the
            // marker and its text. See the text-marker adjacency contract in
            // docs/dom-address-protocol.md.
            let value = escape_html(&value);
            Ok(format!(
                "<!--plec:text:{}:{}-->{}",
                scope.path,
                index,
                if value.is_empty() {
                    "<!---->".to_owned()
                } else {
                    value
                }
            ))
        }

        Node::Conditional {
            test,
            consequent,
            alternate,
        } => {
            let truthy = truthy(&evaluate(component, *test, scope, state));
            let selected = if truthy {
                SsrSelectedBranch::Consequent
            } else if alternate.is_none() {
                SsrSelectedBranch::None
            } else {
                SsrSelectedBranch::Alternate
            };
            // Root-component conditionals record into the graph instance's
            // own branch map; nested component conditionals record into the
            // nested entry addressed by the component's marker path, so
            // adoption claims the server-selected branch instead of
            // inferring it from DOM shape.
            if scope.nested_key.is_some() {
                if let Some(record) = nested_record(scope, state) {
                    record.branches.insert(index, selected);
                }
            } else if component_index == scope.root_component && scope.row.is_none() {
                state
                    .branches
                    .entry(scope.instance.clone())
                    .or_default()
                    .insert(index, selected);
            }
            let inner = if truthy {
                render_node(app, component_index, *consequent, scope, state)?
            } else if let Some(alternate) = alternate {
                render_node(app, component_index, *alternate, scope, state)?
            } else {
                String::new()
            };
            // The boundary grammar matches the runtime's marker-index
            // adoption contract; the region between the markers is the
            // branch DOM the adopter will claim.
            Ok(format!(
                "<!--plec:conditional:{path}:{index}-->{inner}<!--plec:conditional-end:{path}:{index}-->",
                path = scope.path
            ))
        }

        Node::Loop { r#loop } => render_loop(app, component_index, index, *r#loop, scope, state),

        Node::Slot => {
            let inner = match &scope.slot {
                Some(frame) => frame
                    .nodes
                    .iter()
                    .map(|child| {
                        render_node(frame.app, frame.component, *child, &frame.scope, state)
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .join(""),
                None => String::new(),
            };
            Ok(format!(
                "<!--plec:slot:{path}:{index}-->{inner}<!--plec:slot-end:{path}:{index}-->",
                path = scope.path
            ))
        }

        Node::Component {
            component,
            props,
            children,
        } => {
            let target = Some(ComponentTarget {
                app,
                component: *component,
            });
            render_component_node(
                app,
                component_index,
                index,
                target,
                props,
                children,
                scope,
                state,
            )
        }

        Node::DynamicComponent {
            prop,
            props,
            children,
        } => {
            if let Some(target) = scope.host_component_props.get(prop) {
                return render_host_node(index, target, scope);
            }
            let target = scope.component_props.get(prop).copied();
            render_component_node(
                app,
                component_index,
                index,
                target,
                props,
                children,
                scope,
                state,
            )
        }

        Node::Element { tag, children } => {
            render_element(app, component_index, index, *tag, children, scope, state)
        }

        Node::HostComponent {
            provider,
            component,
            ..
        } => render_host_node(
            index,
            &HostComponentTarget {
                provider: provider.clone(),
                component: component.clone(),
            },
            scope,
        ),

        // Unknown ops render as nothing, mirroring the TS host.
        Node::Unknown => Ok(String::new()),
    }
}

#[allow(clippy::too_many_arguments)]
fn render_loop(
    app: &ComponentApplication,
    component_index: usize,
    index: usize,
    loop_handle: usize,
    scope: &Scope<'_>,
    state: &mut RenderState,
) -> Result<String, RenderError> {
    let component = app
        .components
        .get(component_index)
        .ok_or(RenderError::MissingComponent(component_index))?;
    let loop_program = component
        .loops
        .get(loop_handle)
        .ok_or(RenderError::MissingLoop(component_index, loop_handle))?;
    let values = evaluate(component, loop_program.source_expression, scope, state);
    let Value::Array(values) = values else {
        return Err(RenderError::LoopSourceNotArray);
    };
    let mut keys = Vec::with_capacity(values.len());
    let mut seen = BTreeSet::new();
    let mut rows = String::new();
    for value in values {
        let Value::Object(row) = value else {
            return Err(RenderError::LoopRowNotObject);
        };
        let key = canonical_key(&evaluate(
            component,
            loop_program.key_expression,
            &Scope {
                row: Some(row.clone()),
                ..scope.clone()
            },
            state,
        ));
        if keys.len() >= MAX_SNAPSHOT_LOOP_KEYS {
            // Failing the render closed is cheaper than shipping a snapshot
            // the browser's snapshot validation would reject wholesale.
            return Err(RenderError::LoopKeyLimit(component_index, index));
        }
        if !seen.insert(key.clone()) {
            return Err(RenderError::DuplicateLoopKey(key));
        }
        keys.push(key.clone());
        let row_path = format!(
            "{}/loop:{index}/key:{}",
            scope.path,
            super::escape_instance_segment(&key)
        );
        let inner = render_node(
            app,
            component_index,
            loop_program.row_template,
            &Scope {
                path: row_path.clone(),
                row: Some(row),
                row_key: Some(key),
                row_root: true,
                ..scope.clone()
            },
            state,
        )?;
        rows.push_str(&format!(
            "<!--plec:loop:{row_path}-->{inner}<!--plec:loop-end:{row_path}-->"
        ));
    }
    // Loop records follow the same split as branch records: graph-level
    // loops belong to the instance map, nested component loops to the
    // component's marker-path entry. Without the nested record the client
    // adopter fails closed with `missing:ssr-loop`.
    if scope.nested_key.is_some() {
        if let Some(record) = nested_record(scope, state) {
            record.loops.insert(index, keys);
        }
    } else if component_index == scope.root_component {
        state
            .loops
            .entry(scope.instance.clone())
            .or_default()
            .insert(index, keys);
    }
    Ok(rows)
}

/// Returns (creating if needed) the nested record for the instance being
/// rendered. A component id is required: snapshot validation resolves the
/// node table through it, so an unnamed component cannot carry records.
fn nested_record<'s>(
    scope: &Scope<'_>,
    state: &'s mut RenderState,
) -> Option<&'s mut NestedRecord> {
    let key = scope.nested_key.clone()?;
    let graph_id = scope.nested_graph_id.clone()?;
    Some(state.nested.entry(key).or_insert_with(|| NestedRecord {
        graph_id: Some(graph_id),
        branches: BTreeMap::new(),
        loops: BTreeMap::new(),
    }))
}

#[allow(clippy::too_many_arguments)]
fn render_component_node(
    app: &ComponentApplication,
    component_index: usize,
    index: usize,
    target: Option<ComponentTarget<'_>>,
    node_props: &[ComponentProp],
    node_children: &[usize],
    scope: &Scope<'_>,
    state: &mut RenderState,
) -> Result<String, RenderError> {
    let target = target
        .ok_or_else(|| RenderError::DynamicComponentUnavailable(scope.path.clone(), index))?;
    let current = app
        .components
        .get(component_index)
        .ok_or(RenderError::MissingComponent(component_index))?;
    let child = target
        .app
        .components
        .get(target.component)
        .ok_or(RenderError::MissingComponent(target.component))?;
    let mut props: Vec<Option<Value>> = vec![None; child.parameters.len()];
    // Dynamic targets keep their props as named values (`className`), while a
    // direct `(props)` parameter reads the whole named record — the SSR
    // mirror of `component_runtime_props` (crates/plec-runtime).
    let mut named_props = serde_json::Map::new();
    let mut component_props: HashMap<usize, ComponentTarget<'_>> = HashMap::new();
    let mut host_component_props = HashMap::new();
    for prop in node_props {
        match prop {
            ComponentProp::Component {
                name,
                component,
                host,
            } => {
                let Some(name) = current.strings.get(*name) else {
                    continue;
                };
                if let Some(parameter) = child
                    .parameters
                    .iter()
                    .position(|candidate| child.strings.get(candidate.name) == Some(name))
                {
                    if let Some(host) = host {
                        host_component_props.insert(parameter, host.clone());
                    } else {
                        component_props.insert(
                            parameter,
                            ComponentTarget {
                                app,
                                component: *component,
                            },
                        );
                    }
                }
            }
            ComponentProp::Value { name, expression } => {
                let Some(name) = current.strings.get(*name) else {
                    continue;
                };
                let value = evaluate(current, *expression, scope, state);
                named_props.insert(name.clone(), value.clone());
                if let Some(parameter) = child
                    .parameters
                    .iter()
                    .position(|candidate| child.strings.get(candidate.name) == Some(name))
                {
                    props[parameter] = Some(value);
                }
            }
            // Callable props never fire during SSR.
            _ => {}
        }
    }
    let declares_direct_props = |name: &Option<usize>| {
        name.and_then(|name| current.strings.get(name))
            .map(String::as_str)
            == Some("__plec_props")
    };
    if let Some(parameter) = child.parameters.iter().position(|candidate| {
        child.strings.get(candidate.name).map(String::as_str) == Some("__plec_props")
    }) {
        if props[parameter].is_none()
            && !node_props.iter().any(|prop| match prop {
                ComponentProp::Value { name, .. }
                | ComponentProp::Callable { name, .. }
                | ComponentProp::Component { name, .. } => declares_direct_props(&Some(*name)),
                _ => false,
            })
        {
            props[parameter] = Some(Value::Object(named_props));
        }
    }
    let props = props
        .into_iter()
        .map(|value| value.unwrap_or(Value::Null))
        .collect();
    let component_path = format!("{}/component:{index}", scope.path);
    let frame = Rc::new(SlotFrame {
        app,
        component: component_index,
        nodes: node_children.to_vec(),
        scope: Box::new(scope.clone()),
    });
    // Records rendered below this call address the child's node table under
    // the child's marker path, exactly the address the client adopter
    // derives for the instance.
    let inner = render_component(
        target.app,
        target.component,
        &Scope {
            props,
            component_props,
            host_component_props,
            path: component_path.clone(),
            nested_key: Some(component_path),
            nested_graph_id: child.id.clone(),
            slot: Some(frame),
            ..scope.clone()
        },
        state,
    )?;
    Ok(format!(
        "<!--plec:component:{path}:{index}-->{inner}<!--plec:component-end:{path}:{index}-->",
        path = scope.path
    ))
}

fn render_host_node(
    index: usize,
    target: &HostComponentTarget,
    scope: &Scope<'_>,
) -> Result<String, RenderError> {
    Ok(format!(
        "<span data-plec-node=\"{}\" data-plec-host=\"{}:{}\"></span>",
        escape_attribute(&format!("{}/node:{index}", scope.path)),
        escape_attribute(&target.provider),
        escape_attribute(&target.component),
    ))
}

#[allow(clippy::too_many_arguments)]
fn render_element(
    app: &ComponentApplication,
    component_index: usize,
    index: usize,
    tag: usize,
    children: &[usize],
    scope: &Scope<'_>,
    state: &mut RenderState,
) -> Result<String, RenderError> {
    let component = app
        .components
        .get(component_index)
        .ok_or(RenderError::MissingComponent(component_index))?;
    let tag = component
        .strings
        .get(tag)
        .ok_or(RenderError::MissingString(component_index, tag))?;
    // DOM-sink policy (plec_ir::sink): the tag string is interpolated
    // verbatim into markup, so anything outside the strict HTML/SVG grammar
    // is a markup injection channel. Element identity is additionally
    // allowlisted per namespace (mirroring the CSR runtime's instantiation
    // policy, including the server manifest's trusted custom elements), so
    // substituted artifacts cannot activate `script`-class elements. The
    // server artifact carries no namespace field, so the tag passes when
    // either namespace's allowlist admits it; fail the render closed.
    let policy = &state.tag_policy;
    if !plec_ir::sink::is_allowed_element_tag_with_policy(tag, "html", policy)
        && !plec_ir::sink::is_allowed_element_tag_with_policy(tag, "svg", policy)
    {
        return Err(RenderError::UnsafeTag(tag.to_owned()));
    }
    // Writes keep program order: a spread bag is written where it occurs, and
    // later writes overwrite earlier attribute names (same-key overwrites keep
    // their original position), mirroring the runtime's sequential application.
    let mut attributes: IndexMap<String, Option<String>> = IndexMap::new();
    for program in component
        .prop_programs
        .iter()
        .filter(|program| program.target == index)
    {
        for write in &program.writes {
            if write.spread {
                // Component props reach an element through a `{...props}`
                // spread (e.g. generated icons); serialize the record so the
                // first paint already carries final attributes like `class`.
                // This mirrors `typed_apply_spread` (crates/plec-runtime
                // bindings), which also iterates an unordered record map.
                if let Some(expression) = write.expression {
                    let bag = evaluate(component, expression, scope, state);
                    if let Value::Object(fields) = bag {
                        for (name, value) in &fields {
                            write_attribute(&mut attributes, name, value)?;
                        }
                    }
                }
                continue;
            }
            let Some(name) = write.name.and_then(|name| component.strings.get(name)) else {
                continue;
            };
            let value = match write.expression {
                Some(expression) => evaluate(component, expression, scope, state),
                None => write
                    .constant
                    .and_then(|constant| component.constants.get(constant))
                    .cloned()
                    .unwrap_or(Value::Null),
            };
            write_attribute(&mut attributes, name, &value)?;
        }
    }
    if scope.row_root {
        if let Some(key) = &scope.row_key {
            attributes.insert(
                "data-runtime-row-key".to_owned(),
                Some(escape_attribute(key)),
            );
        }
    }
    attributes.insert(
        "data-plec-node".to_owned(),
        Some(escape_attribute(&format!("{}/node:{index}", scope.path))),
    );
    let attribute_text = attributes
        .iter()
        .map(|(attribute, value)| match value {
            None => attribute.clone(),
            Some(value) => format!("{attribute}=\"{}\"", escape_attribute(value)),
        })
        .collect::<Vec<_>>()
        .join(" ");
    let children = children
        .iter()
        .map(|child| {
            render_node(
                app,
                component_index,
                *child,
                &Scope {
                    row_root: false,
                    ..scope.clone()
                },
                state,
            )
        })
        .collect::<Result<Vec<_>, _>>()?
        .join("");
    let outlet = component
        .route_outlets
        .iter()
        .find(|entry| entry.node == index);
    let outlet_html = match (outlet, scope.outlet) {
        (Some(outlet), Some(route)) if outlet.id == route.route.outlet_id => render_component(
            route.graph,
            route.graph.root_component,
            // The outlet child composes its instance from the escaped parent
            // instance and the outlet id; its root component is the child
            // graph's own, so instance-level records address the child.
            &Scope {
                outlet: route.child.as_deref(),
                path: format!("{}/outlet:{}", scope.path, outlet.id),
                instance: route.instance.clone(),
                root_component: route.graph.root_component,
                loader_data: match route.loader.map(|loader| &loader.state) {
                    Some(plec_ir::SsrLoaderState::Resolved { value }) => {
                        crate::loader::snapshot_value_to_json(value)
                    }
                    _ => Value::Null,
                },
                ..scope.clone()
            },
            state,
        )?,
        _ => String::new(),
    };
    Ok(format!(
        "<{tag}{attribute_text}>{children}{outlet_html}</{tag}>",
        attribute_text = if attribute_text.is_empty() {
            String::new()
        } else {
            format!(" {attribute_text}")
        }
    ))
}

/// Backstop for the DOM-sink policy (`plec_ir::sink`) and the reserved DOM
/// metadata namespace (docs/dom-address-protocol.md): literal JSX collisions
/// already fail at compile time, but spread bags are evaluated at render
/// time, so the only cheap enforcement left is here. The shared sink
/// authority owns every policy decision — reserved prefixes, event-handler
/// and document sinks, and the strict HTML/SVG-safe name grammar are all
/// matched ASCII-case-insensitively there. Fail the render closed — a
/// reserved name that reached markup would collide with adoption markers,
/// and a grammar-violating name (`a href`, `x=y`) would smuggle markup into
/// the serialized document.
fn write_attribute(
    attributes: &mut IndexMap<String, Option<String>>,
    name: &str,
    value: &Value,
) -> Result<(), RenderError> {
    if plec_ir::sink::is_reserved_attribute_name(name) {
        return Err(RenderError::ReservedAttribute(name.to_owned()));
    }
    let lower = name.to_ascii_lowercase();
    if lower == "srcdoc" {
        return Err(RenderError::UnsafeAttribute(name.to_owned()));
    }
    if lower.starts_with("on") {
        return Ok(());
    }
    if !plec_ir::sink::is_safe_attribute_name(name) {
        return Err(RenderError::UnsafeAttribute(name.to_owned()));
    }
    if matches!(value, Value::Bool(false) | Value::Null) {
        return Ok(());
    }
    let attribute = if name == "className" { "class" } else { name };
    let text = if matches!(value, Value::Bool(true)) {
        String::new()
    } else {
        dom_string(value)
    };
    // URL-scheme policy comes from the shared sink authority, not a local
    // mirror of it.
    if !plec_ir::sink::is_safe_attribute_value(attribute, &text) {
        return Err(RenderError::UnsafeUrlAttribute(attribute.to_owned()));
    }
    attributes.insert(
        attribute.to_owned(),
        if matches!(value, Value::Bool(true)) {
            None
        } else {
            Some(text)
        },
    );
    Ok(())
}

/// The SSR expression evaluator. It mirrors the runtime's typed VM op set
/// (`plec-eval`) over the transport value representation, so a resolved
/// binding, loop source, or loader URL observes exactly the value the
/// browser recomputes.
pub(crate) fn evaluate(
    component: &Component,
    expression: usize,
    scope: &Scope<'_>,
    state: &mut RenderState,
) -> Value {
    // The SSR evaluator mirrors the runtime's typed VM, including its
    // execution budgets: one shared fuel counter across nested Filter/Map
    // work, bounded nesting, and a bounded value stack (live value count and
    // estimated bytes). Exhaustion (or a hostile shape) fails soft to
    // `null` — compiled artifacts are compiler-validated, so the budgets are
    // defense in depth, not the primary contract.
    let mut fuel = plec_ir::limits::MAX_EXPRESSION_STEPS;
    evaluate_bounded(component, expression, scope, state, &mut fuel, 0)
}

/// Estimated live byte size of one transport value (iterative; values are
/// validated acyclic trees). Mirrors the runtime evaluator's stack-byte
/// accounting so both sides bound `Constant` deep-clone memory the same way.
fn value_size_bytes(value: &Value) -> usize {
    let mut bytes = 0usize;
    let mut pending = vec![value];
    while let Some(value) = pending.pop() {
        match value {
            Value::Null | Value::Bool(_) | Value::Number(_) => bytes += 8,
            Value::String(value) => bytes += value.len(),
            Value::Array(values) => {
                bytes += 16 + 8 * values.len();
                pending.extend(values.iter());
            }
            Value::Object(values) => {
                bytes += 16 + 8 * values.len();
                pending.extend(values.values());
            }
        }
    }
    bytes
}

/// Pushes one value with the runtime evaluator's stack budgets. Returns
/// `false` — the caller fails soft to `Value::Null` — when the live value
/// count or byte estimate would exceed `MAX_EVAL_STACK_VALUES` /
/// `MAX_EVAL_STACK_BYTES`.
fn evaluate_stack_push(
    stack: &mut Vec<Value>,
    sizes: &mut Vec<usize>,
    bytes: &mut usize,
    value: Value,
) -> bool {
    if stack.len() >= plec_ir::limits::MAX_EVAL_STACK_VALUES {
        return false;
    }
    let size = value_size_bytes(&value);
    if *bytes + size > plec_ir::limits::MAX_EVAL_STACK_BYTES {
        return false;
    }
    stack.push(value);
    sizes.push(size);
    *bytes += size;
    true
}

fn evaluate_stack_pop(
    stack: &mut Vec<Value>,
    sizes: &mut Vec<usize>,
    bytes: &mut usize,
) -> Option<Value> {
    let value = stack.pop()?;
    *bytes -= sizes.pop().unwrap_or_default();
    Some(value)
}

fn evaluate_bounded(
    component: &Component,
    expression: usize,
    scope: &Scope<'_>,
    state: &mut RenderState,
    fuel: &mut usize,
    nesting: usize,
) -> Value {
    if nesting > plec_ir::limits::MAX_EVAL_NESTING {
        return Value::Null;
    }
    let Some(instructions) = component
        .expressions
        .get(expression)
        .map(|program| &program.instructions)
    else {
        return Value::Null;
    };
    let mut stack: Vec<Value> = Vec::new();
    let mut stack_sizes: Vec<usize> = Vec::new();
    let mut stack_bytes = 0usize;
    let mut pc = 0usize;
    while pc < instructions.len() {
        if *fuel == 0 {
            return Value::Null;
        }
        *fuel -= 1;
        match &instructions[pc] {
            ExpressionInstruction::Constant { constant } => {
                if !evaluate_stack_push(
                    &mut stack,
                    &mut stack_sizes,
                    &mut stack_bytes,
                    component
                        .constants
                        .get(*constant)
                        .cloned()
                        .unwrap_or(Value::Null),
                ) {
                    return Value::Null;
                }
            }
            ExpressionInstruction::LoadState { state: slot } => {
                if !evaluate_stack_push(
                    &mut stack,
                    &mut stack_sizes,
                    &mut stack_bytes,
                    scope.states.get(*slot).cloned().unwrap_or(Value::Null),
                ) {
                    return Value::Null;
                }
            }
            ExpressionInstruction::LoadProp { prop } => {
                if !evaluate_stack_push(
                    &mut stack,
                    &mut stack_sizes,
                    &mut stack_bytes,
                    scope.props.get(*prop).cloned().unwrap_or(Value::Null),
                ) {
                    return Value::Null;
                }
            }
            ExpressionInstruction::LoadFrame { slot } => {
                if !evaluate_stack_push(
                    &mut stack,
                    &mut stack_sizes,
                    &mut stack_bytes,
                    scope.frame.get(*slot).cloned().unwrap_or(Value::Null),
                ) {
                    return Value::Null;
                }
            }
            ExpressionInstruction::LoadHost { host } => {
                let slot = component.host_slots.get(*host);
                let name = slot
                    .and_then(|slot| slot.name)
                    .and_then(|name| component.strings.get(name).cloned());
                match slot.map(|slot| slot.kind.as_str()) {
                    // Request cookies can never satisfy `validate_public_export`
                    // (not explicitly public, server-owned private state), so
                    // the render evaluates them as absent and records each
                    // gate for the development-only diagnostic header.
                    Some("cookie") => {
                        if let Some(gate) = &mut state.gate {
                            if gate.development {
                                gate.gated.push(format!(
                                    "cookie:{}",
                                    name.unwrap_or_else(|| "<unnamed>".to_owned())
                                ));
                            }
                        }
                        if !evaluate_stack_push(
                            &mut stack,
                            &mut stack_sizes,
                            &mut stack_bytes,
                            Value::Null,
                        ) {
                            return Value::Null;
                        }
                    }
                    Some("loaderData") => {
                        if !evaluate_stack_push(
                            &mut stack,
                            &mut stack_sizes,
                            &mut stack_bytes,
                            scope.loader_data.clone(),
                        ) {
                            return Value::Null;
                        }
                    }
                    Some("location") => {
                        let mut location = serde_json::Map::new();
                        location.insert(
                            "pathname".to_owned(),
                            Value::String(scope.request.pathname.clone()),
                        );
                        location.insert(
                            "search".to_owned(),
                            Value::String(super::url_search(&scope.request.url)),
                        );
                        if !evaluate_stack_push(
                            &mut stack,
                            &mut stack_sizes,
                            &mut stack_bytes,
                            Value::Object(location),
                        ) {
                            return Value::Null;
                        }
                    }
                    _ => {
                        if !evaluate_stack_push(
                            &mut stack,
                            &mut stack_sizes,
                            &mut stack_bytes,
                            Value::Null,
                        ) {
                            return Value::Null;
                        }
                    }
                }
            }
            ExpressionInstruction::LoadRowRecord => {
                if !evaluate_stack_push(
                    &mut stack,
                    &mut stack_sizes,
                    &mut stack_bytes,
                    scope.row.clone().map(Value::Object).unwrap_or(Value::Null),
                ) {
                    return Value::Null;
                }
            }
            ExpressionInstruction::LoadRowField { field } => {
                let value = component
                    .strings
                    .get(*field)
                    .and_then(|name| scope.row.as_ref().and_then(|row| row.get(name)))
                    .cloned();
                if !evaluate_stack_push(
                    &mut stack,
                    &mut stack_sizes,
                    &mut stack_bytes,
                    value.unwrap_or(Value::Null),
                ) {
                    return Value::Null;
                }
            }
            ExpressionInstruction::Field { field } => {
                let object = evaluate_stack_pop(&mut stack, &mut stack_sizes, &mut stack_bytes)
                    .unwrap_or(Value::Null);
                let name = component.strings.get(*field).map(String::as_str);
                if !evaluate_stack_push(
                    &mut stack,
                    &mut stack_sizes,
                    &mut stack_bytes,
                    match (object, name) {
                        (Value::Object(fields), Some(name)) => {
                            fields.get(name).cloned().unwrap_or(Value::Null)
                        }
                        // JS property access: `array.length` / `string.length`
                        // are real values, and SSR expressions rely on them.
                        (Value::Array(values), Some("length")) => {
                            number_value(values.len() as f64)
                        }
                        (Value::String(value), Some("length")) => {
                            number_value(value.chars().count() as f64)
                        }
                        _ => Value::Null,
                    },
                ) {
                    return Value::Null;
                }
            }
            ExpressionInstruction::Index => {
                let key = evaluate_stack_pop(&mut stack, &mut stack_sizes, &mut stack_bytes)
                    .unwrap_or(Value::Null);
                let object = evaluate_stack_pop(&mut stack, &mut stack_sizes, &mut stack_bytes)
                    .unwrap_or(Value::Null);
                let key = match key {
                    Value::String(value) => value,
                    Value::Number(value) => value
                        .as_f64()
                        .filter(|value| value.is_finite() && value.fract() == 0.0)
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                    _ => String::new(),
                };
                if !evaluate_stack_push(
                    &mut stack,
                    &mut stack_sizes,
                    &mut stack_bytes,
                    match object {
                        Value::Object(fields) => fields.get(&key).cloned().unwrap_or(Value::Null),
                        Value::Array(values) => key
                            .parse::<usize>()
                            .ok()
                            .and_then(|index| values.get(index))
                            .cloned()
                            .unwrap_or(Value::Null),
                        Value::String(value) => key
                            .parse::<usize>()
                            .ok()
                            .and_then(|index| value.chars().nth(index))
                            .map(|character| Value::String(character.to_string()))
                            .unwrap_or(Value::Null),
                        _ => Value::Null,
                    },
                ) {
                    return Value::Null;
                }
            }
            // Filter/Map evaluate the predicate per item with the item as the
            // row context (`LoadRowField`), exactly like the runtime's typed
            // VM; item/index frame slots are ignored there too. Filter keeps
            // truthy items as records; Map keeps every mapped value.
            ExpressionInstruction::Filter { predicate, .. }
            | ExpressionInstruction::Map {
                mapper: predicate, ..
            } => {
                let is_map = matches!(
                    instructions.get(pc),
                    Some(ExpressionInstruction::Map { .. })
                );
                let source = evaluate_stack_pop(&mut stack, &mut stack_sizes, &mut stack_bytes)
                    .unwrap_or(Value::Null);
                let items = match source {
                    Value::Array(values) => values,
                    _ => Vec::new(),
                };
                let mut output = Vec::with_capacity(items.len());
                for item in items {
                    let record = match item {
                        Value::Object(fields) => fields,
                        _ => serde_json::Map::new(),
                    };
                    let value = evaluate_bounded(
                        component,
                        *predicate,
                        &Scope {
                            row: Some(record.clone()),
                            ..scope.clone()
                        },
                        state,
                        fuel,
                        nesting + 1,
                    );
                    if is_map {
                        output.push(value);
                    } else if truthy(&value) {
                        output.push(Value::Object(record));
                    }
                }
                if !evaluate_stack_push(
                    &mut stack,
                    &mut stack_sizes,
                    &mut stack_bytes,
                    Value::Array(output),
                ) {
                    return Value::Null;
                }
            }
            ExpressionInstruction::String { kind, count } => {
                // `split_off` already preserves push order (first pushed
                // first); the runtime pops LIFO and reverses to the same
                // effect.
                let start = stack.len().saturating_sub(*count);
                let popped_sizes = stack_sizes.split_off(start);
                let parts = stack.split_off(start);
                stack_bytes -= popped_sizes.iter().sum::<usize>();
                let first = parts.first().cloned().unwrap_or(Value::Null);
                let value = match kind.as_str() {
                    "trim" => Value::String(dom_string(&first).trim().to_owned()),
                    "lower" => Value::String(dom_string(&first).to_lowercase()),
                    "upper" => Value::String(dom_string(&first).to_uppercase()),
                    "encodeUriComponent" => {
                        Value::String(encode_uri_component(&dom_string(&first)))
                    }
                    "jsonStringify" => {
                        Value::String(serde_json::to_string(&first).unwrap_or_default())
                    }
                    "includes" => {
                        let needle = dom_string(parts.get(1).unwrap_or(&Value::Null));
                        let haystack = dom_string(&first);
                        Value::Bool(haystack.contains(&needle))
                    }
                    _ => Value::String(parts.iter().map(dom_string).collect::<String>()),
                };
                if !evaluate_stack_push(&mut stack, &mut stack_sizes, &mut stack_bytes, value) {
                    return Value::Null;
                }
            }
            ExpressionInstruction::OmitFields { fields } => {
                let value = evaluate_stack_pop(&mut stack, &mut stack_sizes, &mut stack_bytes)
                    .unwrap_or(Value::Null);
                let mut record = match value {
                    Value::Object(fields) => fields,
                    _ => serde_json::Map::new(),
                };
                for field in fields {
                    if let Some(name) = component.strings.get(*field) {
                        record.remove(name);
                    }
                }
                if !evaluate_stack_push(
                    &mut stack,
                    &mut stack_sizes,
                    &mut stack_bytes,
                    Value::Object(record),
                ) {
                    return Value::Null;
                }
            }
            // Refs are browser-owned values; the SSR host has none, exactly
            // like the runtime's empty ref table.
            ExpressionInstruction::LoadRef { .. } => {
                if !evaluate_stack_push(
                    &mut stack,
                    &mut stack_sizes,
                    &mut stack_bytes,
                    Value::Null,
                ) {
                    return Value::Null;
                }
            }
            ExpressionInstruction::Unary { kind } => {
                let value = evaluate_stack_pop(&mut stack, &mut stack_sizes, &mut stack_bytes)
                    .unwrap_or(Value::Null);
                if !evaluate_stack_push(
                    &mut stack,
                    &mut stack_sizes,
                    &mut stack_bytes,
                    match kind.as_str() {
                        "not" => Value::Bool(!truthy(&value)),
                        "minus" => number_value(-to_number(&value)),
                        _ => value,
                    },
                ) {
                    return Value::Null;
                }
            }
            ExpressionInstruction::Binary { kind } => {
                let right = evaluate_stack_pop(&mut stack, &mut stack_sizes, &mut stack_bytes)
                    .unwrap_or(Value::Null);
                let left = evaluate_stack_pop(&mut stack, &mut stack_sizes, &mut stack_bytes)
                    .unwrap_or(Value::Null);
                if !evaluate_stack_push(
                    &mut stack,
                    &mut stack_sizes,
                    &mut stack_bytes,
                    binary(kind, left, right),
                ) {
                    return Value::Null;
                }
            }
            ExpressionInstruction::MakeArray { count, spreads } => {
                let start = stack.len().saturating_sub(*count);
                let popped_sizes = stack_sizes.split_off(start);
                let values = stack.split_off(start);
                stack_bytes -= popped_sizes.iter().sum::<usize>();
                if !evaluate_stack_push(
                    &mut stack,
                    &mut stack_sizes,
                    &mut stack_bytes,
                    Value::Array(apply_array_spreads(values, spreads)),
                ) {
                    return Value::Null;
                }
            }
            ExpressionInstruction::MakeRecord { fields, spreads } => {
                let start = stack.len().saturating_sub(fields.len());
                let popped_sizes = stack_sizes.split_off(start);
                let values = stack.split_off(start);
                stack_bytes -= popped_sizes.iter().sum::<usize>();
                let mut record = serde_json::Map::new();
                for (position, (field, value)) in fields.iter().zip(values).enumerate() {
                    if spreads.get(position).copied().unwrap_or(false) {
                        if let Value::Object(values) = value {
                            record.extend(values);
                        }
                    } else if let Some(name) = component.strings.get(*field) {
                        record.insert(name.clone(), value);
                    }
                }
                if !evaluate_stack_push(
                    &mut stack,
                    &mut stack_sizes,
                    &mut stack_bytes,
                    Value::Object(record),
                ) {
                    return Value::Null;
                }
            }
            ExpressionInstruction::Jump { target } => {
                pc = *target;
                continue;
            }
            ExpressionInstruction::JumpIfFalse { target } => {
                let condition = evaluate_stack_pop(&mut stack, &mut stack_sizes, &mut stack_bytes)
                    .unwrap_or(Value::Null);
                if !truthy(&condition) {
                    pc = *target;
                    continue;
                }
            }
            ExpressionInstruction::Return => {
                return evaluate_stack_pop(&mut stack, &mut stack_sizes, &mut stack_bytes)
                    .unwrap_or(Value::Null);
            }
            // Unknown instructions are skipped without stack effect, matching
            // the TS host; SSR-path expressions never contain them.
            _ => {}
        }
        pc += 1;
    }
    evaluate_stack_pop(&mut stack, &mut stack_sizes, &mut stack_bytes).unwrap_or(Value::Null)
}

fn apply_array_spreads(values: Vec<Value>, spreads: &[bool]) -> Vec<Value> {
    let mut output = Vec::with_capacity(values.len());
    for (index, value) in values.into_iter().enumerate() {
        if spreads.get(index).copied().unwrap_or(false) {
            if let Value::Array(values) = value {
                output.extend(values);
            }
        } else {
            output.push(value);
        }
    }
    output
}

fn binary(kind: &str, left: Value, right: Value) -> Value {
    match kind {
        "equal" => Value::Bool(left == right),
        "notEqual" => Value::Bool(left != right),
        "and" => {
            if truthy(&left) {
                right
            } else {
                left
            }
        }
        "or" => {
            if truthy(&left) {
                left
            } else {
                right
            }
        }
        "coalesce" => {
            if left.is_null() {
                right
            } else {
                left
            }
        }
        "add" => match (&left, &right) {
            (Value::Number(left), Value::Number(right)) => {
                number_value(left.as_f64().unwrap_or(0.0) + right.as_f64().unwrap_or(0.0))
            }
            _ => Value::String(format!("{}{}", dom_string(&left), dom_string(&right))),
        },
        "subtract" => number_value(to_number(&left) - to_number(&right)),
        "multiply" => number_value(to_number(&left) * to_number(&right)),
        "divide" => number_value(to_number(&left) / to_number(&right)),
        "greater" => Value::Bool(to_number(&left) > to_number(&right)),
        "greaterEqual" => Value::Bool(to_number(&left) >= to_number(&right)),
        "less" => Value::Bool(to_number(&left) < to_number(&right)),
        "lessEqual" => Value::Bool(to_number(&left) <= to_number(&right)),
        _ => Value::Null,
    }
}

/// Mirrors `typed_truthy` so the branch the server instantiates is exactly
/// the branch the runtime reconciles to.
pub(crate) fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64().map(|value| value != 0.0).unwrap_or(false),
        Value::String(value) => !value.is_empty(),
        Value::Array(values) => !values.is_empty(),
        Value::Object(_) => true,
    }
}

/// Mirrors `typed_value_string`: the canonical DOM string and the canonical
/// loop key are the same runtime function.
pub(crate) fn dom_string(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Null => String::new(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value
            .as_f64()
            .map(|value| value.to_string())
            .unwrap_or_default(),
        Value::Array(_) | Value::Object(_) => serde_json::to_string(value).unwrap_or_default(),
    }
}

fn canonical_key(value: &Value) -> String {
    dom_string(value)
}

/// JavaScript `Number()` coercion for arithmetic sinks.
fn to_number(value: &Value) -> f64 {
    match value {
        Value::Null => 0.0,
        Value::Bool(value) => {
            if *value {
                1.0
            } else {
                0.0
            }
        }
        Value::Number(value) => value.as_f64().unwrap_or(0.0),
        Value::String(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                0.0
            } else {
                trimmed.parse().unwrap_or(f64::NAN)
            }
        }
        Value::Array(values) if values.is_empty() => 0.0,
        Value::Array(values) if values.len() == 1 => to_number(&values[0]),
        _ => f64::NAN,
    }
}

/// JSON cannot represent NaN or infinities; those coerce to `null` exactly
/// like `JSON.stringify` would.
fn number_value(value: f64) -> Value {
    serde_json::Number::from_f64(value)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

/// JavaScript `encodeURIComponent`: unreserved characters pass through,
/// everything else is percent-encoded as UTF-8.
fn encode_uri_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        let keep = byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')'
            );
        if keep {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

pub(crate) fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub(crate) fn escape_attribute(value: &str) -> String {
    escape_html(value).replace('"', "&quot;")
}
