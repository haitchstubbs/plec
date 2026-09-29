//! SSR execution over the executable component graph. It deliberately has no
//! JSX/VDOM path: every rendered fact comes from the compiled artifact, and
//! expression semantics come from `plec-eval`; the server supplies only the
//! request-scoped values needed to instantiate the graph the browser resumes.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    rc::Rc,
};

use indexmap::IndexMap;
use plec_ir::{limits::MAX_SNAPSHOT_LOOP_KEYS, SsrSelectedBranch};
use plec_schema::delta::{json_dom_string, json_truthy, runtime_from_json, RuntimeValue};
use serde_json::Value;

use crate::{
    artifact::{
        Component, ComponentApplication, ComponentProp, ExpressionInstruction, HostComponentTarget,
        Node,
    },
    request::RequestContext,
};

use super::{HostRender, NestedRecord, RenderError, RenderState, RouteRender};

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
    pub route_params: Value,
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
            route_params: Value::Object(serde_json::Map::new()),
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
                return render_host_node(app, component_index, index, target, props, scope, state);
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
            props,
        } => render_host_node(
            app,
            component_index,
            index,
            &HostComponentTarget {
                provider: provider.clone(),
                component: component.clone(),
            },
            props,
            scope,
            state,
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
    app: &ComponentApplication,
    component_index: usize,
    index: usize,
    target: &HostComponentTarget,
    props: &[ComponentProp],
    scope: &Scope<'_>,
    state: &mut RenderState,
) -> Result<String, RenderError> {
    let component = app
        .components
        .get(component_index)
        .ok_or(RenderError::MissingComponent(component_index))?;
    let mut values = serde_json::Map::new();
    for prop in props {
        let ComponentProp::Value { name, expression } = prop else {
            // Callback/event props are client-only handles. They never cross
            // the sidecar boundary and cannot be serialized into SSR markup.
            continue;
        };
        let name = component
            .strings
            .get(*name)
            .ok_or(RenderError::MissingString(component_index, *name))?;
        let value = evaluate(component, *expression, scope, state);
        if name == "__plec_props" {
            let Value::Object(record) = value else {
                return Err(RenderError::HostPropsNotRecord);
            };
            values.extend(record);
        } else {
            values.insert(name.clone(), value);
        }
    }
    let placeholder = format!("plec:host-render:{}", state.host_renders.len());
    state.host_renders.push(HostRender {
        placeholder: placeholder.clone(),
        provider: target.provider.clone(),
        component: target.component.clone(),
        props: Value::Object(values),
    });
    Ok(format!(
        "<span data-plec-node=\"{}\" data-plec-host=\"{}:{}\"><!--{placeholder}--></span>",
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
                route_params: Value::Object(
                    route
                        .params
                        .iter()
                        .map(|(name, value)| (name.clone(), Value::String(value.clone())))
                        .collect(),
                ),
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

struct SsrProgram<'a>(&'a Component);

impl plec_eval::core::ExpressionProgram for SsrProgram<'_> {
    fn program_len(&self, p: usize) -> Option<usize> {
        self.0.expressions.get(p).map(|p| p.instructions.len())
    }
    fn constant(&self, h: usize) -> RuntimeValue {
        self.0
            .constants
            .get(h)
            .cloned()
            .and_then(|v| runtime_from_json(v).ok())
            .unwrap_or_default()
    }
    fn string(&self, h: usize) -> Option<&str> {
        self.0.strings.get(h).map(String::as_str)
    }
    fn instruction(&self, p: usize, pc: usize) -> Option<plec_eval::core::EvalInstruction> {
        use plec_eval::core::EvalInstruction as E;
        Some(match self.0.expressions.get(p)?.instructions.get(pc)? {
            ExpressionInstruction::Constant { constant } => E::Constant(*constant),
            ExpressionInstruction::LoadState { state } => E::State(*state),
            ExpressionInstruction::LoadFrame { slot } => E::Frame(*slot),
            ExpressionInstruction::LoadProp { prop } => E::Prop(*prop),
            ExpressionInstruction::LoadHost { host } => E::Host(*host),
            ExpressionInstruction::LoadRowRecord => E::RowRecord,
            ExpressionInstruction::LoadRowField { field } => E::RowField(*field),
            ExpressionInstruction::LoadEventField { field } => E::Event(*field),
            ExpressionInstruction::Field { field } => E::Field(*field),
            ExpressionInstruction::Index => E::Index,
            ExpressionInstruction::LoadRef { reference } => E::Ref(*reference),
            ExpressionInstruction::Filter { predicate, .. } => E::Filter(*predicate),
            ExpressionInstruction::Map { mapper, .. } => E::Map(*mapper),
            ExpressionInstruction::String { kind, count } => E::String(kind.clone(), *count),
            ExpressionInstruction::OmitFields { fields } => E::OmitFields(fields.clone()),
            ExpressionInstruction::Unary { kind } => E::Unary(kind.clone()),
            ExpressionInstruction::Binary { kind } => E::Binary(kind.clone()),
            ExpressionInstruction::MakeArray { count, spreads } => {
                E::MakeArray(*count, spreads.clone())
            }
            ExpressionInstruction::MakeRecord { fields, spreads } => {
                E::MakeRecord(fields.clone(), spreads.clone())
            }
            ExpressionInstruction::Jump { target } => E::Jump(*target),
            ExpressionInstruction::JumpIfFalse { target } => E::JumpIfFalse(*target),
            ExpressionInstruction::JumpIfTrue { target } => E::JumpIfTrue(*target),
            ExpressionInstruction::Return => E::Return,
            ExpressionInstruction::Unknown => E::Unknown,
        })
    }
}

struct SsrHost<'a> {
    component: &'a Component,
    scope: &'a Scope<'a>,
    state: &'a mut RenderState,
}
impl plec_eval::core::ExpressionHost for SsrHost<'_> {
    fn load_state(&mut self, i: usize) -> RuntimeValue {
        self.scope
            .states
            .get(i)
            .map(runtime_json)
            .unwrap_or_default()
    }
    fn load_prop(&mut self, i: usize) -> RuntimeValue {
        self.scope
            .props
            .get(i)
            .map(runtime_json)
            .unwrap_or_default()
    }
    fn load_frame(&mut self, i: usize) -> RuntimeValue {
        self.scope
            .frame
            .get(i)
            .map(runtime_json)
            .unwrap_or_default()
    }
    fn load_row_record(&mut self) -> RuntimeValue {
        self.scope
            .row
            .as_ref()
            .map(|row| runtime_json(&Value::Object(row.clone())))
            .unwrap_or_default()
    }
    fn load_row_field(&mut self, f: &str) -> RuntimeValue {
        self.scope
            .row
            .as_ref()
            .and_then(|r| r.get(f))
            .map(runtime_json)
            .unwrap_or_default()
    }
    fn load_host(&mut self, i: usize) -> RuntimeValue {
        use plec_schema::delta::RuntimeValue as V;
        let Some(slot) = self.component.host_slots.get(i) else {
            return V::Null;
        };
        match slot.kind.as_str() {
            "cookie" => {
                if let Some(g) = &mut self.state.gate {
                    if g.development {
                        let name = slot
                            .name
                            .and_then(|n| self.component.strings.get(n))
                            .cloned()
                            .unwrap_or_else(|| "<unnamed>".into());
                        g.gated.push(format!("cookie:{name}"));
                    }
                }
                V::Null
            }
            "loaderData" => runtime_json(&self.scope.loader_data),
            "routeParams" => runtime_json(&self.scope.route_params),
            "routeSearch" => V::Record(
                self.scope
                    .request
                    .query
                    .iter()
                    .map(|(k, v)| {
                        (
                            k.clone(),
                            match v {
                                crate::request::QueryValue::One(s) => V::String(s.clone()),
                                crate::request::QueryValue::Many(v) => {
                                    V::Array(v.iter().cloned().map(V::String).collect())
                                }
                            },
                        )
                    })
                    .collect(),
            ),
            "location" => V::Record(HashMap::from([
                (
                    "pathname".into(),
                    V::String(self.scope.request.pathname.clone()),
                ),
                (
                    "search".into(),
                    V::String(super::url_search(&self.scope.request.url)),
                ),
            ])),
            _ => V::Null,
        }
    }
}

fn runtime_json(value: &serde_json::Value) -> RuntimeValue {
    runtime_from_json(value.clone()).unwrap_or_default()
}

/// Evaluator errors intentionally remain soft failures during SSR: compiler
/// validation is primary, while runtime budgets fail closed to JSON null.
pub(crate) fn evaluate(
    component: &Component,
    expression: usize,
    scope: &Scope<'_>,
    state: &mut RenderState,
) -> Value {
    let mut host = SsrHost {
        component,
        scope,
        state,
    };
    plec_eval::core::evaluate(&SsrProgram(component), &mut host, expression)
        .map(RuntimeValue::into_json_value)
        .unwrap_or(Value::Null)
}

/// Truthiness for JSON-valued render facts outside expression evaluation.
pub(crate) fn truthy(value: &Value) -> bool {
    json_truthy(value)
}

/// Canonical string conversion for JSON-valued render facts and loop keys.
pub(crate) fn dom_string(value: &Value) -> String {
    json_dom_string(value)
}

fn canonical_key(value: &Value) -> String {
    dom_string(value)
}

#[cfg(test)]
mod shared_evaluator_tests {
    use super::*;
    use crate::request::RequestContext;
    use axum::http::{HeaderMap, Method};
    use plec_schema::typed::TypedApplication;

    fn request() -> RequestContext {
        RequestContext {
            url: "http://localhost/".into(),
            pathname: "/".into(),
            method: Method::GET,
            headers: HeaderMap::new(),
            cookies: HashMap::new(),
            params: HashMap::new(),
            query: HashMap::new(),
        }
    }

    #[test]
    fn ssr_adapter_executes_the_shared_core_and_keeps_soft_failure() {
        let component: Component = serde_json::from_value(serde_json::json!({
            "strings": [], "constants": [3, 4],
            "expressions": [{"instructions": [
                {"op":"constant","constant":0}, {"op":"constant","constant":1},
                {"op":"binary","kind":"add"}, {"op":"return"}
            ]}]
        }))
        .unwrap();
        let request = request();
        let scope = Scope::for_loader(&request);
        let mut state = RenderState::bare();
        assert_eq!(
            evaluate(&component, 0, &scope, &mut state),
            serde_json::json!(7.0)
        );
        assert_eq!(
            evaluate(&component, usize::MAX, &scope, &mut state),
            Value::Null
        );
    }

    #[test]
    fn browser_and_ssr_adapters_match_for_the_same_expression_fixture() {
        let fixture = serde_json::json!({
            "version":"0.10", "rootNode":0,
            "strings":["name", "users"],
            "constants":[[{"name":"  Ada  "}], 0, "!", true],
            "routeErrorState":null,
            "nodes":[{"op":"element","tag":0}],
            "expressions":[
                {"instructions":[
                    {"op":"constant","constant":0}, {"op":"map","mapper":1,"itemSlot":0},
                    {"op":"makeRecord","fields":[1]}, {"op":"field","field":1},
                    {"op":"constant","constant":1}, {"op":"index"},
                    {"op":"constant","constant":2}, {"op":"binary","kind":"add"},
                    {"op":"constant","constant":3}, {"op":"jumpIfFalse","target":10},
                    {"op":"return"}
                ]},
                {"instructions":[
                    {"op":"loadRowRecord"}, {"op":"field","field":0},
                    {"op":"string","kind":"trim","count":1}, {"op":"return"}
                ]}
            ]
        });
        let browser_app: TypedApplication = serde_json::from_value(fixture.clone()).unwrap();
        let server_component: Component = serde_json::from_value(fixture).unwrap();
        let browser = plec_eval::eval::typed_eval(&browser_app, None, 0, &[], None, 0).unwrap();

        let request = request();
        let scope = Scope::for_loader(&request);
        let mut state = RenderState::bare();
        let ssr = evaluate(&server_component, 0, &scope, &mut state);

        assert_eq!(browser.into_json_value(), ssr);
        assert_eq!(ssr, Value::String("Ada!".into()));
    }
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
