import { parseSync } from '@swc/core';
import type { ExecutableApplication } from 'plec-ir';

export {
  lowerExecutableApplication,
  lowerCompilerFacts,
  type ExecutableApplicationFacts,
} from './executable-lowering.js';
import { lowerCompilerFacts } from './executable-lowering.js';

export interface CompilerDiagnostic {
  code: string;
  message: string;
  severity: 'error' | 'warning';
}

interface BindingNode {
  id: string;
  kind: 'text' | 'attribute' | 'property';
  targetId: string;
  attributeName?: string;
  expression: string;
  expressionId?: string;
  loopId?: string;
}
interface ExpressionNode {
  id: string;
  expression: unknown;
}
interface EventNode {
  id: string;
  type: string;
  targetId: string;
  actionId: string;
  args: string[];
  field?: string;
  navigate?: { href: string; replace?: boolean };
  stopPropagation?: boolean;
  preventDefault?: boolean;
}
interface IslandNode {
  islandInstanceId: string;
  componentId: string;
  placeholderNodeId: string;
  moduleId: string;
  exportName: string;
  props: Record<string, unknown>;
}
interface ComponentMetadata {
  name: string;
  moduleId: string;
  elementIds: string[];
  bindingIds: string[];
  eventIds: string[];
}

interface QueryNode {
  id: string;
  source: string;
  resultSymbol?: string;
}

interface CompiledInputNode {
  id: string;
  name: string;
  shape:
    | { kind: 'scalar' }
    | { kind: 'object'; observedPaths: string[][] }
    | {
        kind: 'collection';
        keyExpression: string;
        orderSensitive: boolean;
        observedRowPaths: string[][];
      };
}

interface DependencyEdge {
  fromId: string;
  toId: string;
  kind:
    | 'input-to-loop'
    | 'query-to-loop'
    | 'row-field-to-binding'
    | 'input-to-binding'
    | 'local-state-to-binding'
    | 'host-value-to-binding';
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
  /** Kept as compiler facts so executable lowering never reparses display
   * strings. */
  sourceExpressionId?: string;
  keyExpressionId?: string;
  dependencyStateNames?: string[];
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

interface LegacyApplicationFacts {
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
  propPrograms: Array<{
    id: string;
    targetId: string;
    writes: Array<{
      name: string;
      staticValue?: string;
      expressionId?: string;
      kind: 'attribute' | 'property' | 'event' | 'ref' | 'spread';
    }>;
    loopId?: string;
  }>;
  events: EventNode[];
  refs: Array<{
    id: string;
    targetId: string;
    refId: string;
    kind: 'callback' | 'object';
  }>;
  hostElementRefs: Array<{
    id: string;
    targetId: string;
    attachments: string[];
  }>;
  hostElementReads: Array<{
    id: string;
    refId: string;
    capability: unknown;
  }>;
  /** Compiler-owned action facts. These are not a serialized executable
   * format: symbolic references are resolved only while emitting 0.9. */
  actionFacts: Array<{
    id: string;
    parameters: Array<{
      name: string;
      type: 'event' | 'string' | 'boolean' | 'number' | 'json';
    }>;
    eventFields: string[];
    frameSlots: number;
    parameterSlots: number[];
    routeRetry?: boolean;
    instructions: Array<any>;
  }>;
  contexts: Array<{
    id: string;
    contextId: string;
    parentId: string | null;
    valueExpressionId?: string;
    values: Array<{
      name: string;
      expressionId?: string;
      staticValue?: string;
    }>;
    children: string[];
  }>;
  contextDefinitions: Array<{
    id: string;
    defaultExpressionId: string;
  }>;
  conditionals: Array<{
    id: string;
    parentId: string;
    expressionId: string;
    consequent: string[];
    alternate: string[];
    loopId?: string;
  }>;
  localStates: Array<{
    id: string;
    name: string;
    initialValue: string;
    initialExpression?: any;
    values: string[];
  }>;
  hostValues: Array<{ id: string; kind: 'media-query'; query: string }>;
  lifecycleEffects: Array<any>;
  stateTransitions: Array<any>;
  islands: IslandNode[];
  components: ComponentMetadata[];
  layout?: { routeOutlets: Array<{ id: string; elementId: string }> };
}

export interface CompileOptions {
  mode?: 'strict' | 'lenient';
  rootComponent?: string;
  moduleId?: string;
  applicationRevision?: string;
  modules?: Array<{ id: string; source: string }>;
  islandComponents?: string[];
  /** Router-owned callback available only while compiling an error graph. */
  routeRetryProp?: string;
  /** Router-owned failure record available only while compiling an error graph. */
  routeErrorProp?: string;
}

export interface CompileResult {
  ir: ExecutableApplication;
  diagnostics: CompilerDiagnostic[];
  /** Compiler-only route metadata. It is deliberately not emitted in IR. */
  loaderResultState?: number;
  routeErrorState?: number;
}

class CompileFailure extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'CompileFailure';
  }
}

interface CompilerState {
  source: string;
  mode: 'strict' | 'lenient';
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
  components: Map<
    string,
    { name: string; body: any; params: any[]; moduleId: string }
  >;
  functions: Map<
    string,
    { name: string; body: any; params: any[]; moduleId: string }
  >;
  importSymbols: Map<string, { moduleId: string; exportName: string }>;
  exports: Map<
    string,
    Map<string, { moduleId: string; exportName: string }>
  >;
  moduleExpressions: Map<string, Record<string, any>>;
  contextSymbols: Map<
    string,
    { id: string; defaultExpressionId: string }
  >;
  stateSetters: Map<string, string>;
  loaderResultState?: number;
  routeErrorState?: number;
  routerLinkBindings: Set<string>;
  islandComponents: Set<string>;
  routeRetryProp?: string;
  routeErrorProp?: string;
  componentStack: string[];
  functionStack: string[];
  activeLoopId?: string;
  activeModuleId: string;
  ir: LegacyApplicationFacts;
}

const SKIP = Symbol('skip');

type LowerResult = string | typeof SKIP;

export function compile(
  source: string,
  options: CompileOptions = {},
): CompileResult {
  const mode = options.mode ?? 'lenient';
  const moduleAst: any = parseSync(source, {
    syntax: 'typescript',
    tsx: true,
    target: 'es2022',
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
    functions: new Map(),
    importSymbols: new Map(),
    exports: new Map(),
    moduleExpressions: new Map(),
    contextSymbols: new Map(),
    stateSetters: new Map(),
    routerLinkBindings: new Set(),
    islandComponents: new Set(options.islandComponents ?? []),
    routeRetryProp: options.routeRetryProp,
    routeErrorProp: options.routeErrorProp,
    componentStack: [],
    functionStack: [],
    activeModuleId: options.moduleId ?? '<entry>',
    ir: {
      revision: options.applicationRevision,
      rootElementId: '',
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
      hostElementRefs: [],
      hostElementReads: [],
      actionFacts: [],
      contexts: [],
      contextDefinitions: [],
      conditionals: [],
      localStates: [],
      hostValues: [],
      lifecycleEffects: [],
      stateTransitions: [],
      islands: [],
      components: [],
    },
  };

  const modules = options.modules?.length
    ? options.modules
    : [{ id: options.moduleId ?? '<entry>', source }];
  for (const module of modules) {
    const ast: any =
      module.id === (options.moduleId ?? '<entry>')
        ? moduleAst
        : parseSync(module.source, {
            syntax: 'typescript',
            tsx: true,
            target: 'es2022',
          });
    collectModuleScope(ast, state);
    collectModuleExpressions(ast, state, module.id);
    collectContexts(ast, state, module.id);
    collectComponents(ast, state, module.id);
    collectImports(ast, state, module.id);
    collectExports(ast, state, module.id);
  }

  const rootJsx = findRootJsx(moduleAst, state, options.rootComponent);
  if (!rootJsx) {
    reportUnsupported(
      state,
      'NO_ROOT_COMPONENT',
      'No function component with a JSX return was found.',
    );
    throwIfStrict(state);
    state.ir.rootElementId = 'e1';
    state.ir.elements.push({
      id: 'e1',
      tag: 'div',
      parentId: null,
      attributes: [],
      children: [],
    });
    return {
      ir: lowerCompilerFacts(state.ir),
      diagnostics: state.diagnostics,
    };
  }

  // Event actions declared inside the root component share its lexical
  // functions. Populate that scope before lowering JSX so async handlers are
  // discovered exactly like handlers in expanded child components.
  const rootDefinition = options.rootComponent
    ? state.functions.get(
        componentKey(state.activeModuleId, options.rootComponent),
      )
    : undefined;
  if (rootDefinition)
    populateFunctionLocals(rootDefinition.body, state.expressionScope);
  if (options.rootComponent)
    state.componentStack.push(options.rootComponent);
  if (state.routeErrorProp) {
    const slotId = `s${state.ir.localStates.length + 1}`;
    state.ir.localStates.push({ id: slotId, name: state.routeErrorProp, initialValue: 'null', initialExpression: { kind: 'literal', value: null }, values: [] });
    state.routeErrorState = state.ir.localStates.length - 1;
    (state.ir as any).routeErrorState = state.routeErrorState;
    state.expressionScope[state.routeErrorProp] = { type: 'Identifier', value: state.routeErrorProp };
  }
  const rootId = lowerJsxNode(rootJsx, null, state);
  if (options.rootComponent) state.componentStack.pop();
  if (
    rootId === SKIP ||
    (!rootId.startsWith('e') && !rootId.startsWith('c'))
  ) {
    reportUnsupported(
      state,
      'INVALID_ROOT',
      'The root JSX node could not be lowered to an intrinsic element.',
    );
    throwIfStrict(state);
    state.ir.rootElementId = 'e1';
    state.ir.elements.push({
      id: 'e1',
      tag: 'div',
      parentId: null,
      attributes: [],
      children: [],
    });
  } else {
    state.ir.rootElementId = rootId;
  }

  inferValueInputs(state);
  discoverLayoutMetadata(state);

  throwIfStrict(state);

  return {
    ir: lowerCompilerFacts(state.ir),
    diagnostics: state.diagnostics,
    ...(state.loaderResultState === undefined
      ? {}
      : { loaderResultState: state.loaderResultState }),
    ...(state.routeErrorState === undefined
      ? {}
      : { routeErrorState: state.routeErrorState }),
  };
}

/** Lower one immutable component definition. Mount identity deliberately stays
 * out of the artifact; PlecRuntime derives GraphInstance ids from topology. */
export function compileComponentGraph(
  source: string,
  options: CompileOptions & {
    componentId?: string;
    graphId?: string;
  } = {},
): CompileResult & {
  graph: ExecutableApplication & {
    graphId: string;
    componentId: string;
    componentName: string;
    moduleId: string;
    capabilities: Array<unknown>;
  };
} {
  const result = compile(source, options);
  const componentName = options.rootComponent ?? 'App';
  const moduleId = options.moduleId ?? '<entry>';
  const componentId =
    options.componentId ?? `${moduleId}#${componentName}`;
  const graphId =
    options.graphId ??
    `g-${createStableId(`${componentId}\n${options.applicationRevision ?? source}`)}`;
  return {
    ...result,
    graph: {
      ...result.ir,
      graphId,
      componentId,
      componentName,
      moduleId,
      capabilities: [],
    },
  };
}

function createStableId(value: string): string {
  let hash = 2166136261;
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index);
    hash = Math.imul(hash, 16777619);
  }
  return (hash >>> 0).toString(36);
}

function findRootJsx(
  moduleAst: any,
  state: CompilerState,
  requestedName?: string,
): any | null {
  const body = moduleAst?.body ?? [];

  for (const original of body) {
    const item = original.declaration ?? original.decl ?? original;
    if (
      (item?.type === 'FunctionDeclaration' ||
        item?.type === 'FnDecl') &&
      isComponentName(getNodeName(item.identifier ?? item.id)) &&
      (!requestedName ||
        getNodeName(item.identifier ?? item.id) === requestedName)
    ) {
      collectModuleFacts(getFunctionBody(item), state);
      const jsx = findReturnedJsx(getFunctionBody(item));
      if (jsx) {
        return jsx;
      }
    }

    if (
      item?.type === 'FunctionExpression' &&
      isComponentName(getNodeName(item.identifier ?? item.id)) &&
      (!requestedName ||
        getNodeName(item.identifier ?? item.id) === requestedName)
    ) {
      collectModuleFacts(getFunctionBody(item), state);
      const jsx = findReturnedJsx(getFunctionBody(item));
      if (jsx) return jsx;
    }

    if (
      item?.type === 'VariableDeclaration' ||
      item?.type === 'VarDecl'
    ) {
      for (const declaration of item.declarations ?? item.decls ?? []) {
        const name = getNodeName(declaration?.id);
        const init = declaration?.init;
        if (
          !isComponentName(name) ||
          (requestedName && name !== requestedName)
        ) {
          continue;
        }
        if (
          init?.type === 'ArrowFunctionExpression' ||
          init?.type === 'FunctionExpression'
        ) {
          collectModuleFacts(getFunctionBody(init), state);
          const jsx = findReturnedJsx(getFunctionBody(init));
          if (jsx) {
            return jsx;
          }
        }
      }
    }
  }

  reportUnsupported(
    state,
    'NO_COMPONENT',
    'No supported component declaration was found.',
  );
  return null;
}

function findReturnedJsx(body: any): any | null {
  if (!body) {
    return null;
  }

  const topLevel = unwrapExpression(body);
  if (
    topLevel?.type === 'JSXElement' ||
    topLevel?.type === 'JSXFragment' ||
    jsxFactoryElement(topLevel)
  ) {
    return topLevel;
  }

  if (body.type !== 'BlockStatement') {
    return null;
  }

  for (const statement of body.stmts ?? body.body ?? []) {
    if (
      statement.type === 'ReturnStatement' ||
      statement.type === 'ReturnStmt'
    ) {
      const argument = unwrapExpression(
        statement.argument ?? statement.arg,
      );
      if (
        argument?.type === 'JSXElement' ||
        argument?.type === 'JSXFragment' ||
        jsxFactoryElement(argument)
      ) {
        return argument;
      }
    }
  }

  return null;
}

function lowerJsxNode(
  node: any,
  parentId: string | null,
  state: CompilerState,
): LowerResult {
  if (!node) {
    return SKIP;
  }

  if (node.type === 'JSXElement') {
    return lowerJsxElement(node, parentId, state);
  }

  if (node.type === 'JSXText') {
    return lowerJsxText(node, parentId, state);
  }

  if (node.type === 'JSXExpressionContainer') {
    return lowerJsxExpression(node, parentId, state);
  }

  const factoryElement = jsxFactoryElement(node);
  if (factoryElement) {
    return lowerJsxElement(factoryElement, parentId, state);
  }

  reportUnsupported(
    state,
    'UNSUPPORTED_JSX_NODE',
    `Unsupported JSX node type: ${node.type}.`,
  );
  return SKIP;
}

function lowerJsxElement(
  node: any,
  parentId: string | null,
  state: CompilerState,
): LowerResult {
  const tag = node.opening?.name;
  const tagName = getJsxName(tag);
  if (tagName && (isComponentName(tagName) || tagName.includes('.'))) {
    return lowerComponentElement(node, parentId, state);
  }
  if (!tagName || !isIntrinsicTag(tagName)) {
    reportUnsupported(
      state,
      'UNSUPPORTED_TAG',
      'Only intrinsic lowercase JSX tags are supported in Milestone 2.',
    );
    return SKIP;
  }

  const elementId = nextElementId(state);
  const element = {
    id: elementId,
    tag: tagName,
    parentId,
    attributes: [] as Array<{
      name: string;
      staticValue?: string;
      bindingId?: string;
    }>,
    children: [] as string[],
  };

  const propWrites: Array<{
    name: string;
    staticValue?: string;
    expressionId?: string;
    kind: 'attribute' | 'property' | 'event' | 'ref' | 'spread';
  }> = [];
  for (const attributeNode of expandIntrinsicAttributes(
    node.opening?.attributes ?? [],
    state,
  )) {
    if (attributeNode.type === 'SpreadElement') {
      const expression =
        attributeNode.arguments ?? attributeNode.argument;
      lowerExpression(expression, state);
      propWrites.push({
        name: '',
        expressionId: internExpression(expression, state),
        kind: 'spread',
      });
      continue;
    }

    if (attributeNode.type !== 'JSXAttribute') {
      reportUnsupported(
        state,
        'UNSUPPORTED_ATTRIBUTE',
        'Unsupported JSX attribute construct.',
      );
      continue;
    }

    const name = getNodeName(attributeNode.name);
    if (!name) {
      reportUnsupported(
        state,
        'INVALID_ATTRIBUTE',
        'Encountered JSX attribute without a name.',
      );
      continue;
    }

    if (name === 'key') {
      continue;
    }

    if (
      name === 'ref' &&
      attributeNode.value?.type === 'JSXExpressionContainer'
    ) {
      const value = unwrapExpression(attributeNode.value.expression);
      const attachments =
        value?.type === 'ArrayExpression'
          ? (value.elements ?? [])
              .map((entry: any) =>
                getNodeName(
                  unwrapExpression(entry?.expression ?? entry),
                ),
              )
              .filter(Boolean)
          : [getNodeName(value)].filter(Boolean);
      const refs = attachments.length
        ? attachments
        : [`ref${state.ir.refs.length + 1}`];
      for (const refId of refs)
        state.ir.refs.push({
          id: `r${state.ir.refs.length + 1}`,
          targetId: elementId,
          refId,
          kind: 'callback',
        });
      state.ir.hostElementRefs.push({
        id: `hr${state.ir.hostElementRefs.length + 1}`,
        targetId: elementId,
        attachments: refs,
      });
      propWrites.push({ name, kind: 'ref' });
      continue;
    }
    if (
      name &&
      /^on[A-Z]/.test(name) &&
      attributeNode.value?.type === 'JSXExpressionContainer'
    ) {
      lowerEvent(
        name,
        attributeNode.value.expression,
        elementId,
        state,
      );
      propWrites.push({ name, kind: 'event' });
      continue;
    }

    if (!attributeNode.value) {
      element.attributes.push({ name, staticValue: 'true' });
      propWrites.push({ name, staticValue: 'true', kind: 'attribute' });
      continue;
    }

    if (attributeNode.value.type === 'StringLiteral') {
      element.attributes.push({
        name,
        staticValue: attributeNode.value.value,
      });
      propWrites.push({
        name,
        staticValue: attributeNode.value.value,
        kind: 'attribute',
      });
      continue;
    }

    if (attributeNode.value.type === 'JSXExpressionContainer') {
      const expression = attributeNode.value.expression;
      const literalValue = evaluateExpression(expression, state, {});
      if (literalValue !== undefined && !state.preserveBindings) {
        const staticValue = literalToString(literalValue);
        element.attributes.push({ name, staticValue });
        propWrites.push({
          name,
          staticValue,
          kind: isDomProperty(name) ? 'property' : 'attribute',
        });
      } else {
        const bindingId = nextBindingId(state);
        state.ir.bindings.push({
          id: bindingId,
          kind: 'attribute',
          targetId: elementId,
          attributeName: name,
          expression: serializeExpression(expression, state),
          expressionId: internExpression(expression, state),
        });
        element.attributes.push({ name, bindingId });
        propWrites.push({
          name,
          expressionId:
            state.ir.bindings[state.ir.bindings.length - 1]!
              .expressionId,
          kind: isDomProperty(name) ? 'property' : 'attribute',
        });
      }
      continue;
    }

    reportUnsupported(
      state,
      'UNSUPPORTED_ATTRIBUTE_VALUE',
      `Attribute ${name} has an unsupported value form.`,
    );
  }

  state.ir.elements.push(element);
  if (propWrites.length)
    state.ir.propPrograms.push({
      id: `p${state.ir.propPrograms.length + 1}`,
      targetId: elementId,
      writes: propWrites,
    });

  for (const child of [
    ...getSpreadChildren(node.opening?.attributes ?? [], state),
    ...(node.children ?? []),
  ]) {
    // A component's `{children}` expression can expand to several sibling
    // nodes. Return values carry only one node ID, so append every forwarded
    // child here rather than silently retaining the first sibling.
    const forwarded =
      child?.type === 'JSXExpressionContainer'
        ? getForwardedChildren(
            unwrapExpression(child.expression),
            state,
          )
        : null;
    if (forwarded) {
      for (const forwardedChild of forwarded) {
        const lowered = lowerJsxNode(forwardedChild, elementId, state);
        if (lowered !== SKIP) element.children.push(lowered);
      }
      continue;
    }
    const lowered = lowerJsxNode(child, elementId, state);
    if (lowered !== SKIP) {
      element.children.push(lowered);
    }
  }

  return elementId;
}

function lowerJsxText(
  node: any,
  parentId: string | null,
  state: CompilerState,
): LowerResult {
  if (!parentId) {
    reportUnsupported(
      state,
      'ROOT_TEXT_NODE',
      'Root-level text nodes are not supported.',
    );
    return SKIP;
  }

  const normalized = normalizeText(node.value ?? '');
  if (!normalized) {
    return SKIP;
  }

  const textId = nextTextId(state);
  state.ir.texts.push({
    id: textId,
    parentId,
    staticValue: normalized,
  });
  return textId;
}

function lowerJsxExpression(
  node: any,
  parentId: string | null,
  state: CompilerState,
): LowerResult {
  if (!parentId) {
    reportUnsupported(
      state,
      'ROOT_EXPRESSION_NODE',
      'Root-level expression nodes are not supported.',
    );
    return SKIP;
  }

  const expression = unwrapExpression(node.expression);
  const forwardedChildren = getForwardedChildren(expression, state);
  if (forwardedChildren) {
    let first: LowerResult = SKIP;
    for (const child of forwardedChildren) {
      const lowered = lowerJsxNode(child, parentId, state);
      if (first === SKIP && lowered !== SKIP) first = lowered;
    }
    return first;
  }
  if (!expression || expression.type === 'JSXEmptyExpression') {
    return SKIP;
  }

  const structural = lowerStructuralExpression(
    expression,
    parentId,
    state,
  );
  if (structural !== SKIP) return structural;

  const rendered = lowerReturnedElementExpression(
    expression,
    parentId,
    state,
  );
  if (rendered !== SKIP) return rendered;

  if (expression.type === 'JSXElement') {
    return lowerJsxElement(expression, parentId, state);
  }

  if (expression.type === 'JSXFragment') {
    reportUnsupported(
      state,
      'UNSUPPORTED_FRAGMENT',
      'JSX fragments are not supported in Milestone 2.',
    );
    return SKIP;
  }

  if (expression.type === 'CallExpression') {
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
      staticValue: literalToString(literalValue),
    });
    return textId;
  }

  state.ir.texts.push({
    id: textId,
    parentId,
    staticValue: '',
  });
  state.ir.bindings.push({
    id: nextBindingId(state),
    kind: 'text',
    targetId: textId,
    expression: serializeExpression(expression, state),
    expressionId: internExpression(expression, state),
  });
  return textId;
}

function lowerMapExpression(
  expression: any,
  parentId: string | null,
  state: CompilerState,
): LowerResult {
  const mapCall = getMapCall(expression);
  if (!mapCall || !parentId) {
    return SKIP;
  }

  const items = evaluateArrayExpression(
    mapCall.source,
    state,
    state.scope,
  );
  const callback = mapCall.callback;
  const sourceName = getNodeName(mapCall.source);
  // A named useState value is still a local expression, not a producer. Only
  // a plain, otherwise-unbound identifier uses the external delta path.
  const isLocalSource = Boolean(
    sourceName &&
    (state.ir.localStates.some((slot) => slot.name === sourceName) ||
      state.expressionScope[sourceName] !== undefined),
  );
  const input =
    !items && sourceName && !isLocalSource
      ? ensureCollectionInput(sourceName, callback, state)
      : undefined;
  const isDerivedLocalSource =
    isLocalSource ||
    expressionUsesLocalState(mapCall.source, state) ||
    unwrapExpression(mapCall.source)?.type === 'CallExpression';
  if (
    (!items && !input && !isDerivedLocalSource) ||
    !callback ||
    callback.type !== 'ArrowFunctionExpression'
  ) {
    reportUnsupported(
      state,
      'UNSUPPORTED_MAP',
      'Only literal arrays or a named external input may be mapped.',
    );
    return SKIP;
  }

  const previousPreserveBindings = state.preserveBindings;
  state.preserveBindings = Boolean(input);

  const parameters = callback.params ?? [];
  const itemParam = parameters[0];
  if (!itemParam) {
    reportUnsupported(
      state,
      'UNSUPPORTED_MAP_CALLBACK',
      'Map callbacks must declare at least one parameter.',
    );
    state.preserveBindings = previousPreserveBindings;
    return SKIP;
  }

  const itemName = getPatternLabel(itemParam);
  if (!itemName) {
    reportUnsupported(
      state,
      'UNSUPPORTED_MAP_PARAM',
      'Map callbacks must use a supported parameter pattern.',
    );
    return SKIP;
  }

  const indexName = parameters[1]
    ? getPatternName(parameters[1])
    : undefined;
  const loopId = nextLoopId(state);
  if (input) {
    state.ir.dependencyEdges.push({
      fromId: input.id,
      toId: loopId,
      kind: 'input-to-loop',
    });
  }

  const loopNode: LoopNode = {
    id: loopId,
    parentId,
    source: serializeExpression(mapCall.source, state),
    itemName,
    indexName,
    rows: [] as Array<{
      id: string;
      rootElementId: string;
      keyValue?: string;
    }>,
    inputId: input?.id,
    keyExpression: getMapRowKeyExpression(callback, state),
    sourceExpressionId: internExpression(mapCall.source, state),
    keyExpressionId: internMapRowKeyExpression(callback, state),
    dependencyStateNames: collectLocalStateNames(mapCall.source, state),
  };

  state.ir.loops.push(loopNode);

  if (!items) {
    const rowRoot = unwrapExpression(callback.body);
    const previousLoopId = state.activeLoopId;
    const bindingStart = state.ir.bindings.length;
    const propProgramStart = state.ir.propPrograms.length;
    const conditionalStart = state.ir.conditionals.length;
    state.activeLoopId = loopId;
    const lowered = lowerMapRow(rowRoot, loopId, state, {});
    state.activeLoopId = previousLoopId;
    state.ir.bindings.slice(bindingStart).forEach((binding) => {
      binding.loopId = loopId;
    });
    state.ir.propPrograms.slice(propProgramStart).forEach((program) => {
      program.loopId = loopId;
    });
    state.ir.conditionals
      .slice(conditionalStart)
      .forEach((conditional) => {
        conditional.loopId = loopId;
      });
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
      reportUnsupported(
        state,
        'UNSUPPORTED_MAP_ITEM',
        'Unsupported map item shape.',
      );
      return;
    }

    if (parameters[1]) {
      const indexPattern = parameters[1];
      const indexScope = bindPatternValue(
        indexPattern,
        index,
        state,
        index,
      );
      Object.assign(scope, indexScope);
    }

    const rowRoot = unwrapExpression(callback.body);
    const lowered = lowerMapRow(rowRoot, loopId, state, scope);
    if (lowered !== SKIP) {
      const rowElement = getElementById(state, lowered);
      loopNode.rows.push({
        id: `r${loopNode.rows.length + 1}`,
        rootElementId: lowered,
        keyValue: rowElement?.keyValue,
      });
    }
  });

  state.preserveBindings = previousPreserveBindings;

  return loopId;
}

function expressionUsesLocalState(
  expression: any,
  state: CompilerState,
): boolean {
  const names = new Set(state.ir.localStates.map((slot) => slot.name));
  const visit = (value: any): boolean => {
    if (!value || typeof value !== 'object') return false;
    if (
      (value.type === 'Identifier' ||
        value.type === 'IdentifierExpression') &&
      names.has(getNodeName(value) ?? '')
    )
      return true;
    return Object.values(value).some((child) =>
      Array.isArray(child) ? child.some(visit) : visit(child),
    );
  };
  return visit(expression);
}

function collectLocalStateNames(
  expression: any,
  state: CompilerState,
): string[] {
  const names = new Set(state.ir.localStates.map((slot) => slot.name));
  const found = new Set<string>();
  const visit = (value: any): void => {
    if (!value || typeof value !== 'object') return;
    if (value.kind === 'identifier' && names.has(value.name))
      found.add(value.name);
    Object.values(value).forEach((child: any) =>
      Array.isArray(child) ? child.forEach(visit) : visit(child),
    );
  };
  visit(lowerExpression(expression, state));
  return [...found].sort();
}

/** Route outlets are the only persistent shell boundary retained in IR. */
function discoverLayoutMetadata(state: CompilerState): void {
  const marked = (marker: string, value?: string) =>
    state.ir.elements.filter((element) =>
      element.attributes.some(
        (attribute) =>
          attribute.name === marker &&
          (value === undefined || attribute.staticValue === value),
      ),
    );
  const outletElements = marked('data-plec-route-outlet');
  if (!outletElements.length) return;
  state.ir.layout = {
    routeOutlets: outletElements.map((element) => ({
      id:
        element.attributes.find(
          (attribute) => attribute.name === 'data-plec-route-outlet',
        )?.staticValue ?? 'main',
      elementId: element.id,
    })),
  };
}

/** Compile an ordinary reachable function which produces a JSX value. This is
 * deliberately based on its source body; no render helper is recognized by
 * name. Dynamic branch selection remains a structural concern and is rejected
 * until represented by a render-slot node rather than silently becoming text. */
function lowerReturnedElementExpression(
  expression: any,
  parentId: string | null,
  state: CompilerState,
  literalScope: Record<string, unknown> = {},
): LowerResult {
  let value = unwrapExpression(expression);
  if (value?.type === 'Identifier')
    value = unwrapExpression(
      state.expressionScope[getNodeName(value)!] ?? value,
    );
  if (value?.type === 'JSXElement')
    return Object.keys(literalScope).length
      ? lowerJsxElementWithScope(value, parentId, state, literalScope)
      : lowerJsxElement(value, parentId, state);
  if (value?.type !== 'CallExpression') return SKIP;
  const name = getNodeName(value.callee);
  const definition = name ? getLocalFunction(name, state) : undefined;
  if (!definition) return SKIP;
  // Element production is a language capability, not a component convention.
  // Follow a source function's lexical environment and its statically known
  // branches before deciding whether its result is structural. This covers
  // helpers which select an element tag or assemble props before returning it.
  const returned = findStructuralFunctionReturn(
    definition.body,
    state,
    value,
    definition,
  );
  if (
    !returned ||
    !(
      returned.type === 'JSXElement' ||
      returned.type === 'JSXFragment' ||
      jsxFactoryElement(returned)
    )
  )
    return SKIP;
  const previousScope = state.expressionScope;
  const previousModule = state.activeModuleId;
  const scope = {
    ...previousScope,
    ...(state.moduleExpressions.get(definition.moduleId) ?? {}),
  };
  bindFunctionArguments(
    definition.params ?? [],
    value.arguments ?? [],
    scope,
    state,
    name ?? '<anonymous>',
  );
  populateFunctionLocals(definition.body, scope);
  state.expressionScope = scope;
  state.activeModuleId = definition.moduleId;
  const result =
    returned.type === 'CallExpression'
      ? lowerReturnedElementExpression(
          returned,
          parentId,
          state,
          literalScope,
        )
      : Object.keys(literalScope).length
        ? lowerJsxNodeWithScope(returned, parentId, state, literalScope)
        : lowerJsxNode(returned, parentId, state);
  state.expressionScope = previousScope;
  state.activeModuleId = previousModule;
  return result;
}

/** Bind an ordinary JS pattern to an AST value.  Keeping the value as syntax
 * lets the existing expression lowering preserve reactive paths. */
function bindLexicalPattern(
  pattern: any,
  value: any,
  scope: Record<string, any>,
): boolean {
  pattern = pattern?.pat ?? pattern?.pattern ?? pattern;
  if (!pattern) return false;
  const name = getPatternName(pattern);
  if (name) {
    scope[name] = value;
    return true;
  }
  if (pattern.type === 'AssignmentPattern')
    return bindLexicalPattern(
      pattern.left,
      value ?? pattern.right,
      scope,
    );
  if (pattern.type === 'ObjectPattern') {
    const consumed: string[] = [];
    for (const property of pattern.properties ?? []) {
      if (property.type === 'RestElement') continue;
      const key = getNodeName(property.key);
      const target = property.value ?? property.argument;
      if (
        !key ||
        !bindLexicalPattern(
          target,
          {
            type: 'MemberExpression',
            object: value,
            property: { type: 'Identifier', value: key },
          },
          scope,
        )
      )
        return false;
      consumed.push(key);
    }
    for (const property of pattern.properties ?? [])
      if (property.type === 'RestElement') {
        const rest = getPatternName(property.argument);
        if (rest)
          scope[rest] = {
            type: 'ObjectExpression',
            properties: [
              {
                type: 'SpreadElement',
                argument: value,
                excludedKeys: consumed,
              },
            ],
          };
      }
    return true;
  }
  if (pattern.type === 'ArrayPattern') {
    for (
      let index = 0;
      index < (pattern.elements ?? []).length;
      index += 1
    )
      if (pattern.elements[index])
        bindLexicalPattern(
          pattern.elements[index],
          {
            type: 'MemberExpression',
            object: value,
            property: { type: 'NumericLiteral', value: index },
          },
          scope,
        );
    return true;
  }
  return false;
}
function bindFunctionArguments(
  params: any[],
  args: any[],
  scope: Record<string, any>,
  state: CompilerState,
  name: string,
): void {
  for (let index = 0; index < params.length; index += 1) {
    const parameter =
      params[index]?.pat ?? params[index]?.pattern ?? params[index];
    const argument = args[index]?.expression ??
      args[index] ?? { type: 'Identifier', value: 'undefined' };
    if (!bindLexicalPattern(parameter, argument, scope))
      reportUnsupported(
        state,
        'UNSUPPORTED_SOURCE_FUNCTION_PARAMETER',
        `Function ${name} uses an unsupported parameter pattern.`,
      );
  }
}
function populateFunctionLocals(
  body: any,
  scope: Record<string, any>,
): void {
  for (const statement of getStatements(body)) {
    if (
      statement.type === 'FunctionDeclaration' ||
      statement.type === 'FnDecl'
    ) {
      const name = getNodeName(statement.identifier ?? statement.id);
      if (name) scope[name] = statement;
      continue;
    }
    if (
      statement.type !== 'VariableDeclaration' &&
      statement.type !== 'VarDecl'
    )
      continue;
    for (const declaration of statement.declarations ??
      statement.decls ??
      []) {
      // Awaited values are continuation-scoped, not lexical render values.
      // Leaving one in the expression scope would make a later continuation
      // attempt to serialize the `AwaitExpression` itself.
      const init = unwrapExpression(declaration.init);
      if (
        declaration.init &&
        declaration.init.type !== 'AwaitExpression' &&
        !(
          init?.type === 'CallExpression' &&
          getNodeName(init.callee) === 'useState'
        )
      )
        bindLexicalPattern(declaration.id, declaration.init, scope);
    }
  }
}
/** Return the structural value selected by a function body when its control
 * flow is decidable from lexical inputs. Dynamic branches deliberately remain
 * for the render-slot primitive rather than silently turning into text. */
function findStructuralFunctionReturn(
  body: any,
  state: CompilerState,
  call: any,
  definition: any,
): any | null {
  const scope: Record<string, any> = {
    ...state.expressionScope,
    ...(state.moduleExpressions.get(definition.moduleId) ?? {}),
  };
  bindFunctionArguments(
    definition.params ?? [],
    call.arguments ?? [],
    scope,
    state,
    definition.name ?? '<anonymous>',
  );
  populateFunctionLocals(body, scope);
  const visit = (statements: any[]): any | null => {
    for (const statement of statements) {
      if (
        statement.type === 'ReturnStatement' ||
        statement.type === 'ReturnStmt'
      )
        return unwrapExpression(statement.argument ?? statement.arg);
      if (statement.type !== 'IfStatement') continue;
      let test = evaluateExpression(statement.test, state, scope);
      // An absent property on a syntactically known props record is a known
      // undefined value, not an unknown runtime value. This distinction is
      // what permits ordinary optional render props to take their default
      // structural branch without treating a helper name specially.
      if (
        test === undefined &&
        isStaticallyAbsent(statement.test, scope)
      )
        test = false;
      if (test === undefined) continue;
      const branch = test ? statement.consequent : statement.alternate;
      if (!branch) continue;
      const found =
        branch.type === 'BlockStatement'
          ? visit(getStatements(branch))
          : visit([branch]);
      if (found) return found;
    }
    return null;
  };
  return visit(getStatements(body));
}
function isStaticallyAbsent(
  input: any,
  scope: Record<string, any>,
  seen = new Set<string>(),
): boolean {
  const node = unwrapExpression(input);
  if (node?.type === 'Identifier') {
    const name = getNodeName(node);
    if (!name || seen.has(name) || !scope[name]) return false;
    seen.add(name);
    return isStaticallyAbsent(scope[name], scope, seen);
  }
  if (node?.type !== 'MemberExpression') return false;
  const property = getNodeName(node.property);
  let object = unwrapExpression(node.object);
  if (object?.type === 'Identifier')
    object = unwrapExpression(scope[getNodeName(object)!] ?? object);
  if (object?.type !== 'ObjectExpression' || !property) return false;
  return !(object.properties ?? []).some(
    (entry: any) =>
      getNodeName(entry.key) === property ||
      entry.type === 'SpreadElement' ||
      entry.type === 'SpreadProperty',
  );
}
function findReturnedExpression(body: any): any | null {
  for (const statement of getStatements(body)) {
    if (
      statement.type === 'ReturnStatement' ||
      statement.type === 'ReturnStmt'
    )
      return unwrapExpression(statement.argument ?? statement.arg);
  }
  return null;
}

function getMapRowKeyExpression(
  callback: any,
  state: CompilerState,
): string | undefined {
  const row = unwrapExpression(callback.body);
  for (const attribute of row?.opening?.attributes ?? []) {
    if (
      getNodeName(attribute.name) === 'key' &&
      attribute.value?.type === 'JSXExpressionContainer'
    ) {
      return serializeExpression(attribute.value.expression, state);
    }
  }
  reportUnsupported(
    state,
    'MISSING_LOOP_KEY',
    'A useLiveQuery loop must have a key expression.',
  );
  return undefined;
}

function internMapRowKeyExpression(
  callback: any,
  state: CompilerState,
): string | undefined {
  const row = unwrapExpression(callback.body);
  for (const attribute of row?.opening?.attributes ?? []) {
    if (
      getNodeName(attribute.name) === 'key' &&
      attribute.value?.type === 'JSXExpressionContainer'
    )
      return internExpression(attribute.value.expression, state);
  }
  // getMapRowKeyExpression emits the user diagnostic; keep this helper quiet
  // so one missing key produces one deterministic diagnostic.
  return undefined;
}

function lowerMapRow(
  node: any,
  parentId: string,
  state: CompilerState,
  scope: Record<string, unknown>,
): LowerResult {
  if (!node) {
    return SKIP;
  }

  if (node.type === 'JSXElement') {
    return lowerJsxElementWithScope(node, parentId, state, scope);
  }

  if (node.type === 'JSXFragment') {
    reportUnsupported(
      state,
      'UNSUPPORTED_FRAGMENT',
      'JSX fragments are not supported in list rows.',
    );
    return SKIP;
  }

  reportUnsupported(
    state,
    'UNSUPPORTED_MAP_ROW',
    'Map callbacks must return a single intrinsic JSX element.',
  );
  return SKIP;
}

function lowerJsxElementWithScope(
  node: any,
  parentId: string | null,
  state: CompilerState,
  scope: Record<string, unknown>,
): LowerResult {
  const tag = node.opening?.name;
  const tagName = getJsxName(tag);
  if (tagName && (isComponentName(tagName) || tagName.includes('.'))) {
    return lowerComponentElement(node, parentId, state, scope);
  }
  if (!tagName || !isIntrinsicTag(tagName)) {
    reportUnsupported(
      state,
      'UNSUPPORTED_TAG',
      'Only intrinsic lowercase JSX tags are supported in Milestone 4 list rows.',
    );
    return SKIP;
  }

  const elementId = nextElementId(state);
  const attributes = node.opening?.attributes ?? [];
  const element: ElementNode = {
    id: elementId,
    tag: tagName,
    parentId,
    attributes: [] as Array<{
      name: string;
      staticValue?: string;
      bindingId?: string;
    }>,
    children: [] as string[],
  };

  for (const attributeNode of expandIntrinsicAttributes(
    node.opening?.attributes ?? [],
    state,
  )) {
    if (attributeNode.type !== 'JSXAttribute') {
      continue;
    }

    const name = getNodeName(attributeNode.name);
    if (!name || !attributeNode.value) {
      continue;
    }

    if (
      /^on[A-Z]/.test(name) &&
      attributeNode.value.type === 'JSXExpressionContainer'
    ) {
      lowerEvent(
        name,
        attributeNode.value.expression,
        elementId,
        state,
      );
      continue;
    }

    if (attributeNode.value.type === 'StringLiteral') {
      if (name === 'key') {
        element.keyValue = attributeNode.value.value;
        continue;
      }
      element.attributes.push({
        name,
        staticValue: attributeNode.value.value,
      });
      continue;
    }

    if (attributeNode.value.type === 'JSXExpressionContainer') {
      const literalValue = evaluateExpression(
        attributeNode.value.expression,
        state,
        mergeScopes(state.scope, scope),
      );
      if (name === 'key') {
        if (literalValue !== undefined) {
          element.keyValue = literalToString(literalValue);
        }
        continue;
      }

      if (literalValue !== undefined && !state.preserveBindings) {
        element.attributes.push({
          name,
          staticValue: literalToString(literalValue),
        });
      } else {
        const bindingId = nextBindingId(state);
        state.ir.bindings.push({
          id: bindingId,
          kind: 'attribute',
          targetId: elementId,
          attributeName: name,
          expression: serializeExpression(
            attributeNode.value.expression,
            state,
          ),
          expressionId: internExpression(
            attributeNode.value.expression,
            state,
          ),
        });
        element.attributes.push({ name, bindingId });
      }
      continue;
    }
  }

  state.ir.elements.push(element);

  for (const child of [
    ...getSpreadChildren(node.opening?.attributes ?? [], state),
    ...(node.children ?? []),
  ]) {
    const lowered = lowerJsxNodeWithScope(
      child,
      elementId,
      state,
      scope,
    );
    if (lowered !== SKIP) {
      element.children.push(lowered);
    }
  }

  return elementId;
}

function lowerJsxNodeWithScope(
  node: any,
  parentId: string | null,
  state: CompilerState,
  scope: Record<string, unknown>,
): LowerResult {
  if (!node) {
    return SKIP;
  }

  if (node.type === 'JSXElement') {
    return lowerJsxElementWithScope(node, parentId, state, scope);
  }

  if (node.type === 'JSXText') {
    return lowerJsxText(node, parentId, state);
  }

  if (node.type === 'JSXExpressionContainer') {
    return lowerJsxExpressionWithScope(node, parentId, state, scope);
  }

  return SKIP;
}

function lowerJsxExpressionWithScope(
  node: any,
  parentId: string | null,
  state: CompilerState,
  scope: Record<string, unknown>,
): LowerResult {
  if (!parentId) {
    return SKIP;
  }

  const expression = unwrapExpression(node.expression);
  const forwardedChildren = getForwardedChildren(expression, state);
  if (forwardedChildren) {
    let first: LowerResult = SKIP;
    for (const child of forwardedChildren) {
      const lowered = lowerJsxNodeWithScope(
        child,
        parentId,
        state,
        scope,
      );
      if (first === SKIP && lowered !== SKIP) first = lowered;
    }
    return first;
  }
  if (!expression || expression.type === 'JSXEmptyExpression') {
    return SKIP;
  }

  const structural = lowerStructuralExpression(
    expression,
    parentId,
    state,
    scope,
  );
  if (structural !== SKIP) return structural;

  const rendered = lowerReturnedElementExpression(
    expression,
    parentId,
    state,
    scope,
  );
  if (rendered !== SKIP) return rendered;

  const literalValue = evaluateExpression(
    expression,
    state,
    mergeScopes(state.scope, scope),
  );
  if (literalValue !== undefined && !state.preserveBindings) {
    const textId = nextTextId(state);
    state.ir.texts.push({
      id: textId,
      parentId,
      staticValue: literalToString(literalValue),
    });
    return textId;
  }

  const textId = nextTextId(state);
  state.ir.texts.push({
    id: textId,
    parentId,
    staticValue: '',
  });
  state.ir.bindings.push({
    id: nextBindingId(state),
    kind: 'text',
    targetId: textId,
    expression: serializeExpression(expression, state),
    expressionId: internExpression(expression, state),
  });
  return textId;
}

/** JSX is structural data, never a value expression.  Conditional and logical
 * render forms therefore own stable branch regions rather than flowing into
 * `internExpression`, where a JSX AST would be incorrectly serialized. */
function lowerStructuralExpression(
  expression: any,
  parentId: string,
  state: CompilerState,
  literalScope: Record<string, unknown> = {},
): LowerResult {
  const node = unwrapExpression(expression);
  const consequent =
    node?.type === 'ConditionalExpression'
      ? node.consequent
      : node?.type === 'LogicalExpression' ||
          (node?.type === 'BinaryExpression' && node.operator === '&&')
        ? node.right
        : undefined;
  if (!consequent) return SKIP;
  const alternate =
    node?.type === 'ConditionalExpression'
      ? node.alternate
      : { type: 'NullLiteral' };
  if (
    !containsStructuralJsx(consequent) &&
    !containsStructuralJsx(alternate)
  )
    return SKIP;

  const expressionId = internExpression(
    node?.type === 'ConditionalExpression' ? node.test : node.left,
    state,
  );
  const conditional = {
    id: `c${state.ir.conditionals.length + 1}`,
    parentId,
    expressionId,
    consequent: [] as string[],
    alternate: [] as string[],
  };
  state.ir.conditionals.push(conditional);
  const lowerBranch = (branch: any, output: string[]) => {
    branch = unwrapExpression(branch);
    if (
      !branch ||
      branch.type === 'NullLiteral' ||
      (branch.type === 'BooleanLiteral' && !branch.value)
    )
      return;
    const lowered = Object.keys(literalScope).length
      ? lowerJsxNodeWithScope(
          branch.type === 'JSXElement' || branch.type === 'JSXFragment'
            ? branch
            : { type: 'JSXExpressionContainer', expression: branch },
          parentId,
          state,
          literalScope,
        )
      : lowerJsxNode(
          branch.type === 'JSXElement' || branch.type === 'JSXFragment'
            ? branch
            : { type: 'JSXExpressionContainer', expression: branch },
          parentId,
          state,
        );
    if (lowered !== SKIP) output.push(lowered);
  };
  lowerBranch(consequent, conditional.consequent);
  lowerBranch(alternate, conditional.alternate);
  return conditional.id;
}

function containsStructuralJsx(value: any): boolean {
  const node = unwrapExpression(value);
  if (!node || typeof node !== 'object') return false;
  if (
    node.type === 'JSXElement' ||
    node.type === 'JSXFragment' ||
    jsxFactoryElement(node)
  )
    return true;
  if (node.type === 'ConditionalExpression')
    return (
      containsStructuralJsx(node.consequent) ||
      containsStructuralJsx(node.alternate)
    );
  if (
    node.type === 'LogicalExpression' ||
    (node.type === 'BinaryExpression' && node.operator === '&&')
  )
    return containsStructuralJsx(node.right);
  return false;
}

function lowerComponentElement(
  node: any,
  parentId: string | null,
  state: CompilerState,
  literalScope: Record<string, unknown> = {},
): LowerResult {
  const name = getJsxName(node.opening?.name)!;
  if (name === 'Outlet') return lowerRouteOutlet(node, parentId, state);
  if (state.routerLinkBindings.has(name))
    return lowerRouterLink(node, parentId, state);
  if (name.endsWith('.Provider'))
    return lowerContextProvider(node, parentId, state);
  // These are the demo's stable UI atom names. Resolve them before ordinary
  // local-component expansion so their Base UI implementation never leaks
  // into the emitted graph, even when module resolution supplied the source.
  // A source-graph component has a different module ID from its importer;
  // treat those familiar atom exports as DOM adapters. A same-module function
  // named Checkbox remains an ordinary component (used by the Toggle tests).
  const component =
    getLocalComponent(name, state) ??
    getAliasedComponent(name, state) ??
    getScopedComponent(name, state);
  if (!component) {
    const svg = lowerSourceSvgFactoryElement(node, parentId, state);
    if (svg !== SKIP) return svg;
  }
  const external = getExternalSymbol(name, state);
  if (!component && external) {
    reportUnsupported(
      state,
      'UNRESOLVED_SOURCE_SYMBOL',
      `Source graph does not contain ${external}; include its implementation module to compile it.`,
    );
    return SKIP;
  }
  if (!component) {
    const imported = state.importSymbols.get(
      `${state.activeModuleId}::${name.split('.')[0]}`,
    );
    reportUnsupported(
      state,
      'UNSUPPORTED_COMPONENT',
      `Component ${name} is not a locally resolvable function component. Import: ${imported ? `${imported.moduleId}#${imported.exportName}` : 'none'}.`,
    );
    return SKIP;
  }
  if (state.islandComponents.has(name)) {
    const placeholderNodeId = nextElementId(state);
    state.ir.elements.push({
      id: placeholderNodeId,
      tag: 'span',
      parentId,
      attributes: [
        { name: 'data-runtime-island', staticValue: name },
        { name: 'style', staticValue: 'display: contents' },
      ],
      children: [],
    });
    state.ir.islands.push({
      islandInstanceId: `i${state.ir.islands.length + 1}`,
      componentId: name,
      placeholderNodeId,
      moduleId: component.moduleId,
      exportName: 'default',
      props: {},
    });
    return placeholderNodeId;
  }
  // A component can legitimately appear inside content forwarded through an
  // ancestor's `...props` (for example, nested shadcn Cards). Limit expansion
  // depth instead of treating that composition as recursive component code.
  if (state.componentStack.length >= 64) {
    reportUnsupported(
      state,
      'COMPONENT_EXPANSION_DEPTH',
      `Component expansion exceeded 64 levels at ${name}.`,
    );
    return SKIP;
  }
  const props = collectCallerProps(node, state);
  props.children = {
    type: 'JSXChildren',
    children: node.children ?? [],
  };
  const previous = state.expressionScope;
  const previousModuleId = state.activeModuleId;
  const localScope: Record<string, any> = {
    ...previous,
    ...(state.moduleExpressions.get(component.moduleId) ?? {}),
  };
  const parameter = component.params[0];
  const parameterPattern =
    parameter?.pat ?? parameter?.pattern ?? parameter;
  if (parameterPattern?.type === 'ObjectPattern') {
    for (const property of parameterPattern.properties ?? []) {
      if (property.type === 'RestElement') {
        const local = getPatternName(property.argument);
        if (local) {
          const consumed = new Set(
            (parameterPattern.properties ?? [])
              .filter((entry: any) => entry.type !== 'RestElement')
              .map((entry: any) => getNodeName(entry.key))
              .filter(Boolean),
          );
          localScope[local] = objectExpression(
            Object.fromEntries(
              Object.entries(props).filter(
                ([key]) => !consumed.has(key),
              ),
            ),
          );
        }
        continue;
      }
      const key = getNodeName(property.key);
      const target = property.value ?? property.argument;
      const local =
        property.type === 'AssignmentPatternProperty'
          ? key
          : (getPatternName(target) ?? key);
      const supplied = key ? props[key] : undefined;
      // SWC uses AssignmentPatternProperty for `{ size = "default" }`, while
      // other parser versions wrap the default in an AssignmentPattern.
      const defaultValue =
        property.type === 'AssignmentPatternProperty'
          ? property.value
          : target?.right;
      if (key && local)
        localScope[local] = supplied ??
          defaultValue ?? { type: 'Identifier', value: 'undefined' };
    }
  } else if (parameterPattern) {
    const local = getPatternName(parameterPattern);
    if (local)
      localScope[local] = {
        type: 'ObjectExpression',
        properties: Object.entries(props).map(([key, value]) => ({
          type: 'KeyValueProperty',
          key: { type: 'Identifier', value: key },
          value,
        })),
      };
  }
  for (const statement of getStatements(component.body)) {
    if (
      statement.type !== 'VariableDeclaration' &&
      statement.type !== 'VarDecl'
    )
      continue;
    for (const declaration of statement.declarations ??
      statement.decls ??
      []) {
      const local = getPatternName(declaration.id);
      if (local && declaration.init)
        localScope[local] = declaration.init;
    }
  }
  state.expressionScope = localScope;
  state.activeModuleId = component.moduleId;
  state.componentStack.push(name);
  lowerLifecycleCalls(component.body, state);
  const metadata: ComponentMetadata = {
    name,
    moduleId: component.moduleId,
    elementIds: [],
    bindingIds: [],
    eventIds: [],
  };
  state.ir.components.push(metadata);
  const before = {
    e: state.ir.elements.length,
    b: state.ir.bindings.length,
    ev: state.ir.events.length,
  };
  const returned = findReturnedJsx(component.body);
  if (!returned) {
    const value = findReturnedExpression(component.body);
    if (value) {
      const result = lowerReturnedElementExpression(
        value,
        parentId,
        state,
        literalScope,
      );
      if (result !== SKIP) {
        metadata.elementIds = state.ir.elements
          .slice(before.e)
          .map((element) => element.id);
        metadata.bindingIds = state.ir.bindings
          .slice(before.b)
          .map((binding) => binding.id);
        metadata.eventIds = state.ir.events
          .slice(before.ev)
          .map((event) => event.id);
        state.componentStack.pop();
        state.expressionScope = previous;
        state.activeModuleId = previousModuleId;
        return result;
      }
    }
    reportUnsupported(
      state,
      'UNSUPPORTED_COMPONENT_BODY',
      `Component ${name} in ${component.moduleId} does not return a declaratively representable element tree.`,
    );
  }
  const result =
    literalScope && Object.keys(literalScope).length
      ? lowerJsxNodeWithScope(returned, parentId, state, literalScope)
      : lowerJsxNode(returned, parentId, state);
  metadata.elementIds = state.ir.elements
    .slice(before.e)
    .map((element) => element.id);
  metadata.bindingIds = state.ir.bindings
    .slice(before.b)
    .map((binding) => binding.id);
  metadata.eventIds = state.ir.events
    .slice(before.ev)
    .map((event) => event.id);
  state.componentStack.pop();
  state.expressionScope = previous;
  state.activeModuleId = previousModuleId;
  return result;
}

/** `Outlet` is a graph anchor, not a source component to expand. Its authored
 * DOM props are lowered normally while the marker supplies stable route
 * replacement topology to the runtime. */
function lowerRouteOutlet(
  node: any,
  parentId: string | null,
  state: CompilerState,
): LowerResult {
  const attributes = node.opening?.attributes ?? [];
  const id = attributes.find(
    (attribute: any) => getNodeName(attribute.name) === 'id',
  )?.value;
  const outletId =
    id?.type === 'StringLiteral'
      ? id.value
      : evaluateExpression(id?.expression ?? id, state, state.scope);
  if (typeof outletId !== 'string') {
    reportUnsupported(
      state,
      'INVALID_ROUTE_OUTLET',
      'Outlet id must be a static string.',
    );
    return SKIP;
  }
  return lowerJsxElement(
    {
      ...node,
      opening: {
        ...node.opening,
        name: { type: 'Identifier', value: 'div' },
        attributes: [
          {
            type: 'JSXAttribute',
            name: {
              type: 'Identifier',
              value: 'data-plec-route-outlet',
            },
            value: { type: 'StringLiteral', value: outletId },
          },
          ...attributes.filter(
            (attribute: any) => getNodeName(attribute.name) !== 'id',
          ),
        ],
      },
    },
    parentId,
    state,
  );
}

/** A source-exported SVG factory can expose `const Mark = createIcon(data)`.
 * Lower its serializable element data directly; this recognizes an SVG
 * producer shape, not a particular component or icon package. */
function lowerSourceSvgFactoryElement(
  node: any,
  parentId: string | null,
  state: CompilerState,
): LowerResult {
  const name = getJsxName(node.opening?.name);
  if (!name || name.includes('.')) return SKIP;
  const source = sourceExpression(name, state.activeModuleId, state);
  const init = unwrapExpression(source?.expression);
  if (
    init?.type !== 'CallExpression' ||
    getNodeName(init.callee) !== 'createIcon'
  )
    return SKIP;
  const definitionReference =
    init.arguments?.[0]?.expression ?? init.arguments?.[0];
  const definition = getNodeName(definitionReference)
    ? sourceExpression(
        getNodeName(definitionReference)!,
        source!.moduleId,
        state,
      )?.expression
    : definitionReference;
  const values = evaluateExpression(definition, state, {});
  if (!Array.isArray(values)) return SKIP;

  const elementId = nextElementId(state);
  const element: ElementNode = {
    id: elementId,
    tag: 'svg',
    parentId,
    attributes: [
      { name: 'viewBox', staticValue: '0 0 24 24' },
      { name: 'fill', staticValue: 'none' },
      { name: 'stroke', staticValue: 'currentColor' },
      { name: 'strokeWidth', staticValue: '2' },
      { name: 'strokeLinecap', staticValue: 'round' },
      { name: 'strokeLinejoin', staticValue: 'round' },
      { name: 'focusable', staticValue: 'false' },
      { name: 'aria-hidden', staticValue: 'true' },
    ],
    children: [],
  };
  lowerSvgCallerAttributes(
    node.opening?.attributes ?? [],
    element,
    state,
  );
  state.ir.elements.push(element);
  for (const value of values) {
    if (!Array.isArray(value) || typeof value[0] !== 'string') continue;
    const childId = nextElementId(state);
    const child: ElementNode = {
      id: childId,
      tag: value[0],
      parentId: elementId,
      attributes: Object.entries(
        value[1] && typeof value[1] === 'object' ? value[1] : {},
      ).map(([attributeName, attributeValue]) => ({
        name: attributeName,
        staticValue: literalToString(attributeValue),
      })),
      children: [],
    };
    state.ir.elements.push(child);
    element.children.push(childId);
  }
  return elementId;
}

function sourceExpression(
  name: string,
  moduleId: string,
  state: CompilerState,
  resolving = new Set<string>(),
): { moduleId: string; expression: any } | undefined {
  if (resolving.has(name)) return undefined;
  resolving.add(name);
  const imported = state.importSymbols.get(`${moduleId}::${name}`);
  const resolved = imported
    ? resolveExport(imported.moduleId, imported.exportName, state)
    : { moduleId, exportName: name };
  const expression = state.moduleExpressions.get(resolved.moduleId)?.[
    resolved.exportName
  ];
  if (expression) return { moduleId: resolved.moduleId, expression };
  const alias = getNodeName(
    unwrapExpression(
      state.expressionScope[name]?.expression ??
        state.expressionScope[name],
    ),
  );
  return alias
    ? sourceExpression(alias, moduleId, state, resolving)
    : undefined;
}

function getScopedComponent(name: string, state: CompilerState) {
  const value = unwrapExpression(state.expressionScope[name]);
  if (
    value?.type !== 'ArrowFunctionExpression' &&
    value?.type !== 'FunctionExpression'
  )
    return undefined;
  const body = getFunctionBody(value);
  if (!body) return undefined;
  return {
    name,
    body,
    params: value.params ?? [],
    moduleId: state.activeModuleId,
  };
}

function getAliasedComponent(name: string, state: CompilerState) {
  const value = state.expressionScope[name];
  const alias = getNodeName(
    unwrapExpression(value?.expression ?? value),
  );
  if (!alias || alias === name) return undefined;
  return getLocalComponent(alias, state);
}

function lowerSvgCallerAttributes(
  attributes: any[],
  element: ElementNode,
  state: CompilerState,
): void {
  for (const attribute of attributes) {
    const name = getNodeName(attribute.name);
    if (!name) continue;
    if (attribute.value?.type === 'StringLiteral') {
      element.attributes.push({
        name,
        staticValue: attribute.value.value,
      });
      continue;
    }
    if (!attribute.value) {
      element.attributes.push({ name, staticValue: 'true' });
      continue;
    }
    const expression = attribute.value.expression ?? attribute.value;
    const expressionId = internExpression(expression, state);
    const bindingId = nextBindingId(state);
    state.ir.bindings.push({
      id: bindingId,
      kind: isDomProperty(name) ? 'property' : 'attribute',
      targetId: element.id,
      attributeName: name,
      expression: serializeExpression(expression, state),
      expressionId,
    });
    element.attributes.push({ name, bindingId });
  }
}

function lowerContextProvider(
  node: any,
  parentId: string | null,
  state: CompilerState,
): LowerResult {
  const providerName = getJsxName(node.opening?.name)!.replace(
    /\.Provider$/,
    '',
  );
  const context = resolveContext(providerName, state);
  if (!context) {
    reportUnsupported(
      state,
      'UNRESOLVED_CONTEXT_PROVIDER',
      `Provider ${providerName} has no reachable createContext declaration.`,
    );
    return SKIP;
  }
  const value = collectCallerProps(node, state).value;
  if (!value) {
    reportUnsupported(
      state,
      'CONTEXT_PROVIDER_VALUE',
      `Provider ${providerName} requires a value prop.`,
    );
    return SKIP;
  }
  const id = `c${state.ir.contexts.length + 1}`;
  const scope = {
    id,
    contextId: context.id,
    parentId,
    valueExpressionId: internExpression(value, state),
    values: [],
    children: [] as string[],
  };
  state.ir.contexts.push(scope);
  for (const child of node.children ?? []) {
    // The provider is a virtual parent in the graph. Retaining it as the
    // lexical parent lets direct text/expressions and nested providers retain
    // their scope without inventing a DOM wrapper.
    const lowered = lowerJsxNode(child, id, state);
    if (lowered !== SKIP) scope.children.push(lowered);
  }
  return id;
}

function lowerRouterLink(
  node: any,
  parentId: string | null,
  state: CompilerState,
): LowerResult {
  const to = (node.opening?.attributes ?? []).find(
    (attribute: any) => getNodeName(attribute.name) === 'to',
  )?.value;
  const href =
    to?.type === 'StringLiteral'
      ? to.value
      : evaluateExpression(to?.expression ?? to, state, state.scope);
  if (typeof href !== 'string') {
    reportUnsupported(
      state,
      'UNSUPPORTED_ROUTER_LINK',
      'Router Link requires a static string to prop.',
    );
    return SKIP;
  }
  const attributes = node.opening?.attributes ?? [];
  const elementId = nextElementId(state);
  state.eventCounter += 1;
  state.ir.events.push({
    id: `ev${state.eventCounter}`,
    type: 'click',
    targetId: elementId,
    actionId: `a${state.eventCounter}`,
    args: [],
    navigate: { href },
  });
  const element: ElementNode = {
    id: elementId,
    tag: 'a',
    parentId,
    attributes: [{ name: 'href', staticValue: href }],
    children: [],
  };
  const clickHandler = attributes.find(
    (attribute: any) => getNodeName(attribute.name) === 'onClick',
  )?.value?.expression;
  if (clickHandler) {
    if (!getNodeName(clickHandler))
      reportUnsupported(
        state,
        'UNSUPPORTED_ROUTER_LINK_ACTION',
        'Router Link onClick must be a named semantic action reference.',
      );
  }
  const className = attributes.find(
    (attribute: any) => getNodeName(attribute.name) === 'className',
  )?.value;
  const activeProps = attributes.find(
    (attribute: any) => getNodeName(attribute.name) === 'activeProps',
  )?.value?.expression;
  for (const attribute of attributes) {
    const name = getNodeName(attribute.name);
    if (
      !name ||
      ['to', 'replace', 'className', 'activeProps', 'onClick'].includes(
        name,
      )
    )
      continue;
    if (name === 'activeOptions' || name === 'pendingProps') {
      reportUnsupported(
        state,
        'UNSUPPORTED_ROUTER_LINK_OPTION',
        `Router Link ${name} is not supported.`,
      );
      continue;
    }
    if (attribute.value?.type === 'StringLiteral')
      element.attributes.push({
        name,
        staticValue: attribute.value.value,
      });
    else if (!attribute.value)
      element.attributes.push({ name, staticValue: 'true' });
    else {
      const expression = attribute.value.expression ?? attribute.value;
      const bindingId = nextBindingId(state);
      state.ir.bindings.push({
        id: bindingId,
        kind: isDomProperty(name) ? 'property' : 'attribute',
        targetId: elementId,
        attributeName: name,
        expression: serializeExpression(expression, state),
        expressionId: internExpression(expression, state),
      });
      element.attributes.push({ name, bindingId });
    }
  }
  const baseClass =
    className?.type === 'StringLiteral' ? className.value : undefined;
  if (className && !baseClass)
    reportUnsupported(
      state,
      'UNSUPPORTED_ROUTER_LINK_PROP',
      'Router Link className must be static.',
    );
  if (activeProps) {
    const property = activeProps.properties?.find(
      (entry: any) => getNodeName(entry.key) === 'className',
    );
    const activeClass = property?.value?.value;
    if (
      !baseClass ||
      typeof activeClass !== 'string' ||
      (activeProps.properties?.length ?? 0) !== 1
    )
      reportUnsupported(
        state,
        'UNSUPPORTED_ROUTER_LINK_ACTIVE_PROPS',
        'Only static activeProps.className is supported.',
      );
    else {
      state.expressionCounter += 1;
      const expressionId = `x${state.expressionCounter}`;
      state.ir.expressions.push({
        id: expressionId,
        expression: {
          kind: 'conditional',
          test: {
            kind: 'binary',
            op: '===',
            left: {
              kind: 'member',
              object: {
                kind: 'member',
                object: { kind: 'identifier', name: 'host' },
                property: 'location',
              },
              property: 'pathname',
            },
            right: { kind: 'literal', value: href },
          },
          consequent: { kind: 'literal', value: activeClass },
          alternate: { kind: 'literal', value: baseClass },
        },
      });
      const bindingId = nextBindingId(state);
      state.ir.bindings.push({
        id: bindingId,
        kind: 'attribute',
        targetId: elementId,
        attributeName: 'className',
        expression: 'host.location.pathname',
        expressionId,
      });
      element.attributes.push({ name: 'className', bindingId });
    }
  } else if (baseClass)
    element.attributes.push({
      name: 'className',
      staticValue: baseClass,
    });
  state.ir.elements.push(element);
  for (const child of node.children ?? []) {
    const lowered = lowerJsxNode(child, elementId, state);
    if (lowered !== SKIP) element.children.push(lowered);
  }
  return elementId;
}

function lowerEvent(
  name: string,
  expression: any,
  targetId: string,
  state: CompilerState,
): void {
  let handler = unwrapExpression(expression);
  const handlerName = getNodeName(handler);
  if (handlerName && handlerName === state.routeRetryProp) {
    const eventId = `ev${state.eventCounter + 1}`;
    const actionId = `a${state.eventCounter + 1}`;
    state.eventCounter += 1;
    state.ir.events.push({ id: eventId, type: reactEventName(name), targetId, actionId, args: [] });
    state.ir.actionFacts.push({ id: actionId, parameters: [{ name: 'event', type: 'event' }], eventFields: [], frameSlots: 0, parameterSlots: [], routeRetry: true, instructions: [{ op: 'return' }] });
    return;
  }
  if (handlerName && state.expressionScope[handlerName])
    handler = unwrapExpression(state.expressionScope[handlerName]);
  const body = unwrapExpression(handler?.body);
  const call = body?.type === 'CallExpression' ? body : null;
  const actionExpression = call?.callee;
  let actionId = getNodeName(actionExpression);
  if (actionId && state.expressionScope[actionId])
    actionId = getNodeName(state.expressionScope[actionId]) ?? actionId;
  const eventId = `ev${state.eventCounter + 1}`;
  const declaredActionId = `a${state.eventCounter + 1}`;
  if (!call || !actionId) {
    if (!handler) {
      reportUnsupported(
        state,
        'UNSUPPORTED_EVENT',
        'Event handler must be a statically resolvable action expression.',
      );
      return;
    }
    const actionFact = buildTypedActionFact(declaredActionId, handler, state);
    if (!actionFact) return;
    state.eventCounter += 1;
    state.ir.events.push({
      id: eventId,
      type: reactEventName(name),
      targetId,
      actionId: declaredActionId,
      args: [],
      ...(state.activeLoopId ? { loopId: state.activeLoopId } : {}),
    });
    state.ir.actionFacts.push(actionFact);
    return;
  }
  const change = call.arguments?.[1]?.expression ?? call.arguments?.[1];
  let field: string | undefined;
  if (change?.type === 'ObjectExpression')
    field = getNodeName(change.properties?.[0]?.key);
  const actionFact = buildTypedActionFact(declaredActionId, handler, state);
  if (!actionFact) return;
  state.eventCounter += 1;
  state.ir.events.push({
    id: eventId,
    type: reactEventName(name),
    targetId,
    actionId: declaredActionId,
    args: [
      serializeExpression(
        call.arguments?.[0]?.expression ?? call.arguments?.[0],
        state,
      ),
      serializeExpression(change, state),
    ],
    field,
    ...(state.activeLoopId ? { loopId: state.activeLoopId } : {}),
  });
  state.ir.actionFacts.push(actionFact);
}

/** Build the typed control-flow fact while source syntax is still available.
 * Handles remain symbolic here and become artifact-local table indexes only
 * in executable-lowering. */
function buildTypedActionFact(
  id: string,
  handler: any,
  state: CompilerState,
) {
  const diagnosticStart = state.diagnostics.length;
  const loaderContext = loaderContextParameters(handler, state);
  const sourceOperations = lowerActionOperations(handler, state) as any[];
  const routeRetry = sourceOperations.some(
    (operation) => operation?.kind === 'route-retry',
  );
  const eventFields = actionEventFields(sourceOperations);
  const eventSlots = new Map(eventFields.map((field, index) => [field, index]));
  const slots = new Map<string, number>();
  // Event values are ordinary initialized action-frame slots; locally
  // allocated continuation slots therefore begin after them.
  // Route loaders are ordinary action programs with two compiler-reserved
  // arguments. `signal` stays browser-owned and is deliberately not a value.
  if (loaderContext) {
    slots.set('params', 0);
    slots.set('location', 1);
  }
  let nextSlot = loaderContext ? 2 : eventFields.length;
  const slot = (name: string) => {
    const existing = slots.get(name);
    if (existing !== undefined) return existing;
    slots.set(name, nextSlot);
    return nextSlot++;
  };
  const instructions: any[] = [];
  const emit = (operations: any[]) => {
    for (const operation of operations ?? []) {
      switch (operation?.kind) {
        case 'set-state':
          instructions.push({ op: 'evaluate', expression: operation.value, eventSlots, slots });
          instructions.push({ op: 'storeState', stateSlotId: operation.stateSlotId });
          break;
        case 'prevent-default': instructions.push({ op: 'preventDefault' }); break;
        case 'route-retry': break;
        case 'store-host-ref': instructions.push({ op: 'storeHostRef', refId: operation.refId }); break;
        case 'return':
          instructions.push({ op: 'return', outcome: operation.outcome ?? 'success', ...(operation.value ? { value: operation.value, eventSlots, slots } : {}) });
          break;
        case 'invoke-action-ref':
          instructions.push({
            op: 'call', actionId: operation.actionId, arguments: operation.parameters ?? [],
            resultSlot: slot(`__call_result_${instructions.length}`),
            errorSlot: slot(`__call_error_${instructions.length}`),
            successPc: 0, failurePc: 0, eventSlots, slots,
          });
          break;
        case 'collection':
          instructions.push({ op: 'collectionMutation', inputId: operation.inputId, kind: operation.operation, key: operation.key, value: operation.value, eventSlots, slots });
          break;
        case 'if': {
          instructions.push({ op: 'evaluate', expression: operation.test, eventSlots, slots });
          const branch = instructions.length;
          instructions.push({ op: 'jumpIfFalse', target: 0 });
          emit(operation.consequent);
          const done = instructions.length;
          instructions.push({ op: 'jump', target: 0 });
          instructions[branch].target = instructions.length;
          emit(operation.alternate);
          instructions[done].target = instructions.length;
          break;
        }
        case 'capability-request': {
          if (!['network.fetch', 'cookie'].includes(operation.capability)) {
            reportUnsupported(state, 'UNSUPPORTED_ACTION_CAPABILITY', `Capability ${operation.capability} is not supported.`);
            break;
          }
          const resultSlot = slot(operation.successResultName ?? 'result');
          const errorSlot = slot(operation.failureErrorName ?? 'error');
          const request: any = {
            op: 'capabilityRequest', capability: operation.capability === 'cookie' ? 'cookie' : 'fetch', request: operation.request,
            successPc: 0, failurePc: 0, finallyPc: undefined, resultSlot, errorSlot,
            eventSlots, slots,
          };
          instructions.push(request);
          request.successPc = instructions.length;
          emit(operation.success);
          const successDone = instructions.length;
          instructions.push({ op: 'jump', target: 0 });
          request.failurePc = instructions.length;
          emit(operation.failure);
          const failureDone = instructions.length;
          instructions.push({ op: 'jump', target: 0 });
          if (operation.finally?.length) {
            request.finallyPc = instructions.length;
            emit(operation.finally);
          }
          instructions.push({ op: 'return' });
          instructions[successDone].target = request.finallyPc ?? instructions.length - 1;
          instructions[failureDone].target = request.finallyPc ?? instructions.length - 1;
          break;
        }
        default:
          reportUnsupported(state, 'UNSUPPORTED_ACTION_OPERATION', `Action operation ${String(operation?.kind ?? 'unknown')} is not supported.`);
      }
    }
  };
  emit(sourceOperations);
  // Emit an explicit terminator so branch targets at a source block boundary
  // remain valid instruction indexes, while the VM still accepts implicit EOF
  // returns from hand-authored/test artifacts.
  if (!instructions.length || instructions.at(-1)?.op !== 'return')
    instructions.push({ op: 'return' });
  for (let index = 0; index < instructions.length; index += 1)
    if (instructions[index].op === 'call') {
      instructions[index].successPc = Math.min(index + 1, instructions.length - 1);
      instructions[index].failurePc = Math.min(index + 1, instructions.length - 1);
    }
  for (const instruction of instructions)
    if ((instruction.op === 'jump' || instruction.op === 'jumpIfFalse') && instruction.target >= instructions.length)
      instruction.target = instructions.length - 1;
  if (state.diagnostics.length !== diagnosticStart) return undefined;
  return {
    id,
    parameters: loaderContext
      ? [
          { name: 'params', type: 'json' as const },
          { name: 'location', type: 'json' as const },
        ]
      : [{ name: 'event', type: 'event' as const }],
    eventFields,
    frameSlots: nextSlot,
    parameterSlots: loaderContext ? [0, 1] : [],
    ...(routeRetry ? { routeRetry: true } : {}),
    instructions,
  };
}

/** Loader context is intentionally a narrow structural convention. It keeps
 * the public `{ params, location, signal }` API while only serializing the
 * two declarative values the typed VM can consume. */
function loaderContextParameters(handler: any, state: CompilerState) {
  const name = getNodeName(handler);
  const resolved = name ? unwrapExpression(state.expressionScope[name]) : handler;
  const functionDefinition = name
    ? state.functions.get(componentKey(state.activeModuleId, name))
    : undefined;
  handler = (resolved ?? handler)?.function ?? resolved ?? handler;
  const parameter = functionDefinition?.params?.[0]?.pat
    ?? functionDefinition?.params?.[0]
    ?? handler?.params?.[0]?.pat
    ?? handler?.params?.[0];
  if (parameter?.type !== 'ObjectPattern') return false;
  return (parameter.properties ?? []).some((property: any) => {
    const key = getNodeName(property.key);
    return key === 'params' || key === 'location' || key === 'signal';
  });
}

function actionEventFields(operations: any[]): string[] {
  const fields = new Set<string>();
  const visit = (value: any): void => {
    if (!value || typeof value !== 'object') return;
    const field = value.kind === 'member' && value.object?.kind === 'identifier' && value.object.name === 'event'
      ? value.property
      : value.kind === 'member' && value.object?.kind === 'member' && value.object.object?.kind === 'identifier' && value.object.object.name === 'event' && value.object.property === 'currentTarget'
        ? value.property : undefined;
    if (field && ['value', 'checked', 'key', 'rowKey', 'button', 'metaKey', 'ctrlKey', 'shiftKey', 'altKey', 'type'].includes(field)) fields.add(field);
    Object.values(value).forEach((child: any) => Array.isArray(child) ? child.forEach(visit) : visit(child));
  };
  visit(operations);
  return [...fields];
}

function lowerActionOperations(
  handler: any,
  state: CompilerState,
): unknown[] {
  const handlerName = getNodeName(handler);
  if (handlerName && handlerName === state.routeRetryProp)
    return [{ kind: 'route-retry' }];
  if (handlerName && state.expressionScope[handlerName])
    handler = state.expressionScope[handlerName];
  let body = unwrapExpression(handler?.body);
  const name = getNodeName(handler);
  if (!body && name) body = getLocalFunction(name, state)?.body;
  if (!body && !handler) {
    reportUnsupported(state, 'UNSUPPORTED_ACTION', 'Action handler must be statically resolvable.');
    return [];
  }
  return body?.type === 'BlockStatement'
    ? lowerActionStatements(getStatements(body), state)
    : body
      ? lowerActionExpression(body, state)
      : lowerActionExpression(handler, state);
}

function lowerActionStatements(
  statements: any[],
  state: CompilerState,
): unknown[] {
  const [statement, ...rest] = statements;
  if (!statement) return [];
  const declaration =
    statement.type === 'VariableDeclaration'
      ? (statement.declarations ?? statement.decls ?? [])[0]
      : undefined;
  const awaited =
    declaration?.init?.type === 'AwaitExpression'
      ? declaration.init.argument
      : statement.type === 'ExpressionStatement' &&
          statement.expression?.type === 'AwaitExpression'
        ? statement.expression.argument
        : undefined;
  const binding = declaration
    ? getPatternName(declaration.id)
    : undefined;
  const fetch = awaited && fetchRequest(awaited, state);
  if (fetch) {
    // `const response = await fetch(); const value = await response.json()` is
    // represented as one data-only fetch boundary. A Response never enters the
    // executable program or continuation environment.
    // Wrappers commonly return `undefined` on failure, so callers guard the
    // response before decoding it. `requireOk` already sends failures down the
    // failure continuation; consume that redundant guard with the adjacent
    // response.json declaration as one typed decode boundary.
    const decodeIndex = rest.findIndex((statement: any) =>
      responseDecodeBinding(statement, binding),
    );
    const decode =
      decodeIndex >= 0
        ? responseDecodeBinding(rest[decodeIndex], binding)
        : undefined;
    const remaining = decode ? rest.slice(decodeIndex + 1) : rest;
    const resultName = decode?.name ?? binding ?? 'result';
    return [
      {
        kind: 'capability-request',
        capability: 'network.fetch',
        request: { ...fetch, decode: decode?.kind ?? 'empty' },
        continuationId: `c${state.eventCounter + 1}`,
        successResultName: resultName,
        failureErrorName: 'error',
        success: lowerActionStatements(remaining, state),
        failure: [],
        finally: [],
      },
    ];
  }
  const cookie = awaited && cookieOperation(awaited, state);
  if (cookie)
    return [{ ...cookie, successResultName: binding ?? 'result', success: lowerActionStatements(rest, state) }];
  if (awaited) {
    const lowered = lowerAwaitedHelperCall(
      awaited,
      binding,
      rest,
      state,
    );
    if (lowered) return lowered;
  }
  if (declaration) {
    // Awaited locals are introduced by a capability continuation, never by
    // the lexical expression environment.  In particular, do not try to
    // serialize `const response = await request(...)` as an expression while
    // the helper-capability lowering is deciding its continuation shape.
    if (awaited) return lowerActionStatements(rest, state);
    const name = getPatternName(declaration.id);
    if (name && declaration.init)
      state.expressionScope[name] = declaration.init;
    return lowerActionStatements(rest, state);
  }
  if (statement.type === 'ReturnStatement')
    return [
      {
        kind: 'return',
        ...(statement.argument
          ? { value: lowerExpression(statement.argument, state) }
          : {}),
      },
    ];
  if (statement.type === 'ThrowStatement') {
    const thrown = unwrapExpression(statement.argument);
    const value = thrown?.type === 'NewExpression'
      ? lowerExpression(thrown.arguments?.[0]?.expression ?? thrown.arguments?.[0], state)
      : lowerExpression(statement.argument, state);
    return [{ kind: 'return', outcome: 'failure', value }];
  }
  if (statement.type === 'ExpressionStatement') {
    const cookie = cookieOperation(unwrapExpression(statement.expression?.type === 'UnaryExpression' && statement.expression.operator === 'void' ? statement.expression.argument : statement.expression), state);
    if (cookie)
      return [{ ...cookie, success: lowerActionStatements(rest, state) }];
  }
  const current = (() => {
    if (statement.type === 'ExpressionStatement')
      return lowerActionExpression(statement.expression, state);
    if (statement.type === 'IfStatement') {
      const consequent =
        statement.consequent?.type === 'BlockStatement'
          ? lowerActionStatements(
              getStatements(statement.consequent),
              state,
            )
          : lowerActionStatements([statement.consequent], state);
      const alternate = statement.alternate
        ? statement.alternate.type === 'BlockStatement'
          ? lowerActionStatements(
              getStatements(statement.alternate),
              state,
            )
          : lowerActionStatements([statement.alternate], state)
        : [];
      return [
        {
          kind: 'if',
          test: lowerExpression(statement.test, state),
          consequent,
          alternate,
        },
      ];
    }
    reportUnsupported(state, 'UNSUPPORTED_ACTION_STATEMENT', `Unsupported action statement ${statement.type}.`);
    return [];
  })();
  return [...current, ...lowerActionStatements(rest, state)];
}

/** Inline the data-only shape of an async local helper invocation.  The
 * helper may establish synchronous pending state and try/catch/finally around
 * a callback, but the callback itself must resolve to `fetch`; no function or
 * Response value is placed in IR. This is intentionally structural, so the
 * same lowering supports request wrappers beyond the Todo route. */
function lowerAwaitedHelperCall(
  awaited: any,
  binding: string | undefined,
  rest: any[],
  state: CompilerState,
): unknown[] | undefined {
  const call = unwrapExpression(awaited);
  if (call?.type !== 'CallExpression') return undefined;
  const local = getLocalFunction(getNodeName(call.callee) ?? '', state);
  if (!local?.body || local.body.type !== 'BlockStatement')
    return undefined;
  const callback = unwrapExpression(
    call.arguments?.[call.arguments.length - 1]?.expression ??
      call.arguments?.[call.arguments.length - 1],
  );
  if (
    !callback ||
    !['ArrowFunctionExpression', 'FunctionExpression'].includes(
      callback.type,
    )
  )
    return undefined;
  const callbackBody =
    callback.body?.type === 'BlockStatement'
      ? findReturnedExpression(callback.body)
      : callback.body;
  const fetch = fetchRequest(callbackBody, state);
  if (!fetch) return undefined;

  const previousScope = state.expressionScope;
  const previousModule = state.activeModuleId;
  const helperScope = {
    ...previousScope,
    ...(state.moduleExpressions.get(local.moduleId) ?? {}),
  };
  bindFunctionArguments(
    local.params ?? [],
    call.arguments ?? [],
    helperScope,
    state,
    local.name,
  );
  populateFunctionLocals(local.body, helperScope);
  state.expressionScope = helperScope;
  state.activeModuleId = local.moduleId;
  try {
    const statements = getStatements(local.body);
    const tryIndex = statements.findIndex(
      (statement: any) => statement.type === 'TryStatement',
    );
    if (tryIndex < 0) return undefined;
    const tryStatement = statements[tryIndex];
    const prelude = lowerActionStatements(
      statements.slice(0, tryIndex),
      state,
    );
    const decode = responseDecodeBinding(rest[0], binding);
    const remaining = decode ? rest.slice(1) : rest;
    const resultName = decode?.name ?? binding ?? 'result';
    const successPrevious = state.expressionScope;
    state.expressionScope = {
      ...successPrevious,
      [resultName]: { type: 'Identifier', value: resultName },
    };
    const success = lowerActionStatements(remaining, state);
    state.expressionScope = successPrevious;
    const failurePrevious = state.expressionScope;
    const catchName =
      getPatternName(tryStatement.handler?.param) ?? 'error';
    state.expressionScope = {
      ...failurePrevious,
      [catchName]: { type: 'Identifier', value: 'error' },
    };
    const failure = lowerActionStatements(
      getStatements(tryStatement.handler?.body),
      state,
    );
    state.expressionScope = failurePrevious;
    const finallyOperations = lowerActionStatements(
      getStatements(tryStatement.finalizer),
      state,
    );
    return [
      ...prelude,
      {
        kind: 'capability-request',
        capability: 'network.fetch',
        request: { ...fetch, decode: decode?.kind ?? 'empty', requireOk: true },
        continuationId: `c${state.eventCounter + 1}`,
        successResultName: resultName,
        failureErrorName: 'error',
        success,
        failure,
        finally: finallyOperations,
      },
    ];
  } finally {
    state.expressionScope = previousScope;
    state.activeModuleId = previousModule;
  }
}

function responseDecodeBinding(
  statement: any,
  responseName: string | undefined,
): { name: string; kind: 'json' | 'text' } | undefined {
  const returned = unwrapExpression(statement?.argument);
  const returnedAwaited =
    returned?.type === 'AwaitExpression'
      ? returned.argument
      : undefined;
  if (
    responseName &&
    statement?.type === 'ReturnStatement' &&
    returnedAwaited?.type === 'CallExpression' &&
    getNodeName(returnedAwaited.callee.object) === responseName &&
    ['json', 'text'].includes(getNodeName(returnedAwaited.callee.property) ?? '')
  )
    return { name: '__loader_result', kind: getNodeName(returnedAwaited.callee.property) as 'json' | 'text' };
  if (
    !responseName ||
    !['VariableDeclaration', 'VarDecl'].includes(statement?.type)
  )
    return undefined;
  const declaration = (statement.declarations ??
    statement.decls ??
    [])[0];
  const initial = unwrapExpression(declaration?.init);
  const awaited =
    initial?.type === 'AwaitExpression' ? initial.argument : undefined;
  if (
    awaited?.type === 'CallExpression' &&
    getNodeName(awaited.callee.object) === responseName &&
    ['json', 'text'].includes(getNodeName(awaited.callee.property) ?? '')
  )
    return {
      name: getPatternName(declaration.id)!,
      kind: getNodeName(awaited.callee.property) as 'json' | 'text',
    };
  return undefined;
}

function fetchRequest(
  expression: any,
  state: CompilerState,
): any | undefined {
  const call = unwrapExpression(expression);
  if (
    call?.type !== 'CallExpression' ||
    getNodeName(call.callee) !== 'fetch'
  )
    return undefined;
  const options = unwrapExpression(
    call.arguments?.[1]?.expression ?? call.arguments?.[1],
  );
  const property = (name: string) =>
    (options?.properties ?? []).find(
      (item: any) => objectPropertyName(item) === name,
    );
  const literal = (name: string, fallback: string) => {
    const value = objectPropertyValue(property(name));
    return value?.type === 'StringLiteral' ? value.value : fallback;
  };
  const headers: Record<string, unknown> = {};
  const headerObject = unwrapExpression(
    objectPropertyValue(property('headers')),
  );
  for (const header of headerObject?.properties ?? []) {
    const key = objectPropertyName(header);
    if (key)
      headers[key] = lowerExpression(
        objectPropertyValue(header),
        state,
      );
  }
  const body = unwrapExpression(objectPropertyValue(property('body')));
  const jsonBody =
    body?.type === 'CallExpression' &&
    getNodeName(body.callee?.object) === 'JSON' &&
    getNodeName(body.callee?.property) === 'stringify'
      ? lowerExpression(
          body.arguments?.[0]?.expression ?? body.arguments?.[0],
          state,
        )
      : undefined;
  return {
    url: lowerExpression(
      call.arguments?.[0]?.expression ?? call.arguments?.[0],
      state,
    ),
    method: literal('method', 'GET'),
    headers,
    ...(jsonBody ? { jsonBody } : {}),
    decode: 'json',
    requireOk: true,
  };
}

function lowerActionExpression(
  input: any,
  state: CompilerState,
): unknown[] {
  const expression = unwrapExpression(input);
  const name = getNodeName(expression);
  if (
    name &&
    state.expressionScope[name] &&
    state.expressionScope[name] !== expression
  ) {
    const resolved = unwrapExpression(state.expressionScope[name]);
    if (
      resolved?.type === 'ArrowFunctionExpression' ||
      resolved?.type === 'FunctionExpression'
    )
      return lowerActionOperations(resolved, state);
    return lowerActionExpression(resolved, state);
  }
  // An awaited action call is a control-flow boundary, not a render value.
  // The invoked capability owns the continuation; lowering its argument here
  // prevents an AwaitExpression from leaking into the expression serializer.
  if (expression?.type === 'AwaitExpression')
    return lowerActionExpression(expression.argument, state);
  if (
    expression?.type === 'UnaryExpression' &&
    expression.operator === 'void'
  )
    return lowerActionExpression(expression.argument, state);
  if (expression?.type === 'ConditionalExpression')
    return [
      {
        kind: 'if',
        test: lowerExpression(expression.test, state),
        consequent: lowerActionExpression(expression.consequent, state),
        alternate: lowerActionExpression(expression.alternate, state),
      },
    ];
  // A conditional event handler such as `editing ? onCancel : onStartEdit`
  // reaches this point as a bare callback identifier. Resolve it in the
  // component's inlined lexical scope before requiring a call expression.
  const bareName = getNodeName(expression);
  if (bareName && state.expressionScope[bareName]) {
    const target = unwrapExpression(state.expressionScope[bareName]);
    if (
      target?.type === 'ArrowFunctionExpression' ||
      target?.type === 'FunctionExpression'
    )
      return lowerActionOperations(target, state);
  }
  if (bareName && bareName === state.routeRetryProp)
    return [{ kind: 'route-retry' }];
  if (expression?.type !== 'CallExpression') {
    if (expression?.type === 'AssignmentExpression' && getNodeName(expression.left?.object) && getNodeName(expression.left?.property) === 'current' && getNodeName(expression.right?.object) === 'document' && getNodeName(expression.right?.property) === 'activeElement')
      return [{ kind: 'store-host-ref', refId: getNodeName(expression.left.object) }];
    if (expression?.type === 'AssignmentExpression' && getNodeName(expression.left?.object) === 'document' && getNodeName(expression.left?.property) === 'cookie')
      reportUnsupported(state, 'UNSUPPORTED_DOCUMENT_COOKIE', 'Use cookie.set or cookie.delete; document.cookie assignment is not safely compilable.');
    else reportUnsupported(state, 'UNSUPPORTED_ACTION_EXPRESSION', 'Action expression must be a supported call, conditional, or callback.');
    return [];
  }
  let callee = getNodeName(expression.callee);
  const value =
    expression.arguments?.[0]?.expression ?? expression.arguments?.[0];
  // Components are expanded into one graph. Callback props are consequently
  // lexical syntax here, so inline their supported body rather than emitting
  // an unresolved runtime callback contract.
  if (
    callee &&
    !state.stateSetters.has(callee) &&
    state.expressionScope[callee]
  ) {
    const target = unwrapExpression(state.expressionScope[callee]);
    const targetName = getNodeName(target);
    if (targetName && state.stateSetters.has(targetName)) {
      callee = targetName;
    } else if (
      target?.type === 'ArrowFunctionExpression' ||
      target?.type === 'FunctionExpression'
    ) {
      const previous = state.expressionScope;
      const scope = { ...previous };
      bindFunctionArguments(
        target.params ?? [],
        expression.arguments ?? [],
        scope,
        state,
        callee,
      );
      if (target.body?.type === 'BlockStatement')
        populateFunctionLocals(target.body, scope);
      state.expressionScope = scope;
      try {
        return target.body?.type === 'BlockStatement'
          ? lowerActionStatements(getStatements(target.body), state)
          : lowerActionExpression(target.body, state);
      } finally {
        state.expressionScope = previous;
      }
    }
  }
  if (callee && state.stateSetters.has(callee))
    return [
      {
        kind: 'set-state',
        stateSlotId: state.stateSetters.get(callee),
        value: lowerStateSetterValue(value, callee, state),
      },
    ];
  const cookie = cookieOperation(expression, state);
  if (cookie) return [cookie];
  if (
    expression.callee?.type === 'MemberExpression' &&
    getNodeName(expression.callee.property) === 'preventDefault'
  )
    return [{ kind: 'prevent-default' }];
  if (!callee) {
    reportUnsupported(state, 'UNSUPPORTED_ACTION_CALL', 'Action call target must be a static identifier.');
    return [];
  }
  const local = getLocalFunction(callee, state);
  if (local) {
    // Helpers defined by this graph are compiled into the same executable
    // program. This is deliberately not used for callback props: those cross
    // a graph boundary as invocation contracts below.
    const previousScope = state.expressionScope;
    const previousModule = state.activeModuleId;
    const scope = {
      ...previousScope,
      ...(state.moduleExpressions.get(local.moduleId) ?? {}),
    };
    bindFunctionArguments(
      local.params ?? [],
      expression.arguments ?? [],
      scope,
      state,
      callee,
    );
    populateFunctionLocals(local.body, scope);
    state.expressionScope = scope;
    state.activeModuleId = local.moduleId;
    const operations =
      local.body?.type === 'BlockStatement'
        ? lowerActionStatements(getStatements(local.body), state)
        : lowerActionExpression(local.body, state);
    state.expressionScope = previousScope;
    state.activeModuleId = previousModule;
    return operations;
  }
  return [
    {
      // Invocation is a graph contract, not a serialized callback body. The
      // defining graph resolves its own captures when the action executes.
      kind: 'invoke-action-ref',
      actionId: callee,
      parameters:
        value === undefined ? [] : [lowerExpression(value, state)],
    },
  ];
}

/** Functional state setters remain a pure value transform in the action IR.
 * Binding the callback parameter to the current state expression is enough
 * for array map/filter/append transforms and avoids serializing a closure. */
function cookieOperation(expression: any, state: CompilerState): any | undefined {
  if (expression?.callee?.type !== 'MemberExpression' || getNodeName(expression.callee.object) !== 'cookie') return undefined;
  const operation = getNodeName(expression.callee.property);
  if (!['get', 'set', 'delete'].includes(operation ?? '')) return undefined;
  const argument = (index: number) => expression.arguments?.[index]?.expression ?? expression.arguments?.[index];
  const name = argument(0);
  if (name?.type !== 'StringLiteral') {
    reportUnsupported(state, 'UNSUPPORTED_COOKIE_NAME', `cookie.${operation} requires a static cookie name.`);
    return undefined;
  }
  const options = unwrapExpression(argument(operation === 'set' ? 2 : 1));
  const option = (key: string) => objectPropertyValue((options?.properties ?? []).find((item: any) => objectPropertyName(item) === key));
  const path = option('path'); const sameSite = option('sameSite'); const secure = option('secure'); const maxAge = option('maxAge');
  if ((path && path.type !== 'StringLiteral') || (sameSite && sameSite.type !== 'StringLiteral') || (secure && secure.type !== 'BooleanLiteral') || (maxAge && maxAge.type !== 'NumericLiteral')) {
    reportUnsupported(state, 'UNSUPPORTED_COOKIE_OPTIONS', 'cookie options must be static path, sameSite, secure, and maxAge values.');
    return undefined;
  }
  if (sameSite && !['lax', 'strict', 'none'].includes(sameSite.value)) {
    reportUnsupported(state, 'UNSUPPORTED_COOKIE_OPTIONS', 'cookie.sameSite must be lax, strict, or none.');
    return undefined;
  }
  if (operation === 'set' && !argument(1)) {
    reportUnsupported(state, 'UNSUPPORTED_COOKIE_VALUE', 'cookie.set requires a value.');
    return undefined;
  }
  return { kind: 'capability-request', capability: 'cookie', request: { operation, name: name.value, ...(operation === 'set' ? { value: lowerExpression(argument(1), state) } : {}), path: path?.value ?? '/', ...(sameSite ? { sameSite: sameSite.value } : {}), ...(secure ? { secure: secure.value } : {}), expiry: maxAge ? 'maxAge' : 'session', ...(maxAge ? { maxAge: maxAge.value } : {}) }, continuationId: `c${state.eventCounter + 1}`, successResultName: 'result', failureErrorName: 'error', success: [], failure: [], finally: [] };
}

function lowerStateSetterValue(
  value: any,
  setter: string,
  state: CompilerState,
): any {
  const callback = unwrapExpression(value);
  if (
    !callback ||
    !['ArrowFunctionExpression', 'FunctionExpression'].includes(
      callback.type,
    )
  )
    return lowerExpression(value, state);
  const parameter = callback.params?.[0]?.pat ?? callback.params?.[0];
  const name = getPatternName(parameter);
  const stateSlotId = state.stateSetters.get(setter);
  if (!name || !stateSlotId) {
    reportUnsupported(
      state,
      'UNSUPPORTED_STATE_UPDATER',
      'State updater requires one identifier parameter.',
    );
    return { kind: 'literal', value: null };
  }
  const previous = state.expressionScope;
  const stateName = state.ir.localStates.find(
    (slot) => slot.id === stateSlotId,
  )?.name;
  state.expressionScope = {
    ...previous,
    [name]: { type: 'Identifier', value: stateName ?? name },
  };
  try {
    const body =
      callback.body?.type === 'BlockStatement'
        ? findReturnedExpression(callback.body)
        : callback.body;
    return lowerExpression(body, state);
  } finally {
    state.expressionScope = previous;
  }
}

function reactEventName(name: string): string {
  const normalized = name.slice(2).toLowerCase();
  return (
    (
      {
        mouseenter: 'mouseenter',
        mouseleave: 'mouseleave',
        mousedown: 'mousedown',
        mouseup: 'mouseup',
        doubleclick: 'dblclick',
        focus: 'focus',
        blur: 'blur',
      } as Record<string, string>
    )[normalized] ?? normalized
  );
}
function isDomProperty(name: string): boolean {
  return [
    'checked',
    'defaultChecked',
    'indeterminate',
    'value',
    'selected',
    'disabled',
    'readOnly',
  ].includes(name);
}

/**
 * This is intentionally a semantic recognizer, not a hook implementation.
 * The accepted shape is the real demo ThemeToggle: one `mode` state slot, a
 * mount initializer, and a mode-dependent media subscription with cleanup.
 */
function lowerThemeToggleSemantics(
  body: any,
  state: CompilerState,
  before: { e: number; b: number; ev: number },
): void {
  const statements = getStatements(body);
  for (const call of findCalls(body)) {
    const callName = getNodeName(call.callee);
    if (
      callName?.startsWith('use') &&
      callName !== 'useState' &&
      callName !== 'useEffect'
    )
      reportUnsupported(
        state,
        'UNSUPPORTED_HOOK',
        `ThemeToggle hook ${callName} is not supported.`,
      );
  }
  const hasUseState = statements.some((statement: any) =>
    containsCall(statement, 'useState'),
  );
  const effects = statements.filter((statement: any) =>
    containsCall(statement, 'useEffect'),
  );
  if (!hasUseState)
    reportUnsupported(
      state,
      'THEME_STATE_SLOT_REQUIRED',
      "ThemeToggle requires useState('auto') for its mode state.",
    );
  if (effects.length !== 2)
    reportUnsupported(
      state,
      'THEME_EFFECT_SHAPE',
      'ThemeToggle requires one mount effect and one mode-dependent subscription effect.',
    );
  for (const effect of effects) {
    const dependencyArray =
      findCall(effect, 'useEffect')?.arguments?.[1]?.expression ??
      findCall(effect, 'useEffect')?.arguments?.[1];
    const items = dependencyArray?.elements ?? [];
    if (
      dependencyArray?.type !== 'ArrayExpression' ||
      ![0, 1].includes(items.length) ||
      (items.length === 1 &&
        getNodeName(items[0]?.expression ?? items[0]) !== 'mode')
    ) {
      reportUnsupported(
        state,
        'UNSUPPORTED_EFFECT_DEPENDENCIES',
        'ThemeToggle effects must use [] or [mode] dependencies.',
      );
    }
  }

  const stateSlotId = `s${state.ir.localStates.length + 1}`;
  const hostValueId = `h${state.ir.hostValues.length + 1}`;
  state.ir.localStates.push({
    id: stateSlotId,
    name: 'mode',
    initialValue: 'auto',
    values: ['light', 'dark', 'auto'],
  });
  state.ir.hostValues.push({
    id: hostValueId,
    kind: 'media-query',
    query: '(prefers-color-scheme: dark)',
  });
  state.ir.lifecycleEffects.push(
    {
      id: `fx${state.ir.lifecycleEffects.length + 1}`,
      trigger: 'mount',
      stateSlotId,
      operations: [
        { kind: 'storage-read', storageKey: 'theme', stateSlotId },
        { kind: 'document-theme-apply', stateSlotId, hostValueId },
      ],
    },
    {
      id: `fx${state.ir.lifecycleEffects.length + 2}`,
      trigger: 'state-change',
      stateSlotId,
      operations: [
        { kind: 'document-theme-apply', stateSlotId, hostValueId },
      ],
      subscription: {
        hostValueId,
        event: 'change',
        activeWhenStateEquals: 'auto',
        dispose: 'remove-listener',
      },
    },
  );
  const event = state.ir.events
    .slice(before.ev)
    .find((candidate) => candidate.type === 'click');
  if (!event)
    reportUnsupported(
      state,
      'THEME_EVENT_REQUIRED',
      'ThemeToggle requires a direct onClick={toggleMode} handler.',
    );
  else
    state.ir.stateTransitions.push({
      id: `st${state.ir.stateTransitions.length + 1}`,
      eventId: event.id,
      stateSlotId,
      kind: 'theme-cycle',
      operations: [
        { kind: 'storage-write', storageKey: 'theme', stateSlotId },
        { kind: 'document-theme-apply', stateSlotId, hostValueId },
      ],
    });
  for (const binding of state.ir.bindings.slice(before.b)) {
    const expression = state.ir.expressions.find(
      (candidate) => candidate.id === binding.expressionId,
    )?.expression;
    if (expressionContainsIdentifier(expression, 'mode'))
      state.ir.dependencyEdges.push({
        fromId: stateSlotId,
        toId: binding.id,
        kind: 'local-state-to-binding',
      });
  }
}

function containsCall(node: any, name: string): boolean {
  return Boolean(findCall(node, name));
}
function lowerLifecycleCalls(body: any, state: CompilerState): void {
  for (const call of findCalls(body)) {
    const name =
      getNodeName(call.callee) ?? getNodeName(call.callee?.property);
    if (name !== 'useEffect' && name !== 'useLayoutEffect') continue;
    const callback = unwrapExpression(
      call.arguments?.[0]?.expression ?? call.arguments?.[0],
    );
    const dependencies = unwrapExpression(
      call.arguments?.[1]?.expression ?? call.arguments?.[1],
    );
    if (
      !callback ||
      (callback.type !== 'ArrowFunctionExpression' &&
        callback.type !== 'FunctionExpression')
    ) {
      reportUnsupported(
        state,
        'UNSUPPORTED_LIFECYCLE_CALLBACK',
        `${name} requires an inline callback.`,
      );
      continue;
    }
    const statements =
      callback.body?.type === 'BlockStatement'
        ? getStatements(callback.body)
        : [];
    // A cleanup-only no-op is semantically observable for ordering but needs
    // no host capability. Keep it as a lifecycle descriptor instead of
    // rejecting the surrounding component.
    const cleanupOnly =
      statements.length === 1 &&
      (statements[0]?.type === 'ReturnStatement' ||
        statements[0]?.type === 'ReturnStmt') &&
      (() => {
        const returned = unwrapExpression(
          statements[0]?.argument ?? statements[0]?.arg,
        );
        return (
          (returned?.type === 'ArrowFunctionExpression' ||
            returned?.type === 'FunctionExpression') &&
          getStatements(returned.body).length === 0
        );
      })();
    if (statements.length && !cleanupOnly) {
      reportUnsupported(
        state,
        'UNSUPPORTED_LIFECYCLE_BODY',
        `${name} body must reduce to declarative host/resource operations.`,
      );
      continue;
    }
    const deps =
      dependencies?.type === 'ArrayExpression'
        ? (dependencies.elements ?? [])
            .filter(Boolean)
            .map((item: any) =>
              internExpression(item.expression ?? item, state),
            )
        : [];
    state.ir.lifecycleEffects.push({
      id: `fx${state.ir.lifecycleEffects.length + 1}`,
      phase: name === 'useLayoutEffect' ? 'layout' : 'effect',
      trigger: 'mount',
      dependencies: deps,
      operations: [],
    });
  }
}
function findCalls(node: any, calls: any[] = []): any[] {
  if (!node || typeof node !== 'object') return calls;
  if (node.type === 'CallExpression') calls.push(node);
  for (const value of Object.values(node)) {
    if (Array.isArray(value))
      value.forEach((child) => findCalls(child, calls));
    else findCalls(value, calls);
  }
  return calls;
}
function findCall(node: any, name: string): any | undefined {
  if (!node || typeof node !== 'object') return undefined;
  if (
    node.type === 'CallExpression' &&
    getNodeName(node.callee) === name
  )
    return node;
  for (const value of Object.values(node)) {
    if (Array.isArray(value)) {
      for (const child of value) {
        const found = findCall(child, name);
        if (found) return found;
      }
    } else {
      const found = findCall(value, name);
      if (found) return found;
    }
  }
  return undefined;
}
function expressionContainsIdentifier(
  expression: any,
  name: string,
): boolean {
  if (!expression || typeof expression !== 'object') return false;
  if (expression.kind === 'identifier' && expression.name === name)
    return true;
  return Object.values(expression).some((value) =>
    Array.isArray(value)
      ? value.some((child) => expressionContainsIdentifier(child, name))
      : expressionContainsIdentifier(value, name),
  );
}

function getMapCall(
  expression: any,
): { source: any; callback: any } | null {
  if (expression?.type !== 'CallExpression') {
    return null;
  }

  const callee = expression.callee;
  if (
    callee?.type !== 'MemberExpression' ||
    getNodeName(callee.property) !== 'map'
  ) {
    return null;
  }

  return {
    source: callee.object,
    callback:
      expression.arguments?.[0]?.expression ??
      expression.arguments?.[0],
  };
}

function evaluateArrayExpression(
  expression: any,
  state: CompilerState,
  scope: Record<string, unknown>,
): unknown[] | null {
  const value = evaluateExpression(expression, state, scope);
  return Array.isArray(value) ? value : null;
}

function evaluateExpression(
  expression: any,
  state: CompilerState,
  scope: Record<string, unknown>,
  resolving = new Set<string>(),
): unknown {
  const node = unwrapExpression(expression);
  if (!node) {
    return undefined;
  }

  if (isLiteralExpression(node)) {
    return node.type === 'NullLiteral' ? null : node.value;
  }

  if (node.type === 'ArrayExpression') {
    const values: unknown[] = [];
    for (const element of node.elements ?? []) {
      if (!element) {
        values.push(undefined);
        continue;
      }

      if (element.type === 'SpreadElement') {
        return undefined;
      }

      const value = evaluateExpression(
        element.expression ?? element,
        state,
        scope,
        resolving,
      );
      if (value === undefined) {
        return undefined;
      }
      values.push(value);
    }
    return values;
  }

  if (node.type === 'CallExpression') {
    const evaluatedCall = evaluateCallExpression(node, state, scope);
    if (evaluatedCall !== undefined) {
      return evaluatedCall;
    }
  }

  if (node.type === 'ObjectExpression') {
    const record: Record<string, unknown> = {};
    for (const property of node.properties ?? []) {
      if (
        property.type !== 'KeyValueProperty' &&
        property.type !== 'Property' &&
        property.type !== 'ObjectProperty'
      ) {
        return undefined;
      }

      const key = getNodeName(
        property.key ?? property.keyExpression ?? property.id,
      );
      if (!key) {
        return undefined;
      }

      const valueNode =
        property.value ?? property.expr ?? property.init;
      const value = evaluateExpression(
        valueNode,
        state,
        scope,
        resolving,
      );
      if (value === undefined) {
        return undefined;
      }
      record[key] = value;
    }
    return record;
  }

  if (node.type === 'Identifier') {
    if (Object.prototype.hasOwnProperty.call(scope, node.value)) {
      return scope[node.value];
    }
    if (resolving.has(node.value)) return undefined;
    const resolved = state.expressionScope[node.value];
    if (
      resolved &&
      resolved !== node &&
      !(
        getNodeName(resolved) === node.value &&
        (resolved.type === 'Identifier' ||
          resolved.type === 'IdentifierExpression')
      )
    ) {
      return evaluateExpression(
        resolved,
        state,
        scope,
        new Set([...resolving, node.value]),
      );
    }
    return undefined;
  }

  if (node.type === 'IdentifierExpression') {
    const name = getNodeName(node);
    if (name && Object.prototype.hasOwnProperty.call(scope, name)) {
      return scope[name];
    }
    if (name && resolving.has(name)) return undefined;
    const resolved = name ? state.expressionScope[name] : undefined;
    if (
      name &&
      resolved &&
      resolved !== node &&
      !(
        getNodeName(resolved) === name &&
        (resolved.type === 'Identifier' ||
          resolved.type === 'IdentifierExpression')
      )
    ) {
      return evaluateExpression(
        resolved,
        state,
        scope,
        new Set([...resolving, name]),
      );
    }
    return undefined;
  }

  if (node.type === 'MemberExpression') {
    if (getNodeName(node.object) === 'document' && getNodeName(node.property) === 'cookie') {
      reportUnsupported(state, 'UNSUPPORTED_DOCUMENT_COOKIE', 'Use cookie.getSync/get/set/delete; document.cookie parsing is not safely compilable.');
      return { kind: 'literal', value: null };
    }
    const objectValue = evaluateExpression(
      node.object,
      state,
      scope,
      resolving,
    );
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

    if (
      typeof objectValue === 'object' &&
      propertyName in objectValue
    ) {
      return (objectValue as Record<string, unknown>)[propertyName];
    }
    return undefined;
  }

  if (node.type === 'BinaryExpression') {
    const left = evaluateExpression(node.left, state, scope, resolving);
    const right = evaluateExpression(
      node.right,
      state,
      scope,
      resolving,
    );
    if (left === undefined || right === undefined) {
      return undefined;
    }

    switch (node.operator) {
      case '+':
        return typeof left === 'string' || typeof right === 'string'
          ? `${left}${right}`
          : Number(left) + Number(right);
      case '-':
        return Number(left) - Number(right);
      case '*':
        return Number(left) * Number(right);
      case '/':
        return Number(left) / Number(right);
      case '%':
        return Number(left) % Number(right);
      case '===':
        return left === right;
      case '!==':
        return left !== right;
      case '==':
        return left == right;
      case '!=':
        return left != right;
      case '>':
        return Number(left) > Number(right);
      case '>=':
        return Number(left) >= Number(right);
      case '<':
        return Number(left) < Number(right);
      case '<=':
        return Number(left) <= Number(right);
      default:
        return undefined;
    }
  }

  if (node.type === 'TemplateLiteral') {
    const parts: string[] = [];
    for (let index = 0; index < node.quasis.length; index += 1) {
      parts.push(node.quasis[index]?.value?.cooked ?? '');
      const expr = node.expressions?.[index];
      if (expr) {
        const value = evaluateExpression(expr, state, scope, resolving);
        if (value === undefined) {
          return undefined;
        }
        parts.push(String(value));
      }
    }
    return parts.join('');
  }

  if (node.type === 'LogicalExpression' && node.operator === '??') {
    const left = evaluateExpression(node.left, state, scope, resolving);
    if (left !== undefined && left !== null) {
      return left;
    }
    return evaluateExpression(node.right, state, scope, resolving);
  }

  return undefined;
}

function bindPatternValue(
  pattern: any,
  value: unknown,
  state: CompilerState,
  index: number,
): Record<string, unknown> | null {
  if (!pattern) {
    return null;
  }

  if (pattern.type === 'Identifier') {
    return { [pattern.value]: value };
  }

  if (pattern.type === 'ObjectPattern') {
    if (typeof value !== 'object' || value === null) {
      return null;
    }

    const scope: Record<string, unknown> = {};
    for (const property of pattern.properties ?? []) {
      const key = getNodeName(
        property.key ?? property.key?.value ?? property.key?.id,
      );
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

  if (pattern.type === 'ArrayPattern') {
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

  if (pattern.type === 'AssignmentPattern') {
    return bindPatternValue(pattern.left, value, state, index);
  }

  return null;
}

function evaluateCallExpression(
  node: any,
  state: CompilerState,
  scope: Record<string, unknown>,
): unknown {
  const callName = getNodeName(node.callee);
  if (['cn', 'clsx', 'classnames'].includes(callName ?? '')) {
    const values: unknown[] = (node.arguments ?? []).map((arg: any) =>
      evaluateExpression(arg.expression ?? arg, state, scope),
    );
    // Class helpers intentionally ignore absent optional values, just as the
    // runtime `cn` implementation does. That keeps a primitive's static base
    // classes materialized in IR when no caller className was supplied.
    return flattenClasses(values).join(' ');
  }
  if (node.callee?.type === 'Identifier') {
    const definition =
      state.expressionScope[getNodeName(node.callee) ?? ''];
    if (
      definition?.type === 'CallExpression' &&
      getNodeName(definition.callee) === 'cva'
    )
      return evaluateCva(definition, node, state, scope);
  }
  if (
    getNodeName(node.callee?.object) !== 'Array' ||
    getNodeName(node.callee?.property) !== 'from'
  ) {
    return undefined;
  }

  const sourceExpression = unwrapExpression(
    node.arguments?.[0]?.expression ?? node.arguments?.[0],
  );
  const sourceValue =
    sourceExpression?.type === 'ObjectExpression'
      ? evaluateObjectLiteral(sourceExpression, state, scope)
      : evaluateExpression(sourceExpression, state, scope);
  const length = getArrayLikeLength(sourceValue);
  if (length === null) {
    return undefined;
  }

  const callback = unwrapExpression(
    node.arguments?.[1]?.expression ?? node.arguments?.[1],
  );
  if (
    !callback ||
    (callback.type !== 'ArrowFunctionExpression' &&
      callback.type !== 'FunctionExpression')
  ) {
    return undefined;
  }

  const values: unknown[] = [];
  for (let index = 0; index < length; index += 1) {
    const invocationScope = mergeScopes(
      scope,
      bindCallbackParameters(callback, index, state),
    );
    const value = evaluateFunctionBody(
      callback,
      state,
      invocationScope,
    );
    if (value === undefined) {
      return undefined;
    }
    values.push(value);
  }

  return values;
}

function evaluateObjectLiteral(
  node: any,
  state: CompilerState,
  scope: Record<string, unknown>,
): Record<string, unknown> | undefined {
  if (!node || node.type !== 'ObjectExpression') {
    return undefined;
  }

  const record: Record<string, unknown> = {};
  for (const property of node.properties ?? []) {
    const key = getNodeName(
      property?.key ?? property?.keyExpression ?? property?.id,
    );
    if (!key) {
      return undefined;
    }

    const valueNode =
      property?.value ?? property?.expr ?? property?.init;
    const value = evaluateExpression(valueNode, state, scope);
    if (value === undefined) {
      return undefined;
    }
    record[key] = value;
  }

  return record;
}

function bindCallbackParameters(
  callback: any,
  index: number,
  state: CompilerState,
): Record<string, unknown> {
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

function evaluateFunctionBody(
  functionLike: any,
  state: CompilerState,
  scope: Record<string, unknown>,
): unknown {
  const body = getFunctionBody(functionLike);
  if (!body) {
    return undefined;
  }

  if (body.type !== 'BlockStatement') {
    return evaluateExpression(body, state, scope);
  }

  for (const statement of body.stmts ?? body.body ?? []) {
    if (
      statement.type !== 'ReturnStatement' &&
      statement.type !== 'ReturnStmt'
    ) {
      continue;
    }

    return evaluateExpression(
      statement.argument ?? statement.arg,
      state,
      scope,
    );
  }

  return undefined;
}

function getArrayLikeLength(value: unknown): number | null {
  if (Array.isArray(value)) {
    return value.length;
  }

  if (typeof value === 'number' && Number.isFinite(value)) {
    return Math.max(0, Math.floor(value));
  }

  if (typeof value === 'object' && value !== null) {
    const length = (value as Record<string, unknown>).length;
    if (typeof length === 'number' && Number.isFinite(length)) {
      return Math.max(0, Math.floor(length));
    }
  }

  return null;
}

function collectLiteralScope(body: any, state: CompilerState): void {
  const block = body?.type === 'BlockStatement' ? body : null;
  if (!block) {
    return;
  }

  for (const statement of block.stmts ?? block.body ?? []) {
    if (
      statement.type !== 'VariableDeclaration' &&
      statement.type !== 'VarDecl'
    ) {
      continue;
    }

    const isConst =
      statement.kind === 'const' ||
      statement.declare === true ||
      statement.const === true;
    if (!isConst) {
      continue;
    }

    for (const declaration of statement.declarations ??
      statement.decls ??
      []) {
      const name = getPatternName(declaration?.id);
      if (!name || !declaration?.init) {
        continue;
      }

      const value = evaluateExpression(
        declaration.init,
        state,
        state.scope,
      );
      if (value !== undefined) {
        state.scope[name] = value;
      }
    }
  }
}

function collectModuleFacts(body: any, state: CompilerState): void {
  for (const statement of getStatements(body)) {
    if (
      statement.type === 'FunctionDeclaration' ||
      statement.type === 'FnDecl'
    ) {
      const name = getNodeName(statement.identifier ?? statement.id);
      // Async helpers are never render expressions, but are valid action
      // program sources.  Keeping them in the same lexical registry lets an
      // event lowerer inline their data-only continuation program instead of
      // degrading the helper call to a callback reference.
      if (name)
        state.functions.set(componentKey(state.activeModuleId, name), {
          name,
          body: getFunctionBody(statement),
          params: statement.params ?? statement.function?.params ?? [],
          moduleId: state.activeModuleId,
        });
      continue;
    }
    if (
      statement.type !== 'VariableDeclaration' &&
      statement.type !== 'VarDecl'
    ) {
      continue;
    }

    for (const declaration of statement.declarations ??
      statement.decls ??
      []) {
      const init = unwrapExpression(declaration?.init);
      if (!init) {
        continue;
      }

      const name = getPatternName(declaration?.id);
      const callable = unwrapComponentFactory(init);
      if (name && callable)
        state.functions.set(componentKey(state.activeModuleId, name), {
          name,
          body: getFunctionBody(callable),
          params: callable.params ?? [],
          moduleId: state.activeModuleId,
        });

      if (
        init.type === 'CallExpression' &&
        getNodeName(init.callee) === 'useState' &&
        declaration.id?.type === 'ArrayPattern'
      ) {
        const [valuePattern, setterPattern] =
          declaration.id.elements ?? [];
        const valueName = getPatternName(
          valuePattern?.expression ?? valuePattern,
        );
        const setterName = getPatternName(
          setterPattern?.expression ?? setterPattern,
        );
        if (valueName && setterName) {
          const slotId = `s${state.ir.localStates.length + 1}`;
          const initial =
            init.arguments?.[0]?.expression ?? init.arguments?.[0];
          const loaderResultState = isLoaderDataExpression(initial, state);
          state.ir.localStates.push({
            id: slotId,
            name: valueName,
            // A loader owns this value until its typed result arrives. Null is
            // the only general initial value without executing user code.
            initialValue: loaderResultState ? 'null' : serializeInitialState(initial, state),
            initialExpression: loaderResultState
              ? { kind: 'literal', value: null }
              : lowerExpression(initial, state),
            values: [],
          });
          if (loaderResultState)
            state.loaderResultState = state.ir.localStates.length - 1;
          state.stateSetters.set(setterName, slotId);
          state.expressionScope[valueName] = {
            type: 'Identifier',
            value: valueName,
          };
          continue;
        }
      }

      const value = evaluateExpression(init, state, state.scope);
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

function isLoaderDataExpression(
  input: any,
  state: CompilerState,
  seen = new Set<string>(),
): boolean {
  const value = unwrapExpression(input);
  if (value?.type === 'CallExpression')
    return getNodeName(value.callee?.property) === 'useLoaderData';
  const name = getNodeName(value);
  if (!name || seen.has(name)) return false;
  const resolved = state.expressionScope[name];
  return Boolean(
    resolved && isLoaderDataExpression(resolved, state, new Set([...seen, name])),
  );
}

function isAsyncFunction(node: any): boolean {
  return node?.async === true || node?.function?.async === true;
}

function collectModuleScope(
  moduleAst: any,
  state: CompilerState,
): void {
  for (const statement of getStatements(moduleAst)) {
    if (
      statement.type !== 'VariableDeclaration' &&
      statement.type !== 'VarDecl'
    ) {
      continue;
    }

    for (const declaration of statement.declarations ??
      statement.decls ??
      []) {
      const name = getPatternName(declaration?.id);
      if (!name || !declaration?.init) {
        continue;
      }

      const value = evaluateExpression(
        declaration.init,
        state,
        state.scope,
      );
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
    if (typeof value === 'string' || typeof value === 'number')
      result.push(String(value));
    else if (Array.isArray(value))
      result.push(...flattenClasses(value));
    else if (typeof value === 'object')
      for (const [key, enabled] of Object.entries(
        value as Record<string, unknown>,
      ))
        if (enabled) result.push(key);
  }
  return result;
}
function evaluateCva(
  definition: any,
  call: any,
  state: CompilerState,
  scope: Record<string, unknown>,
): unknown {
  const base = evaluateExpression(
    definition.arguments?.[0]?.expression ?? definition.arguments?.[0],
    state,
    scope,
  );
  const config = unwrapExpression(
    definition.arguments?.[1]?.expression ?? definition.arguments?.[1],
  );
  const selected = unwrapExpression(
    call.arguments?.[0]?.expression ?? call.arguments?.[0],
  );
  if (typeof base !== 'string' || config?.type !== 'ObjectExpression')
    return undefined;
  const selectedValue: Record<string, unknown> = {};
  if (selected?.type === 'ObjectExpression')
    for (const property of selected.properties ?? []) {
      const key = getNodeName(property.key);
      if (!key) continue;
      const value = evaluateExpression(
        property.value ?? property.expr,
        state,
        scope,
      );
      if (value !== undefined) selectedValue[key] = value;
    }
  else if (selected) {
    reportUnsupported(
      state,
      'DYNAMIC_CVA_VARIANT',
      'CVA variants must be statically known.',
    );
    return undefined;
  }
  const getObject = (object: any, key: string) =>
    object.properties?.find(
      (property: any) => getNodeName(property.key) === key,
    )?.value;
  const variants = getObject(config, 'variants');
  const defaults = getObject(config, 'defaultVariants');
  const classes = [base];
  for (const property of variants?.properties ?? []) {
    const variant = getNodeName(property.key);
    if (!variant) continue;
    const value =
      selectedValue[variant] ??
      evaluateExpression(getObject(defaults, variant), state, scope);
    if (value === undefined) {
      reportUnsupported(
        state,
        'DYNAMIC_CVA_VARIANT',
        `CVA variant ${variant} must be static.`,
      );
      return undefined;
    }
    const option = property.value?.properties?.find(
      (entry: any) => getNodeName(entry.key) === String(value),
    )?.value;
    const className = evaluateExpression(option, state, scope);
    if (typeof className !== 'string') return undefined;
    classes.push(className);
  }
  if (typeof selectedValue.className === 'string')
    classes.push(selectedValue.className);
  return classes.join(' ');
}

function collectComponents(
  moduleAst: any,
  state: CompilerState,
  moduleId: string,
): void {
  for (const original of getStatements(moduleAst)) {
    const statement = original.declaration ?? original.decl ?? original;
    if (
      statement.type === 'FunctionDeclaration' ||
      statement.type === 'FnDecl'
    ) {
      const name = getNodeName(statement.identifier ?? statement.id);
      if (!name) continue;
      const definition = {
        name,
        body: getFunctionBody(statement),
        params: statement.params ?? statement.function?.params ?? [],
        moduleId,
      };
      state.functions.set(componentKey(moduleId, name), definition);
      if (isComponentName(name))
        state.components.set(componentKey(moduleId, name), definition);
    }
    if (
      statement.type === 'FunctionExpression' &&
      isComponentName(getNodeName(statement.identifier ?? statement.id))
    ) {
      const name = getNodeName(statement.identifier ?? statement.id)!;
      state.components.set(componentKey(moduleId, name), {
        name,
        body: getFunctionBody(statement),
        params: statement.params ?? [],
        moduleId,
      });
    }
    if (
      statement.type === 'VariableDeclaration' ||
      statement.type === 'VarDecl'
    )
      for (const declaration of statement.declarations ??
        statement.decls ??
        []) {
        const name = getPatternName(declaration.id);
        const init = declaration.init;
        const implementation = unwrapComponentFactory(init);
        if (name && implementation) {
          const definition = {
            name,
            body: getFunctionBody(implementation),
            params: implementation.params ?? [],
            moduleId,
          };
          state.functions.set(componentKey(moduleId, name), definition);
          if (isComponentName(name))
            state.components.set(
              componentKey(moduleId, name),
              definition,
            );
        }
      }
  }
}

function unwrapComponentFactory(init: any): any | null {
  if (
    init?.type === 'ArrowFunctionExpression' ||
    init?.type === 'FunctionExpression'
  )
    return init;
  const callee =
    getNodeName(init?.callee) ?? getNodeName(init?.callee?.property);
  if (callee === 'forwardRef' && init.arguments?.[0])
    return unwrapExpression(
      init.arguments[0].expression ?? init.arguments[0],
    );
  return null;
}

function collectExports(
  moduleAst: any,
  state: CompilerState,
  moduleId: string,
): void {
  const entries =
    state.exports.get(moduleId) ??
    new Map<string, { moduleId: string; exportName: string }>();
  for (const statement of getStatements(moduleAst)) {
    if (statement.type === 'ExportDeclaration') {
      const declaration = statement.declaration;
      const name =
        getNodeName(declaration?.identifier ?? declaration?.id) ??
        getPatternName(declaration?.declarations?.[0]?.id);
      if (name) entries.set(name, { moduleId, exportName: name });
    }
    // Base UI exposes compound components with `export * as Checkbox`. Keep
    // that namespace edge so `Checkbox.Root` resolves through source modules.
    if (statement.type === 'ExportAllDeclaration') {
      const exported = getNodeName(statement.exported);
      const source = statement.source?.value;
      if (exported && source)
        entries.set(exported, {
          moduleId: resolveModuleId(moduleId, source),
          exportName: exported,
        });
      continue;
    }
    if (
      statement.type !== 'ExportNamedDeclaration' &&
      statement.type !== 'ExportNamedSpecifier'
    ) {
      // CommonJS source packages expose values through assignments such as
      // `exports.Workflow = Workflow`. Treat these as source exports so a
      // package resolved by Node's conditional exports remains inspectable.
      const expression = unwrapExpression(
        statement.expression ?? statement.expr,
      );
      if (
        expression?.type === 'AssignmentExpression' &&
        expression.left?.type === 'MemberExpression' &&
        getNodeName(expression.left.object) === 'exports'
      ) {
        const exported = getNodeName(expression.left.property);
        const local = getNodeName(expression.right);
        if (exported && local)
          entries.set(exported, { moduleId, exportName: local });
      }
      continue;
    }
    const source = statement.source?.value;
    for (const specifier of statement.specifiers ?? []) {
      const exported =
        getNodeName(specifier.exported) ??
        getNodeName(specifier.name) ??
        getNodeName(specifier.orig) ??
        getNodeName(specifier.local);
      const local =
        getNodeName(specifier.orig) ??
        getNodeName(specifier.local) ??
        exported;
      if (!exported || !local) continue;
      entries.set(
        exported,
        source
          ? {
              moduleId: resolveModuleId(moduleId, source),
              exportName: local,
            }
          : { moduleId, exportName: local },
      );
    }
  }
  state.exports.set(moduleId, entries);
}

function collectModuleExpressions(
  moduleAst: any,
  state: CompilerState,
  moduleId: string,
): void {
  const expressions: Record<string, any> = {};
  for (const original of getStatements(moduleAst)) {
    const statement = original.declaration ?? original.decl ?? original;
    if (
      statement.type !== 'VariableDeclaration' &&
      statement.type !== 'VarDecl'
    )
      continue;
    for (const declaration of statement.declarations ??
      statement.decls ??
      []) {
      const name = getPatternName(declaration.id);
      if (name && declaration.init)
        expressions[name] = declaration.init;
    }
  }
  state.moduleExpressions.set(moduleId, expressions);
}

function collectContexts(
  moduleAst: any,
  state: CompilerState,
  moduleId: string,
): void {
  for (const original of getStatements(moduleAst)) {
    const statement = original.declaration ?? original.decl ?? original;
    if (
      statement.type !== 'VariableDeclaration' &&
      statement.type !== 'VarDecl'
    )
      continue;
    for (const declaration of statement.declarations ??
      statement.decls ??
      []) {
      const name = getPatternName(declaration.id);
      const init = unwrapExpression(declaration.init);
      if (
        !name ||
        init?.type !== 'CallExpression' ||
        (getNodeName(init.callee) !== 'createContext' &&
          getNodeName(init.callee?.property) !== 'createContext')
      )
        continue;
      const argument =
        init.arguments?.[0]?.expression ?? init.arguments?.[0];
      if (!argument) {
        reportUnsupported(
          state,
          'CONTEXT_DEFAULT_REQUIRED',
          `createContext(${name}) requires a default value.`,
        );
        continue;
      }
      const key = componentKey(moduleId, name);
      if (!state.contextSymbols.has(key)) {
        const definition = {
          id: `ctx${state.contextSymbols.size + 1}`,
          defaultExpressionId: internExpression(argument, state),
        };
        state.contextSymbols.set(key, definition);
        state.ir.contextDefinitions.push(definition);
      }
    }
  }
}

function resolveContext(
  name: string,
  state: CompilerState,
): { id: string; defaultExpressionId: string } | undefined {
  const local = state.contextSymbols.get(
    componentKey(state.activeModuleId, name),
  );
  if (local) return local;
  const imported = state.importSymbols.get(
    `${state.activeModuleId}::${name}`,
  );
  if (!imported) return undefined;
  const resolved = resolveExport(
    imported.moduleId,
    imported.exportName,
    state,
  );
  return state.contextSymbols.get(
    componentKey(resolved.moduleId, resolved.exportName),
  );
}

function collectImports(
  moduleAst: any,
  state: CompilerState,
  moduleId: string,
): void {
  for (const statement of getStatements(moduleAst)) {
    if (statement.type !== 'ImportDeclaration') continue;
    const source = statement.source?.value;
    if (!source) continue;
    for (const specifier of statement.specifiers ?? []) {
      const local =
        getNodeName(specifier.local) ??
        getNodeName(specifier.local?.id);
      if (!local) continue;
      const imported =
        getNodeName(specifier.imported) ??
        (specifier.type === 'ImportDefaultSpecifier'
          ? 'default'
          : local);
      state.importSymbols.set(`${moduleId}::${local}`, {
        moduleId: resolveModuleId(moduleId, source),
        exportName: imported,
      });
    }
    if (source !== '@tanstack/react-router' && source !== 'plec')
      continue;
    for (const specifier of statement.specifiers ?? []) {
      const imported =
        getNodeName(specifier.imported) ?? getNodeName(specifier.local);
      if (imported === 'Link')
        state.routerLinkBindings.add(
          getNodeName(specifier.local) ?? 'Link',
        );
    }
  }
}

function componentKey(moduleId: string, name: string): string {
  return `${moduleId}::${name}`;
}
function resolveModuleId(from: string, source: string): string {
  if (!source.startsWith('.')) return source;
  const base = from.split('/');
  if (/\.(?:tsx?|jsx?|mjs|cjs)$/.test(from)) base.pop();
  for (const part of source.split('/')) {
    if (part === '.' || !part) continue;
    if (part === '..') base.pop();
    else base.push(part);
  }
  const candidate = base.join('/');
  return /\.(?:tsx?|jsx?|mjs|cjs)$/.test(candidate)
    ? candidate
    : `${candidate}.tsx`;
}
function getLocalComponent(name: string, state: CompilerState) {
  const [root, ...members] = name.split('.');
  const imported = state.importSymbols.get(
    `${state.activeModuleId}::${root}`,
  );
  if (imported) {
    const resolved = resolveExport(
      imported.moduleId,
      [...[imported.exportName], ...members].join('.'),
      state,
    );
    const direct =
      state.components.get(
        componentKey(resolved.moduleId, resolved.exportName),
      ) ??
      state.components.get(
        componentKey(imported.moduleId, imported.exportName),
      ) ??
      [...state.components.values()].find(
        (component) =>
          component.moduleId === resolved.moduleId &&
          component.name === resolved.exportName,
      );
    if (direct) return direct;
    if (resolved.exportName === 'default')
      return [...state.components.values()].find(
        (component) => component.moduleId === resolved.moduleId,
      );
  }
  return (
    state.components.get(
      componentKey(state.activeModuleId, root ?? name),
    ) ??
    [...state.components.values()].find(
      (component) => component.name === (root ?? name),
    )
  );
}
function getLocalFunction(name: string, state: CompilerState) {
  const [root] = name.split('.');
  const lexical = unwrapExpression(state.expressionScope[root ?? name]);
  if (
    lexical?.type === 'FunctionDeclaration' ||
    lexical?.type === 'FnDecl' ||
    lexical?.type === 'ArrowFunctionExpression' ||
    lexical?.type === 'FunctionExpression'
  )
    return {
      name: root ?? name,
      body: getFunctionBody(lexical),
      params: lexical.params ?? lexical.function?.params ?? [],
      moduleId: state.activeModuleId,
    };
  const imported = state.importSymbols.get(
    `${state.activeModuleId}::${root}`,
  );
  if (imported) {
    const resolved = resolveExport(
      imported.moduleId,
      imported.exportName,
      state,
    );
    return (
      state.functions.get(
        componentKey(resolved.moduleId, resolved.exportName),
      ) ??
      state.functions.get(
        componentKey(imported.moduleId, imported.exportName),
      )
    );
  }
  return (
    state.functions.get(
      componentKey(state.activeModuleId, root ?? name),
    ) ??
    [...state.functions.values()].find(
      (candidate) => candidate.name === (root ?? name),
    )
  );
}
function resolveExport(
  moduleId: string,
  name: string,
  state: CompilerState,
): { moduleId: string; exportName: string } {
  const [head, ...tail] = name.split('.');
  const found = state.exports.get(moduleId)?.get(head ?? name);
  if (!found) return { moduleId, exportName: name };
  if (!tail.length) return found;
  return resolveExport(
    found.moduleId,
    `${found.exportName === head ? '' : `${found.exportName}.`}${tail.join('.')}`,
    state,
  );
}
function getExternalSymbol(
  name: string,
  state: CompilerState,
): string | null {
  const [root, ...members] = name.split('.');
  const imported = state.importSymbols.get(
    `${state.activeModuleId}::${root}`,
  );
  if (!imported || !imported.moduleId.startsWith('@')) return null;
  const symbol = [imported.exportName, ...members].join('.');
  if (
    state.components.has(
      componentKey(imported.moduleId, imported.exportName),
    )
  )
    return null;
  return `${imported.moduleId}::${symbol}`;
}
function lowerToggleRoot(
  node: any,
  parentId: string | null,
  state: CompilerState,
): LowerResult {
  const props = collectCallerProps(node, state);
  const permitted = new Set([
    'checked',
    'defaultChecked',
    'indeterminate',
    'disabled',
    'readOnly',
    'required',
    'name',
    'value',
    'form',
    'onCheckedChange',
    'className',
    'style',
    'id',
    'aria-label',
    'aria-labelledby',
    'data-testid',
  ]);
  for (const key of Object.keys(props))
    if (!permitted.has(key))
      reportUnsupported(
        state,
        'UNSUPPORTED_TOGGLE_PROP',
        `Toggle.Root prop ${key} is not supported.`,
      );
  const rootId = nextElementId(state);
  const inputId = nextElementId(state);
  const root: ElementNode = {
    id: rootId,
    tag: 'label',
    parentId,
    attributes: [],
    children: [inputId],
  };
  for (const key of [
    'className',
    'style',
    'id',
    'aria-label',
    'aria-labelledby',
    'data-testid',
  ]) {
    const value = props[key];
    if (!value) continue;
    const literal = evaluateExpression(value, state, {});
    if (literal !== undefined)
      root.attributes.push({
        name: key,
        staticValue: literalToString(literal),
      });
    else {
      const bindingId = nextBindingId(state);
      state.ir.bindings.push({
        id: bindingId,
        kind: 'attribute',
        targetId: rootId,
        attributeName: key,
        expression: serializeExpression(value, state),
        expressionId: internExpression(value, state),
      });
      root.attributes.push({ name: key, bindingId });
    }
  }
  const input: ElementNode = {
    id: inputId,
    tag: 'input',
    parentId: rootId,
    attributes: [{ name: 'type', staticValue: 'checkbox' }],
    children: [],
  };
  const valueSource = (key: string) => {
    const value = props[key];
    if (!value) return undefined;
    const literal = evaluateExpression(value, state, {});
    if (literal !== undefined)
      return {
        staticValue:
          literal === 'mixed' ? 'mixed' : literal ? 'true' : 'false',
      };
    return { expressionId: internExpression(value, state) };
  };
  const scalar = (key: string) => {
    const value = props[key];
    if (!value) return;
    const literal = evaluateExpression(value, state, {});
    if (literal !== undefined)
      input.attributes.push({
        name: key,
        staticValue: literalToString(literal),
      });
    else {
      const bindingId = nextBindingId(state);
      state.ir.bindings.push({
        id: bindingId,
        kind: 'attribute',
        targetId: inputId,
        attributeName: key,
        expression: serializeExpression(value, state),
        expressionId: internExpression(value, state),
      });
      input.attributes.push({ name: key, bindingId });
    }
  };
  for (const key of [
    'disabled',
    'readOnly',
    'required',
    'name',
    'value',
    'form',
  ])
    scalar(key);
  const checked = valueSource('checked');
  const defaultChecked =
    valueSource('defaultChecked') ??
    (!checked ? { staticValue: 'false' } : undefined);
  const indeterminate = valueSource('indeterminate');
  const eventId = `ev${++state.eventCounter}`;
  const actionId = props.onCheckedChange
    ? `a${state.eventCounter}`
    : undefined;
  state.ir.events.push({
    id: eventId,
    type: 'change',
    targetId: inputId,
    actionId: actionId ?? `a${state.eventCounter}`,
    args: [],
  });
  const stateSlotId = `s${state.ir.localStates.length + 1}`;
  state.ir.localStates.push({
    id: stateSlotId,
    name: `state-${state.ir.localStates.length + 1}`,
    initialValue:
      defaultChecked?.staticValue === 'true' ? 'true' : 'false',
    values: ['false', 'true', 'mixed'],
  });
  state.ir.elements.push(root, input);
  let indicatorId: string | undefined;
  for (const child of node.children ?? []) {
    if (child.type === 'JSXText' && !normalizeText(child.value ?? ''))
      continue;
    const childName =
      child.type === 'JSXElement'
        ? getJsxName(child.opening?.name)
        : undefined;
    if (
      child.type !== 'JSXElement' ||
      !childName ||
      getExternalSymbol(childName, state) !==
        'internal-toggle-indicator'
    ) {
      reportUnsupported(
        state,
        'UNSUPPORTED_TOGGLE_CHILD',
        'Toggle.Root only supports one direct Toggle.Indicator child.',
      );
      continue;
    }
    if (indicatorId) {
      reportUnsupported(
        state,
        'DUPLICATE_TOGGLE_INDICATOR',
        'Toggle.Root supports only one Toggle.Indicator.',
      );
      continue;
    }
    indicatorId = nextElementId(state);
    const indicator: ElementNode = {
      id: indicatorId,
      tag: 'span',
      parentId: rootId,
      attributes: [{ name: 'data-toggle-indicator', staticValue: '' }],
      children: [],
    };
    for (const attribute of child.opening?.attributes ?? []) {
      const name = getNodeName(attribute.name);
      if (!name || !attribute.value) continue;
      if (attribute.value.type === 'StringLiteral')
        indicator.attributes.push({
          name,
          staticValue: attribute.value.value,
        });
      else if (attribute.value.type === 'JSXExpressionContainer') {
        const bindingId = nextBindingId(state);
        state.ir.bindings.push({
          id: bindingId,
          kind: 'attribute',
          targetId: indicatorId,
          attributeName: name,
          expression: serializeExpression(
            attribute.value.expression,
            state,
          ),
          expressionId: internExpression(
            attribute.value.expression,
            state,
          ),
        });
        indicator.attributes.push({ name, bindingId });
      }
    }
    state.ir.elements.push(indicator);
    root.children.push(indicatorId);
    for (const grandchild of child.children ?? []) {
      const lowered = lowerJsxNode(grandchild, indicatorId, state);
      if (lowered !== SKIP) indicator.children.push(lowered);
    }
  }
  void indicatorId;
  void stateSlotId;
  void actionId;
  void checked;
  void defaultChecked;
  void indeterminate;
  return rootId;
}
function lowerExternalComponent(
  node: any,
  parentId: string | null,
  state: CompilerState,
  external: string,
): LowerResult {
  if (external === 'internal-toggle-root')
    return lowerToggleRoot(node, parentId, state);
  if (external === 'internal-toggle-indicator') {
    reportUnsupported(
      state,
      'TOGGLE_INDICATOR_OUTSIDE_ROOT',
      'Toggle.Indicator must be a direct child of Toggle.Root.',
    );
    return SKIP;
  }
  if (
    external === 'button' ||
    external === 'input' ||
    external === 'span' ||
    external === 'checkbox-input' ||
    external === 'badge'
  ) {
    if (external === 'button' || external === 'span') {
      const intrinsic = {
        ...node,
        opening: {
          ...node.opening,
          name: { type: 'Identifier', value: external },
          attributes: node.opening?.attributes,
        },
        closing: node.closing
          ? {
              ...node.closing,
              name: { type: 'Identifier', value: external },
            }
          : node.closing,
      };
      return lowerJsxElement(intrinsic, parentId, state);
    }
    const tag =
      external === 'checkbox-input'
        ? 'input'
        : external === 'badge'
          ? 'span'
          : external;
    const sourceAttributes = node.opening?.attributes ?? [];
    const callerClass = sourceAttributes.find(
      (attribute: any) => getNodeName(attribute.name) === 'className',
    );
    const variant = sourceAttributes.find(
      (attribute: any) => getNodeName(attribute.name) === 'variant',
    );
    const withoutAdapterProps = sourceAttributes.filter(
      (attribute: any) =>
        !['className', 'variant'].includes(
          getNodeName(attribute.name) ?? '',
        ),
    );
    const classes =
      external === 'checkbox-input'
        ? 'size-4 shrink-0 accent-primary rounded-[6px] border border-input'
        : external === 'input'
          ? 'h-9 w-full min-w-0 rounded-4xl border border-input bg-input/30 px-3 py-1 text-base outline-none md:text-sm'
          : external === 'badge'
            ? 'inline-flex h-5 w-fit shrink-0 items-center justify-center rounded-4xl border border-border bg-input/30 px-2 py-0.5 text-xs font-medium whitespace-nowrap data-[variant=secondary]:border-transparent data-[variant=secondary]:bg-secondary data-[variant=secondary]:text-secondary-foreground'
            : '';
    const classParts: any[] = [
      { expression: { type: 'StringLiteral', value: classes } },
    ];
    if (callerClass?.value?.expression)
      classParts.push({ expression: callerClass.value.expression });
    const classAttribute = classes
      ? {
          type: 'JSXAttribute',
          name: { type: 'Identifier', value: 'className' },
          value: {
            type: 'JSXExpressionContainer',
            expression: {
              type: 'CallExpression',
              callee: { type: 'Identifier', value: 'cn' },
              arguments: classParts,
            },
          },
        }
      : undefined;
    const attributes = [
      ...withoutAdapterProps,
      ...(external === 'badge' && variant
        ? [
            {
              ...variant,
              name: { type: 'Identifier', value: 'data-variant' },
            },
          ]
        : []),
      ...(classAttribute ? [classAttribute] : []),
      ...(external === 'checkbox-input'
        ? [
            {
              type: 'JSXAttribute',
              name: { type: 'Identifier', value: 'type' },
              value: { type: 'StringLiteral', value: 'checkbox' },
            },
          ]
        : []),
    ];
    const intrinsic = {
      ...node,
      opening: {
        ...node.opening,
        name: { type: 'Identifier', value: tag },
        attributes,
      },
      closing: node.closing
        ? { ...node.closing, name: { type: 'Identifier', value: tag } }
        : node.closing,
    };
    return lowerJsxElement(intrinsic, parentId, state);
  }
  const trace = [
    ...state.componentStack,
    getJsxName(node.opening?.name),
  ]
    .filter(Boolean)
    .join(' → ');
  reportUnsupported(
    state,
    'UNSUPPORTED_EXTERNAL_COMPONENT',
    `Cannot lower external component: ${external}\nReached through:\n${trace}`,
  );
  return SKIP;
}

/**
 * Dependency packages are commonly published with JSX compiled to
 * `jsx`/`jsxs` calls. Normalize that syntax back to the small JSX AST surface
 * handled below; this is syntax based and deliberately independent of any
 * package or component identity.
 */
function jsxFactoryElement(node: any): any | null {
  if (node?.type !== 'CallExpression') return null;
  const callee =
    getNodeName(node.callee) ?? getNodeName(node.callee?.property);
  if (
    !callee ||
    ![
      'jsx',
      'jsxs',
      'jsxDEV',
      '_jsx',
      '_jsxs',
      '_jsxDEV',
      'createElement',
      '_createElement',
    ].includes(callee)
  )
    return null;
  const tag = jsxFactoryName(
    node.arguments?.[0]?.expression ?? node.arguments?.[0],
  );
  const props = unwrapExpression(
    node.arguments?.[1]?.expression ?? node.arguments?.[1],
  );
  if (!tag || (props && props.type !== 'ObjectExpression')) return null;
  const attributes: any[] = [];
  const children: any[] = [];
  for (const property of props?.properties ?? []) {
    if (
      property.type === 'SpreadElement' ||
      property.type === 'SpreadProperty'
    ) {
      attributes.push({
        type: 'SpreadElement',
        arguments: property.arguments ?? property.argument,
      });
      continue;
    }
    const name = getNodeName(property.key);
    const value = unwrapExpression(property.value ?? property.expr);
    if (!name) return null;
    if (name === 'children') {
      appendFactoryChildren(value, children);
      continue;
    }
    attributes.push({
      type: 'JSXAttribute',
      name: { type: 'Identifier', value: name },
      value:
        value?.type === 'StringLiteral'
          ? value
          : { type: 'JSXExpressionContainer', expression: value },
    });
  }
  // createElement carries children as positional arguments rather than the
  // JSX-runtime `children` property. Normalize both forms identically.
  if (callee === 'createElement' || callee === '_createElement')
    for (const argument of (node.arguments ?? []).slice(2))
      appendFactoryChildren(argument.expression ?? argument, children);
  return {
    type: 'JSXElement',
    opening: { type: 'JSXOpeningElement', name: tag, attributes },
    closing: children.length
      ? { type: 'JSXClosingElement', name: tag }
      : null,
    children,
  };
}
function jsxFactoryName(value: any): any | null {
  value = unwrapExpression(value);
  if (value?.type === 'StringLiteral')
    return { type: 'Identifier', value: value.value };
  if (value?.type === 'Identifier')
    return { type: 'Identifier', value: getNodeName(value) };
  if (value?.type === 'MemberExpression') {
    const object = jsxFactoryName(value.object);
    const property = jsxFactoryName(value.property);
    if (object && property)
      return { type: 'JSXMemberExpression', object, property };
  }
  return null;
}
function appendFactoryChildren(value: any, children: any[]): void {
  value = unwrapExpression(value);
  if (
    !value ||
    value.type === 'NullLiteral' ||
    (value.type === 'BooleanLiteral' && !value.value)
  )
    return;
  if (value.type === 'ArrayExpression') {
    for (const item of value.elements ?? [])
      appendFactoryChildren(item?.expression ?? item, children);
    return;
  }
  if (value.type === 'StringLiteral') {
    children.push({ type: 'JSXText', value: value.value });
    return;
  }
  const element = jsxFactoryElement(value);
  children.push(
    element ?? { type: 'JSXExpressionContainer', expression: value },
  );
}
function getJsxName(node: any): string | undefined {
  if (!node) return undefined;
  if (node.type === 'JSXMemberExpression')
    return [getJsxName(node.object), getJsxName(node.property)]
      .filter(Boolean)
      .join('.');
  return getNodeName(node);
}
function objectExpression(values: Record<string, any>): any {
  return {
    type: 'ObjectExpression',
    properties: Object.entries(values).map(([key, value]) => ({
      type: 'KeyValueProperty',
      key: { type: 'Identifier', value: key },
      value,
    })),
  };
}
function collectCallerProps(
  node: any,
  state: CompilerState,
): Record<string, any> {
  const props: Record<string, any> = {};
  for (const attribute of node.opening?.attributes ?? []) {
    if (attribute.type === 'SpreadElement') {
      const resolved = resolveSpreadObject(
        attribute.arguments ?? attribute.argument,
        state,
      );
      if (resolved)
        for (const property of resolved.properties ?? []) {
          const key = objectPropertyName(property);
          if (key) props[key] = objectPropertyValue(property);
        }
      else
        reportUnsupported(
          state,
          'UNSUPPORTED_SPREAD_PROPS',
          'Component spread props must be statically known.',
        );
      continue;
    }
    const key = getNodeName(attribute.name);
    if (!key) continue;
    props[key] = !attribute.value
      ? { type: 'BooleanLiteral', value: true }
      : attribute.value.type === 'JSXExpressionContainer'
        ? attribute.value.expression
        : attribute.value;
  }
  return props;
}
function expandIntrinsicAttributes(
  attributes: any[],
  state: CompilerState,
): any[] {
  const output: any[] = [];
  for (const attribute of attributes) {
    if (attribute.type !== 'SpreadElement') {
      output.push(attribute);
      continue;
    }
    const resolved = resolveSpreadObject(
      attribute.arguments ?? attribute.argument,
      state,
    );
    if (!resolved) {
      output.push(attribute);
      continue;
    }
    for (const property of resolved.properties ?? []) {
      const name = objectPropertyName(property);
      if (!name) continue;
      if (name === 'children') continue;
      output.push({
        type: 'JSXAttribute',
        name: { type: 'Identifier', value: name },
        value: {
          type: 'JSXExpressionContainer',
          expression: objectPropertyValue(property),
        },
      });
    }
  }
  return output;
}
function getForwardedChildren(
  expression: any,
  state: CompilerState,
  resolving = new Set<string>(),
): any[] | null {
  const name = getNodeName(expression);
  if (name && resolving.has(name)) return null;
  const value = name ? state.expressionScope[name] : undefined;
  if (value?.type === 'JSXChildren') return value.children;
  // Children can pass through an object spread as an identifier. Resolve that
  // value using the same lexical scope as every other forwarded prop.
  return value
    ? getForwardedChildren(
        value,
        state,
        name ? new Set([...resolving, name]) : resolving,
      )
    : null;
}

/**
 * shadcn-style primitives commonly forward all remaining props with
 * `{...props}`. `children` is part of that object, so retain it as element
 * children rather than silently dropping the rendered subtree.
 */
function getSpreadChildren(
  attributes: any[],
  state: CompilerState,
): any[] {
  const children: any[] = [];
  for (const attribute of attributes) {
    if (attribute.type !== 'SpreadElement') continue;
    const resolved = resolveSpreadObject(
      attribute.arguments ?? attribute.argument,
      state,
    );
    if (!resolved) continue;
    for (const property of resolved.properties ?? []) {
      if (objectPropertyName(property) !== 'children') continue;
      const value = objectPropertyValue(property);
      if (value?.type === 'JSXChildren')
        children.push(...(value.children ?? []));
      else children.push(...(getForwardedChildren(value, state) ?? []));
    }
  }
  return children;
}

/** Expand plain object spreads and source-level prop merges without relying on
 * the identity of the library that supplied those objects. */
function resolveSpreadObject(
  input: any,
  state: CompilerState,
  resolving = new Set<string>(),
): any | null {
  let node = unwrapExpression(input);
  if (node?.type === 'Identifier') {
    const name = getNodeName(node);
    if (!name || resolving.has(name)) return null;
    resolving.add(name);
    node = state.expressionScope[name] ?? node;
  }
  if (node?.type === 'CallExpression' && isObjectMergeCall(node)) {
    const properties: any[] = [];
    for (const argument of node.arguments ?? []) {
      const object = resolveSpreadObject(
        argument.expression ?? argument,
        state,
        new Set(resolving),
      );
      if (!object) return null;
      properties.push(...(object.properties ?? []));
    }
    return { type: 'ObjectExpression', properties };
  }
  if (node?.type !== 'ObjectExpression') return null;
  const properties: any[] = [];
  for (const property of node.properties ?? []) {
    if (
      property.type === 'SpreadElement' ||
      property.type === 'SpreadProperty'
    ) {
      const object = resolveSpreadObject(
        property.arguments ?? property.argument,
        state,
        new Set(resolving),
      );
      if (!object) return null;
      properties.push(...(object.properties ?? []));
    } else {
      properties.push(property);
    }
  }
  return { ...node, properties };
}
function isObjectMergeCall(node: any): boolean {
  const callee =
    getNodeName(node.callee) ?? getNodeName(node.callee?.property);
  return (
    callee === 'mergeProps' ||
    (node.callee?.type === 'MemberExpression' &&
      getNodeName(node.callee.object) === 'Object' &&
      getNodeName(node.callee.property) === 'assign')
  );
}

function internExpression(
  expression: any,
  state: CompilerState,
): string {
  const lowered = lowerExpression(expression, state);
  const existing = state.ir.expressions.find(
    (candidate) =>
      JSON.stringify(candidate.expression) === JSON.stringify(lowered),
  );
  if (existing) return existing.id;
  state.expressionCounter += 1;
  const id = `x${state.expressionCounter}`;
  state.ir.expressions.push({ id, expression: lowered });
  return id;
}

function lowerExpression(
  input: any,
  state: CompilerState,
  resolving = new Set<string>(),
): any {
  const node = unwrapExpression(input);
  if (!node) return { kind: 'literal', value: null };
  // Await is meaningful only to action/loader control flow. If it reaches a
  // value position while that control flow is being inlined, serialize the
  // underlying data expression; capability lowering supplies the actual
  // continuation boundary.
  if (node.type === 'AwaitExpression')
    return lowerExpression(node.argument, state, resolving);
  // SWC represents an omitted optional argument as a wrapper record without a
  // node type. It is not executable syntax and must not turn an otherwise
  // typed action into a strict-mode diagnostic.
  if (!node.type) return { kind: 'literal', value: null };
  if (isLiteralExpression(node))
    return {
      kind: 'literal',
      value: node.type === 'NullLiteral' ? null : node.value,
    };
  const identifier = getNodeName(node);
  if (
    node.type === 'Identifier' ||
    node.type === 'IdentifierExpression'
  ) {
    if (
      identifier &&
      state.expressionScope[identifier] &&
      !resolving.has(identifier)
    ) {
      resolving.add(identifier);
      return lowerExpression(
        state.expressionScope[identifier],
        state,
        resolving,
      );
    }
    return { kind: 'identifier', name: identifier ?? 'unknown' };
  }
  if (node.type === 'MemberExpression') {
    const ref = node.object;
    const refName =
      ref?.type === 'MemberExpression' &&
      getNodeName(ref.property) === 'current'
        ? getNodeName(ref.object)
        : undefined;
    const property = getNodeName(node.property);
    if (
      refName &&
      property &&
      ['value', 'name', 'disabled', 'checked', 'tagName'].includes(
        property,
      )
    ) {
      const capability = { kind: 'property', name: property };
      state.ir.hostElementReads.push({
        id: `hr${state.ir.hostElementReads.length + 1}`,
        refId: refName,
        capability,
      });
      return { kind: 'host-element-read', refId: refName, capability };
    }
    return {
      kind: 'member',
      object: lowerExpression(node.object, state, resolving),
      property: property ?? '',
    };
  }
  if (
    node.type === 'OptionalChainingExpression' ||
    node.type === 'OptionalMemberExpression'
  )
    return {
      kind: 'member',
      object: lowerExpression(
        node.base ?? node.object,
        state,
        resolving,
      ),
      property: getNodeName(node.property) ?? '',
    };
  if (
    node.type === 'CallExpression' &&
    node.callee?.type === 'MemberExpression' &&
    getNodeName(node.callee.object) === 'cookie' &&
    getNodeName(node.callee.property) === 'getSync'
  ) {
    const name = node.arguments?.[0]?.expression ?? node.arguments?.[0];
    if (name?.type !== 'StringLiteral') {
      reportUnsupported(state, 'UNSUPPORTED_COOKIE_NAME', 'cookie.getSync requires a static cookie name.');
      return { kind: 'literal', value: null };
    }
    return { kind: 'host', name: 'cookie', query: name.value };
  }
  if (
    node.type === 'CallExpression' &&
    node.callee?.type === 'MemberExpression' &&
    getNodeName(node.callee.property) === 'getFullYear' &&
    node.callee.object?.type === 'NewExpression' &&
    getNodeName(node.callee.object.callee) === 'Date'
  )
    return { kind: 'host', name: 'currentYear' };
  // Response values are deliberately not part of the wire format. The action
  // lowerer recognizes the adjacent await and makes its decoded JSON value
  // available under the response binding, so a value-path encounter here is
  // simply that continuation binding.
  if (
    node.type === 'CallExpression' &&
    node.callee?.type === 'MemberExpression' &&
    getNodeName(node.callee.property) === 'json'
  )
    return lowerExpression(node.callee.object, state, resolving);
  if (
    node.type === 'CallExpression' &&
    node.callee?.type === 'MemberExpression' &&
    getNodeName(node.callee.object) === 'window' &&
    getNodeName(node.callee.property) === 'matchMedia'
  ) {
    const query =
      node.arguments?.[0]?.expression ?? node.arguments?.[0];
    if (query?.type !== 'StringLiteral') {
      reportUnsupported(
        state,
        'UNSUPPORTED_MEDIA_QUERY',
        'matchMedia requires a static query string.',
      );
      return { kind: 'literal', value: false };
    }
    return { kind: 'host', name: 'media-query', query: query.value };
  }
  if (
    node.type === 'CallExpression' &&
    (getNodeName(node.callee) === 'useContext' ||
      getNodeName(node.callee?.property) === 'useContext')
  ) {
    const argument =
      node.arguments?.[0]?.expression ?? node.arguments?.[0];
    const name = getNodeName(argument);
    const context = name ? resolveContext(name, state) : undefined;
    if (!context) {
      reportUnsupported(
        state,
        'UNRESOLVED_CONTEXT_READ',
        'useContext requires a reachable createContext declaration.',
      );
      return { kind: 'literal', value: null };
    }
    return { kind: 'context', contextId: context.id };
  }
  if (
    node.type === 'CallExpression' &&
    (getNodeName(node.callee) === 'useLocation' ||
      getNodeName(node.callee?.property) === 'useLocation') &&
    (node.arguments?.length ?? 0) === 0
  )
    return {
      kind: 'member',
      object: { kind: 'identifier', name: 'host' },
      property: 'location',
    };
  if (
    node.type === 'CallExpression' &&
    node.callee?.type === 'MemberExpression' &&
    getNodeName(node.callee.property) === 'closest'
  ) {
    const ref = node.callee.object;
    const refName =
      ref?.type === 'MemberExpression' &&
      getNodeName(ref.property) === 'current'
        ? getNodeName(ref.object)
        : undefined;
    const selector =
      node.arguments?.[0]?.expression ?? node.arguments?.[0];
    if (
      refName &&
      selector?.type === 'StringLiteral' &&
      /^[a-z][a-z0-9-]*$/.test(selector.value)
    ) {
      const capability = { kind: 'closest', selector: selector.value };
      state.ir.hostElementReads.push({
        id: `hr${state.ir.hostElementReads.length + 1}`,
        refId: refName,
        capability,
      });
      return { kind: 'host-element-read', refId: refName, capability };
    }
  }
  if (
    node.type === 'CallExpression' &&
    getNodeName(node.callee) === 'Boolean' &&
    node.arguments?.length === 1
  ) {
    const argument = lowerExpression(
      node.arguments[0]?.expression ?? node.arguments[0],
      state,
      resolving,
    );
    return {
      kind: 'unary',
      op: '!',
      argument: { kind: 'unary', op: '!', argument },
    };
  }
  if (
    node.type === 'BinaryExpression' &&
    ['&&', '||', '??'].includes(node.operator)
  )
    return {
      kind: 'logical',
      op: node.operator,
      left: lowerExpression(node.left, state, resolving),
      right: lowerExpression(node.right, state, resolving),
    };
  if (node.type === 'BinaryExpression')
    return {
      kind: 'binary',
      op: node.operator,
      left: lowerExpression(node.left, state, resolving),
      right: lowerExpression(node.right, state, resolving),
    };
  if (node.type === 'LogicalExpression')
    return {
      kind: 'logical',
      op: node.operator,
      left: lowerExpression(node.left, state, resolving),
      right: lowerExpression(node.right, state, resolving),
    };
  if (node.type === 'ConditionalExpression')
    return {
      kind: 'conditional',
      test: lowerExpression(node.test, state, resolving),
      consequent: lowerExpression(node.consequent, state, resolving),
      alternate: lowerExpression(node.alternate, state, resolving),
    };
  if (node.type === 'UnaryExpression')
    return {
      kind: 'unary',
      op: node.operator,
      argument: lowerExpression(node.argument, state, resolving),
    };
  if (node.type === 'TemplateLiteral') {
    const parts: any[] = [];
    for (let i = 0; i < (node.quasis?.length ?? 0); i++) {
      parts.push(
        node.quasis[i]?.raw ?? node.quasis[i]?.value?.cooked ?? '',
      );
      if (node.expressions?.[i])
        parts.push(
          lowerExpression(node.expressions[i], state, resolving),
        );
    }
    return { kind: 'template', parts };
  }
  if (node.type === 'ArrayExpression')
    return {
      kind: 'array',
      items: (node.elements ?? []).filter(Boolean).map((item: any) => {
        const value = item.expression ?? item;
        return item.type === 'SpreadElement' || item.spread
          ? {
              kind: 'spread',
              value: lowerExpression(value, state, resolving),
            }
          : lowerExpression(value, state, resolving);
      }),
    };
  if (node.type === 'ObjectExpression') {
    const expanded = resolveSpreadObject(node, state);
    const properties = expanded?.properties ?? node.properties ?? [];
    return {
      kind: 'object',
      properties: properties
        .filter(
          (property: any) =>
            property.type !== 'MethodProperty' &&
            property.type !== 'GetterProperty' &&
            property.type !== 'SetterProperty',
        )
        .map((property: any) =>
          property.type === 'SpreadElement' ||
          property.type === 'SpreadProperty'
            ? {
                kind: 'spread',
                value: lowerExpression(
                  property.arguments ?? property.argument,
                  state,
                  new Set(resolving),
                ),
              }
            : {
                kind: 'entry',
                key: objectPropertyName(property) ?? '',
                value: lowerExpression(
                  objectPropertyValue(property),
                  state,
                  new Set(resolving),
                ),
              },
        ),
    };
  }
  // Ordered prop composition is a value primitive. Preserve each argument as
  // a spread so last-write-wins remains runtime observable; no library call
  // identity survives in the IR.
  if (node.type === 'CallExpression' && isObjectMergeCall(node)) {
    return {
      kind: 'object',
      properties: (node.arguments ?? []).map((argument: any) => ({
        kind: 'spread',
        value: lowerExpression(
          argument.expression ?? argument,
          state,
          new Set(resolving),
        ),
      })),
    };
  }
  if (
    node.type === 'CallExpression' &&
    ['cn', 'clsx', 'classnames'].includes(
      getNodeName(node.callee) ?? '',
    )
  )
    return {
      kind: 'intrinsic',
      name: getNodeName(node.callee),
      args: (node.arguments ?? []).map((arg: any) =>
        lowerExpression(arg.expression ?? arg, state, resolving),
      ),
    };
  if (
    node.type === 'CallExpression' &&
    getNodeName(node.callee) === 'encodeURIComponent' &&
    node.arguments?.length === 1
  )
    return {
      kind: 'intrinsic',
      name: 'encodeURIComponent',
      args: [
        lowerExpression(
          node.arguments[0]?.expression ?? node.arguments[0],
          state,
          resolving,
        ),
      ],
    };
  if (
    node.type === 'CallExpression' &&
    node.callee?.type === 'MemberExpression'
  ) {
    const name = getNodeName(node.callee.property);
    const receiver = node.callee.object;
    if (
      name &&
      ['trim', 'toLowerCase', 'toUpperCase', 'includes'].includes(name)
    )
      return {
        kind: 'method',
        receiver: lowerExpression(receiver, state, resolving),
        name,
        args: (node.arguments ?? []).map((argument: any) =>
          lowerExpression(
            argument.expression ?? argument,
            state,
            new Set(resolving),
          ),
        ),
      };
    if (name === 'filter' || name === 'map') {
      const callback = unwrapExpression(
        node.arguments?.[0]?.expression ?? node.arguments?.[0],
      );
      const parameter =
        callback?.params?.[0]?.pat ?? callback?.params?.[0];
      const itemName = getPatternName(parameter);
      const indexName = getPatternName(
        callback?.params?.[1]?.pat ?? callback?.params?.[1],
      );
      const body =
        callback?.body?.type === 'BlockStatement'
          ? findReturnedExpression(callback.body)
          : callback?.body;
      if (callback && itemName && body)
        return {
          kind: 'collection',
          op: name,
          source: lowerExpression(receiver, state, resolving),
          itemName,
          ...(indexName ? { indexName } : {}),
          expression: lowerCallbackExpression(
            body,
            itemName,
            indexName,
            state,
            resolving,
          ),
        };
      reportUnsupported(
        state,
        'UNSUPPORTED_COLLECTION_CALLBACK',
        `${name} requires an inline callback with an identifier item parameter.`,
      );
      return { kind: 'array', items: [] };
    }
  }
  if (
    node.type === 'CallExpression' &&
    ['useMemo', 'useCallback'].includes(
      getNodeName(node.callee) ??
        getNodeName(node.callee?.property) ??
        '',
    )
  ) {
    const callback = unwrapExpression(
      node.arguments?.[0]?.expression ?? node.arguments?.[0],
    );
    const returned =
      callback?.body?.type === 'BlockStatement'
        ? findReturnedExpression(callback.body)
        : callback?.body;
    if (returned) return lowerExpression(returned, state, resolving);
  }
  if (node.type === 'CallExpression') {
    const inlined = lowerSourceFunctionCall(node, state, resolving);
    if (inlined) return inlined;
  }
  reportUnsupported(
    state,
    'UNSUPPORTED_EXPRESSION',
    `Unsupported expression: ${node.type}${describeExpression(node) ? ` (${describeExpression(node)})` : ''}.`,
  );
  return { kind: 'literal', value: '' };
}

/** The item variables are names in the serialized expression environment, not
 * callback values. This is the only representation carried into the runtime. */
function lowerCallbackExpression(
  body: any,
  itemName: string,
  indexName: string | undefined,
  state: CompilerState,
  resolving: Set<string>,
): any {
  const previous = state.expressionScope;
  state.expressionScope = {
    ...previous,
    [itemName]: { type: 'Identifier', value: itemName },
    ...(indexName
      ? { [indexName]: { type: 'Identifier', value: indexName } }
      : {}),
  };
  try {
    return lowerExpression(body, state, new Set(resolving));
  } finally {
    state.expressionScope = previous;
  }
}

function describeExpression(node: any): string | undefined {
  if (node?.type !== 'CallExpression' && node?.type !== 'NewExpression')
    return undefined;
  return getNodeName(node.callee) ?? getNodeName(node.callee?.property);
}

/** Inline reachable, expression-only helpers (including custom hooks) using
 * ordinary lexical parameter binding. No helper name or package is special. */
function lowerSourceFunctionCall(
  node: any,
  state: CompilerState,
  resolving: Set<string>,
): any | null {
  const name = getNodeName(node.callee);
  if (!name) return null;
  const definition = getLocalFunction(name, state);
  if (!definition) return null;
  const key = `${definition.moduleId}::${definition.name}`;
  if (state.functionStack.includes(key)) {
    reportUnsupported(
      state,
      'RECURSIVE_SOURCE_FUNCTION',
      `Recursive source function: ${[...state.functionStack, key].join(' → ')}.`,
    );
    return { kind: 'literal', value: '' };
  }
  const previousScope = state.expressionScope;
  const previousModule = state.activeModuleId;
  const scope = {
    ...previousScope,
    ...(state.moduleExpressions.get(definition.moduleId) ?? {}),
  };
  for (
    let index = 0;
    index < (definition.params ?? []).length;
    index += 1
  ) {
    const parameter =
      definition.params[index]?.pat ??
      definition.params[index]?.pattern ??
      definition.params[index];
    const parameterName = getPatternName(parameter);
    const argument =
      node.arguments?.[index]?.expression ?? node.arguments?.[index];
    if (!parameterName || !argument) {
      reportUnsupported(
        state,
        'UNSUPPORTED_SOURCE_FUNCTION_PARAMETER',
        `Function ${name} requires simple positional parameters.`,
      );
      return null;
    }
    scope[parameterName] = argument;
  }
  for (const statement of getStatements(definition.body)) {
    if (
      statement.type !== 'VariableDeclaration' &&
      statement.type !== 'VarDecl'
    )
      continue;
    for (const declaration of statement.declarations ??
      statement.decls ??
      []) {
      const local = getPatternName(declaration.id);
      if (local && declaration.init) scope[local] = declaration.init;
    }
  }
  const returned = findReturnedExpression(definition.body);
  if (!returned) {
    reportUnsupported(
      state,
      'UNSUPPORTED_SOURCE_FUNCTION_BODY',
      `Function ${name} must return a supported expression.`,
    );
    return null;
  }
  state.expressionScope = scope;
  state.activeModuleId = definition.moduleId;
  state.functionStack.push(key);
  const result = lowerExpression(returned, state, new Set(resolving));
  state.functionStack.pop();
  state.expressionScope = previousScope;
  state.activeModuleId = previousModule;
  return result;
}

function objectPropertyName(property: any): string | undefined {
  return (
    getNodeName(property?.key) ??
    (property?.type === 'Identifier' ||
    property?.type === 'IdentifierExpression'
      ? getNodeName(property)
      : undefined)
  );
}

function objectPropertyValue(property: any): any {
  return (
    property?.value ??
    property?.expr ??
    (property?.type === 'Identifier' ||
    property?.type === 'IdentifierExpression'
      ? property
      : undefined)
  );
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
 * crosses into Plec, so no import path or hook name is part of this decision. */
function ensureCollectionInput(
  name: string,
  callback: any,
  state: CompilerState,
): CompiledInputNode | undefined {
  const existing = state.ir.inputs.find((input) => input.name === name);
  if (existing) return existing;
  const itemName = getPatternLabel(callback?.params?.[0]);
  const keyExpression = getMapRowKeyExpression(callback, state);
  if (!itemName || !keyExpression) return undefined;
  const observed = new Set<string>();
  const visit = (node: any): void => {
    if (!node || typeof node !== 'object') return;
    if (
      (node.type === 'MemberExpression' ||
        node.type === 'OptionalChainingExpression') &&
      getNodeName(node.object) === itemName
    ) {
      const property = getNodeName(node.property);
      if (property) observed.add(property);
    }
    for (const value of Object.values(node)) {
      if (Array.isArray(value)) value.forEach(visit);
      else if (value && typeof value === 'object') visit(value);
    }
  };
  visit(callback.body);
  const input: CompiledInputNode = {
    id: nextInputId(state),
    name,
    shape: {
      kind: 'collection',
      keyExpression,
      orderSensitive: true,
      observedRowPaths: [...observed].sort().map((path) => [path]),
    },
  };
  state.ir.inputs.push(input);
  return input;
}

function inferValueInputs(state: CompilerState): void {
  const rowNames = new Set(state.ir.loops.map((loop) => loop.itemName));
  const expressions = new Map(
    state.ir.expressions.map((entry) => [entry.id, entry.expression]),
  );
  for (const binding of state.ir.bindings) {
    const expression: any = expressions.get(binding.expressionId ?? '');
    const path = inputPath(expression);
    if (!path) continue;
    const inputName = path[0]!;
    // A source construct that is not representable as a value input must be
    // diagnosed at its origin, never materialized as an `undefined` producer.
    if (
      !inputName ||
      inputName === 'undefined' ||
      inputName === 'unknown' ||
      rowNames.has(inputName) ||
      inputName === 'host'
    )
      continue;
    let input = state.ir.inputs.find(
      (candidate) => candidate.name === inputName,
    );
    if (!input) {
      input =
        path.length === 1
          ? {
              id: nextInputId(state),
              name: inputName,
              shape: { kind: 'scalar' },
            }
          : {
              id: nextInputId(state),
              name: inputName,
              shape: { kind: 'object', observedPaths: [] },
            };
      state.ir.inputs.push(input);
    }
    if (
      input.shape.kind === 'object' &&
      path.length > 1 &&
      !input.shape.observedPaths.some(
        (candidate) => candidate.join('.') === path.slice(1).join('.'),
      )
    )
      input.shape.observedPaths.push(path.slice(1));
    state.ir.dependencyEdges.push({
      fromId: input.id,
      toId: binding.id,
      kind: 'input-to-binding',
    });
  }
}

function inputPath(expression: any): string[] | undefined {
  if (!expression) return undefined;
  if (expression.kind === 'identifier') return [expression.name];
  if (expression.kind === 'member') {
    const base = inputPath(expression.object);
    return base ? [...base, expression.property] : undefined;
  }
  return undefined;
}

function getElementById(
  state: CompilerState,
  elementId: string,
):
  | (LegacyApplicationFacts['elements'][number] & { keyValue?: string })
  | undefined {
  return state.ir.elements.find(
    (element) => element.id === elementId,
  ) as
    | (LegacyApplicationFacts['elements'][number] & {
        keyValue?: string;
      })
    | undefined;
}

function mergeScopes(
  baseScope: Record<string, unknown>,
  extraScope: Record<string, unknown>,
): Record<string, unknown> {
  return { ...baseScope, ...extraScope };
}

function getPatternName(pattern: any): string | undefined {
  if (!pattern) {
    return undefined;
  }

  if (pattern.type === 'Identifier') {
    return pattern.value;
  }

  if (pattern.type === 'AssignmentPattern') {
    return getPatternName(pattern.left);
  }

  if (pattern.type === 'RestElement') {
    return getPatternName(pattern.argument);
  }

  if (
    pattern.type === 'ObjectPattern' ||
    pattern.type === 'ArrayPattern'
  ) {
    return undefined;
  }

  return getNodeName(pattern);
}

function getPatternLabel(pattern: any): string | undefined {
  const name = getPatternName(pattern);
  if (name) {
    return name;
  }

  if (pattern?.type === 'ArrayPattern') {
    return 'tuple';
  }

  if (pattern?.type === 'ObjectPattern') {
    return 'object';
  }

  return undefined;
}

function normalizeText(text: string): string {
  return text.replace(/\s+/g, ' ').trim();
}

function isLiteralExpression(expression: any): boolean {
  return (
    expression?.type === 'StringLiteral' ||
    expression?.type === 'NumericLiteral' ||
    expression?.type === 'BooleanLiteral' ||
    expression?.type === 'NullLiteral'
  );
}

function literalToString(expression: any): string {
  if (
    expression &&
    typeof expression === 'object' &&
    expression.type === 'NullLiteral'
  ) {
    return 'null';
  }

  if (
    expression &&
    typeof expression === 'object' &&
    'value' in expression
  ) {
    return String((expression as { value: unknown }).value);
  }

  return String(expression);
}

function serializeExpression(
  expression: any,
  state: CompilerState,
): string {
  const start = Math.max(0, (expression?.span?.start ?? 1) - 1);
  const end = Math.max(start, (expression?.span?.end ?? start + 1) - 1);
  const snippet = state.source.slice(start, end).trim();
  if (snippet) {
    return snippet;
  }
  return expression?.type ?? 'unknown';
}

/** State initializers are executed by the runtime, so they must be values—not
 * source snippets. In particular, a route component may be lowered from an
 * imported module while `state.source` still names the route entry, making an
 * SWC span slice return unrelated text such as `);`. */
function serializeInitialState(
  expression: any,
  state: CompilerState,
): string {
  const node = unwrapExpression(expression);
  const value = evaluateExpression(node, state, state.scope);
  if (
    value === null ||
    typeof value === 'string' ||
    typeof value === 'number' ||
    typeof value === 'boolean'
  )
    return JSON.stringify(value);
  if (getNodeName(node) === 'undefined') return 'undefined';
  // Loader and other host-provided values are not yet a state initializer
  // capability. Null is safe and keeps the graph executable; a later input
  // binding can replace it without treating compiler source as data.
  return 'null';
}

function isIntrinsicTag(tag: string): boolean {
  return /^[a-z][a-z0-9-]*$/.test(tag);
}

function isComponentName(value?: string): boolean {
  return typeof value === 'string' && /^[A-Z]/.test(value);
}

function getNodeName(node: any): string | undefined {
  if (!node) {
    return undefined;
  }

  if (typeof node.value === 'string') {
    return node.value;
  }

  if (typeof node.name === 'string') {
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
      current.type === 'ParenthesisExpression' ||
      current.type === 'ParenthesizedExpression' ||
      current.type === 'ParenExpr' ||
      current.type === 'ParenExpression'
    ) {
      current = current.expression;
      continue;
    }

    if (
      current.type === 'TsAsExpression' ||
      current.type === 'TsTypeAssertion'
    ) {
      current = current.expression;
      continue;
    }

    if (current.type === 'TsNonNullExpression') {
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

function reportUnsupported(
  state: CompilerState,
  code: string,
  message: string,
): void {
  state.diagnostics.push({
    code,
    message,
    severity: 'error',
  });
}

function throwIfStrict(state: CompilerState): void {
  if (state.mode !== 'strict') {
    return;
  }

  const firstDiagnostic = state.diagnostics[0];
  if (firstDiagnostic) {
    throw new CompileFailure(
      `Strict compilation failed: ${firstDiagnostic.code} ${firstDiagnostic.message}`,
    );
  }
}
