export {
  GraphContractError,
  PlecGraphCoordinator,
  type GraphArtifactLoader,
  type GraphInstanceId,
  type GraphRuntimeAdapter,
  type MountGraphRequest,
  type OutputWiring,
  type ReplaceGraphRequest,
} from './graph-coordinator.js';
import {
  validateExecutableApplication,
  validatePlecRouteManifest,
} from '../../plec-ir/dist/index.js';

export type RuntimeDelta =
  | {
      type: 'update';
      inputId: string;
      rowKey: string;
      changes: Record<string, unknown>;
    }
  | {
      type: 'insert';
      inputId: string;
      rowKey: string;
      row: Record<string, unknown>;
      beforeRowKey?: string | null;
    }
  | { type: 'remove'; inputId: string; rowKey: string }
  | {
      type: 'move';
      inputId: string;
      rowKey: string;
      beforeRowKey?: string | null;
    };

export type LiveCollectionChange<Row> =
  | {
      type: 'insert';
      key: string | number;
      value: Row;
      beforeKey?: string | number | null;
    }
  | {
      type: 'update';
      key: string | number;
      value: Row;
      previousValue: Row;
      changes?: Partial<Row>;
    }
  | { type: 'delete'; key: string | number; previousValue: Row }
  | {
      type: 'move';
      key: string | number;
      beforeKey?: string | number | null;
    };

export interface LiveCollection<
  Row extends object = Record<string, unknown>,
> {
  readonly toArray: Row[];
  /** Optional lookup support for callers; the runtime adapter never uses it. */
  get?(key: string | number): Row | undefined;
  subscribeChanges(
    callback: (changes: LiveCollectionChange<Row>[]) => void,
  ): { unsubscribe(): void };
}

export interface SnapshotInput<T> {
  getSnapshot(): T;
  subscribe?(notify: () => void): () => void;
}

export interface DeltaInput<T> extends SnapshotInput<T> {
  subscribeDeltas(notify: (deltas: RuntimeDelta[]) => void): () => void;
}

export type CompiledInputProducer<T = unknown> =
  SnapshotInput<T> | DeltaInput<T>;

/** A small host-owned channel for generated React controllers and vanilla
 * publishers. It is a snapshot producer; richer sources can use DeltaInput. */
export interface CompiledInputChannel<T> extends SnapshotInput<T> {
  publish(value: T): void;
}

export function createCompiledInputChannel<T>(
  initialValue: T,
): CompiledInputChannel<T> {
  let value = initialValue;
  const listeners = new Set<() => void>();
  return {
    getSnapshot: () => value,
    subscribe: (notify) => {
      listeners.add(notify);
      return () => listeners.delete(notify);
    },
    publish: (next) => {
      value = next;
      listeners.forEach((notify) => notify());
    },
  };
}

export interface CompiledMountOptions {
  root: Element;
  /** Set false when this mount intentionally replaces an existing runtime
   * subtree, such as a Plec route outlet. */
  adopt?: boolean;
  /** Package-neutral producer boundary. Prefer this for new integrations. */
  inputs?: Record<string, CompiledInputProducer<any>>;
  /** @deprecated Use inputs. Retained while existing applications migrate. */
  queries?: Record<string, LiveCollection<any>>;
  irUrl?: string;
  runtimeJsUrl?: string;
  runtimeWasmUrl?: string;
  onQueryUpdate?: (update: CompiledQueryUpdate) => void;
  onMount?: (metrics: MountMetrics) => void;
  hostValues?: {
    currentYear?: number | string;
    location?: { pathname: string };
  };
  islands?: Record<
    string,
    (
      placeholder: Element,
      props: Record<string, unknown>,
    ) => void | (() => void)
  >;
  onNavigate?: (navigation: {
    href: string;
    replace?: boolean;
  }) => void;
}
export interface CompiledEventContext {
  type: string;
  target: Element;
  currentTarget: Element;
  rowKey?: string;
  value?: string;
  checked?: boolean;
  button?: number;
  metaKey: boolean;
  ctrlKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
  nativeEvent: Event;
}

export interface CompiledUpdateMetrics {
  domOperations: number;
  nodesTouched: number;
  bindingsTouched: number;
  wasmDomUs: number;
}
export interface RuntimeMountMetrics {
  decodeUs: number;
  staticMountUs: number;
  rowProgramExecuteUs: number;
  rowStateRegistrationUs: number;
  fragmentAppendUs: number;
  programCompileUs: number;
  programRevision?: string;
  instructionCount: number;
  compiledExpressionCount: number;
  fieldSlotCount: number;
  bindingProgramCount: number;
  averageRowProgramExecuteUs: number;
  rowCount: number;
  createdElements: number;
  createdTexts: number;
  bindings: number;
  domOperations: number;
}
export interface MountMetrics extends RuntimeMountMetrics {
  irFetchMs: number;
  irParseMs: number;
  wasmInitMs: number;
  runtimeLoadMs: number;
  renderMode: 'adopt' | 'mount';
}
export interface CompiledQueryUpdate extends CompiledUpdateMetrics {
  adapterMs: number;
  reconciliationMs?: number;
  deltaCount?: number;
}
export interface CompiledControllerDiagnostics {
  activeQuerySubscriptions: number;
  activeInputSubscriptions: number;
  activeActionListeners: number;
  activeIslands: number;
  queryIds: string[];
  inputIds: string[];
  disposed: boolean;
}
export interface CompiledRuntimeController {
  readonly mountMetrics: MountMetrics;
  diagnostics(): CompiledControllerDiagnostics;
  dispose(): void;
  applyDelta(delta: RuntimeDelta): CompiledUpdateMetrics;
  applyHostValues(
    values: NonNullable<CompiledMountOptions['hostValues']>,
  ): void;
  outlet(id?: string): Element | null;
}

interface WasmRuntimeInstance {
  set_host_inputs(values: Record<string, unknown>): void;
  complete_cookie_request(instanceId: string, requestId: number, value: unknown, failure?: string): void;
  load_application(ir: unknown): void;
  adopt(root: Element): RuntimeMountMetrics | null;
  mount(root: Element): RuntimeMountMetrics;
  initialize_input(inputId: string, rows: unknown): RuntimeMountMetrics;
  apply_delta(delta: unknown): CompiledUpdateMetrics;
  apply_deltas(deltas: unknown): CompiledUpdateMetrics;
  dispose(): void;
  register_graph(graphId: string, ir: unknown): void;
  start(root: Element, manifest: unknown): void;
}
interface WasmRuntimeModule {
  default(input?: unknown): Promise<unknown>;
  PlecRuntime: new () => WasmRuntimeInstance;
}
const DEFAULT_IR_URL = '/application.ir.json';
const DEFAULT_RUNTIME_JS_URL = '/runtime/runtime.js';
const DEFAULT_RUNTIME_WASM_URL = '/runtime/runtime_bg.wasm';

export interface PlecRouterMountOptions {
  root: Element;
  manifestUrl?: string;
  graphUrl?: (graphId: string) => string;
  runtimeJsUrl?: string;
  runtimeWasmUrl?: string;
  /** Host authority may be a stricter subset than graph-declared authority. */
  cookiePolicy?: Record<string, { operations: Array<'getSync' | 'get' | 'set' | 'delete'>; path?: string }>;
}
export interface PlecRouterController {
  dispose(): void;
}

export const PLEC_TIMING_MARKS = [
  'plec:artifact-fetch-start',
  'plec:artifact-ready',
  'plec:ir-parse-start',
  'plec:ir-parse-end',
  'plec:runtime-init-start',
  'plec:runtime-ready',
  'plec:runtime-load-start',
  'plec:runtime-load-end',
  'plec:mount-start',
  'plec:mount-end',
  'plec:mount-error',
] as const;
export type PlecTimingMark = (typeof PLEC_TIMING_MARKS)[number];

/** User Timing is deliberately optional so the runtime remains usable in non-browser tests. */
export function markPlecTiming(name: PlecTimingMark): void {
  if (
    typeof performance !== 'undefined' &&
    typeof performance.mark === 'function'
  )
    performance.mark(name);
}

export async function mountPlecApplication(
  options: CompiledMountOptions,
): Promise<CompiledRuntimeController> {
  try {
    const fetchStart = performance.now();
    markPlecTiming('plec:artifact-fetch-start');
    const runtimeModulePromise = loadRuntimeModule(
      options.runtimeJsUrl ?? DEFAULT_RUNTIME_JS_URL,
    );
    const response = await fetch(options.irUrl ?? DEFAULT_IR_URL);
    const irFetchMs = performance.now() - fetchStart;
    if (!response.ok)
      throw new Error(`Failed to load IR: ${response.status}`);
    markPlecTiming('plec:ir-parse-start');
    const parseStart = performance.now();
    const ir = validateExecutableApplication(await response.json());
    const irParseMs = performance.now() - parseStart;
    markPlecTiming('plec:ir-parse-end');
    markPlecTiming('plec:artifact-ready');
    const runtimeModule = await runtimeModulePromise;
    markPlecTiming('plec:runtime-init-start');
    const wasmStart = performance.now();
    await runtimeModule.default({
      module_or_path:
        options.runtimeWasmUrl ?? DEFAULT_RUNTIME_WASM_URL,
    });
    const wasmInitMs = performance.now() - wasmStart;
    markPlecTiming('plec:runtime-ready');
    const runtime = new runtimeModule.PlecRuntime();
    markPlecTiming('plec:runtime-load-start');
    markPlecTiming('plec:mount-start');
    const loadStart = performance.now();
    runtime.load_application(ir);
    // Executable graphs deliberately own their DOM from the start; adoption
    // belonged to the removed string-ID renderer.
    const staticMetrics = runtime.mount(options.root);
    const renderMode = 'mount' as const;
    const runtimeLoadMs = performance.now() - loadStart;
    markPlecTiming('plec:runtime-load-end');
    const disposeIslands = mountIslands(
      options.root,
      ir,
      options.islands ?? {},
    );
    const applyHostValues = (
      values: NonNullable<CompiledMountOptions['hostValues']>,
    ) => {
      void values;
    };
    applyHostValues(
      options.hostValues ?? {
        location: { pathname: window.location.pathname },
      },
    );
    const inputSchemas = new Map<string, any>(
      (ir.inputs ?? []).map((input: any): [string, any] => [
        ir.strings[input.name]!,
        input,
      ]),
    );
    const inputs: Record<string, CompiledInputProducer<any>> = {
      ...Object.fromEntries(
        Object.entries(options.queries ?? {}).map(
          ([id, collection]) => {
            const inputId =
              (ir.inputs ?? []).length === 1
                ? ir.strings[ir.inputs[0]!.name]!
                : id;
            return [
              inputId,
              liveCollectionProducer(inputId, collection),
            ];
          },
        ),
      ),
      ...(options.inputs ?? {}),
    };
    const initialMetrics = Object.entries(inputs).flatMap(
      ([inputId, producer]) => {
        const shape = inputSchemas.get(inputId);
        return shape?.kind === 'collection'
          ? [runtime.initialize_input(inputId, producer.getSnapshot())]
          : [];
      },
    );
    const mountMetrics = mergeMountMetrics(
      staticMetrics,
      initialMetrics,
      { irFetchMs, irParseMs, wasmInitMs, runtimeLoadMs, renderMode },
    );
    options.onMount?.(mountMetrics);
    const subscriptions = Object.entries(inputs).flatMap(
      ([inputId, producer]) =>
        subscribeInput(
          runtime,
          inputId,
          producer,
          inputSchemas.get(inputId),
          options.onQueryUpdate,
        ),
    );
    markPlecTiming('plec:mount-end');
    let disposed = false;
    const diagnostics = (): CompiledControllerDiagnostics => ({
      activeQuerySubscriptions: 0,
      activeInputSubscriptions: disposed ? 0 : subscriptions.length,
      // DOM listeners are registered by the WASM renderer. TypeScript has no
      // normal event-execution role.
      activeActionListeners: 0,
      activeIslands: disposed ? 0 : disposeIslands.length,
      queryIds: Object.keys(options.queries ?? {}),
      inputIds: Object.keys(inputs),
      disposed,
    });
    const outlet = (id = 'main') => {
      const descriptor = ((ir as any).layout?.routeOutlets ?? []).find(
        (entry: any) => entry.id === id,
      );
      return descriptor
        ? options.root.querySelector(
            `[data-runtime-node="${descriptor.elementId}"]`,
          )
        : null;
    };
    return {
      mountMetrics,
      diagnostics,
      applyDelta: (delta) => apply(runtime, delta),
      applyHostValues,
      outlet,
      dispose: () => {
        if (disposed) return;
        disposed = true;
        runtime.dispose();
        disposeIslands.forEach((dispose) => dispose());
        subscriptions.forEach((dispose) => dispose());
      },
    };
  } catch (error) {
    markPlecTiming('plec:mount-error');
    throw error;
  }
}

/** Transport-only router bootstrap. It fetches immutable artifacts and hands
 * them to WASM; route matching, history and outlet ownership stay in Rust. */
export async function startPlecRouter(
  options: PlecRouterMountOptions,
): Promise<PlecRouterController> {
  const manifestResponse = await fetch(
    options.manifestUrl ?? '/route-manifest.json',
  );
  if (!manifestResponse.ok)
    throw new Error(
      `Failed to load route manifest: ${manifestResponse.status}`,
    );
  const manifest = validatePlecRouteManifest(
    await manifestResponse.json(),
  );
  const graphUrl = options.graphUrl ?? ((id) => `/graphs/${id}.json`);
  const graphIds = new Set<string>([
    manifest.rootGraphId,
    ...manifest.routes.flatMap(
      (route) =>
        [
          route.graphId,
          route.pendingGraphId,
          route.errorGraphId,
        ].filter(Boolean) as string[],
    ),
  ]);
  const runtimeModule = await loadRuntimeModule(
    options.runtimeJsUrl ?? DEFAULT_RUNTIME_JS_URL,
  );
  await runtimeModule.default({
    module_or_path: options.runtimeWasmUrl ?? DEFAULT_RUNTIME_WASM_URL,
  });
  const runtime = new runtimeModule.PlecRuntime();
  const graphs = await Promise.all(
    [...graphIds].map(async (graphId) => {
      const response = await fetch(graphUrl(graphId));
      if (!response.ok)
        throw new Error(
          `Failed to load graph ${graphId}: ${response.status}`,
        );
      return validateExecutableApplication(await response.json());
    }),
  );
  const cookieNames = graphs.flatMap((graph: any) => (graph.capabilities ?? []).filter((capability: any) => capability.kind === 'cookie' && capability.operations.includes('getSync')).map((capability: any) => capability.name));
  runtime.set_host_inputs(Object.fromEntries([...new Set(cookieNames)].map((name) => [name, readCookie(name)])));
  graphs.forEach((graph: any, index) => runtime.register_graph([...graphIds][index]!, graph));
  const onCookieRequest = (event: Event) => {
    const request = (event as CustomEvent<any>).detail;
    try {
      const allowed = options.cookiePolicy?.[request.name];
      const operation = request.operation === 'get' ? 'get' : request.operation;
      if (allowed && !allowed.operations.includes(operation)) throw new Error('cookie operation denied by host policy');
      if (allowed?.path && allowed.path !== request.path) throw new Error('cookie path denied by host policy');
      if (request.operation === 'get') runtime.complete_cookie_request(request.instanceId, request.requestId, readCookie(request.name));
      else {
        const attributes = [`path=${request.path}`];
        if (request.expiry === 'maxAge') attributes.push(`max-age=${request.maxAge ?? 0}`);
        if (request.sameSite) attributes.push(`samesite=${request.sameSite}`);
        if (request.secure) attributes.push('secure');
        document.cookie = `${encodeURIComponent(request.name)}=${encodeURIComponent(request.operation === 'delete' ? '' : request.value ?? '')}; ${attributes.join('; ')}`;
        runtime.complete_cookie_request(request.instanceId, request.requestId, null);
      }
    } catch (error) { runtime.complete_cookie_request(request.instanceId, request.requestId, null, error instanceof Error ? error.message : String(error)); }
  };
  window.addEventListener('plec:cookie-request', onCookieRequest);
  runtime.start(options.root, manifest);
  return { dispose: () => { window.removeEventListener('plec:cookie-request', onCookieRequest); runtime.dispose(); } };
}

function readCookie(name: string): string | null {
  const prefix = `${encodeURIComponent(name)}=`;
  const part = document.cookie.split(/;\s*/).find((entry) => entry.startsWith(prefix));
  return part ? decodeURIComponent(part.slice(prefix.length)) : null;
}

function applyStaticHostBindings(
  root: Element,
  ir: any,
  values: NonNullable<CompiledMountOptions['hostValues']>,
) {
  const expressions = new Map(
    (ir.expressions ?? []).map((entry: any) => [
      entry.id,
      entry.expression,
    ]),
  );
  for (const binding of ir.bindings ?? []) {
    const expression = expressions.get(binding.expressionId);
    if (!dependsOnHost(expression)) continue;
    const node = root.querySelector(
      `[data-runtime-node="${binding.targetId}"]`,
    );
    if (!(node instanceof Element) || binding.kind !== 'attribute')
      continue;
    const value = evaluateHostExpression(expression, { host: values });
    node.setAttribute(
      binding.attributeName === 'className'
        ? 'class'
        : binding.attributeName,
      String(value ?? ''),
    );
  }
}
function dependsOnHost(expression: any): boolean {
  return expression?.kind === 'identifier'
    ? expression.name === 'host'
    : expression?.kind === 'member'
      ? dependsOnHost(expression.object)
      : expression?.kind === 'binary'
        ? dependsOnHost(expression.left) ||
          dependsOnHost(expression.right)
        : expression?.kind === 'conditional'
          ? dependsOnHost(expression.test) ||
            dependsOnHost(expression.consequent) ||
            dependsOnHost(expression.alternate)
          : false;
}
function evaluateHostExpression(expression: any, scope: any): any {
  if (expression?.kind === 'literal') return expression.value;
  if (expression?.kind === 'identifier') return scope[expression.name];
  if (expression?.kind === 'member')
    return evaluateHostExpression(expression.object, scope)?.[
      expression.property
    ];
  if (expression?.kind === 'binary')
    return expression.op === '==='
      ? evaluateHostExpression(expression.left, scope) ===
          evaluateHostExpression(expression.right, scope)
      : null;
  if (expression?.kind === 'conditional')
    return evaluateHostExpression(expression.test, scope)
      ? evaluateHostExpression(expression.consequent, scope)
      : evaluateHostExpression(expression.alternate, scope);
  return null;
}

function resolveHostValues(
  ir: any,
  values: { currentYear?: number | string },
) {
  const expressions = new Map<string, any>(
    (ir.expressions ?? []).map((expression: any): [string, any] => [
      expression.id,
      expression.expression,
    ]),
  );
  const textById = new Map<string, any>(
    (ir.texts ?? []).map((text: any): [string, any] => [text.id, text]),
  );
  const elementById = new Map<string, any>(
    (ir.elements ?? []).map((element: any): [string, any] => [
      element.id,
      element,
    ]),
  );
  ir.bindings = (ir.bindings ?? []).filter((binding: any) => {
    const expression = expressions.get(binding.expressionId);
    if (
      expression?.kind !== 'host' ||
      expression.name !== 'currentYear'
    )
      return true;
    const value = String(
      values.currentYear ?? new Date().getFullYear(),
    );
    if (binding.kind === 'text')
      textById.get(binding.targetId)!.staticValue = value;
    else {
      const element = elementById.get(binding.targetId)!;
      const attribute = element.attributes.find(
        (candidate: any) => candidate.name === binding.attributeName,
      );
      if (attribute) {
        attribute.staticValue = value;
        delete attribute.bindingId;
      } else
        element.attributes.push({
          name: binding.attributeName,
          staticValue: value,
        });
    }
    return false;
  });
  // Host values are materialized as static text/attributes before WASM sees
  // the IR. Remove their expression records too: the row-expression runtime
  // intentionally has no host-environment evaluator.
  const referencedExpressionIds = new Set(
    (ir.bindings ?? [])
      .map((binding: any) => binding.expressionId)
      .filter(Boolean),
  );
  ir.expressions = (ir.expressions ?? []).filter(
    (expression: any) =>
      expression.expression?.kind !== 'host' ||
      referencedExpressionIds.has(expression.id),
  );
  return ir;
}

function mountIslands(
  root: Element,
  ir: any,
  registry: NonNullable<CompiledMountOptions['islands']>,
) {
  const disposers: Array<() => void> = [];
  for (const island of ir.islands ?? []) {
    const mount = registry[island.componentId];
    const placeholder = root.querySelector<HTMLElement>(
      `[data-runtime-node="${island.placeholderNodeId}"]`,
    );
    if (!mount || !placeholder) continue;
    const dispose = mount(placeholder, island.props ?? {});
    if (typeof dispose === 'function') disposers.push(dispose);
  }
  return disposers;
}

function mergeMountMetrics(
  staticMetrics: RuntimeMountMetrics,
  initial: RuntimeMountMetrics[],
  browser: Pick<
    MountMetrics,
    | 'irFetchMs'
    | 'irParseMs'
    | 'wasmInitMs'
    | 'runtimeLoadMs'
    | 'renderMode'
  >,
): MountMetrics {
  const total = { ...staticMetrics };
  let totalRowProgramExecuteUs = 0;
  for (const metrics of initial) {
    for (const key of [
      'decodeUs',
      'staticMountUs',
      'rowProgramExecuteUs',
      'rowStateRegistrationUs',
      'fragmentAppendUs',
      'programCompileUs',
      'rowCount',
      'createdElements',
      'createdTexts',
      'bindings',
      'domOperations',
    ] as const)
      total[key] += metrics[key];
    totalRowProgramExecuteUs += metrics.rowProgramExecuteUs;
  }
  total.averageRowProgramExecuteUs =
    total.rowCount === 0
      ? 0
      : totalRowProgramExecuteUs / total.rowCount;
  return { ...total, ...browser };
}

function liveCollectionProducer(
  inputId: string,
  collection: LiveCollection<any>,
): DeltaInput<any[]> {
  return {
    getSnapshot: () => collection.toArray,
    subscribeDeltas: (notify) => {
      const subscription = collection.subscribeChanges((changes) =>
        notify(
          coalesceDeltas(
            changes.map((change) => toRuntimeDelta(inputId, change)),
          ),
        ),
      );
      return () => subscription.unsubscribe();
    },
  };
}

function subscribeInput(
  runtime: WasmRuntimeInstance,
  inputId: string,
  producer: CompiledInputProducer<any>,
  input: any,
  onQueryUpdate?: (update: CompiledQueryUpdate) => void,
): Array<() => void> {
  if (isDeltaInput(producer)) {
    return [
      producer.subscribeDeltas((deltas) =>
        publishDeltas(runtime, deltas, onQueryUpdate),
      ),
    ];
  }
  if (!producer.subscribe) return [];
  let previous = snapshotProjection(
    producer.getSnapshot(),
    input?.shape,
  );
  return [
    producer.subscribe(() => {
      const start = performance.now();
      const nextValue = producer.getSnapshot();
      const next = snapshotProjection(nextValue, input?.shape);
      const deltas = reconcileInputSnapshot(
        inputId,
        previous,
        next,
        input?.shape,
      );
      previous = next;
      const update = publishDeltas(
        runtime,
        deltas,
        onQueryUpdate,
        performance.now() - start,
      );
      return update;
    }),
  ];
}

function isDeltaInput(
  input: CompiledInputProducer<any>,
): input is DeltaInput<any> {
  return (
    'subscribeDeltas' in input &&
    typeof input.subscribeDeltas === 'function'
  );
}

function publishDeltas(
  runtime: WasmRuntimeInstance,
  deltas: RuntimeDelta[],
  onQueryUpdate?: (update: CompiledQueryUpdate) => void,
  reconciliationMs = 0,
): CompiledQueryUpdate {
  const adapterStart = performance.now();
  const total: CompiledQueryUpdate = {
    adapterMs: 0,
    reconciliationMs,
    deltaCount: deltas.length,
    domOperations: 0,
    nodesTouched: 0,
    bindingsTouched: 0,
    wasmDomUs: 0,
  };
  if (deltas.length > 0) addMetrics(total, applyBatch(runtime, deltas));
  total.adapterMs = performance.now() - adapterStart;
  onQueryUpdate?.(total);
  return total;
}

function toRuntimeDelta(
  inputId: string,
  change: LiveCollectionChange<Record<string, unknown>>,
): RuntimeDelta {
  const rowKey = String(change.key);
  switch (change.type) {
    case 'insert':
      return {
        type: 'insert',
        inputId,
        rowKey,
        row: change.value,
        beforeRowKey: normalizeRowKey(change.beforeKey),
      };
    case 'update':
      return {
        type: 'update',
        inputId,
        rowKey,
        changes:
          change.changes ?? diff(change.previousValue, change.value),
      };
    case 'delete':
      return { type: 'remove', inputId, rowKey };
    case 'move':
      return {
        type: 'move',
        inputId,
        rowKey,
        beforeRowKey: normalizeRowKey(change.beforeKey),
      };
  }
}

export type CollectionProjection = {
  kind: 'collection';
  keys: string[];
  rows: Map<string, Record<string, unknown>>;
};
export type ValueProjection = { kind: 'value'; values: unknown[] };

/** Store the observed values, not source object references: mutable stores are
 * allowed to publish the same object identity after changing a field. */
function snapshotProjection(
  value: unknown,
  shape: any,
): CollectionProjection | ValueProjection {
  if (shape?.kind !== 'collection' || !Array.isArray(value)) {
    const paths: string[][] = shape?.observedPaths ?? [];
    return {
      kind: 'value',
      values: paths.length
        ? paths.map((path) => readPath(value, path))
        : [value],
    };
  }
  const keyPath = String(shape.keyExpression ?? 'id')
    .split('.')
    .slice(1);
  const paths: string[][] = shape.observedRowPaths ?? [];
  const rows = new Map<string, Record<string, unknown>>();
  const keys: string[] = [];
  for (const row of value) {
    const key = String(readPath(row, keyPath));
    keys.push(key);
    rows.set(
      key,
      paths.length
        ? Object.fromEntries(
            paths.map((path) => [path[0]!, readPath(row, path)]),
          )
        : { ...(row as Record<string, unknown>) },
    );
  }
  return { kind: 'collection', keys, rows };
}

/** Reconcile already-projected snapshots. Exposed for producer adapters and
 * deterministic tests; normal callers should pass a CompiledInputProducer. */
export function reconcileInputSnapshot(
  inputId: string,
  previous: CollectionProjection | ValueProjection,
  next: CollectionProjection | ValueProjection,
  shape: any,
): RuntimeDelta[] {
  if (
    previous.kind !== 'collection' ||
    next.kind !== 'collection' ||
    shape?.kind !== 'collection'
  )
    return [];
  const result: RuntimeDelta[] = [];
  const previousKeys = new Set(previous.keys);
  const nextKeys = new Set(next.keys);
  for (const key of previous.keys)
    if (!nextKeys.has(key))
      result.push({ type: 'remove', inputId, rowKey: key });
  for (let index = 0; index < next.keys.length; index += 1) {
    const key = next.keys[index]!;
    const beforeRowKey = next.keys[index + 1] ?? null;
    const row = next.rows.get(key)!;
    if (!previousKeys.has(key))
      result.push({
        type: 'insert',
        inputId,
        rowKey: key,
        row,
        beforeRowKey,
      });
    else {
      const changes = diff(previous.rows.get(key)!, row);
      if (Object.keys(changes).length)
        result.push({ type: 'update', inputId, rowKey: key, changes });
      if (shape.orderSensitive && previous.keys[index] !== key)
        result.push({
          type: 'move',
          inputId,
          rowKey: key,
          beforeRowKey,
        });
    }
  }
  return coalesceDeltas(result);
}

function readPath(value: unknown, path: string[]): unknown {
  let current: any = value;
  for (const segment of path)
    current = current == null ? undefined : current[segment];
  return current;
}

function diff(
  previousValue: Record<string, unknown>,
  value: Record<string, unknown>,
): Record<string, unknown> {
  const changes: Record<string, unknown> = {};
  for (const key in value) {
    if (!Object.prototype.hasOwnProperty.call(value, key)) continue;
    if (!Object.is(previousValue[key], value[key]))
      changes[key] = value[key];
  }
  for (const key in previousValue) {
    if (
      Object.prototype.hasOwnProperty.call(previousValue, key) &&
      !Object.prototype.hasOwnProperty.call(value, key)
    )
      changes[key] = undefined;
  }
  return changes;
}

function coalesceDeltas(deltas: RuntimeDelta[]): RuntimeDelta[] {
  const result: RuntimeDelta[] = [];
  for (const delta of deltas) {
    const previous = result[result.length - 1];
    if (
      delta.type === 'update' &&
      previous?.type === 'update' &&
      previous.inputId === delta.inputId &&
      previous.rowKey === delta.rowKey
    ) {
      previous.changes = { ...previous.changes, ...delta.changes };
    } else result.push(delta);
  }
  return result;
}

function normalizeRowKey(
  key: string | number | null | undefined,
): string | null | undefined {
  return key == null ? key : String(key);
}

function addMetrics(
  total: CompiledQueryUpdate,
  next: CompiledUpdateMetrics,
) {
  total.domOperations += next.domOperations;
  total.nodesTouched += next.nodesTouched;
  total.bindingsTouched += next.bindingsTouched;
  total.wasmDomUs += next.wasmDomUs;
}

function apply(
  runtime: WasmRuntimeInstance,
  delta: RuntimeDelta,
): CompiledUpdateMetrics {
  return runtime.apply_delta(delta);
}
function applyBatch(
  runtime: WasmRuntimeInstance,
  deltas: RuntimeDelta[],
): CompiledUpdateMetrics {
  return runtime.apply_deltas(deltas);
}
async function loadRuntimeModule(
  url: string,
): Promise<WasmRuntimeModule> {
  // Vite serves public assets directly but intentionally forbids importing them
  // from source modules. The wasm-bindgen glue has no module dependencies and
  // receives its WASM URL explicitly, so import an in-memory copy instead.
  const response = await fetch(url);
  if (!response.ok)
    throw new Error(
      `Failed to load WASM runtime module: ${response.status}`,
    );
  const moduleUrl = URL.createObjectURL(
    new Blob([await response.text()], { type: 'text/javascript' }),
  );
  try {
    return (await import(
      /* @vite-ignore */ moduleUrl
    )) as WasmRuntimeModule;
  } finally {
    URL.revokeObjectURL(moduleUrl);
  }
}
