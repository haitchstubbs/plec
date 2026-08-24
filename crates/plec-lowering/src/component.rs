use plec_hir::{HirBindingKind, HirComponent};
use plec_ir::{
    ActionInstruction, ActionProgram, ComponentParameter, ExecutableApplication, Input,
    RouteOutlet, StateSlot,
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
    if !action.instructions.iter().any(|instruction| {
        matches!(
            instruction,
            ActionInstruction::CapabilityRequest {
                request: plec_ir::CapabilityRequest::Fetch { .. },
                ..
            }
        )
    }) {
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
    for parameter in &component.parameters {
        let plec_hir::HirParameterSource::Prop { name } = &parameter.source else {
            return Err(ctx.err("direct component parameters are not executable"));
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
        ctx.app
            .parameters
            .push(ComponentParameter { name, callable });
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
                let host = ctx.host("location", None);
                ctx.hosts.insert(input.binding, host);
            }
            _ => return Err(Ctx::new(component, targets).err("input kind is not executable")),
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
    if component.root_nodes.len() != 1 {
        return Err(ctx.err("executable roots require exactly one node"));
    }
    ctx.app.root_node = ctx.node(component.root_nodes[0], None)?;
    Ok(ctx.app)
}
