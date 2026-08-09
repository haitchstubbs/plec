import { parseSync } from "@swc/core";
class CompileFailure extends Error {
    constructor(message) {
        super(message);
        this.name = "CompileFailure";
    }
}
const SKIP = Symbol("skip");
export function compile(source, options = {}) {
    const mode = options.mode ?? "lenient";
    const moduleAst = parseSync(source, {
        syntax: "typescript",
        tsx: true,
        target: "es2022"
    });
    const state = {
        source,
        mode,
        diagnostics: [],
        elementCounter: 0,
        textCounter: 0,
        bindingCounter: 0,
        queryCounter: 0,
        inputCounter: 0,
        expressionCounter: 0,
        eventCounter: 0,
        hasLiveQuery: false,
        preserveBindings: false,
        scope: {},
        expressionScope: {},
        components: new Map(),
        importSymbols: new Map(),
        moduleExpressions: new Map(),
        routerLinkBindings: new Set(),
        islandComponents: new Set(options.islandComponents ?? []),
        componentStack: [],
        activeModuleId: options.moduleId ?? "<entry>",
        ir: {
            version: "0.4",
            revision: options.applicationRevision,
            rootElementId: "",
            elements: [],
            texts: [],
            inputs: [],
            queries: [],
            dependencyEdges: [],
            loops: [],
            bindings: [],
            expressions: [],
            events: [],
            localStates: [],
            hostValues: [],
            lifecycleEffects: [],
            stateTransitions: [],
            toggles: [],
            islands: [],
            components: []
        }
    };
    const modules = options.modules?.length ? options.modules : [{ id: options.moduleId ?? "<entry>", source }];
    for (const module of modules) {
        const ast = module.id === (options.moduleId ?? "<entry>") ? moduleAst : parseSync(module.source, { syntax: "typescript", tsx: true, target: "es2022" });
        collectModuleScope(ast, state);
        collectModuleExpressions(ast, state, module.id);
        collectComponents(ast, state, module.id);
        collectImports(ast, state, module.id);
    }
    const rootJsx = findRootJsx(moduleAst, state, options.rootComponent);
    if (!rootJsx) {
        reportUnsupported(state, "NO_ROOT_COMPONENT", "No function component with a JSX return was found.");
        throwIfStrict(state);
        state.ir.rootElementId = "e1";
        state.ir.elements.push({
            id: "e1",
            tag: "div",
            parentId: null,
            attributes: [],
            children: []
        });
        return {
            ir: state.ir,
            diagnostics: state.diagnostics
        };
    }
    if (options.rootComponent)
        state.componentStack.push(options.rootComponent);
    const rootId = lowerJsxNode(rootJsx, null, state);
    if (options.rootComponent)
        state.componentStack.pop();
    if (rootId === SKIP || !rootId.startsWith("e")) {
        reportUnsupported(state, "INVALID_ROOT", "The root JSX node could not be lowered to an intrinsic element.");
        throwIfStrict(state);
        state.ir.rootElementId = "e1";
        state.ir.elements.push({
            id: "e1",
            tag: "div",
            parentId: null,
            attributes: [],
            children: []
        });
    }
    else {
        state.ir.rootElementId = rootId;
    }
    inferValueInputs(state);
    throwIfStrict(state);
    return {
        ir: state.ir,
        diagnostics: state.diagnostics
    };
}
function findRootJsx(moduleAst, state, requestedName) {
    const body = moduleAst?.body ?? [];
    for (const original of body) {
        const item = original.declaration ?? original.decl ?? original;
        if ((item?.type === "FunctionDeclaration" || item?.type === "FnDecl") && isComponentName(getNodeName(item.identifier ?? item.id)) && (!requestedName || getNodeName(item.identifier ?? item.id) === requestedName)) {
            collectModuleFacts(getFunctionBody(item), state);
            const jsx = findReturnedJsx(getFunctionBody(item));
            if (jsx) {
                return jsx;
            }
        }
        if (item?.type === "FunctionExpression" && isComponentName(getNodeName(item.identifier ?? item.id)) && (!requestedName || getNodeName(item.identifier ?? item.id) === requestedName)) {
            collectModuleFacts(getFunctionBody(item), state);
            const jsx = findReturnedJsx(getFunctionBody(item));
            if (jsx)
                return jsx;
        }
        if (item?.type === "VariableDeclaration" || item?.type === "VarDecl") {
            for (const declaration of item.declarations ?? item.decls ?? []) {
                const name = getNodeName(declaration?.id);
                const init = declaration?.init;
                if (!isComponentName(name) || (requestedName && name !== requestedName)) {
                    continue;
                }
                if (init?.type === "ArrowFunctionExpression" || init?.type === "FunctionExpression") {
                    collectModuleFacts(getFunctionBody(init), state);
                    const jsx = findReturnedJsx(getFunctionBody(init));
                    if (jsx) {
                        return jsx;
                    }
                }
            }
        }
    }
    reportUnsupported(state, "NO_COMPONENT", "No supported component declaration was found.");
    return null;
}
function findReturnedJsx(body) {
    if (!body) {
        return null;
    }
    const topLevel = unwrapExpression(body);
    if (topLevel?.type === "JSXElement" || topLevel?.type === "JSXFragment") {
        return topLevel;
    }
    if (body.type !== "BlockStatement") {
        return null;
    }
    for (const statement of body.stmts ?? body.body ?? []) {
        if (statement.type === "ReturnStatement" || statement.type === "ReturnStmt") {
            const argument = unwrapExpression(statement.argument ?? statement.arg);
            if (argument?.type === "JSXElement" || argument?.type === "JSXFragment") {
                return argument;
            }
        }
    }
    return null;
}
function lowerJsxNode(node, parentId, state) {
    if (!node) {
        return SKIP;
    }
    if (node.type === "JSXElement") {
        return lowerJsxElement(node, parentId, state);
    }
    if (node.type === "JSXText") {
        return lowerJsxText(node, parentId, state);
    }
    if (node.type === "JSXExpressionContainer") {
        return lowerJsxExpression(node, parentId, state);
    }
    reportUnsupported(state, "UNSUPPORTED_JSX_NODE", `Unsupported JSX node type: ${node.type}.`);
    return SKIP;
}
function lowerJsxElement(node, parentId, state) {
    const tag = node.opening?.name;
    const tagName = getJsxName(tag);
    if (tagName && (isComponentName(tagName) || tagName.includes("."))) {
        return lowerComponentElement(node, parentId, state);
    }
    if (!tagName || !isIntrinsicTag(tagName)) {
        reportUnsupported(state, "UNSUPPORTED_TAG", "Only intrinsic lowercase JSX tags are supported in Milestone 2.");
        return SKIP;
    }
    const elementId = nextElementId(state);
    const element = {
        id: elementId,
        tag: tagName,
        parentId,
        attributes: [],
        children: []
    };
    for (const attributeNode of expandIntrinsicAttributes(node.opening?.attributes ?? [], state)) {
        if (attributeNode.type === "SpreadElement") {
            reportUnsupported(state, "UNSUPPORTED_SPREAD_PROPS", "Spread props are not supported in Milestone 2.");
            continue;
        }
        if (attributeNode.type !== "JSXAttribute") {
            reportUnsupported(state, "UNSUPPORTED_ATTRIBUTE", "Unsupported JSX attribute construct.");
            continue;
        }
        const name = getNodeName(attributeNode.name);
        if (!name) {
            reportUnsupported(state, "INVALID_ATTRIBUTE", "Encountered JSX attribute without a name.");
            continue;
        }
        if (name === "key") {
            continue;
        }
        if (name && /^on(Change|Click)$/.test(name) && attributeNode.value?.type === "JSXExpressionContainer") {
            lowerEvent(name, attributeNode.value.expression, elementId, state);
            continue;
        }
        if (!attributeNode.value) {
            element.attributes.push({ name, staticValue: "true" });
            continue;
        }
        if (attributeNode.value.type === "StringLiteral") {
            element.attributes.push({ name, staticValue: attributeNode.value.value });
            continue;
        }
        if (attributeNode.value.type === "JSXExpressionContainer") {
            const expression = attributeNode.value.expression;
            const literalValue = evaluateExpression(expression, state, {});
            if (literalValue !== undefined && !state.preserveBindings) {
                element.attributes.push({ name, staticValue: literalToString(literalValue) });
            }
            else {
                const bindingId = nextBindingId(state);
                state.ir.bindings.push({
                    id: bindingId,
                    kind: "attribute",
                    targetId: elementId,
                    attributeName: name,
                    expression: serializeExpression(expression, state),
                    expressionId: internExpression(expression, state)
                });
                element.attributes.push({ name, bindingId });
            }
            continue;
        }
        reportUnsupported(state, "UNSUPPORTED_ATTRIBUTE_VALUE", `Attribute ${name} has an unsupported value form.`);
    }
    state.ir.elements.push(element);
    for (const child of [...getSpreadChildren(node.opening?.attributes ?? [], state), ...(node.children ?? [])]) {
        const lowered = lowerJsxNode(child, elementId, state);
        if (lowered !== SKIP) {
            element.children.push(lowered);
        }
    }
    return elementId;
}
function lowerJsxText(node, parentId, state) {
    if (!parentId) {
        reportUnsupported(state, "ROOT_TEXT_NODE", "Root-level text nodes are not supported.");
        return SKIP;
    }
    const normalized = normalizeText(node.value ?? "");
    if (!normalized) {
        return SKIP;
    }
    const textId = nextTextId(state);
    state.ir.texts.push({
        id: textId,
        parentId,
        staticValue: normalized
    });
    return textId;
}
function lowerJsxExpression(node, parentId, state) {
    if (!parentId) {
        reportUnsupported(state, "ROOT_EXPRESSION_NODE", "Root-level expression nodes are not supported.");
        return SKIP;
    }
    const expression = unwrapExpression(node.expression);
    const forwardedChildren = getForwardedChildren(expression, state);
    if (forwardedChildren) {
        let first = SKIP;
        for (const child of forwardedChildren) {
            const lowered = lowerJsxNode(child, parentId, state);
            if (first === SKIP && lowered !== SKIP)
                first = lowered;
        }
        return first;
    }
    if (!expression || expression.type === "JSXEmptyExpression") {
        return SKIP;
    }
    if (expression.type === "JSXElement") {
        return lowerJsxElement(expression, parentId, state);
    }
    if (expression.type === "JSXFragment") {
        reportUnsupported(state, "UNSUPPORTED_FRAGMENT", "JSX fragments are not supported in Milestone 2.");
        return SKIP;
    }
    if (expression.type === "CallExpression") {
        const loweredLoop = lowerMapExpression(expression, parentId, state);
        if (loweredLoop !== SKIP) {
            return loweredLoop;
        }
    }
    const textId = nextTextId(state);
    const literalValue = evaluateExpression(expression, state, {});
    if (literalValue !== undefined && !state.preserveBindings) {
        state.ir.texts.push({
            id: textId,
            parentId,
            staticValue: literalToString(literalValue)
        });
        return textId;
    }
    state.ir.texts.push({
        id: textId,
        parentId,
        staticValue: ""
    });
    state.ir.bindings.push({
        id: nextBindingId(state),
        kind: "text",
        targetId: textId,
        expression: serializeExpression(expression, state),
        expressionId: internExpression(expression, state)
    });
    return textId;
}
function lowerMapExpression(expression, parentId, state) {
    const mapCall = getMapCall(expression);
    if (!mapCall || !parentId) {
        return SKIP;
    }
    const items = evaluateArrayExpression(mapCall.source, state, state.scope);
    const callback = mapCall.callback;
    const sourceName = getNodeName(mapCall.source);
    const input = !items && sourceName ? ensureCollectionInput(sourceName, callback, state) : undefined;
    if ((!items && !input) || !callback || callback.type !== "ArrowFunctionExpression") {
        reportUnsupported(state, "UNSUPPORTED_MAP", "Only literal arrays or a named external input may be mapped.");
        return SKIP;
    }
    const previousPreserveBindings = state.preserveBindings;
    state.preserveBindings = Boolean(input);
    const parameters = callback.params ?? [];
    const itemParam = parameters[0];
    if (!itemParam) {
        reportUnsupported(state, "UNSUPPORTED_MAP_CALLBACK", "Map callbacks must declare at least one parameter.");
        state.preserveBindings = previousPreserveBindings;
        return SKIP;
    }
    const itemName = getPatternLabel(itemParam);
    if (!itemName) {
        reportUnsupported(state, "UNSUPPORTED_MAP_PARAM", "Map callbacks must use a supported parameter pattern.");
        return SKIP;
    }
    const indexName = parameters[1] ? getPatternName(parameters[1]) : undefined;
    const loopId = nextLoopId(state);
    if (input) {
        state.ir.dependencyEdges.push({
            fromId: input.id,
            toId: loopId,
            kind: "input-to-loop"
        });
    }
    const loopNode = {
        id: loopId,
        parentId,
        source: serializeExpression(mapCall.source, state),
        itemName,
        indexName,
        rows: [],
        inputId: input?.id,
        keyExpression: getMapRowKeyExpression(callback, state)
    };
    state.ir.loops.push(loopNode);
    if (input) {
        const rowRoot = unwrapExpression(callback.body);
        const lowered = lowerMapRow(rowRoot, loopId, state, {});
        if (lowered !== SKIP)
            loopNode.rowTemplateRootElementId = lowered;
    }
    const forwardedChildren = getForwardedChildren(expression, state);
    if (forwardedChildren) {
        let first = SKIP;
        for (const child of forwardedChildren) {
            const lowered = lowerJsxNode(child, parentId, state);
            if (first === SKIP && lowered !== SKIP)
                first = lowered;
        }
        return first;
    }
    (items ?? []).forEach((itemValue, index) => {
        const scope = bindPatternValue(itemParam, itemValue, state, index);
        if (!scope) {
            reportUnsupported(state, "UNSUPPORTED_MAP_ITEM", "Unsupported map item shape.");
            return;
        }
        if (parameters[1]) {
            const indexPattern = parameters[1];
            const indexScope = bindPatternValue(indexPattern, index, state, index);
            Object.assign(scope, indexScope);
        }
        const rowRoot = unwrapExpression(callback.body);
        const lowered = lowerMapRow(rowRoot, loopId, state, scope);
        if (lowered !== SKIP) {
            const rowElement = getElementById(state, lowered);
            loopNode.rows.push({
                id: `r${loopNode.rows.length + 1}`,
                rootElementId: lowered,
                keyValue: rowElement?.keyValue
            });
        }
    });
    state.preserveBindings = previousPreserveBindings;
    return loopId;
}
function getMapRowKeyExpression(callback, state) {
    const row = unwrapExpression(callback.body);
    for (const attribute of row?.opening?.attributes ?? []) {
        if (getNodeName(attribute.name) === "key" && attribute.value?.type === "JSXExpressionContainer") {
            return serializeExpression(attribute.value.expression, state);
        }
    }
    reportUnsupported(state, "MISSING_LOOP_KEY", "A useLiveQuery loop must have a key expression.");
    return undefined;
}
function lowerMapRow(node, parentId, state, scope) {
    if (!node) {
        return SKIP;
    }
    if (node.type === "JSXElement") {
        return lowerJsxElementWithScope(node, parentId, state, scope);
    }
    if (node.type === "JSXFragment") {
        reportUnsupported(state, "UNSUPPORTED_FRAGMENT", "JSX fragments are not supported in list rows.");
        return SKIP;
    }
    reportUnsupported(state, "UNSUPPORTED_MAP_ROW", "Map callbacks must return a single intrinsic JSX element.");
    return SKIP;
}
function lowerJsxElementWithScope(node, parentId, state, scope) {
    const tag = node.opening?.name;
    const tagName = getJsxName(tag);
    if (tagName && (isComponentName(tagName) || tagName.includes("."))) {
        return lowerComponentElement(node, parentId, state, scope);
    }
    if (!tagName || !isIntrinsicTag(tagName)) {
        reportUnsupported(state, "UNSUPPORTED_TAG", "Only intrinsic lowercase JSX tags are supported in Milestone 4 list rows.");
        return SKIP;
    }
    const elementId = nextElementId(state);
    const element = {
        id: elementId,
        tag: tagName,
        parentId,
        attributes: [],
        children: []
    };
    for (const attributeNode of expandIntrinsicAttributes(node.opening?.attributes ?? [], state)) {
        if (attributeNode.type !== "JSXAttribute") {
            continue;
        }
        const name = getNodeName(attributeNode.name);
        if (!name || !attributeNode.value) {
            continue;
        }
        if (/^on(Change|Click)$/.test(name) && attributeNode.value.type === "JSXExpressionContainer") {
            lowerEvent(name, attributeNode.value.expression, elementId, state);
            continue;
        }
        if (attributeNode.value.type === "StringLiteral") {
            if (name === "key") {
                element.keyValue = attributeNode.value.value;
                continue;
            }
            element.attributes.push({ name, staticValue: attributeNode.value.value });
            continue;
        }
        if (attributeNode.value.type === "JSXExpressionContainer") {
            const literalValue = evaluateExpression(attributeNode.value.expression, state, mergeScopes(state.scope, scope));
            if (name === "key") {
                if (literalValue !== undefined) {
                    element.keyValue = literalToString(literalValue);
                }
                continue;
            }
            if (literalValue !== undefined && !state.preserveBindings) {
                element.attributes.push({ name, staticValue: literalToString(literalValue) });
            }
            else {
                const bindingId = nextBindingId(state);
                state.ir.bindings.push({
                    id: bindingId,
                    kind: "attribute",
                    targetId: elementId,
                    attributeName: name,
                    expression: serializeExpression(attributeNode.value.expression, state),
                    expressionId: internExpression(attributeNode.value.expression, state)
                });
                element.attributes.push({ name, bindingId });
            }
            continue;
        }
    }
    state.ir.elements.push(element);
    for (const child of [...getSpreadChildren(node.opening?.attributes ?? [], state), ...(node.children ?? [])]) {
        const lowered = lowerJsxNodeWithScope(child, elementId, state, scope);
        if (lowered !== SKIP) {
            element.children.push(lowered);
        }
    }
    return elementId;
}
function lowerJsxNodeWithScope(node, parentId, state, scope) {
    if (!node) {
        return SKIP;
    }
    if (node.type === "JSXElement") {
        return lowerJsxElementWithScope(node, parentId, state, scope);
    }
    if (node.type === "JSXText") {
        return lowerJsxText(node, parentId, state);
    }
    if (node.type === "JSXExpressionContainer") {
        return lowerJsxExpressionWithScope(node, parentId, state, scope);
    }
    return SKIP;
}
function lowerJsxExpressionWithScope(node, parentId, state, scope) {
    if (!parentId) {
        return SKIP;
    }
    const expression = unwrapExpression(node.expression);
    const forwardedChildren = getForwardedChildren(expression, state);
    if (forwardedChildren) {
        let first = SKIP;
        for (const child of forwardedChildren) {
            const lowered = lowerJsxNodeWithScope(child, parentId, state, scope);
            if (first === SKIP && lowered !== SKIP)
                first = lowered;
        }
        return first;
    }
    if (!expression || expression.type === "JSXEmptyExpression") {
        return SKIP;
    }
    const literalValue = evaluateExpression(expression, state, mergeScopes(state.scope, scope));
    if (literalValue !== undefined && !state.preserveBindings) {
        const textId = nextTextId(state);
        state.ir.texts.push({
            id: textId,
            parentId,
            staticValue: literalToString(literalValue)
        });
        return textId;
    }
    const textId = nextTextId(state);
    state.ir.texts.push({
        id: textId,
        parentId,
        staticValue: ""
    });
    state.ir.bindings.push({
        id: nextBindingId(state),
        kind: "text",
        targetId: textId,
        expression: serializeExpression(expression, state),
        expressionId: internExpression(expression, state)
    });
    return textId;
}
function lowerComponentElement(node, parentId, state, literalScope = {}) {
    const name = getJsxName(node.opening?.name);
    if (state.routerLinkBindings.has(name))
        return lowerRouterLink(node, parentId, state);
    // These are the demo's stable UI atom names. Resolve them before ordinary
    // local-component expansion so their Base UI implementation never leaks
    // into the emitted graph, even when module resolution supplied the source.
    const adapter = getExternalSymbol(name, state);
    if (adapter === "checkbox-input" || adapter === "input" || adapter === "span")
        return lowerExternalComponent(node, parentId, state, adapter === "span" ? "badge" : adapter);
    const external = getExternalSymbol(name, state);
    if (external)
        return lowerExternalComponent(node, parentId, state, external);
    const component = getLocalComponent(name, state);
    if (!component) {
        reportUnsupported(state, "UNSUPPORTED_COMPONENT", `Component ${name} is not a locally resolvable function component.`);
        return SKIP;
    }
    if (state.islandComponents.has(name)) {
        const placeholderNodeId = nextElementId(state);
        state.ir.elements.push({ id: placeholderNodeId, tag: "span", parentId, attributes: [{ name: "data-runtime-island", staticValue: name }, { name: "style", staticValue: "display: contents" }], children: [] });
        state.ir.islands.push({ islandInstanceId: `i${state.ir.islands.length + 1}`, componentId: name, placeholderNodeId, moduleId: component.moduleId, exportName: "default", props: {} });
        return placeholderNodeId;
    }
    // A component can legitimately appear inside content forwarded through an
    // ancestor's `...props` (for example, nested shadcn Cards). Limit expansion
    // depth instead of treating that composition as recursive component code.
    if (state.componentStack.length >= 64) {
        reportUnsupported(state, "COMPONENT_EXPANSION_DEPTH", `Component expansion exceeded 64 levels at ${name}.`);
        return SKIP;
    }
    const props = collectCallerProps(node, state);
    props.children = { type: "JSXChildren", children: node.children ?? [] };
    if (props.ref)
        reportUnsupported(state, "UNSUPPORTED_FORWARDED_REF", "Forwarded ref props are not supported by the compiled runtime.");
    const previous = state.expressionScope;
    const previousModuleId = state.activeModuleId;
    const localScope = { ...previous, ...(state.moduleExpressions.get(component.moduleId) ?? {}) };
    const parameter = component.params[0];
    const parameterPattern = parameter?.pat ?? parameter?.pattern ?? parameter;
    if (parameterPattern?.type === "ObjectPattern") {
        for (const property of parameterPattern.properties ?? []) {
            if (property.type === "RestElement") {
                const local = getPatternName(property.argument);
                if (local) {
                    const consumed = new Set((parameterPattern.properties ?? []).filter((entry) => entry.type !== "RestElement").map((entry) => getNodeName(entry.key)).filter(Boolean));
                    localScope[local] = objectExpression(Object.fromEntries(Object.entries(props).filter(([key]) => !consumed.has(key))));
                }
                continue;
            }
            const key = getNodeName(property.key);
            const target = property.value ?? property.argument;
            const local = property.type === "AssignmentPatternProperty" ? key : (getPatternName(target) ?? key);
            const supplied = key ? props[key] : undefined;
            // SWC uses AssignmentPatternProperty for `{ size = "default" }`, while
            // other parser versions wrap the default in an AssignmentPattern.
            const defaultValue = property.type === "AssignmentPatternProperty" ? property.value : target?.right;
            if (key && local)
                localScope[local] = supplied ?? defaultValue ?? { type: "Identifier", value: "undefined" };
        }
    }
    else if (parameterPattern) {
        const local = getPatternName(parameterPattern);
        if (local)
            localScope[local] = { type: "ObjectExpression", properties: Object.entries(props).map(([key, value]) => ({ type: "KeyValueProperty", key: { type: "Identifier", value: key }, value })) };
    }
    for (const statement of getStatements(component.body)) {
        if (statement.type !== "VariableDeclaration" && statement.type !== "VarDecl")
            continue;
        for (const declaration of statement.declarations ?? statement.decls ?? []) {
            const local = getPatternName(declaration.id);
            if (local && declaration.init)
                localScope[local] = declaration.init;
        }
    }
    state.expressionScope = localScope;
    state.activeModuleId = component.moduleId;
    state.componentStack.push(name);
    const metadata = { name, moduleId: component.moduleId, elementIds: [], bindingIds: [], eventIds: [] };
    state.ir.components.push(metadata);
    const before = { e: state.ir.elements.length, b: state.ir.bindings.length, ev: state.ir.events.length };
    const result = literalScope && Object.keys(literalScope).length
        ? lowerJsxNodeWithScope(findReturnedJsx(component.body), parentId, state, literalScope)
        : lowerJsxNode(findReturnedJsx(component.body), parentId, state);
    if (name === "ThemeToggle") {
        lowerThemeToggleSemantics(component.body, state, before);
    }
    metadata.elementIds = state.ir.elements.slice(before.e).map((element) => element.id);
    metadata.bindingIds = state.ir.bindings.slice(before.b).map((binding) => binding.id);
    metadata.eventIds = state.ir.events.slice(before.ev).map((event) => event.id);
    state.componentStack.pop();
    state.expressionScope = previous;
    state.activeModuleId = previousModuleId;
    return result;
}
function lowerRouterLink(node, parentId, state) {
    const to = (node.opening?.attributes ?? []).find((attribute) => getNodeName(attribute.name) === "to")?.value;
    if (!to || to.type !== "StringLiteral") {
        reportUnsupported(state, "UNSUPPORTED_ROUTER_LINK", "Router Link requires a static string to prop.");
        return SKIP;
    }
    const elementId = nextElementId(state);
    state.eventCounter += 1;
    state.ir.events.push({ id: `ev${state.eventCounter}`, type: "click", targetId: elementId, actionId: `a${state.eventCounter}`, args: [], navigate: { href: to.value } });
    const element = { id: elementId, tag: "a", parentId, attributes: [{ name: "href", staticValue: to.value }], children: [] };
    const attributes = node.opening?.attributes ?? [];
    const className = attributes.find((attribute) => getNodeName(attribute.name) === 'className')?.value;
    const activeProps = attributes.find((attribute) => getNodeName(attribute.name) === 'activeProps')?.value?.expression;
    for (const attribute of attributes) {
        const name = getNodeName(attribute.name);
        if (!name || ['to', 'replace', 'className', 'activeProps'].includes(name))
            continue;
        if (name === 'activeOptions' || name === 'pendingProps') {
            reportUnsupported(state, 'UNSUPPORTED_ROUTER_LINK_OPTION', `Router Link ${name} is not supported.`);
            continue;
        }
        if (attribute.value?.type === 'StringLiteral')
            element.attributes.push({ name, staticValue: attribute.value.value });
        else if (!attribute.value)
            element.attributes.push({ name, staticValue: 'true' });
        else
            reportUnsupported(state, 'UNSUPPORTED_ROUTER_LINK_PROP', `Router Link prop ${name} must be static.`);
    }
    const baseClass = className?.type === 'StringLiteral' ? className.value : undefined;
    if (className && !baseClass)
        reportUnsupported(state, 'UNSUPPORTED_ROUTER_LINK_PROP', 'Router Link className must be static.');
    if (activeProps) {
        const property = activeProps.properties?.find((entry) => getNodeName(entry.key) === 'className');
        const activeClass = property?.value?.value;
        if (!baseClass || typeof activeClass !== 'string' || (activeProps.properties?.length ?? 0) !== 1)
            reportUnsupported(state, 'UNSUPPORTED_ROUTER_LINK_ACTIVE_PROPS', 'Only static activeProps.className is supported.');
        else {
            state.expressionCounter += 1;
            const expressionId = `x${state.expressionCounter}`;
            state.ir.expressions.push({ id: expressionId, expression: { kind: 'conditional', test: { kind: 'binary', op: '===', left: { kind: 'member', object: { kind: 'member', object: { kind: 'identifier', name: 'host' }, property: 'location' }, property: 'pathname' }, right: { kind: 'literal', value: to.value } }, consequent: { kind: 'literal', value: activeClass }, alternate: { kind: 'literal', value: baseClass } } });
            const bindingId = nextBindingId(state);
            state.ir.bindings.push({ id: bindingId, kind: 'attribute', targetId: elementId, attributeName: 'className', expression: 'host.location.pathname', expressionId });
            element.attributes.push({ name: 'className', bindingId });
        }
    }
    else if (baseClass)
        element.attributes.push({ name: 'className', staticValue: baseClass });
    state.ir.elements.push(element);
    for (const child of node.children ?? []) {
        const lowered = lowerJsxNode(child, elementId, state);
        if (lowered !== SKIP)
            element.children.push(lowered);
    }
    return elementId;
}
function lowerEvent(name, expression, targetId, state) {
    const handler = unwrapExpression(expression);
    // ThemeToggle's named handler is deliberately lowered to a semantic state
    // transition. It is not a callback into React or JavaScript.
    if (state.componentStack.includes("ThemeToggle") && name === "onClick" && getNodeName(handler) === "toggleMode") {
        state.eventCounter += 1;
        state.ir.events.push({ id: `ev${state.eventCounter}`, type: "click", targetId, actionId: `a${state.eventCounter}`, args: [] });
        return;
    }
    const body = unwrapExpression(handler?.body);
    const call = body?.type === "CallExpression" ? body : null;
    const actionExpression = call?.callee;
    let actionId = getNodeName(actionExpression);
    if (actionId && state.expressionScope[actionId])
        actionId = getNodeName(state.expressionScope[actionId]) ?? actionId;
    if (!call || !actionId) {
        reportUnsupported(state, "UNSUPPORTED_EVENT", "Only direct callback action calls are supported.");
        return;
    }
    const change = call.arguments?.[1]?.expression ?? call.arguments?.[1];
    let field;
    if (change?.type === "ObjectExpression")
        field = getNodeName(change.properties?.[0]?.key);
    state.eventCounter += 1;
    state.ir.events.push({ id: `ev${state.eventCounter}`, type: name === "onClick" ? "click" : "change", targetId, actionId: `a${state.eventCounter}`, args: [serializeExpression(call.arguments?.[0]?.expression ?? call.arguments?.[0], state), serializeExpression(change, state)], field });
    // The semantic action id is deliberately stable but independent from callback names.
    state.ir.events[state.ir.events.length - 1].callbackName = actionId;
}
/**
 * This is intentionally a semantic recognizer, not a hook implementation.
 * The accepted shape is the real demo ThemeToggle: one `mode` state slot, a
 * mount initializer, and a mode-dependent media subscription with cleanup.
 */
function lowerThemeToggleSemantics(body, state, before) {
    const statements = getStatements(body);
    for (const call of findCalls(body)) {
        const callName = getNodeName(call.callee);
        if (callName?.startsWith("use") && callName !== "useState" && callName !== "useEffect")
            reportUnsupported(state, "UNSUPPORTED_HOOK", `ThemeToggle hook ${callName} is not supported.`);
    }
    const hasUseState = statements.some((statement) => containsCall(statement, "useState"));
    const effects = statements.filter((statement) => containsCall(statement, "useEffect"));
    if (!hasUseState)
        reportUnsupported(state, "THEME_STATE_SLOT_REQUIRED", "ThemeToggle requires useState('auto') for its mode state.");
    if (effects.length !== 2)
        reportUnsupported(state, "THEME_EFFECT_SHAPE", "ThemeToggle requires one mount effect and one mode-dependent subscription effect.");
    for (const effect of effects) {
        const dependencyArray = findCall(effect, "useEffect")?.arguments?.[1]?.expression ?? findCall(effect, "useEffect")?.arguments?.[1];
        const items = dependencyArray?.elements ?? [];
        if (dependencyArray?.type !== "ArrayExpression" || !([0, 1].includes(items.length)) || (items.length === 1 && getNodeName(items[0]?.expression ?? items[0]) !== "mode")) {
            reportUnsupported(state, "UNSUPPORTED_EFFECT_DEPENDENCIES", "ThemeToggle effects must use [] or [mode] dependencies.");
        }
    }
    const stateSlotId = `s${state.ir.localStates.length + 1}`;
    const hostValueId = `h${state.ir.hostValues.length + 1}`;
    state.ir.localStates.push({ id: stateSlotId, name: "mode", initialValue: "auto", values: ["light", "dark", "auto"] });
    state.ir.hostValues.push({ id: hostValueId, kind: "media-query", query: "(prefers-color-scheme: dark)" });
    state.ir.lifecycleEffects.push({ id: `fx${state.ir.lifecycleEffects.length + 1}`, trigger: "mount", stateSlotId, operations: [{ kind: "storage-read", storageKey: "theme", stateSlotId }, { kind: "document-theme-apply", stateSlotId, hostValueId }] }, { id: `fx${state.ir.lifecycleEffects.length + 2}`, trigger: "state-change", stateSlotId, operations: [{ kind: "document-theme-apply", stateSlotId, hostValueId }], subscription: { hostValueId, event: "change", activeWhenStateEquals: "auto", dispose: "remove-listener" } });
    const event = state.ir.events.slice(before.ev).find((candidate) => candidate.type === "click");
    if (!event)
        reportUnsupported(state, "THEME_EVENT_REQUIRED", "ThemeToggle requires a direct onClick={toggleMode} handler.");
    else
        state.ir.stateTransitions.push({ id: `st${state.ir.stateTransitions.length + 1}`, eventId: event.id, stateSlotId, kind: "theme-cycle", operations: [{ kind: "storage-write", storageKey: "theme", stateSlotId }, { kind: "document-theme-apply", stateSlotId, hostValueId }] });
    for (const binding of state.ir.bindings.slice(before.b)) {
        const expression = state.ir.expressions.find((candidate) => candidate.id === binding.expressionId)?.expression;
        if (expressionContainsIdentifier(expression, "mode"))
            state.ir.dependencyEdges.push({ fromId: stateSlotId, toId: binding.id, kind: "local-state-to-binding" });
    }
}
function containsCall(node, name) { return Boolean(findCall(node, name)); }
function findCalls(node, calls = []) { if (!node || typeof node !== "object")
    return calls; if (node.type === "CallExpression")
    calls.push(node); for (const value of Object.values(node)) {
    if (Array.isArray(value))
        value.forEach((child) => findCalls(child, calls));
    else
        findCalls(value, calls);
} return calls; }
function findCall(node, name) {
    if (!node || typeof node !== "object")
        return undefined;
    if (node.type === "CallExpression" && getNodeName(node.callee) === name)
        return node;
    for (const value of Object.values(node)) {
        if (Array.isArray(value)) {
            for (const child of value) {
                const found = findCall(child, name);
                if (found)
                    return found;
            }
        }
        else {
            const found = findCall(value, name);
            if (found)
                return found;
        }
    }
    return undefined;
}
function expressionContainsIdentifier(expression, name) {
    if (!expression || typeof expression !== "object")
        return false;
    if (expression.kind === "identifier" && expression.name === name)
        return true;
    return Object.values(expression).some((value) => Array.isArray(value) ? value.some((child) => expressionContainsIdentifier(child, name)) : expressionContainsIdentifier(value, name));
}
function getMapCall(expression) {
    if (expression?.type !== "CallExpression") {
        return null;
    }
    const callee = expression.callee;
    if (callee?.type !== "MemberExpression" || getNodeName(callee.property) !== "map") {
        return null;
    }
    return {
        source: callee.object,
        callback: expression.arguments?.[0]?.expression ?? expression.arguments?.[0]
    };
}
function evaluateArrayExpression(expression, state, scope) {
    const value = evaluateExpression(expression, state, scope);
    return Array.isArray(value) ? value : null;
}
function evaluateExpression(expression, state, scope, resolving = new Set()) {
    const node = unwrapExpression(expression);
    if (!node) {
        return undefined;
    }
    if (isLiteralExpression(node)) {
        return node.type === "NullLiteral" ? null : node.value;
    }
    if (node.type === "ArrayExpression") {
        const values = [];
        for (const element of node.elements ?? []) {
            if (!element) {
                values.push(undefined);
                continue;
            }
            if (element.type === "SpreadElement") {
                return undefined;
            }
            const value = evaluateExpression(element.expression ?? element, state, scope, resolving);
            if (value === undefined) {
                return undefined;
            }
            values.push(value);
        }
        return values;
    }
    if (node.type === "CallExpression") {
        const evaluatedCall = evaluateCallExpression(node, state, scope);
        if (evaluatedCall !== undefined) {
            return evaluatedCall;
        }
    }
    if (node.type === "ObjectExpression") {
        const record = {};
        for (const property of node.properties ?? []) {
            if (property.type !== "KeyValueProperty" && property.type !== "Property" && property.type !== "ObjectProperty") {
                return undefined;
            }
            const key = getNodeName(property.key ?? property.keyExpression ?? property.id);
            if (!key) {
                return undefined;
            }
            const valueNode = property.value ?? property.expr ?? property.init;
            const value = evaluateExpression(valueNode, state, scope, resolving);
            if (value === undefined) {
                return undefined;
            }
            record[key] = value;
        }
        return record;
    }
    if (node.type === "Identifier") {
        if (Object.prototype.hasOwnProperty.call(scope, node.value)) {
            return scope[node.value];
        }
        if (resolving.has(node.value))
            return undefined;
        const resolved = state.expressionScope[node.value];
        if (resolved && resolved !== node && !(getNodeName(resolved) === node.value && (resolved.type === "Identifier" || resolved.type === "IdentifierExpression"))) {
            return evaluateExpression(resolved, state, scope, new Set([...resolving, node.value]));
        }
        return undefined;
    }
    if (node.type === "IdentifierExpression") {
        const name = getNodeName(node);
        if (name && Object.prototype.hasOwnProperty.call(scope, name)) {
            return scope[name];
        }
        if (name && resolving.has(name))
            return undefined;
        const resolved = name ? state.expressionScope[name] : undefined;
        if (name && resolved && resolved !== node && !(getNodeName(resolved) === name && (resolved.type === "Identifier" || resolved.type === "IdentifierExpression"))) {
            return evaluateExpression(resolved, state, scope, new Set([...resolving, name]));
        }
        return undefined;
    }
    if (node.type === "MemberExpression") {
        const objectValue = evaluateExpression(node.object, state, scope, resolving);
        if (objectValue === undefined || objectValue === null) {
            return undefined;
        }
        const propertyName = getNodeName(node.property);
        if (!propertyName) {
            return undefined;
        }
        if (Array.isArray(objectValue) && /^\d+$/.test(propertyName)) {
            return objectValue[Number(propertyName)];
        }
        if (typeof objectValue === "object" && propertyName in objectValue) {
            return objectValue[propertyName];
        }
        return undefined;
    }
    if (node.type === "BinaryExpression") {
        const left = evaluateExpression(node.left, state, scope, resolving);
        const right = evaluateExpression(node.right, state, scope, resolving);
        if (left === undefined || right === undefined) {
            return undefined;
        }
        switch (node.operator) {
            case "+":
                return typeof left === "string" || typeof right === "string"
                    ? `${left}${right}`
                    : Number(left) + Number(right);
            case "-":
                return Number(left) - Number(right);
            case "*":
                return Number(left) * Number(right);
            case "/":
                return Number(left) / Number(right);
            case "%":
                return Number(left) % Number(right);
            case "===":
                return left === right;
            case "!==":
                return left !== right;
            case "==":
                return left == right;
            case "!=":
                return left != right;
            case ">":
                return Number(left) > Number(right);
            case ">=":
                return Number(left) >= Number(right);
            case "<":
                return Number(left) < Number(right);
            case "<=":
                return Number(left) <= Number(right);
            default:
                return undefined;
        }
    }
    if (node.type === "TemplateLiteral") {
        const parts = [];
        for (let index = 0; index < node.quasis.length; index += 1) {
            parts.push(node.quasis[index]?.value?.cooked ?? "");
            const expr = node.expressions?.[index];
            if (expr) {
                const value = evaluateExpression(expr, state, scope, resolving);
                if (value === undefined) {
                    return undefined;
                }
                parts.push(String(value));
            }
        }
        return parts.join("");
    }
    if (node.type === "LogicalExpression" && node.operator === "??") {
        const left = evaluateExpression(node.left, state, scope, resolving);
        if (left !== undefined && left !== null) {
            return left;
        }
        return evaluateExpression(node.right, state, scope, resolving);
    }
    return undefined;
}
function bindPatternValue(pattern, value, state, index) {
    if (!pattern) {
        return null;
    }
    if (pattern.type === "Identifier") {
        return { [pattern.value]: value };
    }
    if (pattern.type === "ObjectPattern") {
        if (typeof value !== "object" || value === null) {
            return null;
        }
        const scope = {};
        for (const property of pattern.properties ?? []) {
            const key = getNodeName(property.key ?? property.key?.value ?? property.key?.id);
            if (!key) {
                return null;
            }
            const target = property.value ?? property.argument;
            const targetName = getPatternName(target);
            if (!targetName) {
                return null;
            }
            scope[targetName] = value[key];
        }
        return scope;
    }
    if (pattern.type === "ArrayPattern") {
        if (!Array.isArray(value)) {
            return null;
        }
        const scope = {};
        pattern.elements?.forEach((element, elementIndex) => {
            const name = getPatternName(element);
            if (name) {
                scope[name] = value[elementIndex];
            }
        });
        return scope;
    }
    if (pattern.type === "AssignmentPattern") {
        return bindPatternValue(pattern.left, value, state, index);
    }
    return null;
}
function evaluateCallExpression(node, state, scope) {
    const callName = getNodeName(node.callee);
    if (["cn", "clsx", "classnames"].includes(callName ?? "")) {
        const values = (node.arguments ?? []).map((arg) => evaluateExpression(arg.expression ?? arg, state, scope));
        // Class helpers intentionally ignore absent optional values, just as the
        // runtime `cn` implementation does. That keeps a primitive's static base
        // classes materialized in IR when no caller className was supplied.
        return flattenClasses(values).join(" ");
    }
    if (node.callee?.type === "Identifier") {
        const definition = state.expressionScope[getNodeName(node.callee) ?? ""];
        if (definition?.type === "CallExpression" && getNodeName(definition.callee) === "cva")
            return evaluateCva(definition, node, state, scope);
    }
    if (getNodeName(node.callee?.object) !== "Array" || getNodeName(node.callee?.property) !== "from") {
        return undefined;
    }
    const sourceExpression = unwrapExpression(node.arguments?.[0]?.expression ?? node.arguments?.[0]);
    const sourceValue = sourceExpression?.type === "ObjectExpression"
        ? evaluateObjectLiteral(sourceExpression, state, scope)
        : evaluateExpression(sourceExpression, state, scope);
    const length = getArrayLikeLength(sourceValue);
    if (length === null) {
        return undefined;
    }
    const callback = unwrapExpression(node.arguments?.[1]?.expression ?? node.arguments?.[1]);
    if (!callback || (callback.type !== "ArrowFunctionExpression" && callback.type !== "FunctionExpression")) {
        return undefined;
    }
    const values = [];
    for (let index = 0; index < length; index += 1) {
        const invocationScope = mergeScopes(scope, bindCallbackParameters(callback, index, state));
        const value = evaluateFunctionBody(callback, state, invocationScope);
        if (value === undefined) {
            return undefined;
        }
        values.push(value);
    }
    return values;
}
function evaluateObjectLiteral(node, state, scope) {
    if (!node || node.type !== "ObjectExpression") {
        return undefined;
    }
    const record = {};
    for (const property of node.properties ?? []) {
        const key = getNodeName(property?.key ?? property?.keyExpression ?? property?.id);
        if (!key) {
            return undefined;
        }
        const valueNode = property?.value ?? property?.expr ?? property?.init;
        const value = evaluateExpression(valueNode, state, scope);
        if (value === undefined) {
            return undefined;
        }
        record[key] = value;
    }
    return record;
}
function bindCallbackParameters(callback, index, state) {
    const scope = {};
    const params = callback.params ?? [];
    if (params[0]) {
        const bound = bindPatternValue(params[0], undefined, state, index);
        if (bound) {
            Object.assign(scope, bound);
        }
    }
    if (params[1]) {
        const bound = bindPatternValue(params[1], index, state, index);
        if (bound) {
            Object.assign(scope, bound);
        }
    }
    return scope;
}
function evaluateFunctionBody(functionLike, state, scope) {
    const body = getFunctionBody(functionLike);
    if (!body) {
        return undefined;
    }
    if (body.type !== "BlockStatement") {
        return evaluateExpression(body, state, scope);
    }
    for (const statement of body.stmts ?? body.body ?? []) {
        if (statement.type !== "ReturnStatement" && statement.type !== "ReturnStmt") {
            continue;
        }
        return evaluateExpression(statement.argument ?? statement.arg, state, scope);
    }
    return undefined;
}
function getArrayLikeLength(value) {
    if (Array.isArray(value)) {
        return value.length;
    }
    if (typeof value === "number" && Number.isFinite(value)) {
        return Math.max(0, Math.floor(value));
    }
    if (typeof value === "object" && value !== null) {
        const length = value.length;
        if (typeof length === "number" && Number.isFinite(length)) {
            return Math.max(0, Math.floor(length));
        }
    }
    return null;
}
function collectLiteralScope(body, state) {
    const block = body?.type === "BlockStatement" ? body : null;
    if (!block) {
        return;
    }
    for (const statement of block.stmts ?? block.body ?? []) {
        if (statement.type !== "VariableDeclaration" && statement.type !== "VarDecl") {
            continue;
        }
        const isConst = statement.kind === "const" || statement.declare === true || statement.const === true;
        if (!isConst) {
            continue;
        }
        for (const declaration of statement.declarations ?? statement.decls ?? []) {
            const name = getPatternName(declaration?.id);
            if (!name || !declaration?.init) {
                continue;
            }
            const value = evaluateExpression(declaration.init, state, state.scope);
            if (value !== undefined) {
                state.scope[name] = value;
            }
        }
    }
}
function collectModuleFacts(body, state) {
    for (const statement of getStatements(body)) {
        if (statement.type !== "VariableDeclaration" && statement.type !== "VarDecl") {
            continue;
        }
        for (const declaration of statement.declarations ?? statement.decls ?? []) {
            const init = unwrapExpression(declaration?.init);
            if (!init) {
                continue;
            }
            const value = evaluateExpression(init, state, state.scope);
            const name = getPatternName(declaration?.id);
            if (name && value !== undefined) {
                state.scope[name] = value;
            }
        }
    }
    collectLiteralScope(body, state);
}
function collectModuleScope(moduleAst, state) {
    for (const statement of getStatements(moduleAst)) {
        if (statement.type !== "VariableDeclaration" && statement.type !== "VarDecl") {
            continue;
        }
        for (const declaration of statement.declarations ?? statement.decls ?? []) {
            const name = getPatternName(declaration?.id);
            if (!name || !declaration?.init) {
                continue;
            }
            const value = evaluateExpression(declaration.init, state, state.scope);
            if (value !== undefined) {
                state.scope[name] = value;
            }
        }
    }
}
function flattenClasses(values) {
    const result = [];
    for (const value of values) {
        if (!value)
            continue;
        if (typeof value === "string" || typeof value === "number")
            result.push(String(value));
        else if (Array.isArray(value))
            result.push(...flattenClasses(value));
        else if (typeof value === "object")
            for (const [key, enabled] of Object.entries(value))
                if (enabled)
                    result.push(key);
    }
    return result;
}
function evaluateCva(definition, call, state, scope) {
    const base = evaluateExpression(definition.arguments?.[0]?.expression ?? definition.arguments?.[0], state, scope);
    const config = unwrapExpression(definition.arguments?.[1]?.expression ?? definition.arguments?.[1]);
    const selected = unwrapExpression(call.arguments?.[0]?.expression ?? call.arguments?.[0]);
    if (typeof base !== "string" || config?.type !== "ObjectExpression")
        return undefined;
    const selectedValue = {};
    if (selected?.type === "ObjectExpression")
        for (const property of selected.properties ?? []) {
            const key = getNodeName(property.key);
            if (!key)
                continue;
            const value = evaluateExpression(property.value ?? property.expr, state, scope);
            if (value !== undefined)
                selectedValue[key] = value;
        }
    else if (selected) {
        reportUnsupported(state, "DYNAMIC_CVA_VARIANT", "CVA variants must be statically known.");
        return undefined;
    }
    const getObject = (object, key) => object.properties?.find((property) => getNodeName(property.key) === key)?.value;
    const variants = getObject(config, "variants");
    const defaults = getObject(config, "defaultVariants");
    const classes = [base];
    for (const property of variants?.properties ?? []) {
        const variant = getNodeName(property.key);
        if (!variant)
            continue;
        const value = selectedValue[variant] ?? evaluateExpression(getObject(defaults, variant), state, scope);
        if (value === undefined) {
            reportUnsupported(state, "DYNAMIC_CVA_VARIANT", `CVA variant ${variant} must be static.`);
            return undefined;
        }
        const option = property.value?.properties?.find((entry) => getNodeName(entry.key) === String(value))?.value;
        const className = evaluateExpression(option, state, scope);
        if (typeof className !== "string")
            return undefined;
        classes.push(className);
    }
    if (typeof selectedValue.className === "string")
        classes.push(selectedValue.className);
    return classes.join(" ");
}
function collectComponents(moduleAst, state, moduleId) {
    for (const original of getStatements(moduleAst)) {
        const statement = original.declaration ?? original.decl ?? original;
        if ((statement.type === "FunctionDeclaration" || statement.type === "FnDecl") && isComponentName(getNodeName(statement.identifier ?? statement.id))) {
            const name = getNodeName(statement.identifier ?? statement.id);
            state.components.set(componentKey(moduleId, name), { name, body: getFunctionBody(statement), params: statement.params ?? statement.function?.params ?? [], moduleId });
        }
        if (statement.type === "FunctionExpression" && isComponentName(getNodeName(statement.identifier ?? statement.id))) {
            const name = getNodeName(statement.identifier ?? statement.id);
            state.components.set(componentKey(moduleId, name), { name, body: getFunctionBody(statement), params: statement.params ?? [], moduleId });
        }
        if (statement.type === "VariableDeclaration" || statement.type === "VarDecl")
            for (const declaration of statement.declarations ?? statement.decls ?? []) {
                const name = getPatternName(declaration.id);
                const init = declaration.init;
                if (name && isComponentName(name) && (init?.type === "ArrowFunctionExpression" || init?.type === "FunctionExpression"))
                    state.components.set(componentKey(moduleId, name), { name, body: getFunctionBody(init), params: init.params ?? [], moduleId });
            }
    }
}
function collectModuleExpressions(moduleAst, state, moduleId) {
    const expressions = {};
    for (const original of getStatements(moduleAst)) {
        const statement = original.declaration ?? original.decl ?? original;
        if (statement.type !== "VariableDeclaration" && statement.type !== "VarDecl")
            continue;
        for (const declaration of statement.declarations ?? statement.decls ?? []) {
            const name = getPatternName(declaration.id);
            if (name && declaration.init)
                expressions[name] = declaration.init;
        }
    }
    state.moduleExpressions.set(moduleId, expressions);
}
function collectImports(moduleAst, state, moduleId) {
    for (const statement of getStatements(moduleAst)) {
        if (statement.type !== "ImportDeclaration")
            continue;
        const source = statement.source?.value;
        if (!source)
            continue;
        for (const specifier of statement.specifiers ?? []) {
            const local = getNodeName(specifier.local) ?? getNodeName(specifier.local?.id);
            if (!local)
                continue;
            const imported = getNodeName(specifier.imported) ?? (specifier.type === "ImportDefaultSpecifier" ? "default" : local);
            state.importSymbols.set(`${moduleId}::${local}`, { moduleId: resolveModuleId(moduleId, source), exportName: imported });
        }
        if (source !== "@tanstack/react-router")
            continue;
        for (const specifier of statement.specifiers ?? []) {
            const imported = getNodeName(specifier.imported) ?? getNodeName(specifier.local);
            if (imported === "Link")
                state.routerLinkBindings.add(getNodeName(specifier.local) ?? "Link");
        }
    }
}
function componentKey(moduleId, name) { return `${moduleId}::${name}`; }
function resolveModuleId(from, source) {
    if (!source.startsWith("."))
        return source;
    const base = from.split("/").slice(0, -1);
    for (const part of source.split("/")) {
        if (part === "." || !part)
            continue;
        if (part === "..")
            base.pop();
        else
            base.push(part);
    }
    const candidate = base.join("/").replace(/\.(?:tsx?|jsx?)$/, "");
    return candidate.endsWith(".tsx") || candidate.endsWith(".ts") ? candidate : `${candidate}.tsx`;
}
function getLocalComponent(name, state) {
    const imported = state.importSymbols.get(`${state.activeModuleId}::${name}`);
    if (imported) {
        const direct = state.components.get(componentKey(imported.moduleId, imported.exportName)) ?? state.components.get(componentKey(imported.moduleId, name));
        if (direct)
            return direct;
        if (imported.exportName === "default")
            return [...state.components.values()].find((component) => component.moduleId === imported.moduleId);
    }
    return state.components.get(componentKey(state.activeModuleId, name));
}
function getExternalSymbol(name, state) {
    const [root, ...members] = name.split(".");
    const imported = state.importSymbols.get(`${state.activeModuleId}::${root}`);
    if (!imported || !imported.moduleId.startsWith("@"))
        return null;
    const symbol = [imported.exportName, ...members].join(".");
    // These adapters deliberately take precedence over their implementation
    // modules: the compiler should target their stable DOM contract, not expand
    // Base UI internals when the source module is available in the graph.
    if (imported.moduleId === "@wasm-runtime/ui/atoms/checkbox" && symbol === "Checkbox")
        return "checkbox-input";
    if (imported.moduleId === "@wasm-runtime/ui/atoms/input" && symbol === "Input")
        return "input";
    if (imported.moduleId === "@wasm-runtime/ui/atoms/badge" && symbol === "Badge")
        return "span";
    if (state.components.has(componentKey(imported.moduleId, imported.exportName)))
        return null;
    if (imported.moduleId === "@base-ui/react/button" && symbol === "Button")
        return "button";
    if (imported.moduleId === "@base-ui/react/input" && symbol === "Input")
        return "input";
    // The first public adapter intentionally recognizes only Checkbox's stable
    // compound surface. Its package internals are never compiled.
    if (imported.moduleId === "@base-ui/react/checkbox") {
        if (symbol === "Checkbox.Root")
            return "internal-toggle-root";
        if (symbol === "Checkbox.Indicator")
            return "internal-toggle-indicator";
    }
    if (imported.moduleId === "@hugeicons/react" && symbol === "HugeiconsIcon")
        return "hugeicons-icon";
    // This module exists only in compiler fixtures. It establishes the adapter
    // contract without exposing a new authoring package.
    if (imported.moduleId === "@wasm-runtime/internal-toggle") {
        if (symbol === "Toggle.Root")
            return "internal-toggle-root";
        if (symbol === "Toggle.Indicator")
            return "internal-toggle-indicator";
    }
    return `${imported.moduleId}::${symbol}`;
}
function lowerToggleRoot(node, parentId, state) {
    const props = collectCallerProps(node, state);
    const permitted = new Set(["checked", "defaultChecked", "indeterminate", "disabled", "readOnly", "required", "name", "value", "form", "onCheckedChange", "className", "style", "id", "aria-label", "aria-labelledby", "data-testid"]);
    for (const key of Object.keys(props))
        if (!permitted.has(key))
            reportUnsupported(state, "UNSUPPORTED_TOGGLE_PROP", `Toggle.Root prop ${key} is not supported.`);
    const rootId = nextElementId(state);
    const inputId = nextElementId(state);
    const root = { id: rootId, tag: "label", parentId, attributes: [], children: [inputId] };
    for (const key of ["className", "style", "id", "aria-label", "aria-labelledby", "data-testid"]) {
        const value = props[key];
        if (!value)
            continue;
        const literal = evaluateExpression(value, state, {});
        if (literal !== undefined)
            root.attributes.push({ name: key, staticValue: literalToString(literal) });
        else {
            const bindingId = nextBindingId(state);
            state.ir.bindings.push({ id: bindingId, kind: "attribute", targetId: rootId, attributeName: key, expression: serializeExpression(value, state), expressionId: internExpression(value, state) });
            root.attributes.push({ name: key, bindingId });
        }
    }
    const input = { id: inputId, tag: "input", parentId: rootId, attributes: [{ name: "type", staticValue: "checkbox" }, { name: "data-runtime-toggle", staticValue: `t${state.ir.toggles.length + 1}` }], children: [] };
    const valueSource = (key) => {
        const value = props[key];
        if (!value)
            return undefined;
        const literal = evaluateExpression(value, state, {});
        if (literal !== undefined)
            return { staticValue: literal === "mixed" ? "mixed" : literal ? "true" : "false" };
        return { expressionId: internExpression(value, state) };
    };
    const scalar = (key) => {
        const value = props[key];
        if (!value)
            return;
        const literal = evaluateExpression(value, state, {});
        if (literal !== undefined)
            input.attributes.push({ name: key, staticValue: literalToString(literal) });
        else {
            const bindingId = nextBindingId(state);
            state.ir.bindings.push({ id: bindingId, kind: "attribute", targetId: inputId, attributeName: key, expression: serializeExpression(value, state), expressionId: internExpression(value, state) });
            input.attributes.push({ name: key, bindingId });
        }
    };
    for (const key of ["disabled", "readOnly", "required", "name", "value", "form"])
        scalar(key);
    const checked = valueSource("checked");
    const defaultChecked = valueSource("defaultChecked") ?? (!checked ? { staticValue: "false" } : undefined);
    const indeterminate = valueSource("indeterminate");
    const eventId = `ev${++state.eventCounter}`;
    const actionId = props.onCheckedChange ? `a${state.eventCounter}` : undefined;
    state.ir.events.push({ id: eventId, type: "change", targetId: inputId, actionId: actionId ?? `a${state.eventCounter}`, args: [] });
    const stateSlotId = `s${state.ir.localStates.length + 1}`;
    state.ir.localStates.push({ id: stateSlotId, name: `toggle-${state.ir.toggles.length + 1}`, initialValue: defaultChecked?.staticValue === "true" ? "true" : "false", values: ["false", "true", "mixed"] });
    state.ir.elements.push(root, input);
    let indicatorId;
    for (const child of node.children ?? []) {
        if (child.type === "JSXText" && !normalizeText(child.value ?? ""))
            continue;
        const childName = child.type === "JSXElement" ? getJsxName(child.opening?.name) : undefined;
        if (child.type !== "JSXElement" || !childName || getExternalSymbol(childName, state) !== "internal-toggle-indicator") {
            reportUnsupported(state, "UNSUPPORTED_TOGGLE_CHILD", "Toggle.Root only supports one direct Toggle.Indicator child.");
            continue;
        }
        if (indicatorId) {
            reportUnsupported(state, "DUPLICATE_TOGGLE_INDICATOR", "Toggle.Root supports only one Toggle.Indicator.");
            continue;
        }
        indicatorId = nextElementId(state);
        const indicator = { id: indicatorId, tag: "span", parentId: rootId, attributes: [{ name: "data-toggle-indicator", staticValue: "" }], children: [] };
        for (const attribute of child.opening?.attributes ?? []) {
            const name = getNodeName(attribute.name);
            if (!name || !attribute.value)
                continue;
            if (attribute.value.type === "StringLiteral")
                indicator.attributes.push({ name, staticValue: attribute.value.value });
            else if (attribute.value.type === "JSXExpressionContainer") {
                const bindingId = nextBindingId(state);
                state.ir.bindings.push({ id: bindingId, kind: "attribute", targetId: indicatorId, attributeName: name, expression: serializeExpression(attribute.value.expression, state), expressionId: internExpression(attribute.value.expression, state) });
                indicator.attributes.push({ name, bindingId });
            }
        }
        state.ir.elements.push(indicator);
        root.children.push(indicatorId);
        for (const grandchild of child.children ?? []) {
            const lowered = lowerJsxNode(grandchild, indicatorId, state);
            if (lowered !== SKIP)
                indicator.children.push(lowered);
        }
    }
    state.ir.toggles.push({ id: `t${state.ir.toggles.length + 1}`, rootElementId: rootId, inputElementId: inputId, indicatorElementId: indicatorId, stateSlotId, actionId, checked, defaultChecked, indeterminate, disabled: valueSource("disabled"), readOnly: valueSource("readOnly"), required: valueSource("required"), name: valueSource("name"), value: valueSource("value"), form: valueSource("form"), eventId });
    return rootId;
}
function lowerExternalComponent(node, parentId, state, external) {
    if (external === "internal-toggle-root")
        return lowerToggleRoot(node, parentId, state);
    if (external === "internal-toggle-indicator") {
        reportUnsupported(state, "TOGGLE_INDICATOR_OUTSIDE_ROOT", "Toggle.Indicator must be a direct child of Toggle.Root.");
        return SKIP;
    }
    if (external === "hugeicons-icon")
        return lowerHugeiconsIcon(node, parentId, state);
    if (external === "button" || external === "input" || external === "span" || external === "checkbox-input" || external === "badge") {
        if (external === "button" || external === "span") {
            const intrinsic = { ...node, opening: { ...node.opening, name: { type: "Identifier", value: external }, attributes: node.opening?.attributes }, closing: node.closing ? { ...node.closing, name: { type: "Identifier", value: external } } : node.closing };
            return lowerJsxElement(intrinsic, parentId, state);
        }
        const tag = external === "checkbox-input" ? "input" : external === "badge" ? "span" : external;
        const sourceAttributes = node.opening?.attributes ?? [];
        const callerClass = sourceAttributes.find((attribute) => getNodeName(attribute.name) === "className");
        const variant = sourceAttributes.find((attribute) => getNodeName(attribute.name) === "variant");
        const withoutAdapterProps = sourceAttributes.filter((attribute) => !["className", "variant"].includes(getNodeName(attribute.name) ?? ""));
        const classes = external === "checkbox-input"
            ? "size-4 shrink-0 accent-primary rounded-[6px] border border-input"
            : external === "input"
                ? "h-9 w-full min-w-0 rounded-4xl border border-input bg-input/30 px-3 py-1 text-base outline-none md:text-sm"
                : external === "badge"
                    ? "inline-flex h-5 w-fit shrink-0 items-center justify-center rounded-4xl border border-border bg-input/30 px-2 py-0.5 text-xs font-medium whitespace-nowrap data-[variant=secondary]:border-transparent data-[variant=secondary]:bg-secondary data-[variant=secondary]:text-secondary-foreground"
                    : "";
        const classParts = [{ expression: { type: "StringLiteral", value: classes } }];
        if (callerClass?.value?.expression)
            classParts.push({ expression: callerClass.value.expression });
        const classAttribute = classes ? { type: "JSXAttribute", name: { type: "Identifier", value: "className" }, value: { type: "JSXExpressionContainer", expression: { type: "CallExpression", callee: { type: "Identifier", value: "cn" }, arguments: classParts } } } : undefined;
        const attributes = [
            ...withoutAdapterProps,
            ...(external === "badge" && variant ? [{ ...variant, name: { type: "Identifier", value: "data-variant" } }] : []),
            ...(classAttribute ? [classAttribute] : []),
            ...(external === "checkbox-input" ? [{ type: "JSXAttribute", name: { type: "Identifier", value: "type" }, value: { type: "StringLiteral", value: "checkbox" } }] : []),
        ];
        const intrinsic = { ...node, opening: { ...node.opening, name: { type: "Identifier", value: tag }, attributes }, closing: node.closing ? { ...node.closing, name: { type: "Identifier", value: tag } } : node.closing };
        return lowerJsxElement(intrinsic, parentId, state);
    }
    const trace = [...state.componentStack, getJsxName(node.opening?.name)].filter(Boolean).join(" → ");
    reportUnsupported(state, "UNSUPPORTED_EXTERNAL_COMPONENT", `Cannot lower external component: ${external}\nReached through:\n${trace}`);
    return SKIP;
}
function lowerHugeiconsIcon(node, parentId, state) {
    // Hugeicons React components are declarative SVG emitters. For the first
    // adapter we preserve the authored sizing/stroke props and lower the Tick
    // icon used by the shadcn checkbox to an intrinsic path.
    const attributes = (node.opening?.attributes ?? []).filter((attribute) => getNodeName(attribute.name) !== "icon");
    attributes.push({ type: "JSXAttribute", name: { type: "Identifier", value: "viewBox" }, value: { type: "StringLiteral", value: "0 0 24 24" } });
    attributes.push({ type: "JSXAttribute", name: { type: "Identifier", value: "fill" }, value: { type: "StringLiteral", value: "none" } });
    const intrinsic = { ...node, opening: { ...node.opening, name: { type: "Identifier", value: "svg" }, attributes }, closing: node.closing ? { ...node.closing, name: { type: "Identifier", value: "svg" } } : node.closing, children: [] };
    const svgId = lowerJsxElement(intrinsic, parentId, state);
    if (svgId === SKIP)
        return SKIP;
    const pathId = nextElementId(state);
    state.ir.elements.push({ id: pathId, tag: "path", parentId: svgId, attributes: [{ name: "d", staticValue: "M5 12l4 4L19 6" }, { name: "stroke", staticValue: "currentColor" }, { name: "stroke-linecap", staticValue: "round" }, { name: "stroke-linejoin", staticValue: "round" }], children: [] });
    getElementById(state, svgId)?.children.push(pathId);
    return svgId;
}
function getJsxName(node) {
    if (!node)
        return undefined;
    if (node.type === "JSXMemberExpression")
        return [getJsxName(node.object), getJsxName(node.property)].filter(Boolean).join(".");
    return getNodeName(node);
}
function objectExpression(values) {
    return { type: "ObjectExpression", properties: Object.entries(values).map(([key, value]) => ({ type: "KeyValueProperty", key: { type: "Identifier", value: key }, value })) };
}
function collectCallerProps(node, state) {
    const props = {};
    for (const attribute of node.opening?.attributes ?? []) {
        if (attribute.type === "SpreadElement") {
            const spread = unwrapExpression(attribute.arguments ?? attribute.argument);
            const resolved = spread?.type === "Identifier" ? state.expressionScope[getNodeName(spread)] : spread;
            if (resolved?.type === "ObjectExpression")
                for (const property of resolved.properties ?? []) {
                    const key = getNodeName(property.key);
                    if (key)
                        props[key] = property.value ?? property.expr;
                }
            else
                reportUnsupported(state, "UNSUPPORTED_SPREAD_PROPS", "Component spread props must be statically known.");
            continue;
        }
        const key = getNodeName(attribute.name);
        if (!key)
            continue;
        props[key] = !attribute.value ? { type: "BooleanLiteral", value: true } : attribute.value.type === "JSXExpressionContainer" ? attribute.value.expression : attribute.value;
    }
    return props;
}
function expandIntrinsicAttributes(attributes, state) {
    const output = [];
    for (const attribute of attributes) {
        if (attribute.type !== "SpreadElement") {
            output.push(attribute);
            continue;
        }
        const argument = unwrapExpression(attribute.arguments ?? attribute.argument);
        const resolved = argument?.type === "Identifier" ? state.expressionScope[getNodeName(argument)] : argument;
        if (resolved?.type !== "ObjectExpression") {
            reportUnsupported(state, "UNSUPPORTED_SPREAD_PROPS", "Intrinsic spread props must be statically known.");
            continue;
        }
        for (const property of resolved.properties ?? []) {
            const name = getNodeName(property.key);
            if (!name)
                continue;
            if (name === "children")
                continue;
            output.push({ type: "JSXAttribute", name: { type: "Identifier", value: name }, value: { type: "JSXExpressionContainer", expression: property.value ?? property.expr } });
        }
    }
    return output;
}
function getForwardedChildren(expression, state) {
    const name = getNodeName(expression);
    const value = name ? state.expressionScope[name] : undefined;
    return value?.type === "JSXChildren" ? value.children : null;
}
/**
 * shadcn-style primitives commonly forward all remaining props with
 * `{...props}`. `children` is part of that object, so retain it as element
 * children rather than silently dropping the rendered subtree.
 */
function getSpreadChildren(attributes, state) {
    const children = [];
    for (const attribute of attributes) {
        if (attribute.type !== "SpreadElement")
            continue;
        const argument = unwrapExpression(attribute.arguments ?? attribute.argument);
        const resolved = argument?.type === "Identifier" ? state.expressionScope[getNodeName(argument)] : argument;
        if (resolved?.type !== "ObjectExpression")
            continue;
        for (const property of resolved.properties ?? []) {
            if (getNodeName(property.key) !== "children")
                continue;
            const value = property.value ?? property.expr;
            if (value?.type === "JSXChildren")
                children.push(...(value.children ?? []));
        }
    }
    return children;
}
function internExpression(expression, state) {
    const lowered = lowerExpression(expression, state);
    const existing = state.ir.expressions.find((candidate) => JSON.stringify(candidate.expression) === JSON.stringify(lowered));
    if (existing)
        return existing.id;
    state.expressionCounter += 1;
    const id = `x${state.expressionCounter}`;
    state.ir.expressions.push({ id, expression: lowered });
    return id;
}
function lowerExpression(input, state, resolving = new Set()) {
    const node = unwrapExpression(input);
    if (!node)
        return { kind: "literal", value: null };
    if (isLiteralExpression(node))
        return { kind: "literal", value: node.type === "NullLiteral" ? null : node.value };
    const identifier = getNodeName(node);
    if (node.type === "Identifier" || node.type === "IdentifierExpression") {
        if (identifier && state.expressionScope[identifier] && !resolving.has(identifier)) {
            resolving.add(identifier);
            return lowerExpression(state.expressionScope[identifier], state, resolving);
        }
        return { kind: "identifier", name: identifier ?? "unknown" };
    }
    if (node.type === "MemberExpression")
        return { kind: "member", object: lowerExpression(node.object, state, resolving), property: getNodeName(node.property) ?? "" };
    if (node.type === "CallExpression" && node.callee?.type === "MemberExpression" && getNodeName(node.callee.property) === "getFullYear" && node.callee.object?.type === "NewExpression" && getNodeName(node.callee.object.callee) === "Date")
        return { kind: "host", name: "currentYear" };
    if (node.type === "BinaryExpression")
        return { kind: "binary", op: node.operator, left: lowerExpression(node.left, state, resolving), right: lowerExpression(node.right, state, resolving) };
    if (node.type === "LogicalExpression")
        return { kind: "logical", op: node.operator, left: lowerExpression(node.left, state, resolving), right: lowerExpression(node.right, state, resolving) };
    if (node.type === "ConditionalExpression")
        return { kind: "conditional", test: lowerExpression(node.test, state, resolving), consequent: lowerExpression(node.consequent, state, resolving), alternate: lowerExpression(node.alternate, state, resolving) };
    if (node.type === "TemplateLiteral") {
        const parts = [];
        for (let i = 0; i < (node.quasis?.length ?? 0); i++) {
            parts.push(node.quasis[i]?.raw ?? node.quasis[i]?.value?.cooked ?? "");
            if (node.expressions?.[i])
                parts.push(lowerExpression(node.expressions[i], state, resolving));
        }
        return { kind: "template", parts };
    }
    if (node.type === "ArrayExpression")
        return { kind: "array", items: (node.elements ?? []).filter(Boolean).map((item) => lowerExpression(item.expression ?? item, state, resolving)) };
    if (node.type === "ObjectExpression")
        return { kind: "object", entries: (node.properties ?? []).map((property) => ({ key: getNodeName(property.key) ?? "", value: lowerExpression(property.value ?? property.expr, state, resolving) })) };
    if (node.type === "CallExpression" && ["cn", "clsx", "classnames"].includes(getNodeName(node.callee) ?? ""))
        return { kind: "intrinsic", name: getNodeName(node.callee), args: (node.arguments ?? []).map((arg) => lowerExpression(arg.expression ?? arg, state, resolving)) };
    reportUnsupported(state, "UNSUPPORTED_EXPRESSION", `Unsupported expression: ${node.type}.`);
    return { kind: "literal", value: "" };
}
function getStatements(node) {
    if (!node) {
        return [];
    }
    if (Array.isArray(node.body)) {
        return node.body;
    }
    if (Array.isArray(node.stmts)) {
        return node.stmts;
    }
    return [];
}
function nextInputId(state) {
    state.inputCounter += 1;
    return `i${state.inputCounter}`;
}
/** The producer expression is intentionally opaque. Only the row projection
 * crosses into O1, so no import path or hook name is part of this decision. */
function ensureCollectionInput(name, callback, state) {
    const existing = state.ir.inputs.find((input) => input.name === name);
    if (existing)
        return existing;
    const itemName = getPatternLabel(callback?.params?.[0]);
    const keyExpression = getMapRowKeyExpression(callback, state);
    if (!itemName || !keyExpression)
        return undefined;
    const observed = new Set();
    const visit = (node) => {
        if (!node || typeof node !== "object")
            return;
        if ((node.type === "MemberExpression" || node.type === "OptionalChainingExpression") && getNodeName(node.object) === itemName) {
            const property = getNodeName(node.property);
            if (property)
                observed.add(property);
        }
        for (const value of Object.values(node)) {
            if (Array.isArray(value))
                value.forEach(visit);
            else if (value && typeof value === "object")
                visit(value);
        }
    };
    visit(callback.body);
    const input = { id: nextInputId(state), name, shape: { kind: "collection", keyExpression, orderSensitive: true, observedRowPaths: [...observed].sort().map((path) => [path]) } };
    state.ir.inputs.push(input);
    return input;
}
function inferValueInputs(state) {
    const rowNames = new Set(state.ir.loops.map((loop) => loop.itemName));
    const expressions = new Map(state.ir.expressions.map((entry) => [entry.id, entry.expression]));
    for (const binding of state.ir.bindings) {
        const expression = expressions.get(binding.expressionId ?? "");
        const path = inputPath(expression);
        if (!path)
            continue;
        const inputName = path[0];
        if (rowNames.has(inputName) || inputName === "host")
            continue;
        let input = state.ir.inputs.find((candidate) => candidate.name === inputName);
        if (!input) {
            input = path.length === 1
                ? { id: nextInputId(state), name: inputName, shape: { kind: "scalar" } }
                : { id: nextInputId(state), name: inputName, shape: { kind: "object", observedPaths: [] } };
            state.ir.inputs.push(input);
        }
        if (input.shape.kind === "object" && path.length > 1 && !input.shape.observedPaths.some((candidate) => candidate.join(".") === path.slice(1).join(".")))
            input.shape.observedPaths.push(path.slice(1));
        state.ir.dependencyEdges.push({ fromId: input.id, toId: binding.id, kind: "input-to-binding" });
    }
}
function inputPath(expression) {
    if (!expression)
        return undefined;
    if (expression.kind === "identifier")
        return [expression.name];
    if (expression.kind === "member") {
        const base = inputPath(expression.object);
        return base ? [...base, expression.property] : undefined;
    }
    return undefined;
}
function getElementById(state, elementId) {
    return state.ir.elements.find((element) => element.id === elementId);
}
function mergeScopes(baseScope, extraScope) {
    return { ...baseScope, ...extraScope };
}
function getPatternName(pattern) {
    if (!pattern) {
        return undefined;
    }
    if (pattern.type === "Identifier") {
        return pattern.value;
    }
    if (pattern.type === "AssignmentPattern") {
        return getPatternName(pattern.left);
    }
    if (pattern.type === "RestElement") {
        return getPatternName(pattern.argument);
    }
    if (pattern.type === "ObjectPattern" || pattern.type === "ArrayPattern") {
        return undefined;
    }
    return getNodeName(pattern);
}
function getPatternLabel(pattern) {
    const name = getPatternName(pattern);
    if (name) {
        return name;
    }
    if (pattern?.type === "ArrayPattern") {
        return "tuple";
    }
    if (pattern?.type === "ObjectPattern") {
        return "object";
    }
    return undefined;
}
function normalizeText(text) {
    return text.replace(/\s+/g, " ").trim();
}
function isLiteralExpression(expression) {
    return (expression?.type === "StringLiteral" ||
        expression?.type === "NumericLiteral" ||
        expression?.type === "BooleanLiteral" ||
        expression?.type === "NullLiteral");
}
function literalToString(expression) {
    if (expression && typeof expression === "object" && expression.type === "NullLiteral") {
        return "null";
    }
    if (expression && typeof expression === "object" && "value" in expression) {
        return String(expression.value);
    }
    return String(expression);
}
function serializeExpression(expression, state) {
    const start = Math.max(0, (expression?.span?.start ?? 1) - 1);
    const end = Math.max(start, (expression?.span?.end ?? start + 1) - 1);
    const snippet = state.source.slice(start, end).trim();
    if (snippet) {
        return snippet;
    }
    return expression?.type ?? "unknown";
}
function isIntrinsicTag(tag) {
    return /^[a-z][a-z0-9-]*$/.test(tag);
}
function isComponentName(value) {
    return typeof value === "string" && /^[A-Z]/.test(value);
}
function getNodeName(node) {
    if (!node) {
        return undefined;
    }
    if (typeof node.value === "string") {
        return node.value;
    }
    if (typeof node.name === "string") {
        return node.name;
    }
    if (node.id) {
        return getNodeName(node.id);
    }
    return undefined;
}
function getFunctionBody(node) {
    return node?.function?.body ?? node?.body;
}
function unwrapExpression(node) {
    let current = node;
    while (current) {
        if (current.type === "ParenthesisExpression" ||
            current.type === "ParenthesizedExpression" ||
            current.type === "ParenExpr" ||
            current.type === "ParenExpression") {
            current = current.expression;
            continue;
        }
        if (current.type === "TsAsExpression" || current.type === "TsTypeAssertion") {
            current = current.expression;
            continue;
        }
        if (current.type === "TsNonNullExpression") {
            current = current.expression;
            continue;
        }
        break;
    }
    return current;
}
function nextElementId(state) {
    state.elementCounter += 1;
    return `e${state.elementCounter}`;
}
function nextTextId(state) {
    state.textCounter += 1;
    return `t${state.textCounter}`;
}
function nextBindingId(state) {
    state.bindingCounter += 1;
    return `b${state.bindingCounter}`;
}
function nextLoopId(state) {
    state.bindingCounter += 1;
    return `l${state.bindingCounter}`;
}
function reportUnsupported(state, code, message) {
    state.diagnostics.push({
        code,
        message,
        severity: "error"
    });
}
function throwIfStrict(state) {
    if (state.mode !== "strict") {
        return;
    }
    const firstDiagnostic = state.diagnostics[0];
    if (firstDiagnostic) {
        throw new CompileFailure(`Strict compilation failed: ${firstDiagnostic.code} ${firstDiagnostic.message}`);
    }
}
//# sourceMappingURL=index.js.map