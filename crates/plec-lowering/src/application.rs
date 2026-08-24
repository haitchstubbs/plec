use plec_hir::{HirApplication, HirBindingKind, HirNode, HirParameterSource};
use plec_ir::{ComponentApplication, ExecutableComponent, COMPONENT_VERSION};

use crate::{component::lower_component, ComponentTargets, LoweringError};

pub fn lower_application_to_executable(
    application: &HirApplication,
) -> Result<ComponentApplication, LoweringError> {
    let mut targets = ComponentTargets::new();
    for (index, component) in application.components.iter().enumerate() {
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
                )),
                HirParameterSource::Direct => Err(LoweringError(
                    "direct component parameters are not executable".into(),
                )),
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|(name, _)| name != "children")
            .collect::<Vec<_>>();
        let has_slot = component
            .nodes
            .iter()
            .filter(|node| matches!(node, HirNode::Slot(_)))
            .count();
        if has_slot > 1 {
            return Err(LoweringError("components support one children slot".into()));
        }
        targets.insert(component.id.clone(), (index, props, has_slot == 1));
    }
    let root_component = *targets
        .get(&application.root)
        .map(|(index, _, _)| index)
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
                state_slots: app.state_slots,
                parameters: app.parameters,
                expressions: app.expressions,
                actions: app.actions,
                loops: app.loops,
                dependency_edges: app.dependency_edges,
            })
        })
        .collect::<Result<Vec<_>, LoweringError>>()?;
    Ok(ComponentApplication {
        version: COMPONENT_VERSION,
        root_component,
        components,
    })
}
