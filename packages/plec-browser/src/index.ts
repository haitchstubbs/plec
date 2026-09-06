// The Rust runtime validates the complete manifest. Browser glue only needs
// this field to decide which independently-produced graph to fetch.
type PlecRouteManifest = { rootGraphId: string };

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

export interface CompiledUpdateMetrics {
  reconciliationUs?: number;
  domOperations: number;
  nodesTouched: number;
  bindingsTouched: number;
  propWrites: number;
  rowInserts: number;
  rowRemoves: number;
  rowMoves: number;
  domNodesMoved: number;
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
export interface CompiledQueryUpdate extends CompiledUpdateMetrics {
  adapterMs: number;
  reconciliationMs?: number;
  deltaCount?: number;
}
interface WasmRuntimeInstance {
  set_host_inputs(values: Record<string, unknown>): void;
  set_cookie_policy(
    policy: PlecRouterMountOptions['cookiePolicy'] | null,
  ): void;
  set_fetch_policy(policy: PlecFetchPolicyGrant[] | null): void;
  initialize_input(inputId: string, rows: unknown): RuntimeMountMetrics;
  apply_delta(delta: unknown): CompiledUpdateMetrics;
  apply_deltas(deltas: unknown): CompiledUpdateMetrics;
  dispose(): void;
  register_graph(graphId: string, ir: unknown): void;
  start(root: Element, manifest: unknown): void;
  start_adopt_snapshot(
    root: Element,
    manifest: unknown,
    snapshot: unknown,
  ): void;
  abandon_adoption(): void;
  navigate(href: string, replace: boolean): void;
  ssr_text_divergences(): number;
}
interface WasmRuntimeModule {
  default(input?: unknown): Promise<unknown>;
  PlecRuntime: new () => WasmRuntimeInstance;
}
const DEFAULT_RUNTIME_JS_URL = '/runtime/runtime.js';
const DEFAULT_RUNTIME_WASM_URL = '/runtime/runtime_bg.wasm';

export interface PlecRouterMountOptions {
  root: Element;
  manifestUrl?: string;
  /** A Rust-produced `{ manifest, application }` artifact. */
  applicationUrl?: string;
  graphUrl?: (graphId: string) => string;
  runtimeJsUrl?: string;
  runtimeWasmUrl?: string;
  /** Host authority may be a stricter subset than graph-declared authority. */
  cookiePolicy?: Record<
    string,
    {
      operations: Array<'getSync' | 'get' | 'set' | 'delete'>;
      path?: string;
    }
  >;
  /**
   * Host-owned fetch grants. Default-deny: without a grant matching the
   * request origin, method, and headers, the runtime rejects the fetch.
   * Artifact-declared fetch capability requests never widen this surface.
   * `credentials` controls the credentials mode; the artifact cannot.
   */
  fetchPolicy?: PlecFetchPolicyGrant[];
  /** Host-owned reactive inputs for routed compiled graphs. */
  inputs?: Record<string, CompiledInputProducer<any>>;
  onQueryUpdate?: (update: CompiledQueryUpdate) => void;
  /** Development/test visibility for SSR adoption decisions. Production hosts
   * may omit this and transparently take the normal mount path. */
  onAdoptionDiagnostic?: (diagnostic: PlecAdoptionDiagnostic) => void;
}
export interface PlecFetchPolicyGrant {
  /** Exact serialized origin the grant applies to (`https://api.example.com`). */
  origin: string;
  /** Allowed request methods; an empty list grants none. */
  methods?: string[];
  /** Allowed request header names (case-insensitive); an empty list grants none. */
  headers?: string[];
  /** Whether requests under this grant may carry credentials. */
  credentials?: boolean;
}
export interface PlecAdoptionDiagnostic {
  outcome: 'adopted' | 'fallback';
  expectedRevision?: string;
  observedRevision?: string;
  routeId?: string | null;
  mismatchCodes: string[];
  /** True when the adoption consumed a validated SSR execution snapshot. */
  snapshotImported?: boolean;
  /** Server-rendered text values the deterministic recompute replaced.
   * Divergence is allowed and reported only; present for snapshot
   * adoptions. */
  textDivergences?: number;
}
export interface PlecRouterController {
  dispose(): void;
}

/** The narrow runtime surface used to attach host-owned keyed collections to
 * routed graphs. Exported for adapter tests; it is not a new WASM API. */
export interface RoutedInputRuntime {
  initialize_input(inputId: string, rows: unknown): RuntimeMountMetrics;
  apply_deltas(deltas: unknown): CompiledUpdateMetrics;
}

export function wireRoutedInputs(
  runtime: RoutedInputRuntime,
  inputs: Record<string, CompiledInputProducer<any>>,
  onQueryUpdate?: (update: CompiledQueryUpdate) => void,
): { hydrate(): void; dispose(): void } {
  const hydrate = () => {
    for (const [inputId, producer] of Object.entries(inputs)) {
      const snapshot = producer.getSnapshot();
      // Routed graphs currently accept the same keyed collection inputs as
      // standalone mounts. The typed runtime safely ignores inputs that are
      // not declared by the active route instance.
      if (Array.isArray(snapshot))
        runtime.initialize_input(inputId, snapshot);
    }
  };
  const subscriptions = Object.entries(inputs).flatMap(
    ([inputId, producer]) =>
      isDeltaInput(producer)
        ? [
            producer.subscribeDeltas((deltas) =>
              publishDeltas(runtime, deltas, onQueryUpdate),
            ),
          ]
        : [],
  );
  return {
    hydrate,
    dispose: () =>
      subscriptions.forEach((unsubscribe) => unsubscribe()),
  };
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

/** Transport-only router bootstrap. It fetches immutable artifacts and hands
 * them to WASM; route matching, history and outlet ownership stay in Rust. */

/** Byte ceilings for untrusted JSON payloads (must mirror the Rust decode
 * limits in crates/plec-ir/src/limits.rs; see docs/security-limits.md). */
const MAX_ARTIFACT_JSON_BYTES = 16 * 1024 * 1024;
const MAX_SNAPSHOT_JSON_BYTES = 4 * 1024 * 1024;
const MAX_RUNTIME_JS_BYTES = 16 * 1024 * 1024;

/** Fetches and parses a JSON response under a hard byte ceiling: the
 * declared content-length is rejected before reading, and the actual body
 * length is verified before parsing so hostile payloads fail predictably. */
async function boundedResponseJson(
  response: Response,
  maxBytes: number,
  label: string,
): Promise<unknown> {
  const declared = response.headers.get('content-length');
  if (declared !== null && Number(declared) > maxBytes)
    throw new Error(`${label} exceeds byte limit`);
  const text = await response.text();
  if (text.length > maxBytes)
    throw new Error(`${label} exceeds byte limit`);
  return JSON.parse(text);
}

export async function startPlecRouter(
  options: PlecRouterMountOptions,
): Promise<PlecRouterController> {
  markPlecTiming('plec:mount-start');
  const bootstrap = readSsrBootstrap();
  const manifestResponse = await fetch(
    options.applicationUrl ??
      options.manifestUrl ??
      '/route-manifest.json',
  );
  if (!manifestResponse.ok)
    throw new Error(
      `Failed to load route manifest: ${manifestResponse.status}`,
    );
  const artifact = await boundedResponseJson(
    manifestResponse,
    MAX_ARTIFACT_JSON_BYTES,
    'route manifest',
  );
  const compiled = options.applicationUrl
    ? (artifact as { manifest: PlecRouteManifest; application: any })
    : undefined;
  const manifest = (compiled?.manifest ??
    artifact) as PlecRouteManifest;
  const graphUrl = options.graphUrl ?? ((id) => `/graphs/${id}.json`);
  const runtimeModule = await loadRuntimeModule(
    options.runtimeJsUrl ?? DEFAULT_RUNTIME_JS_URL,
  );
  await runtimeModule.default({
    module_or_path: options.runtimeWasmUrl ?? DEFAULT_RUNTIME_WASM_URL,
  });
  const runtime = new runtimeModule.PlecRuntime();
  runtime.set_cookie_policy(options.cookiePolicy ?? null);
  runtime.set_fetch_policy(options.fetchPolicy ?? null);
  const routedInputs = options.inputs ?? {};
  const hostInputs: Record<string, unknown> = {
    'location.pathname': window.location.pathname,
    'location.search': window.location.search,
    'location.hash': window.location.hash,
  };
  runtime.set_host_inputs(hostInputs);
  const loadedGraphs = new Map<string, any>();
  const loadGraph = async (graphId: string, fresh = false) => {
    if (!fresh && loadedGraphs.has(graphId))
      return loadedGraphs.get(graphId);
    const response = await fetch(graphUrl(graphId));
    if (!response.ok)
      throw new Error(
        `Failed to load graph ${graphId}: ${response.status}`,
      );
    const graph = await boundedResponseJson(
      response,
      MAX_ARTIFACT_JSON_BYTES,
      `graph ${graphId}`,
    );
    loadedGraphs.set(graphId, graph);
    runtime.register_graph(graphId, graph);
    return graph;
  };
  if (compiled) {
    runtime.register_graph(
      '__rust_application__',
      compiled.application,
    );
  } else {
    await loadGraph(manifest.rootGraphId);
  }
  let adopted = false;
  if (bootstrap?.kind === 'invalid') {
    // A present-but-unparseable bootstrap is an SSR contract failure, never a
    // silent normal mount.
    emitAdoptionDiagnostic(options, {
      outcome: 'fallback',
      routeId: null,
      mismatchCodes: ['invalid:ssr-bootstrap'],
    });
  } else if (bootstrap) {
    const expectedRevision = (manifest as any).revision as
      string | undefined;
    // The server-published route chain is the adoption cause. The gate only
    // checks chain shape; whether the chain matches the current URL is the
    // WASM runtime's cross-validation decision.
    const chain: unknown = (bootstrap.snapshot as any).routes;
    const chainDetail = validateSsrRouteChain(
      (manifest as any).routes ?? [],
      chain,
    );
    if (expectedRevision !== bootstrap.revision) {
      emitAdoptionDiagnostic(options, {
        outcome: 'fallback',
        expectedRevision,
        observedRevision: bootstrap.revision,
        routeId: bootstrap.routeId,
        mismatchCodes: ['stale-revision'],
      });
    } else if (chainDetail !== null) {
      emitAdoptionDiagnostic(options, {
        outcome: 'fallback',
        expectedRevision,
        observedRevision: bootstrap.revision,
        routeId: bootstrap.routeId,
        mismatchCodes: [`mismatch:ssr-route-chain:${chainDetail}`],
      });
    } else {
      try {
        // Every chain graph must be registered before WASM claims DOM; graphs
        // outside the initial chain stay lazy. A server-rendered error phase
        // claims DOM built from the route's error graph, so that graph must
        // be registered too before the snapshot import validates structure.
        if (!compiled) {
          for (const entry of chain as Array<{
            routeId: string;
            phase?: string;
          }>) {
            const route = (manifest as any).routes?.find(
              (candidate: any) => candidate.id === entry.routeId,
            );
            if (route?.graphId) await loadGraph(route.graphId);
            if (entry.phase === 'error' && route?.errorGraphId)
              await loadGraph(route.errorGraphId);
          }
        }
        const snapshotImported = true;
        runtime.start_adopt_snapshot(
          options.root,
          manifest,
          bootstrap.snapshot,
        );
        adopted = true;
        emitAdoptionDiagnostic(options, {
          outcome: 'adopted',
          expectedRevision,
          observedRevision: bootstrap.revision,
          routeId: bootstrap.routeId,
          mismatchCodes: [],
          snapshotImported,
          ...(snapshotImported
            ? {
                textDivergences: runtime.ssr_text_divergences(),
              }
            : {}),
        });
      } catch (error) {
        runtime.abandon_adoption();
        emitAdoptionDiagnostic(options, {
          outcome: 'fallback',
          expectedRevision,
          observedRevision: bootstrap.revision,
          routeId: bootstrap.routeId,
          mismatchCodes: [adoptionMismatchCode(error)],
        });
      }
    }
  }
  const routedInputBridge = wireRoutedInputs(
    runtime,
    routedInputs,
    options.onQueryUpdate,
  );
  const onGraphNeeded = (event: Event) => {
    const graphId = (event as CustomEvent<{ graphId?: string }>).detail
      ?.graphId;
    if (!graphId || compiled) return;
    void loadGraph(graphId, true)
      .then(() => {
        runtime.navigate(
          `${window.location.pathname}${window.location.search}${window.location.hash}`,
          true,
        );
        routedInputBridge.hydrate();
      })
      .catch((error) =>
        console.error(`Failed to load Plec graph ${graphId}`, error),
      );
  };
  window.addEventListener('plec:graph-needed', onGraphNeeded);
  if (!adopted) runtime.start(options.root, manifest);
  routedInputBridge.hydrate();
  let hydrationQueued = false;
  const scheduleRoutedInputHydration = () => {
    if (hydrationQueued) return;
    hydrationQueued = true;
    queueMicrotask(() => {
      hydrationQueued = false;
      routedInputBridge.hydrate();
    });
  };
  const onRouteClick = (event: Event) => {
    const target = event.target;
    if (!(target instanceof Element)) return;
    const link = target.closest('a[href]');
    if (link && options.root.contains(link))
      scheduleRoutedInputHydration();
  };
  options.root.addEventListener('click', onRouteClick, true);
  window.addEventListener('popstate', scheduleRoutedInputHydration);
  markPlecTiming('plec:mount-end');

  return {
    dispose: () => {
      window.removeEventListener('plec:graph-needed', onGraphNeeded);
      options.root.removeEventListener('click', onRouteClick, true);
      window.removeEventListener(
        'popstate',
        scheduleRoutedInputHydration,
      );
      routedInputBridge.dispose();
      runtime.dispose();
    },
  };
}

/** The `#plec-bootstrap` payload shapes the adoption gate understands. A
 * page without a v2 snapshot (no script, or any non-v2 payload) mounts
 * fresh; only `kind: 'invalid'` — a present-but-unparseable script — is an
 * SSR contract failure. */
type SsrBootstrap =
  | null
  | { kind: 'invalid' }
  /** v2: the bootstrap carries the typed SSR execution snapshot. */
  | {
      kind: 'snapshot';
      revision?: string;
      routeId?: string | null;
      snapshot: unknown;
    };

/** Exported for adapter tests. */
export type { SsrBootstrap };

export function readSsrBootstrap(): SsrBootstrap {
  const element = document.querySelector(
    '#plec-bootstrap[type="application/json"]',
  );
  if (!element?.textContent) return null;
  if (element.textContent.length > MAX_SNAPSHOT_JSON_BYTES)
    return { kind: 'invalid' };
  let parsed: any;
  try {
    parsed = JSON.parse(element.textContent);
  } catch {
    return { kind: 'invalid' };
  }
  if (
    parsed &&
    typeof parsed === 'object' &&
    parsed.version === 2 &&
    parsed.snapshot &&
    typeof parsed.snapshot === 'object'
  ) {
    const snapshot = parsed.snapshot;
    const firstRoute = Array.isArray(snapshot.routes)
      ? snapshot.routes[0]
      : undefined;
    return {
      kind: 'snapshot',
      revision:
        typeof snapshot.revision === 'string'
          ? snapshot.revision
          : undefined,
      routeId:
        firstRoute && typeof firstRoute.routeId === 'string'
          ? firstRoute.routeId
          : null,
      snapshot,
    };
  }
  // Anything that is not the v2 snapshot shape means there is no execution
  // state to resume (including the removed v1 non-snapshot shape), so the
  // page mounts fresh exactly as if no bootstrap were present.
  return null;
}

function adoptionMismatchCode(error: unknown): string {
  const message =
    error instanceof Error ? error.message : String(error);
  return message.replace(/^Error:\s*/, '') || 'adoption-error';
}

/** Structural validation of the server-published route chain. Returns a
 * mismatch detail string, or null when the chain is well-formed and every
 * entry references a manifest route. This is deliberately weaker than the
 * WASM cross-validation: the gate never re-matches routes against the URL. */
export function validateSsrRouteChain(
  routes: unknown,
  chain: unknown,
): string | null {
  if (!Array.isArray(chain) || chain.length === 0) return 'empty';
  const known = Array.isArray(routes)
    ? new Set(
        routes
          .map((entry: any) => entry?.id)
          .filter((id) => typeof id === 'string'),
      )
    : new Set<string>();
  for (const [index, entry] of chain.entries()) {
    const routeId = (entry as any)?.routeId;
    if (typeof routeId !== 'string' || routeId.length === 0)
      return `instance:${index}`;
    if (!known.has(routeId)) return `unknown-route:${routeId}`;
    const params = (entry as any)?.params;
    if (
      params !== undefined &&
      (params === null ||
        typeof params !== 'object' ||
        Array.isArray(params) ||
        Object.values(params).some(
          (value) => typeof value !== 'string',
        ))
    )
      return `params:${index}`;
    const phase = (entry as any)?.phase;
    if (
      phase !== undefined &&
      !['active', 'pending', 'error'].includes(phase)
    )
      return `phase:${index}`;
  }
  return null;
}

function emitAdoptionDiagnostic(
  options: PlecRouterMountOptions,
  diagnostic: PlecAdoptionDiagnostic,
) {
  options.onAdoptionDiagnostic?.(diagnostic);
  if (typeof window !== 'undefined')
    window.dispatchEvent(
      new CustomEvent('plec:adoption', { detail: diagnostic }),
    );
}

/** Adapt a host collection into the runtime's stable keyed-delta protocol. */
export function adaptLiveCollection<Row extends object>(
  inputId: string,
  collection: LiveCollection<Row>,
): DeltaInput<Row[]> {
  return {
    getSnapshot: () => collection.toArray,
    subscribeDeltas: (notify) => {
      const subscription = collection.subscribeChanges((changes) =>
        notify(
          changes.map((change) => toRuntimeDelta(inputId, change)),
        ),
      );
      return () => subscription.unsubscribe();
    },
  };
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
  runtime: Pick<WasmRuntimeInstance, 'apply_deltas'>,
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
    propWrites: 0,
    rowInserts: 0,
    rowRemoves: 0,
    rowMoves: 0,
    domNodesMoved: 0,
    wasmDomUs: 0,
  };
  if (deltas.length > 0) addMetrics(total, applyBatch(runtime, deltas));
  total.adapterMs = performance.now() - adapterStart;
  onQueryUpdate?.(total);
  return total;
}

function toRuntimeDelta<Row extends object>(
  inputId: string,
  change: LiveCollectionChange<Row>,
): RuntimeDelta {
  const rowKey = String(change.key);
  switch (change.type) {
    case 'insert':
      return {
        type: 'insert',
        inputId,
        rowKey,
        row: change.value as Record<string, unknown>,
        beforeRowKey: normalizeRowKey(change.beforeKey),
      };
    case 'update':
      return {
        type: 'update',
        inputId,
        rowKey,
        changes: change.changes
          ? { ...change.changes }
          : liveChangeFields(
              change.previousValue as Record<string, unknown>,
              change.value as Record<string, unknown>,
            ),
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

/** Rich collection sources may omit a changed-field set. This is source
 * adaptation, not snapshot reconciliation; the latter lives in WASM. */
function liveChangeFields(
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
  total.propWrites += next.propWrites;
  total.rowInserts += next.rowInserts;
  total.rowRemoves += next.rowRemoves;
  total.rowMoves += next.rowMoves;
  total.domNodesMoved += next.domNodesMoved;
  total.wasmDomUs += next.wasmDomUs;
}

function applyBatch(
  runtime: Pick<WasmRuntimeInstance, 'apply_deltas'>,
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
  const text = await response.text();
  if (text.length > MAX_RUNTIME_JS_BYTES)
    throw new Error('WASM runtime module exceeds byte limit');
  const moduleUrl = URL.createObjectURL(
    new Blob([text], { type: 'text/javascript' }),
  );
  try {
    return (await import(
      /* @vite-ignore */ moduleUrl
    )) as WasmRuntimeModule;
  } finally {
    URL.revokeObjectURL(moduleUrl);
  }
}
