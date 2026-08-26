use plec_hir::{HirApplication, HirBindingKind, HirNode, HirParameterSource};
use plec_ir::{ComponentApplication, ExecutableComponent, COMPONENT_VERSION};

use crate::{component::lower_component, ComponentTargets, LoweringError};

pub fn lower_application_to_executable(
    application: &HirApplication,
) -> Result<ComponentApplication, LoweringError> {
    let mut targets = ComponentTargets::new();
    for (index, component) in application.components.iter().enumerate() {
        let direct_props = component.parameters.iter().any(|parameter| matches!(parameter.source, HirParameterSource::Direct));
        if direct_props && component.parameters.len() != 1 {
            return Err(LoweringError("a direct component props bag must be the only parameter".into()));
        }
        let component_props = component.nodes.iter().filter_map(|node| match node {
            plec_hir::HirNode::Component(call) => match call.target {
                plec_hir::HirComponentTarget::Prop(binding) => Some(binding),
                _ => None,
            },
            _ => None,
        }).collect::<std::collections::HashSet<_>>();
        let props = component
            .parameters
            .iter()
            .map(|parameter| match &parameter.source {
                HirParameterSource::Prop { name } => Ok((
                    name.clone(),
                    matches!(
                        component.bindings[parameter.binding.0 as usize].kind,
                        HirBindingKind::Parameter { callable: true }
                    ),
                    component_props.contains(&parameter.binding),
                )),
                HirParameterSource::Direct => Ok(("__plec_props".into(), false, false)),
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
        .filter(|(name, _, _)| name != "children")
            .collect::<Vec<_>>();
        let has_slot = component
            .nodes
            .iter()
            .filter(|node| matches!(node, HirNode::Slot(_)))
            .count();
        if has_slot > 1 {
            return Err(LoweringError("components support one children slot".into()));
        }
        targets.insert(component.id.clone(), (index, props, has_slot == 1, direct_props));
    }
    let root_component = *targets
        .get(&application.root)
        .map(|(index, _, _, _)| index)
        .ok_or_else(|| LoweringError("application root component missing".into()))?;
    let components = application
        .components
        .iter()
        .map(|component| {
            let app = lower_component(component, Some(&targets))?;
            Ok(ExecutableComponent {
                id: format!("{}#{}", component.id.module_id, component.id.local_name),
                root_node: app.root_node,
                strings: app.strings,
                constants: app.constants,
                nodes: app.nodes,
                texts: app.texts,
                bindings: app.bindings,
                prop_programs: app.prop_programs,
                events: app.events,
                inputs: app.inputs,
                host_slots: app.host_slots,
                capabilities: app.capabilities,
                state_slots: app.state_slots,
                ref_slots: app.ref_slots,
                host_refs: app.host_refs,
                reactions: app.reactions,
                listeners: app.listeners,
                parameters: app.parameters,
                expressions: app.expressions,
                actions: app.actions,
                loops: app.loops,
                dependency_edges: app.dependency_edges,
                route_outlets: app.route_outlets,
            })
        })
        .collect::<Result<Vec<_>, LoweringError>>()?;
    Ok(ComponentApplication {
        version: COMPONENT_VERSION,
        root_component,
        components,
    })
}
