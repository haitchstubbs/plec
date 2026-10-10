//! Direct browser decoder for the executable typed-IR schema in `typed.rs`.
//! Keep these conversions adjacent to that schema; this module deliberately
//! does not deserialize through Serde or construct a `serde_json::Value`.

use super::*;
use crate::js_decode as js;
use wasm_bindgen::JsValue;

fn field<T>(
    object: &mut js::ObjectDecoder<'_>,
    name: &str,
    decode: impl FnOnce(&JsValue) -> Result<T, JsValue>,
) -> Result<T, JsValue> {
    decode(&object.get(name)?)
}

fn optional<T>(
    object: &mut js::ObjectDecoder<'_>,
    name: &str,
    decode: impl FnOnce(&JsValue) -> Result<T, JsValue>,
) -> Result<Option<T>, JsValue> {
    object
        .optional(name)?
        .map(|value| decode(&value))
        .transpose()
}

fn defaulted<T>(
    object: &mut js::ObjectDecoder<'_>,
    name: &str,
    default: T,
    decode: impl FnOnce(&JsValue) -> Result<T, JsValue>,
) -> Result<T, JsValue> {
    if object.has(name) {
        field(object, name, decode)
    } else {
        Ok(default)
    }
}

fn array<T>(
    value: &JsValue,
    decode: impl Fn(&JsValue) -> Result<T, JsValue>,
) -> Result<Vec<T>, JsValue> {
    js::array(value)?.iter().map(|item| decode(&item)).collect()
}

fn strings(value: &JsValue) -> Result<Vec<String>, JsValue> {
    array(value, js::string)
}

fn indices(value: &JsValue) -> Result<Vec<usize>, JsValue> {
    array(value, js::usize)
}

pub fn decode_component_application(value: &JsValue) -> Result<TypedComponentApplication, JsValue> {
    let mut object = js::ObjectDecoder::new(value)?;
    let result = TypedComponentApplication {
        version: field(&mut object, "version", js::string)?,
        root_component: field(&mut object, "rootComponent", js::usize)?,
        components: field(&mut object, "components", |value| {
            array(value, decode_typed_application)
        })?,
    };
    Ok(result)
}

pub fn decode_typed_application(value: &JsValue) -> Result<TypedApplication, JsValue> {
    let mut object = js::ObjectDecoder::new(value)?;
    let version = defaulted(&mut object, "version", component_version(), js::string)?;
    let id = defaulted(&mut object, "id", String::new(), js::string)?;
    let root_node = field(&mut object, "rootNode", js::usize)?;
    let strings = field(&mut object, "strings", strings)?;
    let constants = defaulted(&mut object, "constants", Vec::new(), |v| {
        array(v, js::runtime_value)
    })?;
    let nodes = field(&mut object, "nodes", |v| array(v, decode_node))?;
    let texts = defaulted(&mut object, "texts", Vec::new(), |v| array(v, decode_text))?;
    let bindings = defaulted(&mut object, "bindings", Vec::new(), |v| {
        array(v, decode_binding)
    })?;
    let prop_programs = defaulted(&mut object, "propPrograms", Vec::new(), |v| {
        array(v, decode_prop_program)
    })?;
    let events = defaulted(&mut object, "events", Vec::new(), |v| {
        array(v, decode_event)
    })?;
    let inputs = defaulted(&mut object, "inputs", Vec::new(), |v| {
        array(v, decode_input)
    })?;
    let state_slots = defaulted(&mut object, "stateSlots", Vec::new(), |v| {
        array(v, decode_state_slot)
    })?;
    let ref_slots = defaulted(&mut object, "refSlots", Vec::new(), |v| {
        array(v, decode_ref_slot)
    })?;
    let host_refs = defaulted(&mut object, "hostRefs", Vec::new(), |v| {
        array(v, decode_host_ref)
    })?;
    let reactions = defaulted(&mut object, "reactions", Vec::new(), |v| {
        array(v, decode_reaction)
    })?;
    let listeners = defaulted(&mut object, "listeners", Vec::new(), |v| {
        array(v, decode_listener)
    })?;
    let parameters = defaulted(&mut object, "parameters", Vec::new(), |v| {
        array(v, decode_parameter)
    })?;
    let route_error_state = optional(&mut object, "routeErrorState", js::usize)?;
    let expressions = defaulted(&mut object, "expressions", Vec::new(), |v| {
        array(v, decode_program)
    })?;
    let actions = defaulted(&mut object, "actions", Vec::new(), |v| {
        array(v, decode_action)
    })?;
    let loops = defaulted(&mut object, "loops", Vec::new(), |v| array(v, decode_loop))?;
    let dependency_edges = defaulted(&mut object, "dependencyEdges", Vec::new(), |v| {
        array(v, decode_dependency_edge)
    })?;
    let route_outlets = defaulted(&mut object, "routeOutlets", Vec::new(), |v| {
        array(v, decode_route_outlet)
    })?;
    let host_slots = defaulted(&mut object, "hostSlots", Vec::new(), |v| {
        array(v, decode_host_slot)
    })?;
    let capabilities = defaulted(&mut object, "capabilities", Vec::new(), |v| {
        array(v, decode_cookie_capability)
    })?;
    let server_actions = defaulted(&mut object, "serverActions", Vec::new(), |v| {
        array(v, decode_server_action_ref)
    })?;

    Ok(TypedApplication {
        version,
        id,
        root_node,
        strings,
        constants,
        nodes,
        texts,
        bindings,
        prop_programs,
        events,
        inputs,
        state_slots,
        ref_slots,
        host_refs,
        reactions,
        listeners,
        parameters,
        route_error_state,
        expressions,
        actions,
        loops,
        dependency_edges,
        route_outlets,
        host_slots,
        capabilities,
        server_actions,
        host_inputs: HashMap::new(),
        runtime_props: Vec::new(),
        runtime_component_props: Vec::new(),
        runtime_host_component_props: Vec::new(),
        ref_values: Vec::new(),
    })
}

fn decode_node(value: &JsValue) -> Result<TypedNode, JsValue> {
    let mut object = js::ObjectDecoder::new(value)?;
    let op = field(&mut object, "op", js::string)?;
    let parent = optional(&mut object, "parent", js::usize)?;
    Ok(match op.as_str() {
        "element" => TypedNode::Element {
            tag: field(&mut object, "tag", js::usize)?,
            namespace: defaulted(&mut object, "namespace", html_namespace(), js::string)?,
            parent,
            children: defaulted(&mut object, "children", Vec::new(), indices)?,
            host_ref: optional(&mut object, "hostRef", js::usize)?,
        },
        "text" => TypedNode::Text {
            text: field(&mut object, "text", js::usize)?,
            parent,
        },
        "conditional" => TypedNode::Conditional {
            test: field(&mut object, "test", js::usize)?,
            parent,
            consequent: field(&mut object, "consequent", js::usize)?,
            alternate: optional(&mut object, "alternate", js::usize)?,
        },
        "loop" => TypedNode::Loop {
            r#loop: field(&mut object, "loop", js::usize)?,
            parent,
        },
        "component" => TypedNode::Component {
            component: field(&mut object, "component", js::usize)?,
            parent,
            props: defaulted(&mut object, "props", Vec::new(), |v| {
                array(v, decode_component_prop)
            })?,
            children: defaulted(&mut object, "children", Vec::new(), indices)?,
        },
        "dynamicComponent" => TypedNode::DynamicComponent {
            prop: field(&mut object, "prop", js::usize)?,
            parent,
            props: defaulted(&mut object, "props", Vec::new(), |v| {
                array(v, decode_component_prop)
            })?,
            children: defaulted(&mut object, "children", Vec::new(), indices)?,
        },
        "hostComponent" => TypedNode::HostComponent {
            provider: field(&mut object, "provider", js::string)?,
            component: field(&mut object, "component", js::string)?,
            parent,
            props: defaulted(&mut object, "props", Vec::new(), |v| {
                array(v, decode_component_prop)
            })?,
        },
        "slot" => TypedNode::Slot { parent },
        _ => return Err(JsValue::from_str("unknown variant for enum TypedNode")),
    })
}

fn decode_component_prop(value: &JsValue) -> Result<TypedComponentProp, JsValue> {
    let mut object = js::ObjectDecoder::new(value)?;
    let kind = field(&mut object, "kind", js::string)?;
    let name = field(&mut object, "name", js::usize)?;
    let expression = optional(&mut object, "expression", js::usize)?;
    let action = optional(&mut object, "action", js::usize)?;
    let component = optional(&mut object, "component", js::usize)?;
    let host = optional(&mut object, "host", decode_host_component_target)?;
    match (kind.as_str(), expression, action, component, host) {
        ("value", Some(expression), None, None, None) => {
            Ok(TypedComponentProp::Value { name, expression })
        }
        ("callable", None, Some(action), None, None) => {
            Ok(TypedComponentProp::Callable { name, action })
        }
        ("component", None, None, Some(component), host) => Ok(TypedComponentProp::Component {
            name,
            component,
            host,
        }),
        _ => Err(JsValue::from_str("invalid component prop")),
    }
}

fn decode_host_component_target(value: &JsValue) -> Result<TypedHostComponentTarget, JsValue> {
    let mut o = js::ObjectDecoder::new(value)?;
    Ok(TypedHostComponentTarget {
        provider: field(&mut o, "provider", js::string)?,
        component: field(&mut o, "component", js::string)?,
    })
}

fn decode_text(value: &JsValue) -> Result<TypedText, JsValue> {
    let mut o = js::ObjectDecoder::new(value)?;
    Ok(TypedText {
        value: optional(&mut o, "value", js::string)?,
        binding: optional(&mut o, "binding", js::usize)?,
    })
}
fn decode_binding(value: &JsValue) -> Result<TypedBinding, JsValue> {
    let mut o = js::ObjectDecoder::new(value)?;
    Ok(TypedBinding {
        target: field(&mut o, "target", js::usize)?,
        sink: field(&mut o, "sink", js::string)?,
        name: optional(&mut o, "name", js::usize)?,
        expression: field(&mut o, "expression", js::usize)?,
    })
}
fn decode_prop_program(value: &JsValue) -> Result<TypedPropProgram, JsValue> {
    let mut o = js::ObjectDecoder::new(value)?;
    Ok(TypedPropProgram {
        target: field(&mut o, "target", js::usize)?,
        writes: field(&mut o, "writes", |v| array(v, decode_prop_write))?,
    })
}
fn decode_prop_write(value: &JsValue) -> Result<TypedPropWrite, JsValue> {
    let mut o = js::ObjectDecoder::new(value)?;
    Ok(TypedPropWrite {
        name: optional(&mut o, "name", js::usize)?,
        kind: field(&mut o, "kind", js::string)?,
        constant: optional(&mut o, "constant", js::usize)?,
        expression: optional(&mut o, "expression", js::usize)?,
        spread: defaulted(&mut o, "spread", false, js::boolean)?,
    })
}
fn decode_input(value: &JsValue) -> Result<TypedInput, JsValue> {
    let mut o = js::ObjectDecoder::new(value)?;
    Ok(TypedInput {
        name: field(&mut o, "name", js::usize)?,
        kind: field(&mut o, "kind", js::string)?,
    })
}
fn decode_state_slot(value: &JsValue) -> Result<TypedStateSlot, JsValue> {
    let mut o = js::ObjectDecoder::new(value)?;
    Ok(TypedStateSlot {
        name: optional(&mut o, "name", js::usize)?,
        initial_expression: field(&mut o, "initialExpression", js::usize)?,
        frame_slot: field(&mut o, "frameSlot", js::usize)?,
    })
}
fn decode_ref_slot(value: &JsValue) -> Result<TypedRefSlot, JsValue> {
    let mut o = js::ObjectDecoder::new(value)?;
    Ok(TypedRefSlot {
        initial_expression: field(&mut o, "initialExpression", js::usize)?,
    })
}
fn decode_host_ref(value: &JsValue) -> Result<TypedHostRef, JsValue> {
    let _ = js::ObjectDecoder::new(value)?;
    Ok(TypedHostRef {})
}
fn decode_reaction(value: &JsValue) -> Result<TypedReaction, JsValue> {
    let mut o = js::ObjectDecoder::new(value)?;
    Ok(TypedReaction {
        dependencies: field(&mut o, "dependencies", indices)?,
        action: field(&mut o, "action", js::usize)?,
        cleanup_action: optional(&mut o, "cleanupAction", js::usize)?,
    })
}
fn decode_listener(value: &JsValue) -> Result<TypedGlobalListener, JsValue> {
    let mut o = js::ObjectDecoder::new(value)?;
    Ok(TypedGlobalListener {
        source: field(&mut o, "source", js::string)?,
        event: field(&mut o, "event", js::usize)?,
        action: field(&mut o, "action", js::usize)?,
    })
}
fn decode_parameter(value: &JsValue) -> Result<TypedComponentParameter, JsValue> {
    let mut o = js::ObjectDecoder::new(value)?;
    Ok(TypedComponentParameter {
        name: field(&mut o, "name", js::usize)?,
        callable: field(&mut o, "callable", js::boolean)?,
        component: defaulted(&mut o, "component", false, js::boolean)?,
    })
}
fn decode_event(value: &JsValue) -> Result<TypedEvent, JsValue> {
    let mut o = js::ObjectDecoder::new(value)?;
    Ok(TypedEvent {
        target: field(&mut o, "target", js::usize)?,
        event_type: field(&mut o, "type", js::usize)?,
        action: field(&mut o, "action", js::usize)?,
        fields: defaulted(&mut o, "fields", Vec::new(), |v| {
            array(v, decode_event_field)
        })?,
        r#loop: optional(&mut o, "loop", js::usize)?,
    })
}
fn decode_event_field(value: &JsValue) -> Result<TypedEventField, JsValue> {
    let mut o = js::ObjectDecoder::new(value)?;
    Ok(TypedEventField {
        name: field(&mut o, "name", js::usize)?,
        slot: field(&mut o, "slot", js::usize)?,
    })
}
fn decode_cookie_capability(value: &JsValue) -> Result<TypedCookieCapability, JsValue> {
    let mut o = js::ObjectDecoder::new(value)?;
    Ok(TypedCookieCapability {
        kind: field(&mut o, "kind", js::string)?,
        name: field(&mut o, "name", js::string)?,
        operations: field(&mut o, "operations", strings)?,
        path: defaulted(&mut o, "path", default_cookie_path(), js::string)?,
        same_site: optional(&mut o, "sameSite", js::string)?,
        secure: optional(&mut o, "secure", js::boolean)?,
        expiry_modes: field(&mut o, "expiryModes", strings)?,
    })
}
fn decode_server_action_ref(value: &JsValue) -> Result<TypedServerActionRef, JsValue> {
    let mut o = js::ObjectDecoder::new(value)?;
    Ok(TypedServerActionRef {
        id: field(&mut o, "id", js::string)?,
    })
}
fn decode_route_outlet(value: &JsValue) -> Result<TypedRouteOutlet, JsValue> {
    let mut o = js::ObjectDecoder::new(value)?;
    Ok(TypedRouteOutlet {
        id: field(&mut o, "id", js::string)?,
        node: field(&mut o, "node", js::usize)?,
    })
}
fn decode_host_slot(value: &JsValue) -> Result<TypedHostSlot, JsValue> {
    let mut o = js::ObjectDecoder::new(value)?;
    Ok(TypedHostSlot {
        kind: field(&mut o, "kind", js::string)?,
        query: optional(&mut o, "query", js::usize)?,
        name: optional(&mut o, "name", js::usize)?,
    })
}
fn decode_loop(value: &JsValue) -> Result<TypedLoop, JsValue> {
    let mut o = js::ObjectDecoder::new(value)?;
    Ok(TypedLoop {
        source_expression: field(&mut o, "sourceExpression", js::usize)?,
        key_expression: field(&mut o, "keyExpression", js::usize)?,
        item_slot: field(&mut o, "itemSlot", js::usize)?,
        index_slot: optional(&mut o, "indexSlot", js::usize)?,
        row_template: field(&mut o, "rowTemplate", js::usize)?,
        dependency_slots: defaulted(&mut o, "dependencySlots", Vec::new(), indices)?,
        input: optional(&mut o, "input", js::usize)?,
    })
}
fn decode_dependency_edge(value: &JsValue) -> Result<TypedDependencyEdge, JsValue> {
    let mut o = js::ObjectDecoder::new(value)?;
    Ok(TypedDependencyEdge {
        source: field(&mut o, "source", decode_dependency_endpoint)?,
        target: field(&mut o, "target", decode_dependency_endpoint)?,
    })
}
fn decode_dependency_endpoint(value: &JsValue) -> Result<TypedDependencyEndpoint, JsValue> {
    let mut o = js::ObjectDecoder::new(value)?;
    Ok(TypedDependencyEndpoint {
        kind: field(&mut o, "kind", js::string)?,
        handle: field(&mut o, "handle", js::usize)?,
        r#loop: optional(&mut o, "loop", js::usize)?,
    })
}

fn decode_program(value: &JsValue) -> Result<TypedProgram, JsValue> {
    let mut o = js::ObjectDecoder::new(value)?;
    Ok(TypedProgram {
        instructions: field(&mut o, "instructions", |v| {
            array(v, decode_expression_instruction)
        })?,
    })
}

fn decode_expression_instruction(value: &JsValue) -> Result<TypedExpressionInstruction, JsValue> {
    let mut o = js::ObjectDecoder::new(value)?;
    let op = field(&mut o, "op", js::string)?;
    Ok(match op.as_str() {
        "constant" => TypedExpressionInstruction::Constant {
            constant: field(&mut o, "constant", js::usize)?,
        },
        "loadState" => TypedExpressionInstruction::LoadState {
            state: field(&mut o, "state", js::usize)?,
        },
        "loadRef" => TypedExpressionInstruction::LoadRef {
            reference: field(&mut o, "reference", js::usize)?,
        },
        "loadProp" => TypedExpressionInstruction::LoadProp {
            prop: field(&mut o, "prop", js::usize)?,
        },
        "loadRowRecord" => TypedExpressionInstruction::LoadRowRecord,
        "loadRowField" => TypedExpressionInstruction::LoadRowField {
            field: field(&mut o, "field", js::usize)?,
        },
        "loadEventField" => TypedExpressionInstruction::LoadEventField {
            field: field(&mut o, "field", js::usize)?,
        },
        "loadFrame" => TypedExpressionInstruction::LoadFrame {
            slot: field(&mut o, "slot", js::usize)?,
        },
        "loadHost" => TypedExpressionInstruction::LoadHost {
            host: field(&mut o, "host", js::usize)?,
        },
        "field" => TypedExpressionInstruction::Field {
            field: field(&mut o, "field", js::usize)?,
        },
        "index" => TypedExpressionInstruction::Index,
        "unary" => TypedExpressionInstruction::Unary {
            kind: field(&mut o, "kind", js::string)?,
        },
        "binary" => TypedExpressionInstruction::Binary {
            kind: field(&mut o, "kind", js::string)?,
        },
        "string" => TypedExpressionInstruction::String {
            kind: field(&mut o, "kind", js::string)?,
            count: defaulted(&mut o, "count", one(), js::usize)?,
        },
        "makeArray" => TypedExpressionInstruction::MakeArray {
            count: field(&mut o, "count", js::usize)?,
            spreads: defaulted(&mut o, "spreads", Vec::new(), |v| array(v, js::boolean))?,
        },
        "makeRecord" => TypedExpressionInstruction::MakeRecord {
            fields: field(&mut o, "fields", indices)?,
            spreads: defaulted(&mut o, "spreads", Vec::new(), |v| array(v, js::boolean))?,
        },
        "omitFields" => TypedExpressionInstruction::OmitFields {
            fields: field(&mut o, "fields", indices)?,
        },
        "filter" => {
            let item_slot = o.get_alias("item_slot", "itemSlot")?;
            let index_slot = o.optional_alias("index_slot", "indexSlot")?;
            TypedExpressionInstruction::Filter {
                predicate: field(&mut o, "predicate", js::usize)?,
                item_slot: js::usize(&item_slot)?,
                index_slot: index_slot.as_ref().map(js::usize).transpose()?,
            }
        }
        "map" => {
            let item_slot = o.get_alias("item_slot", "itemSlot")?;
            let index_slot = o.optional_alias("index_slot", "indexSlot")?;
            TypedExpressionInstruction::Map {
                mapper: field(&mut o, "mapper", js::usize)?,
                item_slot: js::usize(&item_slot)?,
                index_slot: index_slot.as_ref().map(js::usize).transpose()?,
            }
        }
        "jump" => TypedExpressionInstruction::Jump {
            target: field(&mut o, "target", js::usize)?,
        },
        "jumpIfFalse" => TypedExpressionInstruction::JumpIfFalse {
            target: field(&mut o, "target", js::usize)?,
        },
        "jumpIfTrue" => TypedExpressionInstruction::JumpIfTrue {
            target: field(&mut o, "target", js::usize)?,
        },
        "return" => TypedExpressionInstruction::Return,
        _ => {
            return Err(JsValue::from_str(
                "unknown variant for enum TypedExpressionInstruction",
            ))
        }
    })
}

fn decode_action(value: &JsValue) -> Result<TypedAction, JsValue> {
    let mut o = js::ObjectDecoder::new(value)?;
    Ok(TypedAction {
        instructions: field(&mut o, "instructions", |v| {
            array(v, decode_action_instruction)
        })?,
        frame_slots: defaulted(&mut o, "frameSlots", 0, js::usize)?,
        parameter_slots: defaulted(&mut o, "parameterSlots", Vec::new(), indices)?,
        loader_result_state: optional(&mut o, "loaderResultState", js::usize)?,
        route_loader: defaulted(&mut o, "routeLoader", false, js::boolean)?,
        route_retry: defaulted(&mut o, "routeRetry", false, js::boolean)?,
        loader_decode_body: defaulted(&mut o, "loaderDecodeBody", false, js::boolean)?,
    })
}

fn decode_action_instruction(value: &JsValue) -> Result<TypedActionInstruction, JsValue> {
    let mut o = js::ObjectDecoder::new(value)?;
    let op = field(&mut o, "op", js::string)?;
    Ok(match op.as_str() {
        "evaluate" => TypedActionInstruction::Evaluate {
            expression: field(&mut o, "expression", js::usize)?,
        },
        "storeState" => TypedActionInstruction::StoreState {
            state: field(&mut o, "state", js::usize)?,
        },
        "mutationStart" => TypedActionInstruction::MutationStart {
            generation: field(&mut o, "generation", js::usize)?,
            pending: field(&mut o, "pending", js::usize)?,
            error: field(&mut o, "error", js::usize)?,
        },
        "mutationPublish" => TypedActionInstruction::MutationPublish {
            generation: field(&mut o, "generation", js::usize)?,
            pending: field(&mut o, "pending", js::usize)?,
            error: field(&mut o, "error", js::usize)?,
            data: field(&mut o, "data", js::usize)?,
            invocation_slot: field(&mut o, "invocationSlot", js::usize)?,
            value_slot: field(&mut o, "valueSlot", js::usize)?,
            success: field(&mut o, "success", js::boolean)?,
        },
        "storeFrame" => TypedActionInstruction::StoreFrame {
            slot: field(&mut o, "slot", js::usize)?,
        },
        "storeRef" => TypedActionInstruction::StoreRef {
            reference: field(&mut o, "reference", js::usize)?,
        },
        "captureActiveElement" => TypedActionInstruction::CaptureActiveElement {
            reference: field(&mut o, "reference", js::usize)?,
        },
        "focusHostRef" => TypedActionInstruction::FocusHostRef {
            reference: field(&mut o, "reference", js::usize)?,
        },
        "focusRef" => TypedActionInstruction::FocusRef {
            reference: field(&mut o, "reference", js::usize)?,
        },
        "preventDefault" => TypedActionInstruction::PreventDefault,
        "routeReload" => TypedActionInstruction::RouteReload,
        "callProp" => TypedActionInstruction::CallProp {
            prop: field(&mut o, "prop", js::usize)?,
            arguments: defaulted(&mut o, "arguments", Vec::new(), indices)?,
        },
        "callPropOptional" => TypedActionInstruction::CallPropOptional {
            prop: field(&mut o, "prop", js::usize)?,
            arguments: defaulted(&mut o, "arguments", Vec::new(), indices)?,
        },
        "collectionMutation" => TypedActionInstruction::CollectionMutation {
            input: field(&mut o, "input", js::usize)?,
            kind: field(&mut o, "kind", js::string)?,
            key: field(&mut o, "key", js::usize)?,
            value: optional(&mut o, "value", js::usize)?,
        },
        "storeHostRef" => TypedActionInstruction::StoreHostRef {
            r#ref: field(&mut o, "ref", js::usize)?,
        },
        "call" => TypedActionInstruction::Call {
            action: field(&mut o, "action", js::usize)?,
            arguments: defaulted(&mut o, "arguments", Vec::new(), indices)?,
            success_pc: optional(&mut o, "successPc", js::usize)?,
            failure_pc: optional(&mut o, "failurePc", js::usize)?,
            result_slot: optional(&mut o, "resultSlot", js::usize)?,
            error_slot: optional(&mut o, "errorSlot", js::usize)?,
        },
        "callFrame" => TypedActionInstruction::CallFrame {
            parameter: field(&mut o, "parameter", js::usize)?,
            arguments: defaulted(&mut o, "arguments", Vec::new(), indices)?,
            success_pc: optional(&mut o, "successPc", js::usize)?,
            failure_pc: optional(&mut o, "failurePc", js::usize)?,
            result_slot: optional(&mut o, "resultSlot", js::usize)?,
            error_slot: optional(&mut o, "errorSlot", js::usize)?,
        },
        "jump" => TypedActionInstruction::Jump {
            target: field(&mut o, "target", js::usize)?,
        },
        "jumpIfFalse" => TypedActionInstruction::JumpIfFalse {
            target: field(&mut o, "target", js::usize)?,
        },
        "capabilityRequest" => TypedActionInstruction::CapabilityRequest {
            request: decode_capability_request(value)?,
            success_pc: field(&mut o, "successPc", js::usize)?,
            failure_pc: field(&mut o, "failurePc", js::usize)?,
            finally_pc: optional(&mut o, "finallyPc", js::usize)?,
            result_slot: field(&mut o, "resultSlot", js::usize)?,
            error_slot: field(&mut o, "errorSlot", js::usize)?,
        },
        "return" => TypedActionInstruction::Return {
            outcome: defaulted(
                &mut o,
                "outcome",
                TypedReturnOutcome::Success,
                decode_return_outcome,
            )?,
            value: optional(&mut o, "value", js::usize)?,
        },
        _ => {
            return Err(JsValue::from_str(
                "unknown variant for enum TypedActionInstruction",
            ))
        }
    })
}

fn decode_return_outcome(value: &JsValue) -> Result<TypedReturnOutcome, JsValue> {
    match js::string(value)?.as_str() {
        "success" => Ok(TypedReturnOutcome::Success),
        "failure" => Ok(TypedReturnOutcome::Failure),
        "redirect" => Ok(TypedReturnOutcome::Redirect),
        "notFound" => Ok(TypedReturnOutcome::NotFound),
        _ => Err(JsValue::from_str(
            "unknown variant for enum TypedReturnOutcome",
        )),
    }
}

fn decode_capability_request(value: &JsValue) -> Result<TypedCapabilityRequest, JsValue> {
    let mut o = js::ObjectDecoder::new(value)?;
    let capability = field(&mut o, "capability", js::string)?;
    let request = o.get("request")?;
    match capability.as_str() {
        "fetch" => Ok(TypedCapabilityRequest::Fetch(decode_fetch_request(
            &request,
        )?)),
        "cookie" => Ok(TypedCapabilityRequest::Cookie(decode_cookie_request(
            &request,
        )?)),
        "serverAction" => Ok(TypedCapabilityRequest::ServerAction(
            decode_server_action_request(&request)?,
        )),
        _ => Err(JsValue::from_str(
            "unknown variant for enum TypedCapabilityRequest",
        )),
    }
}

fn decode_fetch_request(value: &JsValue) -> Result<TypedFetchRequest, JsValue> {
    let mut o = js::ObjectDecoder::new(value)?;
    Ok(TypedFetchRequest {
        url: field(&mut o, "url", js::usize)?,
        method: field(&mut o, "method", js::string)?,
        headers: defaulted(&mut o, "headers", Vec::new(), |v| {
            array(v, decode_fetch_header)
        })?,
        body: optional(&mut o, "body", js::usize)?,
        decode: field(&mut o, "decode", js::string)?,
        require_ok: defaulted(&mut o, "requireOk", true, js::boolean)?,
    })
}
fn decode_fetch_header(value: &JsValue) -> Result<TypedFetchHeader, JsValue> {
    let mut o = js::ObjectDecoder::new(value)?;
    Ok(TypedFetchHeader {
        name: field(&mut o, "name", js::usize)?,
        value: field(&mut o, "value", js::usize)?,
    })
}
fn decode_cookie_request(value: &JsValue) -> Result<TypedCookieRequest, JsValue> {
    let mut o = js::ObjectDecoder::new(value)?;
    Ok(TypedCookieRequest {
        operation: field(&mut o, "operation", js::string)?,
        name: field(&mut o, "name", js::usize)?,
        value: optional(&mut o, "value", js::usize)?,
        path: defaulted(&mut o, "path", default_cookie_path(), js::string)?,
        same_site: optional(&mut o, "sameSite", js::string)?,
        secure: optional(&mut o, "secure", js::boolean)?,
        expiry: defaulted(&mut o, "expiry", default_cookie_expiry(), js::string)?,
        max_age: optional(&mut o, "maxAge", js::i64)?,
    })
}
fn decode_server_action_request(value: &JsValue) -> Result<TypedServerActionRequest, JsValue> {
    let mut o = js::ObjectDecoder::new(value)?;
    Ok(TypedServerActionRequest {
        action: field(&mut o, "action", js::usize)?,
        arguments: defaulted(&mut o, "arguments", Vec::new(), indices)?,
    })
}
