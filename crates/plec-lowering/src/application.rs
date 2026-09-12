use plec_hir::{HirApplication, HirBindingKind, HirNode, HirParameterSource, HirProp};
use plec_ir::{
    ComponentApplication, ExecutableApplication, ExecutableComponent, COMPONENT_VERSION,
};

use crate::{component::lower_component, ComponentTarget, ComponentTargets, LoweringError};

pub fn lower_application_to_executable(
    application: &HirApplication,
) -> Result<ComponentApplication, LoweringError> {
    use plec_ir::limits::{
        MAX_COMPONENT_COUNT, MAX_TOTAL_INSTRUCTIONS, MAX_TOTAL_IR_ENTRIES,
        MAX_TOTAL_STRING_POOL_BYTES,
    };

    if application.components.len() > MAX_COMPONENT_COUNT {
        return Err(LoweringError(format!(
            "Lowered application exceeds the maximum component count of {MAX_COMPONENT_COUNT}"
        )));
    }
    let mut targets = ComponentTargets::new();
    for (index, component) in application.components.iter().enumerate() {
        let direct_props = component
            .parameters
            .iter()
            .any(|parameter| matches!(parameter.source, HirParameterSource::Direct));
        if direct_props && component.parameters.len() != 1 {
            return Err(LoweringError(
                "a direct component props bag must be the only parameter".into(),
            ));
        }
        let component_props = component
            .nodes
            .iter()
            .filter_map(|node| match node {
                plec_hir::HirNode::Component(call) => match call.target {
                    plec_hir::HirComponentTarget::Prop(binding) => Some(binding),
                    _ => None,
                },
                _ => None,
            })
            .collect::<std::collections::HashSet<_>>();
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
        targets.insert(
            component.id.clone(),
            ComponentTarget::Native {
                index,
                parameters: props,
                has_slot: has_slot == 1,
                direct_props,
            },
        );
    }
    for component in &application.components {
        for node in &component.nodes {
            let HirNode::Component(call) = node else {
                continue;
            };
            for prop in &call.props {
                let HirProp::Component { target, .. } = prop else {
                    continue;
                };
                if targets.contains_key(target) {
                    continue;
                }
                if let Some(provider) = target.module_id.strip_prefix("host:") {
                    targets.insert(
                        target.clone(),
                        ComponentTarget::Host {
                            provider: provider.to_owned(),
                            component: target.local_name.clone(),
                        },
                    );
                }
            }
        }
    }
    let root_component = targets
        .get(&application.root)
        .map(|target| match target {
            ComponentTarget::Native { index, .. } => *index,
            ComponentTarget::Host { .. } => usize::MAX,
        })
        .ok_or_else(|| LoweringError("application root component missing".into()))?;
    // Aggregate executable-IR budgets: enforced here (compiler side) in
    // addition to the artifact decode boundary so a pathological
    // application fails during lowering instead of growing memory and
    // serialized output without bound.
    let mut total_entries = 0usize;
    let mut total_instructions = 0usize;
    let mut total_string_bytes = 0usize;
    let mut components = Vec::with_capacity(application.components.len());
    for component in &application.components {
        let app = lower_component(component, Some(&targets))?;
        let (entries, instructions, string_bytes) = component_budget_usage(&app)?;
        total_entries = total_entries
            .checked_add(entries)
            .ok_or_else(|| LoweringError("lowered IR entry accounting overflowed".into()))?;
        total_instructions = total_instructions.checked_add(instructions).ok_or_else(|| {
            LoweringError("lowered instruction accounting overflowed".into())
        })?;
        total_string_bytes = total_string_bytes
            .checked_add(string_bytes)
            .ok_or_else(|| LoweringError("string pool accounting overflowed".into()))?;
        if total_entries > MAX_TOTAL_IR_ENTRIES {
            return Err(LoweringError(format!(
                "Lowered application exceeds the maximum total IR entries of {MAX_TOTAL_IR_ENTRIES}"
            )));
        }
        if total_instructions > MAX_TOTAL_INSTRUCTIONS {
            return Err(LoweringError(format!(
                "Lowered application exceeds the maximum total instruction count of {MAX_TOTAL_INSTRUCTIONS}"
            )));
        }
        if total_string_bytes > MAX_TOTAL_STRING_POOL_BYTES {
            return Err(LoweringError(format!(
                "Lowered application exceeds the maximum string pool budget of {MAX_TOTAL_STRING_POOL_BYTES} bytes"
            )));
        }
        components.push(ExecutableComponent {
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
        });
    }
    Ok(ComponentApplication {
        version: COMPONENT_VERSION,
        root_component,
        components,
    })
}

/// Per-component executable-IR budget check plus usage accounting.
///
/// Returns `(entries, instructions, string_bytes)` for aggregate accounting
/// and rejects any single collection beyond
/// `MAX_COMPONENT_COLLECTION_LEN`. Constant-value tree nodes are not walked
/// here; they remain bounded by `MAX_TOTAL_CONSTANT_NODES` at the artifact
/// decode boundary.
pub(crate) fn component_budget_usage(
    app: &ExecutableApplication,
) -> Result<(usize, usize, usize), LoweringError> {
    use plec_ir::limits::{MAX_COMPONENT_COLLECTION_LEN, MAX_TOTAL_INSTRUCTIONS};

    let collections = [
        app.strings.len(),
        app.constants.len(),
        app.nodes.len(),
        app.texts.len(),
        app.bindings.len(),
        app.prop_programs.len(),
        app.events.len(),
        app.inputs.len(),
        app.host_slots.len(),
        app.capabilities.len(),
        app.state_slots.len(),
        app.ref_slots.len(),
        app.host_refs.len(),
        app.reactions.len(),
        app.listeners.len(),
        app.parameters.len(),
        app.expressions.len(),
        app.actions.len(),
        app.loops.len(),
        app.dependency_edges.len(),
        app.route_outlets.len(),
    ];
    for len in collections {
        if len > MAX_COMPONENT_COLLECTION_LEN {
            return Err(LoweringError(format!(
                "Lowered component collection exceeds the maximum length of {MAX_COMPONENT_COLLECTION_LEN}"
            )));
        }
    }
    let entries = collections
        .iter()
        .try_fold(0usize, |total, len| {
            total.checked_add(*len).ok_or_else(|| {
                LoweringError("lowered IR entry accounting overflowed".into())
            })
        })?;
    let instructions = app
        .expressions
        .iter()
        .map(|program| program.instructions.len())
        .chain(app.actions.iter().map(|program| program.instructions.len()))
        .try_fold(0usize, |total, len| {
            total.checked_add(len).ok_or_else(|| {
                LoweringError("lowered instruction accounting overflowed".into())
            })
        })?;
    if instructions > MAX_TOTAL_INSTRUCTIONS {
        return Err(LoweringError(format!(
            "Lowered component exceeds the maximum total instruction count of {MAX_TOTAL_INSTRUCTIONS}"
        )));
    }
    let string_bytes = app
        .strings
        .iter()
        .map(String::len)
        .try_fold(0usize, |total, len| {
            total.checked_add(len).ok_or_else(|| {
                LoweringError("string pool accounting overflowed".into())
            })
        })?;
    Ok((entries, instructions, string_bytes))
}
