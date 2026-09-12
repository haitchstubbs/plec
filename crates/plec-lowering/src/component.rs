use plec_hir::{HirBindingKind, HirComponent, HirNode};
use plec_ir::{
    ActionInstruction, ActionProgram, ComponentParameter, ExecutableApplication, HostRef, Input,
    Listener, Reaction, RefSlot, RouteOutlet, StateSlot,
};

use crate::{ComponentTargets, Ctx, LoweringError};

pub fn lower_component_to_executable(
    component: &HirComponent,
) -> Result<ExecutableApplication, LoweringError> {
    lower_component(component, None)
}

pub fn lower_route_loader_to_executable(
    component: &HirComponent,
    loader_name: &str,
    result_state_name: &str,
    outlet_id: &str,
) -> Result<ExecutableApplication, LoweringError> {
    let mut app = lower_component(component, None)?;
    let loader = component
        .callables
        .iter()
        .position(|callable| component.bindings[callable.binding.0 as usize].name == loader_name)
        .ok_or_else(|| LoweringError("route loader callable missing".into()))?;
    let state = component
        .states
        .iter()
        .position(|state| component.bindings[state.value.0 as usize].name == result_state_name)
        .ok_or_else(|| LoweringError("route loader result state missing".into()))?;
    let action = app
        .actions
        .get_mut(loader)
        .ok_or_else(|| LoweringError("route loader action missing".into()))?;
    let mut fetches = 0;
    for instruction in &mut action.instructions {
        if let ActionInstruction::CapabilityRequest {
            request: plec_ir::CapabilityRequest::Fetch { require_ok, .. },
            ..
        } = instruction
        {
            *require_ok = true;
            fetches += 1;
        }
    }
    if fetches == 0 {
        return Err(LoweringError(
            "route loader requires return await fetch(url)".into(),
        ));
    }
    action.route_loader = true;
    action.loader_result_state = Some(state);
    app.route_outlets.push(RouteOutlet {
        id: outlet_id.into(),
        node: app.root_node,
    });
    Ok(app)
}

pub(crate) fn lower_component(
    component: &HirComponent,
    targets: Option<&ComponentTargets>,
) -> Result<ExecutableApplication, LoweringError> {
    let mut ctx = Ctx::new(component, targets);
    let mut attached_host_refs = std::collections::HashSet::new();
    for node in &component.nodes {
        if let HirNode::Element(element) = node {
            if let Some(reference) = element.host_ref {
                if !attached_host_refs.insert(reference) {
                    return Err(ctx.err("useHostRef may attach to exactly one intrinsic element"));
                }
            }
        }
    }
    // Process inputs BEFORE states/ref slots so that host bindings are available
    // for state/ref initializers that reference them (e.g., useState(Route.useLoaderData())).
    for input in &component.inputs {
        match input.kind.as_str() {
            "collection" => {
                let name = ctx.string(&input.name);
                let slot = ctx.app.inputs.len();
                ctx.inputs.insert(input.binding, slot);
                ctx.app.inputs.push(Input {
                    name,
                    kind: "collection",
                });
            }
            "location" => {
                let host = ctx.host("location", None)?;
                ctx.hosts.insert(input.binding, host);
            }
            "loaderData" => {
                let host = ctx.host("loaderData", None)?;
                ctx.hosts.insert(input.binding, host);
            }
            _ => return Err(Ctx::new(component, targets).err("input kind is not executable")),
        }
    }
    for parameter in &component.parameters {
        let name = match &parameter.source {
            plec_hir::HirParameterSource::Prop { name } => name.as_str(),
            plec_hir::HirParameterSource::Direct => "__plec_props",
        };
        let callable = matches!(
            component.bindings[parameter.binding.0 as usize].kind,
            HirBindingKind::Parameter { callable: true }
        );
        let slot = ctx.app.parameters.len();
        ctx.props.insert(parameter.binding, slot);
        if callable {
            ctx.callback_props.insert(parameter.binding, slot);
        }
        let name = ctx.string(name);
        let component_prop = component.nodes.iter().any(|node| matches!(node,
            HirNode::Component(call) if matches!(call.target, plec_hir::HirComponentTarget::Prop(binding) if binding == parameter.binding)
        ));
        ctx.app.parameters.push(ComponentParameter {
            name,
            callable,
            component: component_prop,
        });
    }
    for state in &component.states {
        let initial_expression = ctx.expression(state.initializer, false)?.0;
        let slot = ctx.app.state_slots.len();
        ctx.states.insert(state.value, slot);
        ctx.app.state_slots.push(StateSlot {
            initial_expression,
            frame_slot: slot,
        });
    }
    for reference in &component.ref_slots {
        let initial_expression = ctx.expression(reference.initializer, false)?.0;
        let slot = ctx.app.ref_slots.len();
        ctx.refs.insert(reference.binding, slot);
        ctx.app.ref_slots.push(RefSlot { initial_expression });
    }
    for binding in &component.bindings {
        if matches!(binding.kind, HirBindingKind::HostRef) {
            let slot = ctx.app.host_refs.len();
            ctx.host_refs.insert(binding.id, slot);
            ctx.app.host_refs.push(HostRef {});
        }
    }
    for callable in &component.callables {
        let action = ctx.app.actions.len();
        ctx.app.actions.push(ActionProgram {
            frame_slots: 0,
            parameter_slots: vec![],
            loader_result_state: None,
            route_loader: false,
            instructions: vec![],
        });
        ctx.callables.insert(callable.binding, action);
    }
    for callable in &component.callables {
        let action = *ctx
            .callables
            .get(&callable.binding)
            .expect("action reserved");
        ctx.action(&callable.body, &callable.parameters, false)?;
        let lowered = ctx.app.actions.pop().expect("action lowered");
        ctx.app.actions[action] = lowered;
    }
    for reaction in &component.reactions {
        let mut dependencies = Vec::new();
        let mut sources = std::collections::BTreeSet::new();
        let mut has_prop_dependency = false;
        for dependency in &reaction.dependencies {
            let (expression, deps) = ctx.expression(*dependency, false)?;
            has_prop_dependency |=
                ctx.app.expressions[expression]
                    .instructions
                    .iter()
                    .any(|instruction| {
                        matches!(instruction, plec_ir::ExpressionInstruction::LoadProp { .. })
                    });
            dependencies.push(expression);
            sources.extend(deps);
        }
        if sources.is_empty() && !has_prop_dependency {
            return Err(ctx.err(&format!(
                "useReaction dependencies must reference reactive state or props in {}",
                component.id.local_name
            )));
        }
        let action = ctx.action(&reaction.body, &[], false)?;
        let cleanup_action = reaction
            .cleanup
            .as_ref()
            .map(|cleanup| ctx.action(cleanup, &[], false))
            .transpose()?;
        let handle = ctx.app.reactions.len();
        ctx.edges(sources, "reaction", handle);
        for expression in &dependencies {
            ctx.prop_edges(*expression, "reaction", handle);
        }
        ctx.app.reactions.push(Reaction {
            dependencies,
            action,
            cleanup_action,
        });
    }
    for listener in &component.listeners {
        let action = ctx.callable(&listener.callable)?;
        let source = match listener.source.as_str() {
            "window" => "window",
            "document" => "document",
            _ => return Err(ctx.err("unsupported listener source")),
        };
        let event = ctx.string(&listener.event);
        ctx.app.listeners.push(Listener {
            source,
            event,
            action,
        });
    }
    if component.root_nodes.len() != 1 {
        return Err(ctx.err("executable roots require exactly one node"));
    }
    ctx.app.root_node = ctx.node(component.root_nodes[0], None)?;
    // Per-component collection and instruction budgets also guard the
    // single-component entry points (route loaders); the application entry
    // adds the cross-component aggregates on top.
    crate::application::component_budget_usage(&ctx.app)?;
    Ok(ctx.app)
}
