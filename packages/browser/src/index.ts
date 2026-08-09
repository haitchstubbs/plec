export type RuntimeDelta =
  | { type: "update"; inputId: string; rowKey: string; changes: Record<string, unknown> }
  | { type: "insert"; inputId: string; rowKey: string; row: Record<string, unknown>; beforeRowKey?: string | null }
  | { type: "remove"; inputId: string; rowKey: string }
  | { type: "move"; inputId: string; rowKey: string; beforeRowKey?: string | null };

export type LiveCollectionChange<Row> =
  | { type: "insert"; key: string | number; value: Row; beforeKey?: string | number | null }
  | { type: "update"; key: string | number; value: Row; previousValue: Row; changes?: Partial<Row> }
  | { type: "delete"; key: string | number; previousValue: Row }
  | { type: "move"; key: string | number; beforeKey?: string | number | null };

export interface LiveCollection<Row extends object = Record<string, unknown>> {
  readonly toArray: Row[];
  /** Optional lookup support for callers; the runtime adapter never uses it. */
  get?(key: string | number): Row | undefined;
  subscribeChanges(callback: (changes: LiveCollectionChange<Row>[]) => void): { unsubscribe(): void };
}

export interface SnapshotInput<T> {
  getSnapshot(): T;
  subscribe?(notify: () => void): () => void;
}

export interface DeltaInput<T> extends SnapshotInput<T> {
  subscribeDeltas(notify: (deltas: RuntimeDelta[]) => void): () => void;
}

export type CompiledInputProducer<T = unknown> = SnapshotInput<T> | DeltaInput<T>;

/** A small host-owned channel for generated React controllers and vanilla
 * publishers. It is a snapshot producer; richer sources can use DeltaInput. */
export interface CompiledInputChannel<T> extends SnapshotInput<T> {
  publish(value: T): void;
}

export function createCompiledInputChannel<T>(initialValue: T): CompiledInputChannel<T> {
  let value = initialValue;
  const listeners = new Set<() => void>();
  return {
    getSnapshot: () => value,
    subscribe: (notify) => { listeners.add(notify); return () => listeners.delete(notify); },
    publish: (next) => { value = next; listeners.forEach((notify) => notify()); }
  };
}

export interface CompiledMountOptions {
  root: Element;
  /** Package-neutral producer boundary. Prefer this for new integrations. */
  inputs?: Record<string, CompiledInputProducer<any>>;
  /** @deprecated Use inputs. Retained while existing applications migrate. */
  queries?: Record<string, LiveCollection<any>>;
  irUrl?: string;
  runtimeJsUrl?: string;
  runtimeWasmUrl?: string;
  onQueryUpdate?: (update: CompiledQueryUpdate) => void;
  /** Semantic event handlers. Keys may be IR action IDs or source callback metadata. */
  actions?: Record<string, (id: string, changes: Record<string, unknown>) => void>;
  onMount?: (metrics: MountMetrics) => void;
  hostValues?: { currentYear?: number | string; location?: { pathname: string } };
  islands?: Record<string, (placeholder: Element, props: Record<string, unknown>) => void | (() => void)>;
  onNavigate?: (navigation: { href: string; replace?: boolean }) => void;
}

export interface CompiledUpdateMetrics { domOperations: number; nodesTouched: number; bindingsTouched: number; wasmDomUs: number }
export interface RuntimeMountMetrics { decodeUs: number; staticMountUs: number; rowProgramExecuteUs: number; rowStateRegistrationUs: number; fragmentAppendUs: number; programCompileUs: number; programRevision?: string; instructionCount: number; compiledExpressionCount: number; fieldSlotCount: number; bindingProgramCount: number; averageRowProgramExecuteUs: number; rowCount: number; createdElements: number; createdTexts: number; bindings: number; domOperations: number }
export interface MountMetrics extends RuntimeMountMetrics { irFetchMs: number; irParseMs: number; wasmInitMs: number; runtimeLoadMs: number; renderMode: "adopt" | "mount" }
export interface CompiledQueryUpdate extends CompiledUpdateMetrics { adapterMs: number; reconciliationMs?: number; deltaCount?: number }
export interface CompiledControllerDiagnostics { activeQuerySubscriptions: number; activeInputSubscriptions: number; activeActionListeners: number; activeIslands: number; queryIds: string[]; inputIds: string[]; disposed: boolean }
export interface CompiledRuntimeController { readonly mountMetrics: MountMetrics; diagnostics(): CompiledControllerDiagnostics; dispose(): void; applyDelta(delta: RuntimeDelta): CompiledUpdateMetrics; applyHostValues(values: NonNullable<CompiledMountOptions['hostValues']>): void }

interface WasmRuntimeInstance {
  load_application(ir: unknown): void;
  adopt(root: Element): RuntimeMountMetrics | null;
  mount(root: Element): RuntimeMountMetrics;
  initialize_input(inputId: string, rows: unknown): RuntimeMountMetrics;
  apply_delta(delta: unknown): CompiledUpdateMetrics;
  apply_deltas(deltas: unknown): CompiledUpdateMetrics;
  dispose(): void;
}
interface WasmRuntimeModule { default(input?: unknown): Promise<unknown>; Runtime: new () => WasmRuntimeInstance }
const DEFAULT_IR_URL = "/application.ir.json";
const DEFAULT_RUNTIME_JS_URL = "/runtime/runtime.js";
const DEFAULT_RUNTIME_WASM_URL = "/runtime/runtime_bg.wasm";

export async function mountCompiledApplication(options: CompiledMountOptions): Promise<CompiledRuntimeController> {
  const fetchStart = performance.now();
  const [runtimeModule, response] = await Promise.all([loadRuntimeModule(options.runtimeJsUrl ?? DEFAULT_RUNTIME_JS_URL), fetch(options.irUrl ?? DEFAULT_IR_URL)]);
  const irFetchMs = performance.now() - fetchStart;
  if (!response.ok) throw new Error(`Failed to load IR: ${response.status}`);
  const parseStart = performance.now();
  const ir = resolveHostValues(await response.json(), options.hostValues ?? { currentYear: new Date().getFullYear() });
  const irParseMs = performance.now() - parseStart;
  const wasmStart = performance.now();
  await runtimeModule.default({ module_or_path: options.runtimeWasmUrl ?? DEFAULT_RUNTIME_WASM_URL });
  const wasmInitMs = performance.now() - wasmStart;
  const runtime = new runtimeModule.Runtime();
  const loadStart = performance.now();
  runtime.load_application(ir);
  const adoptedMetrics = runtime.adopt(options.root);
  const staticMetrics = adoptedMetrics ?? runtime.mount(options.root);
  const renderMode = adoptedMetrics ? "adopt" as const : "mount" as const;
  const runtimeLoadMs = performance.now() - loadStart;
  const disposeIslands = mountIslands(options.root, ir, options.islands ?? {});
  const applyHostValues = (values: NonNullable<CompiledMountOptions['hostValues']>) => applyStaticHostBindings(options.root, ir, values);
  applyHostValues(options.hostValues ?? { location: { pathname: window.location.pathname } });
  const removeActions = bindActions(options.root, ir, options.actions ?? {}, options.onNavigate);
  const inputSchemas = new Map<string, any>((ir.inputs ?? []).map((input: any): [string, any] => [input.id, input]));
  const inputs: Record<string, CompiledInputProducer<any>> = {
    ...Object.fromEntries(Object.entries(options.queries ?? {}).map(([id, collection]) => {
      const inputId = (ir.loops ?? []).find((loop: any) => loop.queryId === id)?.inputId ?? ((ir.inputs ?? []).length === 1 ? ir.inputs[0].id : id);
      return [inputId, liveCollectionProducer(inputId, collection)];
    })),
    ...(options.inputs ?? {})
  };
  const initialMetrics = Object.entries(inputs).flatMap(([inputId, producer]) => {
    const shape = inputSchemas.get(inputId)?.shape;
    return shape?.kind === "collection" ? [runtime.initialize_input(inputId, producer.getSnapshot())] : [];
  });
  const mountMetrics = mergeMountMetrics(staticMetrics, initialMetrics, { irFetchMs, irParseMs, wasmInitMs, runtimeLoadMs, renderMode });
  options.onMount?.(mountMetrics);
  const subscriptions = Object.entries(inputs).flatMap(([inputId, producer]) => subscribeInput(runtime, inputId, producer, inputSchemas.get(inputId), options.onQueryUpdate));
  let disposed = false;
  const diagnostics = (): CompiledControllerDiagnostics => ({ activeQuerySubscriptions: 0, activeInputSubscriptions: disposed ? 0 : subscriptions.length, activeActionListeners: disposed ? 0 : 1, activeIslands: disposed ? 0 : disposeIslands.length, queryIds: Object.keys(options.queries ?? {}), inputIds: Object.keys(inputs), disposed });
  return { mountMetrics, diagnostics, applyDelta: (delta) => apply(runtime, delta), applyHostValues, dispose: () => { if (disposed) return; disposed = true; runtime.dispose(); disposeIslands.forEach((dispose) => dispose()); removeActions(); subscriptions.forEach((dispose) => dispose()); } };
}

function applyStaticHostBindings(root: Element, ir: any, values: NonNullable<CompiledMountOptions['hostValues']>) {
  const expressions = new Map((ir.expressions ?? []).map((entry: any) => [entry.id, entry.expression]));
  for (const binding of ir.bindings ?? []) {
    const expression = expressions.get(binding.expressionId);
    if (!dependsOnHost(expression)) continue;
    const node = root.querySelector(`[data-runtime-node="${binding.targetId}"]`);
    if (!(node instanceof Element) || binding.kind !== 'attribute') continue;
    const value = evaluateHostExpression(expression, { host: values });
    node.setAttribute(binding.attributeName === 'className' ? 'class' : binding.attributeName, String(value ?? ''));
  }
}
function dependsOnHost(expression: any): boolean { return expression?.kind === 'identifier' ? expression.name === 'host' : expression?.kind === 'member' ? dependsOnHost(expression.object) : expression?.kind === 'binary' ? dependsOnHost(expression.left) || dependsOnHost(expression.right) : expression?.kind === 'conditional' ? dependsOnHost(expression.test) || dependsOnHost(expression.consequent) || dependsOnHost(expression.alternate) : false }
function evaluateHostExpression(expression: any, scope: any): any { if (expression?.kind === 'literal') return expression.value; if (expression?.kind === 'identifier') return scope[expression.name]; if (expression?.kind === 'member') return evaluateHostExpression(expression.object, scope)?.[expression.property]; if (expression?.kind === 'binary') return expression.op === '===' ? evaluateHostExpression(expression.left, scope) === evaluateHostExpression(expression.right, scope) : null; if (expression?.kind === 'conditional') return evaluateHostExpression(expression.test, scope) ? evaluateHostExpression(expression.consequent, scope) : evaluateHostExpression(expression.alternate, scope); return null }

function resolveHostValues(ir: any, values: { currentYear?: number | string }) {
  const expressions = new Map<string, any>((ir.expressions ?? []).map((expression: any): [string, any] => [expression.id, expression.expression]));
  const textById = new Map<string, any>((ir.texts ?? []).map((text: any): [string, any] => [text.id, text]));
  const elementById = new Map<string, any>((ir.elements ?? []).map((element: any): [string, any] => [element.id, element]));
  ir.bindings = (ir.bindings ?? []).filter((binding: any) => {
    const expression = expressions.get(binding.expressionId);
    if (expression?.kind !== 'host' || expression.name !== 'currentYear') return true;
    const value = String(values.currentYear ?? new Date().getFullYear());
    if (binding.kind === 'text') textById.get(binding.targetId)!.staticValue = value;
    else {
      const element = elementById.get(binding.targetId)!;
      const attribute = element.attributes.find((candidate: any) => candidate.name === binding.attributeName);
      if (attribute) { attribute.staticValue = value; delete attribute.bindingId; }
      else element.attributes.push({ name: binding.attributeName, staticValue: value });
    }
    return false;
  });
  // Host values are materialized as static text/attributes before WASM sees
  // the IR. Remove their expression records too: the row-expression runtime
  // intentionally has no host-environment evaluator.
  const referencedExpressionIds = new Set((ir.bindings ?? []).map((binding: any) => binding.expressionId).filter(Boolean));
  ir.expressions = (ir.expressions ?? []).filter((expression: any) => expression.expression?.kind !== 'host' || referencedExpressionIds.has(expression.id));
  return ir;
}

function mountIslands(root: Element, ir: any, registry: NonNullable<CompiledMountOptions['islands']>) {
  const disposers: Array<() => void> = [];
  for (const island of ir.islands ?? []) {
    const mount = registry[island.componentId];
    const placeholder = root.querySelector<HTMLElement>(`[data-runtime-node="${island.placeholderNodeId}"]`);
    if (!mount || !placeholder) continue;
    const dispose = mount(placeholder, island.props ?? {});
    if (typeof dispose === 'function') disposers.push(dispose);
  }
  return disposers;
}

function mergeMountMetrics(staticMetrics: RuntimeMountMetrics, initial: RuntimeMountMetrics[], browser: Pick<MountMetrics, 'irFetchMs' | 'irParseMs' | 'wasmInitMs' | 'runtimeLoadMs' | 'renderMode'>): MountMetrics {
  const total = { ...staticMetrics };
  let totalRowProgramExecuteUs = 0;
  for (const metrics of initial) {
    for (const key of ['decodeUs', 'staticMountUs', 'rowProgramExecuteUs', 'rowStateRegistrationUs', 'fragmentAppendUs', 'programCompileUs', 'rowCount', 'createdElements', 'createdTexts', 'bindings', 'domOperations'] as const) total[key] += metrics[key];
    totalRowProgramExecuteUs += metrics.rowProgramExecuteUs;
  }
  total.averageRowProgramExecuteUs = total.rowCount === 0 ? 0 : totalRowProgramExecuteUs / total.rowCount;
  return { ...total, ...browser };
}

function bindActions(root: Element, ir: any, actions: Record<string, (id: string, changes: Record<string, unknown>) => void>, onNavigate?: CompiledMountOptions['onNavigate']) {
  const wasmOwnedEventIds = new Set((ir.stateTransitions ?? []).map((transition: any) => transition.eventId));
  const actionMetadata = new Map<string, any>((ir.events ?? []).filter((event: any) => !wasmOwnedEventIds.has(event.id)).map((event: any) => [event.actionId, event]));
  if (actionMetadata.size === 0) return () => {};
  const handler = (event: Event) => {
    const target = event.target instanceof Element ? event.target.closest<HTMLElement>("[data-runtime-action]") : null;
    if (!target || !root.contains(target)) return;
    const actionId = target.dataset.runtimeAction!;
    const metadata = actionMetadata.get(actionId);
    // The delegated bridge observes both supported event types. Dispatch only
    // the one the compiler declared for this target; otherwise a checkbox
    // click triggers its action once for `click` and again for `change`.
    if (metadata?.type !== event.type) return;
    if (metadata?.navigate) {
      const mouse = event as MouseEvent;
      if (mouse.button !== 0 || mouse.metaKey || mouse.ctrlKey || mouse.shiftKey || mouse.altKey) return;
      event.preventDefault();
      onNavigate?.(metadata.navigate);
      return;
    }
    const callback = actions[actionId] ?? actions[metadata?.callbackName];
    const row = target.closest<HTMLElement>("[data-runtime-row-key]")?.dataset.runtimeRowKey;
    const field = target.dataset.runtimeField;
    if (!callback || !row || !field) return;
    const input = target as HTMLInputElement;
    callback(row, { [field]: input.type === "checkbox" ? input.checked : input.value });
  };
  root.addEventListener("change", handler);
  root.addEventListener("click", handler);
  return () => { root.removeEventListener("change", handler); root.removeEventListener("click", handler); };
}

function liveCollectionProducer(inputId: string, collection: LiveCollection<any>): DeltaInput<any[]> {
  return {
    getSnapshot: () => collection.toArray,
    subscribeDeltas: (notify) => {
      const subscription = collection.subscribeChanges((changes) => notify(coalesceDeltas(changes.map((change) => toRuntimeDelta(inputId, change)))));
      return () => subscription.unsubscribe();
    }
  };
}

function subscribeInput(runtime: WasmRuntimeInstance, inputId: string, producer: CompiledInputProducer<any>, input: any, onQueryUpdate?: (update: CompiledQueryUpdate) => void): Array<() => void> {
  if (isDeltaInput(producer)) {
    return [producer.subscribeDeltas((deltas) => publishDeltas(runtime, deltas, onQueryUpdate))];
  }
  if (!producer.subscribe) return [];
  let previous = snapshotProjection(producer.getSnapshot(), input?.shape);
  return [producer.subscribe(() => {
    const start = performance.now();
    const nextValue = producer.getSnapshot();
    const next = snapshotProjection(nextValue, input?.shape);
    const deltas = reconcileInputSnapshot(inputId, previous, next, input?.shape);
    previous = next;
    const update = publishDeltas(runtime, deltas, onQueryUpdate, performance.now() - start);
    return update;
  })];
}

function isDeltaInput(input: CompiledInputProducer<any>): input is DeltaInput<any> {
  return "subscribeDeltas" in input && typeof input.subscribeDeltas === "function";
}

function publishDeltas(runtime: WasmRuntimeInstance, deltas: RuntimeDelta[], onQueryUpdate?: (update: CompiledQueryUpdate) => void, reconciliationMs = 0): CompiledQueryUpdate {
    const adapterStart = performance.now();
    const total: CompiledQueryUpdate = { adapterMs: 0, reconciliationMs, deltaCount: deltas.length, domOperations: 0, nodesTouched: 0, bindingsTouched: 0, wasmDomUs: 0 };
    if (deltas.length > 0) addMetrics(total, applyBatch(runtime, deltas));
    total.adapterMs = performance.now() - adapterStart;
    onQueryUpdate?.(total);
    return total;
}

function toRuntimeDelta(inputId: string, change: LiveCollectionChange<Record<string, unknown>>): RuntimeDelta {
  const rowKey = String(change.key);
  switch (change.type) {
    case "insert":
      return { type: "insert", inputId, rowKey, row: change.value, beforeRowKey: normalizeRowKey(change.beforeKey) };
    case "update":
      return { type: "update", inputId, rowKey, changes: change.changes ?? diff(change.previousValue, change.value) };
    case "delete":
      return { type: "remove", inputId, rowKey };
    case "move":
      return { type: "move", inputId, rowKey, beforeRowKey: normalizeRowKey(change.beforeKey) };
  }
}

export type CollectionProjection = { kind: "collection"; keys: string[]; rows: Map<string, Record<string, unknown>> };
export type ValueProjection = { kind: "value"; values: unknown[] };

/** Store the observed values, not source object references: mutable stores are
 * allowed to publish the same object identity after changing a field. */
function snapshotProjection(value: unknown, shape: any): CollectionProjection | ValueProjection {
  if (shape?.kind !== "collection" || !Array.isArray(value)) {
    const paths: string[][] = shape?.observedPaths ?? [];
    return { kind: "value", values: paths.length ? paths.map((path) => readPath(value, path)) : [value] };
  }
  const keyPath = String(shape.keyExpression ?? "id").split(".").slice(1);
  const paths: string[][] = shape.observedRowPaths ?? [];
  const rows = new Map<string, Record<string, unknown>>();
  const keys: string[] = [];
  for (const row of value) {
    const key = String(readPath(row, keyPath));
    keys.push(key);
    rows.set(key, Object.fromEntries(paths.map((path) => [path[0]!, readPath(row, path)])));
  }
  return { kind: "collection", keys, rows };
}

/** Reconcile already-projected snapshots. Exposed for producer adapters and
 * deterministic tests; normal callers should pass a CompiledInputProducer. */
export function reconcileInputSnapshot(inputId: string, previous: CollectionProjection | ValueProjection, next: CollectionProjection | ValueProjection, shape: any): RuntimeDelta[] {
  if (previous.kind !== "collection" || next.kind !== "collection" || shape?.kind !== "collection") return [];
  const result: RuntimeDelta[] = [];
  const previousKeys = new Set(previous.keys);
  const nextKeys = new Set(next.keys);
  for (const key of previous.keys) if (!nextKeys.has(key)) result.push({ type: "remove", inputId, rowKey: key });
  for (let index = 0; index < next.keys.length; index += 1) {
    const key = next.keys[index]!;
    const beforeRowKey = next.keys[index + 1] ?? null;
    const row = next.rows.get(key)!;
    if (!previousKeys.has(key)) result.push({ type: "insert", inputId, rowKey: key, row, beforeRowKey });
    else {
      const changes = diff(previous.rows.get(key)!, row);
      if (Object.keys(changes).length) result.push({ type: "update", inputId, rowKey: key, changes });
      if (shape.orderSensitive && previous.keys[index] !== key) result.push({ type: "move", inputId, rowKey: key, beforeRowKey });
    }
  }
  return coalesceDeltas(result);
}

function readPath(value: unknown, path: string[]): unknown {
  let current: any = value;
  for (const segment of path) current = current == null ? undefined : current[segment];
  return current;
}

function diff(previousValue: Record<string, unknown>, value: Record<string, unknown>): Record<string, unknown> {
  const changes: Record<string, unknown> = {};
  for (const key in value) {
    if (!Object.prototype.hasOwnProperty.call(value, key)) continue;
    if (!Object.is(previousValue[key], value[key])) changes[key] = value[key];
  }
  for (const key in previousValue) {
    if (Object.prototype.hasOwnProperty.call(previousValue, key) && !Object.prototype.hasOwnProperty.call(value, key)) changes[key] = undefined;
  }
  return changes;
}

function coalesceDeltas(deltas: RuntimeDelta[]): RuntimeDelta[] {
  const result: RuntimeDelta[] = [];
  for (const delta of deltas) {
    const previous = result[result.length - 1];
    if (delta.type === "update" && previous?.type === "update" && previous.inputId === delta.inputId && previous.rowKey === delta.rowKey) {
      previous.changes = { ...previous.changes, ...delta.changes };
    } else result.push(delta);
  }
  return result;
}

function normalizeRowKey(key: string | number | null | undefined): string | null | undefined {
  return key == null ? key : String(key);
}

function addMetrics(total: CompiledQueryUpdate, next: CompiledUpdateMetrics) {
  total.domOperations += next.domOperations;
  total.nodesTouched += next.nodesTouched;
  total.bindingsTouched += next.bindingsTouched;
  total.wasmDomUs += next.wasmDomUs;
}

function apply(runtime: WasmRuntimeInstance, delta: RuntimeDelta): CompiledUpdateMetrics {
  return runtime.apply_delta(delta);
}
function applyBatch(runtime: WasmRuntimeInstance, deltas: RuntimeDelta[]): CompiledUpdateMetrics {
  return runtime.apply_deltas(deltas);
}
async function loadRuntimeModule(url: string): Promise<WasmRuntimeModule> {
  // Vite serves public assets directly but intentionally forbids importing them
  // from source modules. The wasm-bindgen glue has no module dependencies and
  // receives its WASM URL explicitly, so import an in-memory copy instead.
  const response = await fetch(url);
  if (!response.ok) throw new Error(`Failed to load WASM runtime module: ${response.status}`);
  const moduleUrl = URL.createObjectURL(new Blob([await response.text()], { type: "text/javascript" }));
  try {
    return await import(/* @vite-ignore */ moduleUrl) as WasmRuntimeModule;
  } finally {
    URL.revokeObjectURL(moduleUrl);
  }
}
