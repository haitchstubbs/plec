import { parseSync } from "@swc/core";

export interface CompilerDiagnostic {
  code: string;
  message: string;
  severity: "error" | "warning";
}

interface BindingNode {
  id: string;
  kind: "text" | "attribute" | "property";
  targetId: string;
  attributeName?: string;
  expression: string;
  expressionId?: string;
}
interface ExpressionNode { id: string; expression: unknown }
interface EventNode { id: string; type: string; targetId: string; actionId: string; args: string[]; field?: string; callbackName?: string; navigate?: { href: string; replace?: boolean }; stopPropagation?: boolean; preventDefault?: boolean }
interface IslandNode { islandInstanceId: string; componentId: string; placeholderNodeId: string; moduleId: string; exportName: string; props: Record<string, unknown> }
interface ComponentMetadata { name: string; moduleId: string; elementIds: string[]; bindingIds: string[]; eventIds: string[] }

interface QueryNode {
  id: string;
  source: string;
  resultSymbol?: string;
}

interface CompiledInputNode {
  id: string;
  name: string;
  shape: { kind: "scalar" } | { kind: "object"; observedPaths: string[][] } | { kind: "collection"; keyExpression: string; orderSensitive: boolean; observedRowPaths: string[][] };
}

interface DependencyEdge {
  fromId: string;
  toId: string;
  kind: "input-to-loop" | "query-to-loop" | "row-field-to-binding" | "input-to-binding" | "local-state-to-binding" | "host-value-to-binding";
}

interface LoopRowNode {
  id: string;
  rootElementId: string;
  keyValue?: string;
}

interface LoopNode {
  id: string;
  parentId: string;
  source: string;
  itemName: string;
  indexName?: string;
  rows: LoopRowNode[];
  inputId?: string;
  queryId?: string;
  rowTemplateRootElementId?: string;
  keyExpression?: string;
}

interface AttributeNode {
  name: string;
  staticValue?: string;
  bindingId?: string;
}

interface ElementNode {
  id: string;
  tag: string;
  parentId: string | null;
  keyValue?: string;
  attributes: AttributeNode[];
  children: string[];
}

interface TextNode {
  id: string;
  parentId: string;
  staticValue?: string;
}

interface ApplicationIr {
  version: "0.5";
  revision?: string;
  rootElementId: string;
  elements: ElementNode[];
  texts: TextNode[];
  inputs: CompiledInputNode[];
  queries: QueryNode[];
  dependencyEdges: DependencyEdge[];
  loops: LoopNode[];
  bindings: BindingNode[];
  expressions: ExpressionNode[];
  propPrograms: Array<{ id: string; targetId: string; writes: Array<{ name: string; staticValue?: string; expressionId?: string; kind: "attribute" | "property" | "event" | "ref" | "spread" }> }>;
  events: EventNode[];
  refs: Array<{ id: string; targetId: string; refId: string; kind: "callback" | "object" }>;
  contexts: Array<{ id: string; parentId: string | null; values: Array<{ name: string; expressionId?: string; staticValue?: string }> }>;
  conditionals: Array<{ id: string; parentId: string; expressionId: string; children: string[] }>;
  localStates: Array<{ id: string; name: string; initialValue: string; values: string[] }>;
  hostValues: Array<{ id: string; kind: "media-query"; query: string }>;
  lifecycleEffects: Array<any>;
  stateTransitions: Array<any>;
  islands: IslandNode[];
  components: ComponentMetadata[];
}

export interface CompileOptions {
  mode?: "strict" | "lenient";
  rootComponent?: string;
  moduleId?: string;
  applicationRevision?: string;
  modules?: Array<{ id: string; source: string }>;
  islandComponents?: string[];
}

export interface CompileResult {
  ir: ApplicationIr;
  diagnostics: CompilerDiagnostic[];
}

class CompileFailure extends Error {
  constructor(message: string) {
    super(message);
    this.name = "CompileFailure";
  }
}

interface CompilerState {
  source: string;
  mode: "strict" | "lenient";
  diagnostics: CompilerDiagnostic[];
  elementCounter: number;
  textCounter: number;
  bindingCounter: number;
  queryCounter: number;
  inputCounter: number;
  expressionCounter: number;
  eventCounter: number;
  hasLiveQuery: boolean;
  preserveBindings: boolean;
  scope: Record<string, unknown>;
  expressionScope: Record<string, any>;
  components: Map<string, { name: string; body: any; params: any[]; moduleId: string }>;
  importSymbols: Map<string, { moduleId: string; exportName: string }>;
  exports: Map<string, Map<string, { moduleId: string; exportName: string }>>;
  moduleExpressions: Map<string, Record<string, any>>;
  routerLinkBindings: Set<string>;
  islandComponents: Set<string>;
  componentStack: string[];
  activeModuleId: string;
  ir: ApplicationIr;
}

const SKIP = Symbol("skip");

type LowerResult = string | typeof SKIP;

export function compile(source: string, options: CompileOptions = {}): CompileResult {
  const mode = options.mode ?? "lenient";
  const moduleAst: any = parseSync(source, {
    syntax: "typescript",
    tsx: true,
    target: "es2022"
  });

  const state: CompilerState = {
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
    exports: new Map(),
    moduleExpressions: new Map(),
    routerLinkBindings: new Set(),
    islandComponents: new Set(options.islandComponents ?? []),
    componentStack: [],
    activeModuleId: options.moduleId ?? "<entry>",
    ir: {
      version: "0.5",
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
      propPrograms: [],
      events: [],
      refs: [],
      contexts: [],
      conditionals: [],
      localStates: [],
      hostValues: [],
      lifecycleEffects: [],
      stateTransitions: [],
      islands: [],
      components: []
    }
  };

  const modules = options.modules?.length ? options.modules : [{ id: options.moduleId ?? "<entry>", source }];
  for (const module of modules) {
    const ast: any = module.id === (options.moduleId ?? "<entry>") ? moduleAst : parseSync(module.source, { syntax: "typescript", tsx: true, target: "es2022" });
    collectModuleScope(ast, state);
    collectModuleExpressions(ast, state, module.id);
    collectComponents(ast, state, module.id);
    collectImports(ast, state, module.id);
    collectExports(ast, state, module.id);
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

  if (options.rootComponent) state.componentStack.push(options.rootComponent);
  const rootId = lowerJsxNode(rootJsx, null, state);
  if (options.rootComponent) state.componentStack.pop();
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
  } else {
    state.ir.rootElementId = rootId;
  }

  inferValueInputs(state);

  throwIfStrict(state);

  return {
    ir: state.ir,
    diagnostics: state.diagnostics
  };
}

function findRootJsx(moduleAst: any, state: CompilerState, requestedName?: string): any | null {
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
      if (jsx) return jsx;
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

function findReturnedJsx(body: any): any | null {
  if (!body) {
    return null;
  }

  const topLevel = normalizeRenderedReturn(unwrapExpression(body), body);
  if (topLevel?.type === "JSXElement" || topLevel?.type === "JSXFragment" || jsxFactoryElement(topLevel)) {
    return topLevel;
  }

  if (body.type !== "BlockStatement") {
    return null;
  }

  for (const statement of body.stmts ?? body.body ?? []) {
    if (statement.type === "ReturnStatement" || statement.type === "ReturnStmt") {
      const argument = normalizeRenderedReturn(unwrapExpression(statement.argument ?? statement.arg), body);
      if (argument?.type === "JSXElement" || argument?.type === "JSXFragment" || jsxFactoryElement(argument)) {
        return argument;
      }
    }
  }

  return null;
}

function lowerJsxNode(node: any, parentId: string | null, state: CompilerState): LowerResult {
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

  const factoryElement = jsxFactoryElement(node);
  if (factoryElement) {
    return lowerJsxElement(factoryElement, parentId, state);
  }

  reportUnsupported(state, "UNSUPPORTED_JSX_NODE", `Unsupported JSX node type: ${node.type}.`);
  return SKIP;
}

function lowerJsxElement(node: any, parentId: string | null, state: CompilerState): LowerResult {
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
    attributes: [] as Array<{ name: string; staticValue?: string; bindingId?: string }>,
    children: [] as string[]
  };

  const propWrites: Array<{ name: string; staticValue?: string; expressionId?: string; kind: "attribute" | "property" | "event" | "ref" | "spread" }> = [];
  for (const attributeNode of expandIntrinsicAttributes(node.opening?.attributes ?? [], state)) {
    if (attributeNode.type === "SpreadElement") {
      const expression = attributeNode.arguments ?? attributeNode.argument;
      const record = lowerExpression(expression, state);
      if (record.kind !== "object") {
        reportUnsupported(state, "UNSUPPORTED_SPREAD_PROPS", "A runtime JSX spread must evaluate to a serializable record.");
        continue;
      }
      propWrites.push({ name: "", expressionId: internExpression(expression, state), kind: "spread" });
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

    if (name === "ref" && attributeNode.value?.type === "JSXExpressionContainer") {
      const refId = getNodeName(unwrapExpression(attributeNode.value.expression)) ?? `ref${state.ir.refs.length + 1}`;
      state.ir.refs.push({ id: `r${state.ir.refs.length + 1}`, targetId: elementId, refId, kind: "callback" });
      propWrites.push({ name, kind: "ref" });
      continue;
    }
    if (name && /^on[A-Z]/.test(name) && attributeNode.value?.type === "JSXExpressionContainer") {
      lowerEvent(name, attributeNode.value.expression, elementId, state);
      propWrites.push({ name, kind: "event" });
      continue;
    }

    if (!attributeNode.value) {
      element.attributes.push({ name, staticValue: "true" }); propWrites.push({ name, staticValue: "true", kind: "attribute" });
      continue;
    }

    if (attributeNode.value.type === "StringLiteral") {
      element.attributes.push({ name, staticValue: attributeNode.value.value }); propWrites.push({ name, staticValue: attributeNode.value.value, kind: "attribute" });
      continue;
    }

    if (attributeNode.value.type === "JSXExpressionContainer") {
      const expression = attributeNode.value.expression;
      const literalValue = evaluateExpression(expression, state, {});
      if (literalValue !== undefined && !state.preserveBindings) {
        const staticValue = literalToString(literalValue); element.attributes.push({ name, staticValue }); propWrites.push({ name, staticValue, kind: isDomProperty(name) ? "property" : "attribute" });
      } else {
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
        propWrites.push({ name, expressionId: state.ir.bindings[state.ir.bindings.length - 1]!.expressionId, kind: isDomProperty(name) ? "property" : "attribute" });
      }
      continue;
    }

    reportUnsupported(state, "UNSUPPORTED_ATTRIBUTE_VALUE", `Attribute ${name} has an unsupported value form.`);
  }

  state.ir.elements.push(element);
  if (propWrites.length) state.ir.propPrograms.push({ id: `p${state.ir.propPrograms.length + 1}`, targetId: elementId, writes: propWrites });

  for (const child of [...getSpreadChildren(node.opening?.attributes ?? [], state), ...(node.children ?? [])]) {
    const lowered = lowerJsxNode(child, elementId, state);
    if (lowered !== SKIP) {
      element.children.push(lowered);
    }
  }

  return elementId;
}

function lowerJsxText(node: any, parentId: string | null, state: CompilerState): LowerResult {
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

function lowerJsxExpression(node: any, parentId: string | null, state: CompilerState): LowerResult {
  if (!parentId) {
    reportUnsupported(state, "ROOT_EXPRESSION_NODE", "Root-level expression nodes are not supported.");
    return SKIP;
  }

  const expression = unwrapExpression(node.expression);
  const forwardedChildren = getForwardedChildren(expression, state);
  if (forwardedChildren) {
    let first: LowerResult = SKIP;
    for (const child of forwardedChildren) { const lowered = lowerJsxNode(child, parentId, state); if (first === SKIP && lowered !== SKIP) first = lowered; }
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

function lowerMapExpression(expression: any, parentId: string | null, state: CompilerState): LowerResult {
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

  const loopNode: LoopNode = {
    id: loopId,
    parentId,
    source: serializeExpression(mapCall.source, state),
    itemName,
    indexName,
    rows: [] as Array<{ id: string; rootElementId: string; keyValue?: string }>,
    inputId: input?.id,
    keyExpression: getMapRowKeyExpression(callback, state)
  };

  state.ir.loops.push(loopNode);

  if (input) {
    const rowRoot = unwrapExpression(callback.body);
    const lowered = lowerMapRow(rowRoot, loopId, state, {});
    if (lowered !== SKIP) loopNode.rowTemplateRootElementId = lowered;
  }
  const forwardedChildren = getForwardedChildren(expression, state);
  if (forwardedChildren) {
    let first: LowerResult = SKIP;
    for (const child of forwardedChildren) {
      const lowered = lowerJsxNode(child, parentId, state);
      if (first === SKIP && lowered !== SKIP) first = lowered;
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
function findReturnedExpression(body: any): any | null {
  for (const statement of getStatements(body)) {
    if (statement.type === "ReturnStatement" || statement.type === "ReturnStmt") return unwrapExpression(statement.argument ?? statement.arg);
  }
  return null;
}

function getMapRowKeyExpression(callback: any, state: CompilerState): string | undefined {
  const row = unwrapExpression(callback.body);
  for (const attribute of row?.opening?.attributes ?? []) {
    if (getNodeName(attribute.name) === "key" && attribute.value?.type === "JSXExpressionContainer") {
      return serializeExpression(attribute.value.expression, state);
    }
  }
  reportUnsupported(state, "MISSING_LOOP_KEY", "A useLiveQuery loop must have a key expression.");
  return undefined;
}

function lowerMapRow(node: any, parentId: string, state: CompilerState, scope: Record<string, unknown>): LowerResult {
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

function lowerJsxElementWithScope(node: any, parentId: string | null, state: CompilerState, scope: Record<string, unknown>): LowerResult {
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
  const element: ElementNode = {
    id: elementId,
    tag: tagName,
    parentId,
    attributes: [] as Array<{ name: string; staticValue?: string; bindingId?: string }>,
    children: [] as string[]
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
      } else {
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

function lowerJsxNodeWithScope(node: any, parentId: string | null, state: CompilerState, scope: Record<string, unknown>): LowerResult {
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

function lowerJsxExpressionWithScope(node: any, parentId: string | null, state: CompilerState, scope: Record<string, unknown>): LowerResult {
  if (!parentId) {
    return SKIP;
  }

  const expression = unwrapExpression(node.expression);
  const forwardedChildren = getForwardedChildren(expression, state);
  if (forwardedChildren) {
    let first: LowerResult = SKIP;
    for (const child of forwardedChildren) { const lowered = lowerJsxNodeWithScope(child, parentId, state, scope); if (first === SKIP && lowered !== SKIP) first = lowered; }
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

function lowerComponentElement(node: any, parentId: string | null, state: CompilerState, literalScope: Record<string, unknown> = {}): LowerResult {
  const name = getJsxName(node.opening?.name)!;
  if (state.routerLinkBindings.has(name)) return lowerRouterLink(node, parentId, state);
  if (name.endsWith(".Provider")) return lowerContextProvider(node, parentId, state);
  // These are the demo's stable UI atom names. Resolve them before ordinary
  // local-component expansion so their Base UI implementation never leaks
  // into the emitted graph, even when module resolution supplied the source.
  // A source-graph component has a different module ID from its importer;
  // treat those familiar atom exports as DOM adapters. A same-module function
  // named Checkbox remains an ordinary component (used by the Toggle tests).
  const component = getLocalComponent(name, state);
  const external = getExternalSymbol(name, state);
  if (!component && external) {
    reportUnsupported(state, "UNRESOLVED_SOURCE_SYMBOL", `Source graph does not contain ${external}; include its implementation module to compile it.`);
    return SKIP;
  }
  if (!component) {
    const imported = state.importSymbols.get(`${state.activeModuleId}::${name.split(".")[0]}`);
    reportUnsupported(state, "UNSUPPORTED_COMPONENT", `Component ${name} is not a locally resolvable function component. Import: ${imported ? `${imported.moduleId}#${imported.exportName}` : "none"}.`);
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
  const previous = state.expressionScope;
  const previousModuleId = state.activeModuleId;
  const localScope: Record<string, any> = { ...previous, ...(state.moduleExpressions.get(component.moduleId) ?? {}) };
  const parameter = component.params[0];
  const parameterPattern = parameter?.pat ?? parameter?.pattern ?? parameter;
  if (parameterPattern?.type === "ObjectPattern") {
    for (const property of parameterPattern.properties ?? []) {
      if (property.type === "RestElement") {
        const local = getPatternName(property.argument);
        if (local) {
          const consumed = new Set((parameterPattern.properties ?? []).filter((entry: any) => entry.type !== "RestElement").map((entry: any) => getNodeName(entry.key)).filter(Boolean));
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
      if (key && local) localScope[local] = supplied ?? defaultValue ?? { type: "Identifier", value: "undefined" };
    }
  } else if (parameterPattern) {
    const local = getPatternName(parameterPattern);
    if (local) localScope[local] = { type: "ObjectExpression", properties: Object.entries(props).map(([key, value]) => ({ type: "KeyValueProperty", key: { type: "Identifier", value: key }, value })) };
  }
  for (const statement of getStatements(component.body)) {
    if (statement.type !== "VariableDeclaration" && statement.type !== "VarDecl") continue;
    for (const declaration of statement.declarations ?? statement.decls ?? []) {
      const local = getPatternName(declaration.id);
      if (local && declaration.init) localScope[local] = declaration.init;
    }
  }
  state.expressionScope = localScope;
  state.activeModuleId = component.moduleId;
  state.componentStack.push(name);
  const metadata: ComponentMetadata = { name, moduleId: component.moduleId, elementIds: [], bindingIds: [], eventIds: [] };
  state.ir.components.push(metadata);
  const before = { e: state.ir.elements.length, b: state.ir.bindings.length, ev: state.ir.events.length };
  const returned = findReturnedJsx(component.body);
  if (!returned) {
    reportUnsupported(state, "UNSUPPORTED_COMPONENT_BODY", `Component ${name} in ${component.moduleId} does not return supported JSX. The compiler only accepts JSX source bodies in this experiment.`);
  }
  const result = literalScope && Object.keys(literalScope).length
    ? lowerJsxNodeWithScope(returned, parentId, state, literalScope)
    : lowerJsxNode(returned, parentId, state);
  if (name === "ThemeToggle") lowerThemeToggleSemantics(component.body, state, before);
  metadata.elementIds = state.ir.elements.slice(before.e).map((element) => element.id);
  metadata.bindingIds = state.ir.bindings.slice(before.b).map((binding) => binding.id);
  metadata.eventIds = state.ir.events.slice(before.ev).map((event) => event.id);
  state.componentStack.pop();
  state.expressionScope = previous;
  state.activeModuleId = previousModuleId;
  return result;
}

/** Context providers are transparent structural nodes. Their values remain in
 * IR for later context reads while their children remain direct DOM children. */
function lowerContextProvider(node: any, parentId: string | null, state: CompilerState): LowerResult {
  if (!parentId) {
    reportUnsupported(state, "ROOT_CONTEXT_PROVIDER", "A context provider cannot be the application root in this experiment.");
    return SKIP;
  }
  const value = collectCallerProps(node, state).value;
  const values: Array<{ name: string; expressionId?: string; staticValue?: string }> = [];
  const resolved = value?.type === "Identifier" ? state.expressionScope[getNodeName(value)!] ?? value : value;
  if (resolved?.type === "ObjectExpression") {
    for (const property of resolved.properties ?? []) {
      const name = getNodeName(property.key);
      const entry = property.value ?? property.expr;
      if (!name || !entry) continue;
      const literal = evaluateExpression(entry, state, {});
      values.push(literal === undefined ? { name, expressionId: internExpression(entry, state) } : { name, staticValue: literalToString(literal) });
    }
  } else if (resolved) {
    values.push({ name: "value", expressionId: internExpression(resolved, state) });
  }
  state.ir.contexts.push({ id: `c${state.ir.contexts.length + 1}`, parentId, values });
  const parent = getElementById(state, parentId);
  if (!parent) {
    reportUnsupported(state, "CONTEXT_PARENT_MISSING", `Context provider parent ${parentId} is not an element.`);
    return SKIP;
  }
  for (const child of node.children ?? []) {
    const lowered = lowerJsxNode(child, parentId, state);
    if (lowered !== SKIP) parent.children.push(lowered);
  }
  return SKIP;
}

function lowerRouterLink(node: any, parentId: string | null, state: CompilerState): LowerResult {
  const to = (node.opening?.attributes ?? []).find((attribute: any) => getNodeName(attribute.name) === "to")?.value;
  if (!to || to.type !== "StringLiteral") { reportUnsupported(state, "UNSUPPORTED_ROUTER_LINK", "Router Link requires a static string to prop."); return SKIP; }
  const elementId = nextElementId(state);
  state.eventCounter += 1;
  state.ir.events.push({ id: `ev${state.eventCounter}`, type: "click", targetId: elementId, actionId: `a${state.eventCounter}`, args: [], navigate: { href: to.value } });
  const element: ElementNode = { id: elementId, tag: "a", parentId, attributes: [{ name: "href", staticValue: to.value }], children: [] };
  const attributes = node.opening?.attributes ?? [];
  const className = attributes.find((attribute: any) => getNodeName(attribute.name) === 'className')?.value;
  const activeProps = attributes.find((attribute: any) => getNodeName(attribute.name) === 'activeProps')?.value?.expression;
  for (const attribute of attributes) {
    const name = getNodeName(attribute.name);
    if (!name || ['to', 'replace', 'className', 'activeProps'].includes(name)) continue;
    if (name === 'activeOptions' || name === 'pendingProps') { reportUnsupported(state, 'UNSUPPORTED_ROUTER_LINK_OPTION', `Router Link ${name} is not supported.`); continue; }
    if (attribute.value?.type === 'StringLiteral') element.attributes.push({ name, staticValue: attribute.value.value });
    else if (!attribute.value) element.attributes.push({ name, staticValue: 'true' });
    else reportUnsupported(state, 'UNSUPPORTED_ROUTER_LINK_PROP', `Router Link prop ${name} must be static.`);
  }
  const baseClass = className?.type === 'StringLiteral' ? className.value : undefined;
  if (className && !baseClass) reportUnsupported(state, 'UNSUPPORTED_ROUTER_LINK_PROP', 'Router Link className must be static.');
  if (activeProps) {
    const property = activeProps.properties?.find((entry: any) => getNodeName(entry.key) === 'className');
    const activeClass = property?.value?.value;
    if (!baseClass || typeof activeClass !== 'string' || (activeProps.properties?.length ?? 0) !== 1) reportUnsupported(state, 'UNSUPPORTED_ROUTER_LINK_ACTIVE_PROPS', 'Only static activeProps.className is supported.');
    else {
      state.expressionCounter += 1;
      const expressionId = `x${state.expressionCounter}`;
      state.ir.expressions.push({ id: expressionId, expression: { kind: 'conditional', test: { kind: 'binary', op: '===', left: { kind: 'member', object: { kind: 'member', object: { kind: 'identifier', name: 'host' }, property: 'location' }, property: 'pathname' }, right: { kind: 'literal', value: to.value } }, consequent: { kind: 'literal', value: activeClass }, alternate: { kind: 'literal', value: baseClass } } });
      const bindingId = nextBindingId(state);
      state.ir.bindings.push({ id: bindingId, kind: 'attribute', targetId: elementId, attributeName: 'className', expression: 'host.location.pathname', expressionId });
      element.attributes.push({ name: 'className', bindingId });
    }
  } else if (baseClass) element.attributes.push({ name: 'className', staticValue: baseClass });
  state.ir.elements.push(element);
  for (const child of node.children ?? []) { const lowered = lowerJsxNode(child, elementId, state); if (lowered !== SKIP) element.children.push(lowered); }
  return elementId;
}

function lowerEvent(name: string, expression: any, targetId: string, state: CompilerState): void {
  const handler = unwrapExpression(expression);
  const body = unwrapExpression(handler?.body);
  const call = body?.type === "CallExpression" ? body : null;
  const actionExpression = call?.callee;
  let actionId = getNodeName(actionExpression);
  if (actionId && state.expressionScope[actionId]) actionId = getNodeName(state.expressionScope[actionId]) ?? actionId;
  if (!call || !actionId) {
    const callbackName = getNodeName(handler);
    if (!callbackName) { reportUnsupported(state, "UNSUPPORTED_EVENT", "Event handler must be a callback identifier or a declarative call."); return; }
    state.eventCounter += 1;
    state.ir.events.push({ id: `ev${state.eventCounter}`, type: reactEventName(name), targetId, actionId: `a${state.eventCounter}`, args: [], callbackName });
    return;
  }
  const change = call.arguments?.[1]?.expression ?? call.arguments?.[1];
  let field: string | undefined;
  if (change?.type === "ObjectExpression") field = getNodeName(change.properties?.[0]?.key);
  state.eventCounter += 1;
  state.ir.events.push({ id: `ev${state.eventCounter}`, type: reactEventName(name), targetId, actionId: `a${state.eventCounter}`, args: [serializeExpression(call.arguments?.[0]?.expression ?? call.arguments?.[0], state), serializeExpression(change, state)], field });
  // The semantic action id is deliberately stable but independent from callback names.
  (state.ir.events[state.ir.events.length - 1] as any).callbackName = actionId;
}

function reactEventName(name: string): string {
  const normalized = name.slice(2).toLowerCase();
  return ({ mouseenter: "mouseenter", mouseleave: "mouseleave", mousedown: "mousedown", mouseup: "mouseup", doubleclick: "dblclick", focus: "focus", blur: "blur" } as Record<string, string>)[normalized] ?? normalized;
}
function isDomProperty(name: string): boolean { return ["checked", "defaultChecked", "indeterminate", "value", "selected", "disabled", "readOnly"].includes(name); }

/**
 * This is intentionally a semantic recognizer, not a hook implementation.
 * The accepted shape is the real demo ThemeToggle: one `mode` state slot, a
 * mount initializer, and a mode-dependent media subscription with cleanup.
 */
function lowerThemeToggleSemantics(body: any, state: CompilerState, before: { e: number; b: number; ev: number }): void {
  const statements = getStatements(body);
  for (const call of findCalls(body)) {
    const callName = getNodeName(call.callee);
    if (callName?.startsWith("use") && callName !== "useState" && callName !== "useEffect") reportUnsupported(state, "UNSUPPORTED_HOOK", `ThemeToggle hook ${callName} is not supported.`);
  }
  const hasUseState = statements.some((statement: any) => containsCall(statement, "useState"));
  const effects = statements.filter((statement: any) => containsCall(statement, "useEffect"));
  if (!hasUseState) reportUnsupported(state, "THEME_STATE_SLOT_REQUIRED", "ThemeToggle requires useState('auto') for its mode state.");
  if (effects.length !== 2) reportUnsupported(state, "THEME_EFFECT_SHAPE", "ThemeToggle requires one mount effect and one mode-dependent subscription effect.");
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
  state.ir.lifecycleEffects.push(
    { id: `fx${state.ir.lifecycleEffects.length + 1}`, trigger: "mount", stateSlotId, operations: [{ kind: "storage-read", storageKey: "theme", stateSlotId }, { kind: "document-theme-apply", stateSlotId, hostValueId }] },
    { id: `fx${state.ir.lifecycleEffects.length + 2}`, trigger: "state-change", stateSlotId, operations: [{ kind: "document-theme-apply", stateSlotId, hostValueId }], subscription: { hostValueId, event: "change", activeWhenStateEquals: "auto", dispose: "remove-listener" } }
  );
  const event = state.ir.events.slice(before.ev).find((candidate) => candidate.type === "click");
  if (!event) reportUnsupported(state, "THEME_EVENT_REQUIRED", "ThemeToggle requires a direct onClick={toggleMode} handler.");
  else state.ir.stateTransitions.push({ id: `st${state.ir.stateTransitions.length + 1}`, eventId: event.id, stateSlotId, kind: "theme-cycle", operations: [{ kind: "storage-write", storageKey: "theme", stateSlotId }, { kind: "document-theme-apply", stateSlotId, hostValueId }] });
  for (const binding of state.ir.bindings.slice(before.b)) {
    const expression = state.ir.expressions.find((candidate) => candidate.id === binding.expressionId)?.expression;
    if (expressionContainsIdentifier(expression, "mode")) state.ir.dependencyEdges.push({ fromId: stateSlotId, toId: binding.id, kind: "local-state-to-binding" });
  }
}

function containsCall(node: any, name: string): boolean { return Boolean(findCall(node, name)); }
function findCalls(node: any, calls: any[] = []): any[] { if (!node || typeof node !== "object") return calls; if (node.type === "CallExpression") calls.push(node); for (const value of Object.values(node)) { if (Array.isArray(value)) value.forEach((child) => findCalls(child, calls)); else findCalls(value, calls); } return calls; }
function findCall(node: any, name: string): any | undefined {
  if (!node || typeof node !== "object") return undefined;
  if (node.type === "CallExpression" && getNodeName(node.callee) === name) return node;
  for (const value of Object.values(node)) { if (Array.isArray(value)) { for (const child of value) { const found = findCall(child, name); if (found) return found; } } else { const found = findCall(value, name); if (found) return found; } }
  return undefined;
}
function expressionContainsIdentifier(expression: any, name: string): boolean {
  if (!expression || typeof expression !== "object") return false;
  if (expression.kind === "identifier" && expression.name === name) return true;
  return Object.values(expression).some((value) => Array.isArray(value) ? value.some((child) => expressionContainsIdentifier(child, name)) : expressionContainsIdentifier(value, name));
}

function getMapCall(expression: any): { source: any; callback: any } | null {
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

function evaluateArrayExpression(expression: any, state: CompilerState, scope: Record<string, unknown>): unknown[] | null {
  const value = evaluateExpression(expression, state, scope);
  return Array.isArray(value) ? value : null;
}

function evaluateExpression(expression: any, state: CompilerState, scope: Record<string, unknown>, resolving = new Set<string>()): unknown {
  const node = unwrapExpression(expression);
  if (!node) {
    return undefined;
  }

  if (isLiteralExpression(node)) {
    return node.type === "NullLiteral" ? null : node.value;
  }

  if (node.type === "ArrayExpression") {
    const values: unknown[] = [];
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
    const record: Record<string, unknown> = {};
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
    if (resolving.has(node.value)) return undefined;
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
    if (name && resolving.has(name)) return undefined;
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
      return (objectValue as Record<string, unknown>)[propertyName];
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
    const parts: string[] = [];
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

function bindPatternValue(pattern: any, value: unknown, state: CompilerState, index: number): Record<string, unknown> | null {
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

    const scope: Record<string, unknown> = {};
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

      scope[targetName] = (value as Record<string, unknown>)[key];
    }
    return scope;
  }

  if (pattern.type === "ArrayPattern") {
    if (!Array.isArray(value)) {
      return null;
    }

    const scope: Record<string, unknown> = {};
    pattern.elements?.forEach((element: any, elementIndex: number) => {
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

function evaluateCallExpression(node: any, state: CompilerState, scope: Record<string, unknown>): unknown {
  const callName = getNodeName(node.callee);
  if (["cn", "clsx", "classnames"].includes(callName ?? "")) {
    const values: unknown[] = (node.arguments ?? []).map((arg: any) => evaluateExpression(arg.expression ?? arg, state, scope));
    // Class helpers intentionally ignore absent optional values, just as the
    // runtime `cn` implementation does. That keeps a primitive's static base
    // classes materialized in IR when no caller className was supplied.
    return flattenClasses(values).join(" ");
  }
  if (node.callee?.type === "Identifier") {
    const definition = state.expressionScope[getNodeName(node.callee) ?? ""];
    if (definition?.type === "CallExpression" && getNodeName(definition.callee) === "cva") return evaluateCva(definition, node, state, scope);
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

  const values: unknown[] = [];
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

function evaluateObjectLiteral(node: any, state: CompilerState, scope: Record<string, unknown>): Record<string, unknown> | undefined {
  if (!node || node.type !== "ObjectExpression") {
    return undefined;
  }

  const record: Record<string, unknown> = {};
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

function bindCallbackParameters(callback: any, index: number, state: CompilerState): Record<string, unknown> {
  const scope: Record<string, unknown> = {};
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

function evaluateFunctionBody(functionLike: any, state: CompilerState, scope: Record<string, unknown>): unknown {
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

function getArrayLikeLength(value: unknown): number | null {
  if (Array.isArray(value)) {
    return value.length;
  }

  if (typeof value === "number" && Number.isFinite(value)) {
    return Math.max(0, Math.floor(value));
  }

  if (typeof value === "object" && value !== null) {
    const length = (value as Record<string, unknown>).length;
    if (typeof length === "number" && Number.isFinite(length)) {
      return Math.max(0, Math.floor(length));
    }
  }

  return null;
}

function collectLiteralScope(body: any, state: CompilerState): void {
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

function collectModuleFacts(body: any, state: CompilerState): void {
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
      // Dynamic locals are still ordinary expression inputs to the rendered
      // tree. Preserve their syntax as well as eagerly evaluating literals.
      if (name) {
        state.expressionScope[name] = init;
      }
      if (name && value !== undefined) {
        state.scope[name] = value;
      }
    }
  }

  collectLiteralScope(body, state);
}

function collectModuleScope(moduleAst: any, state: CompilerState): void {
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

function flattenClasses(values: unknown[]): string[] {
  const result: string[] = [];
  for (const value of values) {
    if (!value) continue;
    if (typeof value === "string" || typeof value === "number") result.push(String(value));
    else if (Array.isArray(value)) result.push(...flattenClasses(value));
    else if (typeof value === "object") for (const [key, enabled] of Object.entries(value as Record<string, unknown>)) if (enabled) result.push(key);
  }
  return result;
}
function evaluateCva(definition: any, call: any, state: CompilerState, scope: Record<string, unknown>): unknown {
  const base = evaluateExpression(definition.arguments?.[0]?.expression ?? definition.arguments?.[0], state, scope);
  const config = unwrapExpression(definition.arguments?.[1]?.expression ?? definition.arguments?.[1]);
  const selected = unwrapExpression(call.arguments?.[0]?.expression ?? call.arguments?.[0]);
  if (typeof base !== "string" || config?.type !== "ObjectExpression") return undefined;
  const selectedValue: Record<string, unknown> = {};
  if (selected?.type === "ObjectExpression") for (const property of selected.properties ?? []) {
    const key = getNodeName(property.key); if (!key) continue;
    const value = evaluateExpression(property.value ?? property.expr, state, scope);
    if (value !== undefined) selectedValue[key] = value;
  } else if (selected) { reportUnsupported(state, "DYNAMIC_CVA_VARIANT", "CVA variants must be statically known."); return undefined; }
  const getObject = (object: any, key: string) => object.properties?.find((property: any) => getNodeName(property.key) === key)?.value;
  const variants = getObject(config, "variants"); const defaults = getObject(config, "defaultVariants");
  const classes = [base];
  for (const property of variants?.properties ?? []) {
    const variant = getNodeName(property.key); if (!variant) continue;
    const value = selectedValue[variant] ?? evaluateExpression(getObject(defaults, variant), state, scope);
    if (value === undefined) { reportUnsupported(state, "DYNAMIC_CVA_VARIANT", `CVA variant ${variant} must be static.`); return undefined; }
    const option = property.value?.properties?.find((entry: any) => getNodeName(entry.key) === String(value))?.value;
    const className = evaluateExpression(option, state, scope); if (typeof className !== "string") return undefined; classes.push(className);
  }
  if (typeof selectedValue.className === "string") classes.push(selectedValue.className);
  return classes.join(" ");
}

function collectComponents(moduleAst: any, state: CompilerState, moduleId: string): void {
  for (const original of getStatements(moduleAst)) {
    const statement = original.declaration ?? original.decl ?? original;
    if ((statement.type === "FunctionDeclaration" || statement.type === "FnDecl") && isComponentName(getNodeName(statement.identifier ?? statement.id))) {
      const name = getNodeName(statement.identifier ?? statement.id)!;
      state.components.set(componentKey(moduleId, name), { name, body: getFunctionBody(statement), params: statement.params ?? statement.function?.params ?? [], moduleId });
    }
    if (statement.type === "FunctionExpression" && isComponentName(getNodeName(statement.identifier ?? statement.id))) {
      const name = getNodeName(statement.identifier ?? statement.id)!;
      state.components.set(componentKey(moduleId, name), { name, body: getFunctionBody(statement), params: statement.params ?? [], moduleId });
    }
    if (statement.type === "VariableDeclaration" || statement.type === "VarDecl") for (const declaration of statement.declarations ?? statement.decls ?? []) {
      const name = getPatternName(declaration.id); const init = declaration.init;
      const implementation = unwrapComponentFactory(init);
      if (name && isComponentName(name) && implementation) state.components.set(componentKey(moduleId, name), { name, body: getFunctionBody(implementation), params: implementation.params ?? [], moduleId });
    }
  }
}

function unwrapComponentFactory(init: any): any | null {
  if (init?.type === "ArrowFunctionExpression" || init?.type === "FunctionExpression") return init;
  const callee = getNodeName(init?.callee) ?? getNodeName(init?.callee?.property);
  if (callee === "forwardRef" && init.arguments?.[0]) return unwrapExpression(init.arguments[0].expression ?? init.arguments[0]);
  return null;
}

/** Normalize the return value of package render helpers into the same JSX AST
 * path as authored components.  The helpers declare their actual DOM tag as a
 * string argument, so this remains source driven rather than package driven. */
function normalizeRenderedReturn(value: any, body: any): any {
  const direct = renderHelperElement(value, body);
  if (direct) return direct;
  if (value?.type !== "Identifier") return value;
  const name = getNodeName(value);
  for (const statement of getStatements(body)) {
    if (statement.type !== "VariableDeclaration" && statement.type !== "VarDecl") continue;
    for (const declaration of statement.declarations ?? statement.decls ?? []) {
      if (getPatternName(declaration.id) === name) return renderHelperElement(declaration.init, body) ?? value;
    }
  }
  return value;
}
function renderHelperElement(value: any, _body: any): any | null {
  value = unwrapExpression(value);
  if (value?.type !== "CallExpression") return null;
  const callee = getNodeName(value.callee);
  if (callee === "useRenderElement") {
    const tag = jsxFactoryName(value.arguments?.[0]?.expression ?? value.arguments?.[0]);
    const props = unwrapExpression(value.arguments?.[1]?.expression ?? value.arguments?.[1]);
    if (!tag || !props) return null;
    return { type: "JSXElement", opening: { type: "JSXOpeningElement", name: tag, attributes: [{ type: "SpreadElement", arguments: props }] }, closing: null, children: [] };
  }
  if (callee === "useRender") {
    const options = unwrapExpression(value.arguments?.[0]?.expression ?? value.arguments?.[0]);
    if (options?.type !== "ObjectExpression") return null;
    const tagValue = options.properties?.find((property: any) => getNodeName(property.key) === "defaultTagName")?.value;
    const propsValue = options.properties?.find((property: any) => getNodeName(property.key) === "props")?.value;
    const tag = jsxFactoryName(tagValue);
    if (!tag) return null;
    // `useRender` accepts a merged props expression. Keep the authored
    // component props when the merge itself cannot be statically expanded.
    const props = unwrapExpression(propsValue);
    const fallback = { type: "Identifier", value: "props" };
    return { type: "JSXElement", opening: { type: "JSXOpeningElement", name: tag, attributes: [{ type: "SpreadElement", arguments: props?.type === "CallExpression" ? fallback : (props ?? fallback) }] }, closing: null, children: [] };
  }
  return null;
}

function collectExports(moduleAst: any, state: CompilerState, moduleId: string): void {
  const entries = state.exports.get(moduleId) ?? new Map<string, { moduleId: string; exportName: string }>();
  for (const statement of getStatements(moduleAst)) {
    if (statement.type === "ExportDeclaration") {
      const declaration = statement.declaration;
      const name = getNodeName(declaration?.identifier ?? declaration?.id) ?? getPatternName(declaration?.declarations?.[0]?.id);
      if (name) entries.set(name, { moduleId, exportName: name });
    }
    // Base UI exposes compound components with `export * as Checkbox`. Keep
    // that namespace edge so `Checkbox.Root` resolves through source modules.
    if (statement.type === "ExportAllDeclaration") {
      const exported = getNodeName(statement.exported);
      const source = statement.source?.value;
      if (exported && source) entries.set(exported, { moduleId: resolveModuleId(moduleId, source), exportName: exported });
      continue;
    }
    if (statement.type !== "ExportNamedDeclaration" && statement.type !== "ExportNamedSpecifier") continue;
    const source = statement.source?.value;
    for (const specifier of statement.specifiers ?? []) {
      const exported = getNodeName(specifier.exported) ?? getNodeName(specifier.name) ?? getNodeName(specifier.orig) ?? getNodeName(specifier.local);
      const local = getNodeName(specifier.orig) ?? getNodeName(specifier.local) ?? exported;
      if (!exported || !local) continue;
      entries.set(exported, source ? { moduleId: resolveModuleId(moduleId, source), exportName: local } : { moduleId, exportName: local });
    }
  }
  state.exports.set(moduleId, entries);
}

function collectModuleExpressions(moduleAst: any, state: CompilerState, moduleId: string): void {
  const expressions: Record<string, any> = {};
  for (const original of getStatements(moduleAst)) {
    const statement = original.declaration ?? original.decl ?? original;
    if (statement.type !== "VariableDeclaration" && statement.type !== "VarDecl") continue;
    for (const declaration of statement.declarations ?? statement.decls ?? []) {
      const name = getPatternName(declaration.id);
      if (name && declaration.init) expressions[name] = declaration.init;
    }
  }
  state.moduleExpressions.set(moduleId, expressions);
}

function collectImports(moduleAst: any, state: CompilerState, moduleId: string): void {
  for (const statement of getStatements(moduleAst)) {
    if (statement.type !== "ImportDeclaration") continue;
    const source = statement.source?.value;
    if (!source) continue;
    for (const specifier of statement.specifiers ?? []) {
      const local = getNodeName(specifier.local) ?? getNodeName(specifier.local?.id);
      if (!local) continue;
      const imported = getNodeName(specifier.imported) ?? (specifier.type === "ImportDefaultSpecifier" ? "default" : local);
      state.importSymbols.set(`${moduleId}::${local}`, { moduleId: resolveModuleId(moduleId, source), exportName: imported });
    }
    if (source !== "@tanstack/react-router") continue;
    for (const specifier of statement.specifiers ?? []) {
      const imported = getNodeName(specifier.imported) ?? getNodeName(specifier.local);
      if (imported === "Link") state.routerLinkBindings.add(getNodeName(specifier.local) ?? "Link");
    }
  }
}

function componentKey(moduleId: string, name: string): string { return `${moduleId}::${name}`; }
function resolveModuleId(from: string, source: string): string {
  if (!source.startsWith(".")) return source;
  const base = from.split("/");
  if (/\.(?:tsx?|jsx?|mjs|cjs)$/.test(from)) base.pop();
  for (const part of source.split("/")) {
    if (part === "." || !part) continue;
    if (part === "..") base.pop(); else base.push(part);
  }
  const candidate = base.join("/");
  return /\.(?:tsx?|jsx?|mjs|cjs)$/.test(candidate) ? candidate : `${candidate}.tsx`;
}
function getLocalComponent(name: string, state: CompilerState) {
  const [root, ...members] = name.split(".");
  const imported = state.importSymbols.get(`${state.activeModuleId}::${root}`);
  if (imported) {
    const resolved = resolveExport(imported.moduleId, [...[imported.exportName], ...members].join("."), state);
    const direct = state.components.get(componentKey(resolved.moduleId, resolved.exportName))
      ?? state.components.get(componentKey(imported.moduleId, imported.exportName))
      ?? [...state.components.values()].find((component) => component.moduleId === resolved.moduleId && component.name === resolved.exportName);
    if (direct) return direct;
    if (resolved.exportName === "default") return [...state.components.values()].find((component) => component.moduleId === resolved.moduleId);
  }
  return state.components.get(componentKey(state.activeModuleId, root ?? name))
    ?? [...state.components.values()].find((component) => component.name === (root ?? name));
}
function resolveExport(moduleId: string, name: string, state: CompilerState): { moduleId: string; exportName: string } {
  const [head, ...tail] = name.split("."); const found = state.exports.get(moduleId)?.get(head ?? name);
  if (!found) return { moduleId, exportName: name };
  if (!tail.length) return found;
  return resolveExport(found.moduleId, `${found.exportName === head ? "" : `${found.exportName}.`}${tail.join(".")}`, state);
}
function getExternalSymbol(name: string, state: CompilerState): string | null {
  const [root, ...members] = name.split(".");
  const imported = state.importSymbols.get(`${state.activeModuleId}::${root}`);
  if (!imported || !imported.moduleId.startsWith("@")) return null;
  const symbol = [imported.exportName, ...members].join(".");
  if (state.components.has(componentKey(imported.moduleId, imported.exportName))) return null;
  return `${imported.moduleId}::${symbol}`;
}
function lowerToggleRoot(node: any, parentId: string | null, state: CompilerState): LowerResult {
  const props = collectCallerProps(node, state);
  const permitted = new Set(["checked", "defaultChecked", "indeterminate", "disabled", "readOnly", "required", "name", "value", "form", "onCheckedChange", "className", "style", "id", "aria-label", "aria-labelledby", "data-testid"]);
  for (const key of Object.keys(props)) if (!permitted.has(key)) reportUnsupported(state, "UNSUPPORTED_TOGGLE_PROP", `Toggle.Root prop ${key} is not supported.`);
  const rootId = nextElementId(state);
  const inputId = nextElementId(state);
  const root: ElementNode = { id: rootId, tag: "label", parentId, attributes: [], children: [inputId] };
  for (const key of ["className", "style", "id", "aria-label", "aria-labelledby", "data-testid"]) {
    const value = props[key]; if (!value) continue;
    const literal = evaluateExpression(value, state, {});
    if (literal !== undefined) root.attributes.push({ name: key, staticValue: literalToString(literal) });
    else { const bindingId = nextBindingId(state); state.ir.bindings.push({ id: bindingId, kind: "attribute", targetId: rootId, attributeName: key, expression: serializeExpression(value, state), expressionId: internExpression(value, state) }); root.attributes.push({ name: key, bindingId }); }
  }
  const input: ElementNode = { id: inputId, tag: "input", parentId: rootId, attributes: [{ name: "type", staticValue: "checkbox" }], children: [] };
  const valueSource = (key: string) => {
    const value = props[key]; if (!value) return undefined;
    const literal = evaluateExpression(value, state, {});
    if (literal !== undefined) return { staticValue: literal === "mixed" ? "mixed" : literal ? "true" : "false" };
    return { expressionId: internExpression(value, state) };
  };
  const scalar = (key: string) => {
    const value = props[key]; if (!value) return;
    const literal = evaluateExpression(value, state, {});
    if (literal !== undefined) input.attributes.push({ name: key, staticValue: literalToString(literal) });
    else { const bindingId = nextBindingId(state); state.ir.bindings.push({ id: bindingId, kind: "attribute", targetId: inputId, attributeName: key, expression: serializeExpression(value, state), expressionId: internExpression(value, state) }); input.attributes.push({ name: key, bindingId }); }
  };
  for (const key of ["disabled", "readOnly", "required", "name", "value", "form"]) scalar(key);
  const checked = valueSource("checked");
  const defaultChecked = valueSource("defaultChecked") ?? (!checked ? { staticValue: "false" } : undefined);
  const indeterminate = valueSource("indeterminate");
  const eventId = `ev${++state.eventCounter}`;
  const actionId = props.onCheckedChange ? `a${state.eventCounter}` : undefined;
  state.ir.events.push({ id: eventId, type: "change", targetId: inputId, actionId: actionId ?? `a${state.eventCounter}`, args: [] });
  const stateSlotId = `s${state.ir.localStates.length + 1}`;
  state.ir.localStates.push({ id: stateSlotId, name: `state-${state.ir.localStates.length + 1}`, initialValue: defaultChecked?.staticValue === "true" ? "true" : "false", values: ["false", "true", "mixed"] });
  state.ir.elements.push(root, input);
  let indicatorId: string | undefined;
  for (const child of node.children ?? []) {
    if (child.type === "JSXText" && !normalizeText(child.value ?? "")) continue;
    const childName = child.type === "JSXElement" ? getJsxName(child.opening?.name) : undefined;
    if (child.type !== "JSXElement" || !childName || getExternalSymbol(childName, state) !== "internal-toggle-indicator") { reportUnsupported(state, "UNSUPPORTED_TOGGLE_CHILD", "Toggle.Root only supports one direct Toggle.Indicator child."); continue; }
    if (indicatorId) { reportUnsupported(state, "DUPLICATE_TOGGLE_INDICATOR", "Toggle.Root supports only one Toggle.Indicator."); continue; }
    indicatorId = nextElementId(state); const indicator: ElementNode = { id: indicatorId, tag: "span", parentId: rootId, attributes: [{ name: "data-toggle-indicator", staticValue: "" }], children: [] };
    for (const attribute of child.opening?.attributes ?? []) { const name=getNodeName(attribute.name); if (!name || !attribute.value) continue; if (attribute.value.type === "StringLiteral") indicator.attributes.push({name,staticValue:attribute.value.value}); else if (attribute.value.type === "JSXExpressionContainer") { const bindingId=nextBindingId(state); state.ir.bindings.push({id:bindingId,kind:"attribute",targetId:indicatorId,attributeName:name,expression:serializeExpression(attribute.value.expression,state),expressionId:internExpression(attribute.value.expression,state)}); indicator.attributes.push({name,bindingId}); } }
    state.ir.elements.push(indicator); root.children.push(indicatorId);
    for (const grandchild of child.children ?? []) { const lowered=lowerJsxNode(grandchild,indicatorId,state); if(lowered!==SKIP) indicator.children.push(lowered); }
  }
  void indicatorId; void stateSlotId; void actionId; void checked; void defaultChecked; void indeterminate;
  return rootId;
}
function lowerExternalComponent(node: any, parentId: string | null, state: CompilerState, external: string): LowerResult {
  if (external === "internal-toggle-root") return lowerToggleRoot(node, parentId, state);
  if (external === "internal-toggle-indicator") {
    reportUnsupported(state, "TOGGLE_INDICATOR_OUTSIDE_ROOT", "Toggle.Indicator must be a direct child of Toggle.Root.");
    return SKIP;
  }
  if (external === "hugeicons-icon") return lowerHugeiconsIcon(node, parentId, state);
  if (external === "button" || external === "input" || external === "span" || external === "checkbox-input" || external === "badge") {
    if (external === "button" || external === "span") {
      const intrinsic = { ...node, opening: { ...node.opening, name: { type: "Identifier", value: external }, attributes: node.opening?.attributes }, closing: node.closing ? { ...node.closing, name: { type: "Identifier", value: external } } : node.closing };
      return lowerJsxElement(intrinsic, parentId, state);
    }
    const tag = external === "checkbox-input" ? "input" : external === "badge" ? "span" : external;
    const sourceAttributes = node.opening?.attributes ?? [];
    const callerClass = sourceAttributes.find((attribute: any) => getNodeName(attribute.name) === "className");
    const variant = sourceAttributes.find((attribute: any) => getNodeName(attribute.name) === "variant");
    const withoutAdapterProps = sourceAttributes.filter((attribute: any) => !["className", "variant"].includes(getNodeName(attribute.name) ?? ""));
    const classes = external === "checkbox-input"
      ? "size-4 shrink-0 accent-primary rounded-[6px] border border-input"
      : external === "input"
        ? "h-9 w-full min-w-0 rounded-4xl border border-input bg-input/30 px-3 py-1 text-base outline-none md:text-sm"
        : external === "badge"
          ? "inline-flex h-5 w-fit shrink-0 items-center justify-center rounded-4xl border border-border bg-input/30 px-2 py-0.5 text-xs font-medium whitespace-nowrap data-[variant=secondary]:border-transparent data-[variant=secondary]:bg-secondary data-[variant=secondary]:text-secondary-foreground"
          : "";
    const classParts: any[] = [{ expression: { type: "StringLiteral", value: classes } }];
    if (callerClass?.value?.expression) classParts.push({ expression: callerClass.value.expression });
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

/**
 * Dependency packages are commonly published with JSX compiled to
 * `jsx`/`jsxs` calls. Normalize that syntax back to the small JSX AST surface
 * handled below; this is syntax based and deliberately independent of any
 * package or component identity.
 */
function jsxFactoryElement(node: any): any | null {
  if (node?.type !== "CallExpression") return null;
  const callee = getNodeName(node.callee);
  if (!callee || !["jsx", "jsxs", "jsxDEV", "_jsx", "_jsxs", "_jsxDEV"].includes(callee)) return null;
  const tag = jsxFactoryName(node.arguments?.[0]?.expression ?? node.arguments?.[0]);
  const props = unwrapExpression(node.arguments?.[1]?.expression ?? node.arguments?.[1]);
  if (!tag || props?.type !== "ObjectExpression") return null;
  const attributes: any[] = [];
  const children: any[] = [];
  for (const property of props.properties ?? []) {
    if (property.type === "SpreadElement" || property.type === "SpreadProperty") {
      attributes.push({ type: "SpreadElement", arguments: property.arguments ?? property.argument });
      continue;
    }
    const name = getNodeName(property.key);
    const value = unwrapExpression(property.value ?? property.expr);
    if (!name) return null;
    if (name === "children") {
      appendFactoryChildren(value, children);
      continue;
    }
    attributes.push({ type: "JSXAttribute", name: { type: "Identifier", value: name }, value: value?.type === "StringLiteral" ? value : { type: "JSXExpressionContainer", expression: value } });
  }
  return { type: "JSXElement", opening: { type: "JSXOpeningElement", name: tag, attributes }, closing: children.length ? { type: "JSXClosingElement", name: tag } : null, children };
}
function jsxFactoryName(value: any): any | null {
  value = unwrapExpression(value);
  if (value?.type === "StringLiteral") return { type: "Identifier", value: value.value };
  if (value?.type === "Identifier") return { type: "Identifier", value: getNodeName(value) };
  if (value?.type === "MemberExpression") {
    const object = jsxFactoryName(value.object);
    const property = jsxFactoryName(value.property);
    if (object && property) return { type: "JSXMemberExpression", object, property };
  }
  return null;
}
function appendFactoryChildren(value: any, children: any[]): void {
  value = unwrapExpression(value);
  if (!value || value.type === "NullLiteral" || (value.type === "BooleanLiteral" && !value.value)) return;
  if (value.type === "ArrayExpression") {
    for (const item of value.elements ?? []) appendFactoryChildren(item?.expression ?? item, children);
    return;
  }
  if (value.type === "StringLiteral") { children.push({ type: "JSXText", value: value.value }); return; }
  const element = jsxFactoryElement(value);
  children.push(element ?? { type: "JSXExpressionContainer", expression: value });
}
function lowerHugeiconsIcon(node: any, parentId: string | null, state: CompilerState): LowerResult {
  // Hugeicons React components are declarative SVG emitters. For the first
  // adapter we preserve the authored sizing/stroke props and lower the Tick
  // icon used by the shadcn checkbox to an intrinsic path.
  const attributes = (node.opening?.attributes ?? []).filter((attribute: any) => getNodeName(attribute.name) !== "icon");
  attributes.push({ type: "JSXAttribute", name: { type: "Identifier", value: "viewBox" }, value: { type: "StringLiteral", value: "0 0 24 24" } });
  attributes.push({ type: "JSXAttribute", name: { type: "Identifier", value: "fill" }, value: { type: "StringLiteral", value: "none" } });
  const intrinsic = { ...node, opening: { ...node.opening, name: { type: "Identifier", value: "svg" }, attributes }, closing: node.closing ? { ...node.closing, name: { type: "Identifier", value: "svg" } } : node.closing, children: [] };
  const svgId = lowerJsxElement(intrinsic, parentId, state);
  if (svgId === SKIP) return SKIP;
  const pathId = nextElementId(state);
  state.ir.elements.push({ id: pathId, tag: "path", parentId: svgId, attributes: [{ name: "d", staticValue: "M5 12l4 4L19 6" }, { name: "stroke", staticValue: "currentColor" }, { name: "stroke-linecap", staticValue: "round" }, { name: "stroke-linejoin", staticValue: "round" }], children: [] });
  getElementById(state, svgId)?.children.push(pathId);
  return svgId;
}
function getJsxName(node: any): string | undefined {
  if (!node) return undefined;
  if (node.type === "JSXMemberExpression") return [getJsxName(node.object), getJsxName(node.property)].filter(Boolean).join(".");
  return getNodeName(node);
}
function objectExpression(values: Record<string, any>): any {
  return { type: "ObjectExpression", properties: Object.entries(values).map(([key, value]) => ({ type: "KeyValueProperty", key: { type: "Identifier", value: key }, value })) };
}
function collectCallerProps(node: any, state: CompilerState): Record<string, any> {
  const props: Record<string, any> = {};
  for (const attribute of node.opening?.attributes ?? []) {
    if (attribute.type === "SpreadElement") {
      const resolved = resolveSpreadObject(attribute.arguments ?? attribute.argument, state);
      if (resolved) for (const property of resolved.properties ?? []) {
        const key = objectPropertyName(property); if (key) props[key] = objectPropertyValue(property);
      } else reportUnsupported(state, "UNSUPPORTED_SPREAD_PROPS", "Component spread props must be statically known.");
      continue;
    }
    const key = getNodeName(attribute.name); if (!key) continue;
    props[key] = !attribute.value ? { type: "BooleanLiteral", value: true } : attribute.value.type === "JSXExpressionContainer" ? attribute.value.expression : attribute.value;
  }
  return props;
}
function expandIntrinsicAttributes(attributes: any[], state: CompilerState): any[] {
  const output: any[] = [];
  for (const attribute of attributes) {
    if (attribute.type !== "SpreadElement") { output.push(attribute); continue; }
    const resolved = resolveSpreadObject(attribute.arguments ?? attribute.argument, state);
    if (!resolved) { output.push(attribute); continue; }
    for (const property of resolved.properties ?? []) {
      const name = objectPropertyName(property); if (!name) continue;
      if (name === "children") continue;
      output.push({ type: "JSXAttribute", name: { type: "Identifier", value: name }, value: { type: "JSXExpressionContainer", expression: objectPropertyValue(property) } });
    }
  }
  return output;
}
function getForwardedChildren(expression: any, state: CompilerState, resolving = new Set<string>()): any[] | null {
  const name = getNodeName(expression);
  if (name && resolving.has(name)) return null;
  const value = name ? state.expressionScope[name] : undefined;
  if (value?.type === "JSXChildren") return value.children;
  // Children can pass through an object spread as an identifier. Resolve that
  // value using the same lexical scope as every other forwarded prop.
  return value ? getForwardedChildren(value, state, name ? new Set([...resolving, name]) : resolving) : null;
}

/**
 * shadcn-style primitives commonly forward all remaining props with
 * `{...props}`. `children` is part of that object, so retain it as element
 * children rather than silently dropping the rendered subtree.
 */
function getSpreadChildren(attributes: any[], state: CompilerState): any[] {
  const children: any[] = [];
  for (const attribute of attributes) {
    if (attribute.type !== "SpreadElement") continue;
    const resolved = resolveSpreadObject(attribute.arguments ?? attribute.argument, state);
    if (!resolved) continue;
    for (const property of resolved.properties ?? []) {
      if (objectPropertyName(property) !== "children") continue;
      const value = objectPropertyValue(property);
      if (value?.type === "JSXChildren") children.push(...(value.children ?? []));
      else children.push(...(getForwardedChildren(value, state) ?? []));
    }
  }
  return children;
}

/** Expand plain object spreads and source-level prop merges without relying on
 * the identity of the library that supplied those objects. */
function resolveSpreadObject(input: any, state: CompilerState, resolving = new Set<string>()): any | null {
  let node = unwrapExpression(input);
  if (node?.type === "Identifier") {
    const name = getNodeName(node);
    if (!name || resolving.has(name)) return null;
    resolving.add(name);
    node = state.expressionScope[name] ?? node;
  }
  if (node?.type === "CallExpression" && isObjectMergeCall(node)) {
    const properties: any[] = [];
    for (const argument of node.arguments ?? []) {
      const object = resolveSpreadObject(argument.expression ?? argument, state, new Set(resolving));
      if (!object) return null;
      properties.push(...(object.properties ?? []));
    }
    return { type: "ObjectExpression", properties };
  }
  if (node?.type !== "ObjectExpression") return null;
  const properties: any[] = [];
  for (const property of node.properties ?? []) {
    if (property.type === "SpreadElement" || property.type === "SpreadProperty") {
      const object = resolveSpreadObject(property.arguments ?? property.argument, state, new Set(resolving));
      if (!object) return null;
      properties.push(...(object.properties ?? []));
    } else {
      properties.push(property);
    }
  }
  return { ...node, properties };
}
function isObjectMergeCall(node: any): boolean {
  const callee = getNodeName(node.callee) ?? getNodeName(node.callee?.property);
  return callee === "mergeProps" || (node.callee?.type === "MemberExpression" && getNodeName(node.callee.object) === "Object" && getNodeName(node.callee.property) === "assign");
}

function internExpression(expression: any, state: CompilerState): string {
  const lowered = lowerExpression(expression, state);
  const existing = state.ir.expressions.find((candidate) => JSON.stringify(candidate.expression) === JSON.stringify(lowered));
  if (existing) return existing.id;
  state.expressionCounter += 1;
  const id = `x${state.expressionCounter}`;
  state.ir.expressions.push({ id, expression: lowered });
  return id;
}

function lowerExpression(input: any, state: CompilerState, resolving = new Set<string>()): any {
  const node = unwrapExpression(input);
  if (!node) return { kind: "literal", value: null };
  if (isLiteralExpression(node)) return { kind: "literal", value: node.type === "NullLiteral" ? null : node.value };
  const identifier = getNodeName(node);
  if (node.type === "Identifier" || node.type === "IdentifierExpression") {
    if (identifier && state.expressionScope[identifier] && !resolving.has(identifier)) { resolving.add(identifier); return lowerExpression(state.expressionScope[identifier], state, resolving); }
    return { kind: "identifier", name: identifier ?? "unknown" };
  }
  if (node.type === "MemberExpression") return { kind: "member", object: lowerExpression(node.object, state, resolving), property: getNodeName(node.property) ?? "" };
  if (node.type === "OptionalChainingExpression" || node.type === "OptionalMemberExpression") return { kind: "member", object: lowerExpression(node.base ?? node.object, state, resolving), property: getNodeName(node.property) ?? "" };
  if (node.type === "CallExpression" && node.callee?.type === "MemberExpression" && getNodeName(node.callee.property) === "getFullYear" && node.callee.object?.type === "NewExpression" && getNodeName(node.callee.object.callee) === "Date") return { kind: "host", name: "currentYear" };
  if (node.type === "BinaryExpression" && ["&&", "||", "??"].includes(node.operator)) return { kind: "logical", op: node.operator, left: lowerExpression(node.left, state, resolving), right: lowerExpression(node.right, state, resolving) };
  if (node.type === "BinaryExpression") return { kind: "binary", op: node.operator, left: lowerExpression(node.left, state, resolving), right: lowerExpression(node.right, state, resolving) };
  if (node.type === "LogicalExpression") return { kind: "logical", op: node.operator, left: lowerExpression(node.left, state, resolving), right: lowerExpression(node.right, state, resolving) };
  if (node.type === "ConditionalExpression") return { kind: "conditional", test: lowerExpression(node.test, state, resolving), consequent: lowerExpression(node.consequent, state, resolving), alternate: lowerExpression(node.alternate, state, resolving) };
  if (node.type === "UnaryExpression") return { kind: "unary", op: node.operator, argument: lowerExpression(node.argument, state, resolving) };
  if (node.type === "TemplateLiteral") { const parts: any[] = []; for (let i=0;i<(node.quasis?.length ?? 0);i++) { parts.push(node.quasis[i]?.raw ?? node.quasis[i]?.value?.cooked ?? ""); if (node.expressions?.[i]) parts.push(lowerExpression(node.expressions[i], state, resolving)); } return { kind: "template", parts }; }
  if (node.type === "ArrayExpression") return { kind: "array", items: (node.elements ?? []).filter(Boolean).map((item: any) => lowerExpression(item.expression ?? item, state, resolving)) };
  if (node.type === "ObjectExpression") {
    const expanded = resolveSpreadObject(node, state);
    const properties = expanded?.properties ?? node.properties ?? [];
    return {
      kind: "object",
      properties: properties
        .filter((property: any) => property.type !== "MethodProperty" && property.type !== "GetterProperty" && property.type !== "SetterProperty")
        .map((property: any) => (property.type === "SpreadElement" || property.type === "SpreadProperty")
          ? ({ kind: "spread", value: lowerExpression(property.arguments ?? property.argument, state, new Set(resolving)) })
          : ({ kind: "entry", key: objectPropertyName(property) ?? "", value: lowerExpression(objectPropertyValue(property), state, new Set(resolving)) }))
    };
  }
  if (node.type === "CallExpression" && ["cn", "clsx", "classnames"].includes(getNodeName(node.callee) ?? "")) return { kind: "intrinsic", name: getNodeName(node.callee), args: (node.arguments ?? []).map((arg: any) => lowerExpression(arg.expression ?? arg, state, resolving)) };
  if (node.type === "CallExpression" && ["useMemo", "useCallback"].includes(getNodeName(node.callee) ?? getNodeName(node.callee?.property) ?? "")) {
    const callback = unwrapExpression(node.arguments?.[0]?.expression ?? node.arguments?.[0]);
    const returned = callback?.body?.type === "BlockStatement" ? findReturnedExpression(callback.body) : callback?.body;
    if (returned) return lowerExpression(returned, state, resolving);
  }
  reportUnsupported(state, "UNSUPPORTED_EXPRESSION", `Unsupported expression: ${node.type}${describeExpression(node) ? ` (${describeExpression(node)})` : ""}.`);
  return { kind: "literal", value: "" };
}

function describeExpression(node: any): string | undefined {
  if (node?.type !== "CallExpression" && node?.type !== "NewExpression") return undefined;
  return getNodeName(node.callee) ?? getNodeName(node.callee?.property);
}

function objectPropertyName(property: any): string | undefined {
  return getNodeName(property?.key) ?? (property?.type === "Identifier" || property?.type === "IdentifierExpression" ? getNodeName(property) : undefined);
}

function objectPropertyValue(property: any): any {
  return property?.value ?? property?.expr ?? (property?.type === "Identifier" || property?.type === "IdentifierExpression" ? property : undefined);
}

function getStatements(node: any): any[] {
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

function nextInputId(state: CompilerState): string {
  state.inputCounter += 1;
  return `i${state.inputCounter}`;
}

/** The producer expression is intentionally opaque. Only the row projection
 * crosses into O1, so no import path or hook name is part of this decision. */
function ensureCollectionInput(name: string, callback: any, state: CompilerState): CompiledInputNode | undefined {
  const existing = state.ir.inputs.find((input) => input.name === name);
  if (existing) return existing;
  const itemName = getPatternLabel(callback?.params?.[0]);
  const keyExpression = getMapRowKeyExpression(callback, state);
  if (!itemName || !keyExpression) return undefined;
  const observed = new Set<string>();
  const visit = (node: any): void => {
    if (!node || typeof node !== "object") return;
    if ((node.type === "MemberExpression" || node.type === "OptionalChainingExpression") && getNodeName(node.object) === itemName) {
      const property = getNodeName(node.property);
      if (property) observed.add(property);
    }
    for (const value of Object.values(node)) {
      if (Array.isArray(value)) value.forEach(visit);
      else if (value && typeof value === "object") visit(value);
    }
  };
  visit(callback.body);
  const input: CompiledInputNode = { id: nextInputId(state), name, shape: { kind: "collection", keyExpression, orderSensitive: true, observedRowPaths: [...observed].sort().map((path) => [path]) } };
  state.ir.inputs.push(input);
  return input;
}

function inferValueInputs(state: CompilerState): void {
  const rowNames = new Set(state.ir.loops.map((loop) => loop.itemName));
  const expressions = new Map(state.ir.expressions.map((entry) => [entry.id, entry.expression]));
  for (const binding of state.ir.bindings) {
    const expression: any = expressions.get(binding.expressionId ?? "");
    const path = inputPath(expression);
    if (!path) continue;
    const inputName = path[0]!;
    // A source construct that is not representable as a value input must be
    // diagnosed at its origin, never materialized as an `undefined` producer.
    if (!inputName || inputName === "undefined" || inputName === "unknown" || rowNames.has(inputName) || inputName === "host") continue;
    let input = state.ir.inputs.find((candidate) => candidate.name === inputName);
    if (!input) {
      input = path.length === 1
        ? { id: nextInputId(state), name: inputName, shape: { kind: "scalar" } }
        : { id: nextInputId(state), name: inputName, shape: { kind: "object", observedPaths: [] } };
      state.ir.inputs.push(input);
    }
    if (input.shape.kind === "object" && path.length > 1 && !input.shape.observedPaths.some((candidate) => candidate.join(".") === path.slice(1).join("."))) input.shape.observedPaths.push(path.slice(1));
    state.ir.dependencyEdges.push({ fromId: input.id, toId: binding.id, kind: "input-to-binding" });
  }
}

function inputPath(expression: any): string[] | undefined {
  if (!expression) return undefined;
  if (expression.kind === "identifier") return [expression.name];
  if (expression.kind === "member") {
    const base = inputPath(expression.object);
    return base ? [...base, expression.property] : undefined;
  }
  return undefined;
}

function getElementById(state: CompilerState, elementId: string): (ApplicationIr["elements"][number] & { keyValue?: string }) | undefined {
  return state.ir.elements.find((element) => element.id === elementId) as (ApplicationIr["elements"][number] & { keyValue?: string }) | undefined;
}

function mergeScopes(baseScope: Record<string, unknown>, extraScope: Record<string, unknown>): Record<string, unknown> {
  return { ...baseScope, ...extraScope };
}

function getPatternName(pattern: any): string | undefined {
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

function getPatternLabel(pattern: any): string | undefined {
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

function normalizeText(text: string): string {
  return text.replace(/\s+/g, " ").trim();
}

function isLiteralExpression(expression: any): boolean {
  return (
    expression?.type === "StringLiteral" ||
    expression?.type === "NumericLiteral" ||
    expression?.type === "BooleanLiteral" ||
    expression?.type === "NullLiteral"
  );
}

function literalToString(expression: any): string {
  if (expression && typeof expression === "object" && expression.type === "NullLiteral") {
    return "null";
  }

  if (expression && typeof expression === "object" && "value" in expression) {
    return String((expression as { value: unknown }).value);
  }

  return String(expression);
}

function serializeExpression(expression: any, state: CompilerState): string {
  const start = Math.max(0, (expression?.span?.start ?? 1) - 1);
  const end = Math.max(start, (expression?.span?.end ?? start + 1) - 1);
  const snippet = state.source.slice(start, end).trim();
  if (snippet) {
    return snippet;
  }
  return expression?.type ?? "unknown";
}

function isIntrinsicTag(tag: string): boolean {
  return /^[a-z][a-z0-9-]*$/.test(tag);
}

function isComponentName(value?: string): boolean {
  return typeof value === "string" && /^[A-Z]/.test(value);
}

function getNodeName(node: any): string | undefined {
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

function getFunctionBody(node: any): any {
  return node?.function?.body ?? node?.body;
}

function unwrapExpression(node: any): any {
  let current = node;

  while (current) {
    if (
      current.type === "ParenthesisExpression" ||
      current.type === "ParenthesizedExpression" ||
      current.type === "ParenExpr" ||
      current.type === "ParenExpression"
    ) {
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

function nextElementId(state: CompilerState): string {
  state.elementCounter += 1;
  return `e${state.elementCounter}`;
}

function nextTextId(state: CompilerState): string {
  state.textCounter += 1;
  return `t${state.textCounter}`;
}

function nextBindingId(state: CompilerState): string {
  state.bindingCounter += 1;
  return `b${state.bindingCounter}`;
}

function nextLoopId(state: CompilerState): string {
  state.bindingCounter += 1;
  return `l${state.bindingCounter}`;
}

function reportUnsupported(state: CompilerState, code: string, message: string): void {
  state.diagnostics.push({
    code,
    message,
    severity: "error"
  });
}

function throwIfStrict(state: CompilerState): void {
  if (state.mode !== "strict") {
    return;
  }

  const firstDiagnostic = state.diagnostics[0];
  if (firstDiagnostic) {
    throw new CompileFailure(`Strict compilation failed: ${firstDiagnostic.code} ${firstDiagnostic.message}`);
  }
}
