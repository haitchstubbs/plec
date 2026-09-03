// The Rust runtime validates the complete manifest. Browser glue only needs
// this field to decide which independently-produced graph to fetch.
type PlecRouteManifest = { rootGraphId: string };

// SVG icon function marker and type
export interface SvgIconFunction {
  (props: Record<string, unknown>): Element;
  __plecSvgIcon: true;
}

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
  reconciliationUs?: number;
  domOperations: number;
  nodesTouched: number;
  bindingsTouched: number;
  propWrites: number;
  rowInserts: number;
  rowRemoves: number;
  rowMoves: number;
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
  set_cookie_policy(
    policy: PlecRouterMountOptions['cookiePolicy'] | null,
  ): void;
  load_application(ir: unknown): void;
  adopt(root: Element): RuntimeMountMetrics | null;
  mount(root: Element): RuntimeMountMetrics;
  initialize_input(inputId: string, rows: unknown): RuntimeMountMetrics;
  initialize_snapshot_input(
    inputId: string,
    snapshot: unknown,
    shape: unknown,
  ): RuntimeMountMetrics;
  apply_input_snapshot(
    inputId: string,
    snapshot: unknown,
  ): CompiledUpdateMetrics;
  apply_delta(delta: unknown): CompiledUpdateMetrics;
  apply_deltas(deltas: unknown): CompiledUpdateMetrics;
  dispose(): void;
  register_graph(graphId: string, ir: unknown): void;
  start(root: Element, manifest: unknown): void;
  start_adopt(root: Element, manifest: unknown): void;
  start_adopt_snapshot(
    root: Element,
    manifest: unknown,
    snapshot: unknown,
  ): void;
  abandon_adoption(): void;
  navigate(href: string, replace: boolean): void;
  ssr_text_divergences(): number;
  ssr_conditional_inferences(): number;
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
  islands?: Record<
    string,
    (
      placeholder: Element,
      props: Record<string, unknown>,
    ) => void | (() => void)
  >;
  /** Optional host-owned reactive inputs for routed compiled graphs. This is
   * the same producer boundary accepted by `mountPlecApplication`. */
  inputs?: Record<string, CompiledInputProducer<any>>;
  onQueryUpdate?: (update: CompiledQueryUpdate) => void;
  /** Development/test visibility for SSR adoption decisions. Production hosts
   * may omit this and transparently take the normal mount path. */
  onAdoptionDiagnostic?: (diagnostic: PlecAdoptionDiagnostic) => void;
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
  /** Nested component conditionals whose branch had to be inferred from DOM
   * shape because the snapshot carried no record. Zero proves the v2
   * snapshot's nested records were complete; present for snapshot
   * adoptions. */
  conditionalInferences?: number;
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
    //const ir = validateExecutableApplication(await response.json());
    const ir = await response.json();
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
            return [inputId, adaptLiveCollection(inputId, collection)];
          },
        ),
      ),
      ...(options.inputs ?? {}),
    };
    const initialMetrics = Object.entries(inputs).flatMap(
      ([inputId, producer]) => {
        const shape = inputSchemas.get(inputId);
        if (shape?.kind !== 'collection') return [];
        return [
          isDeltaInput(producer)
            ? runtime.initialize_input(inputId, producer.getSnapshot())
            : runtime.initialize_snapshot_input(
                inputId,
                producer.getSnapshot(),
                shape,
              ),
        ];
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
      // Structural graph lookup through the canonical address protocol
      // (docs/dom-address-protocol.md): the root graph instance always
      // renders at structural path `root`, so a declared outlet resolves to
      // the deterministic address `root/node:{node}` — never a first-match
      // scan. Queries stay scoped to the owning root.
      const application = ir as any;
      const component =
        application?.components?.[application.rootComponent ?? 0] ??
        (application?.rootNode !== undefined ? application : undefined);
      const descriptor = (component?.routeOutlets ?? []).find(
        (entry: any) => entry.id === id,
      );
      if (descriptor && typeof descriptor.node === 'number')
        return options.root.querySelector(
          `[data-plec-node="root/node:${descriptor.node}"]`,
        );
      // Legacy v1 layout descriptors keep the legacy string-id scheme
      // (root-scoped, graph-unique ids); isolated by wasm-runtime-ixk.7.
      const legacy = ((ir as any).layout?.routeOutlets ?? []).find(
        (entry: any) => entry.id === id,
      );
      return legacy
        ? options.root.querySelector(
            `[data-runtime-node="${legacy.elementId}"]`,
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
  const artifact = await manifestResponse.json();
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
    const graph = await response.json();
    loadedGraphs.set(graphId, graph);
    runtime.register_graph(graphId, graph);
    return graph;
  };
  const graphs = compiled
    ? [compiled.application]
    : [await loadGraph(manifest.rootGraphId)];
  if (compiled) {
    runtime.register_graph(
      '__rust_application__',
      compiled.application,
    );
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
    // The server-published route chain is the adoption cause. A legacy v1
    // bootstrap degrades to the single route id it carries. The gate only
    // checks chain shape; whether the chain matches the current URL is the
    // WASM runtime's cross-validation decision.
    const chain: unknown =
      bootstrap.kind === 'snapshot'
        ? (bootstrap.snapshot as any).routes
        : bootstrap.routeId
          ? [{ routeId: bootstrap.routeId }]
          : [];
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
        const snapshotImported = bootstrap.kind === 'snapshot';
        if (snapshotImported) {
          runtime.start_adopt_snapshot(
            options.root,
            manifest,
            bootstrap.snapshot,
          );
        } else {
          runtime.start_adopt(options.root, manifest);
        }
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
                conditionalInferences:
                  runtime.ssr_conditional_inferences(),
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

  // Mount islands for the initial root graph
  const rootGraph =
    !compiled &&
    graphs.find((g: any) => g.graphId === manifest.rootGraphId);
  const disposeIslands = rootGraph
    ? mountIslands(options.root, rootGraph, options.islands ?? {})
    : [];

  return {
    dispose: () => {
      window.removeEventListener('plec:graph-needed', onGraphNeeded);
      options.root.removeEventListener('click', onRouteClick, true);
      window.removeEventListener(
        'popstate',
        scheduleRoutedInputHydration,
      );
      routedInputBridge.dispose();
      disposeIslands.forEach((dispose) => dispose());
      runtime.dispose();
    },
  };
}

/** The `#plec-bootstrap` payload shapes the adoption gate understands. */
type SsrBootstrap =
  | null
  | { kind: 'invalid' }
  /** v2: the bootstrap carries the typed SSR execution snapshot. */
  | {
      kind: 'snapshot';
      revision?: string;
      routeId?: string | null;
      snapshot: unknown;
    }
  /** Legacy v1: adoption may proceed without imported execution state. */
  | { kind: 'legacy'; revision?: string; routeId?: string | null };

/** Exported for adapter tests. */
export type { SsrBootstrap };

export function readSsrBootstrap(): SsrBootstrap {
  const element = document.querySelector(
    '#plec-bootstrap[type="application/json"]',
  );
  if (!element?.textContent) return null;
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
  return {
    kind: 'legacy',
    revision:
      typeof parsed?.revision === 'string'
        ? parsed.revision
        : undefined,
    routeId: parsed?.routeId ?? null,
  };
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

// SVG instance cache for static icons
const svgInstanceCache = new Map<string, Element>();

function mountIslands(
  root: Element,
  ir: any,
  registry: NonNullable<CompiledMountOptions['islands']>,
) {
  // LEGACY bridge (wasm-runtime-ixk.7): island placeholders belong to the
  // legacy string-id renderer (`data-runtime-node` + `ir.islands`, which no
  // current compiler emits). Not part of the structural address protocol;
  // queries are deliberately scoped to the owning root, never document-wide.
  const disposers: Array<() => void> = [];

  // SVG islands (cached)
  for (const island of (ir.islands ?? []).filter(
    (i: any) =>
      (registry[i.componentId] as any)?.__plecSvgIcon === true,
  )) {
    const Icon = registry[
      island.componentId
    ] as unknown as SvgIconFunction;
    const placeholder = root.querySelector<HTMLElement>(
      `[data-runtime-node="${island.placeholderNodeId}"]`,
    );
    if (!Icon || !placeholder) continue;

    // Build cache key from icon ID and static props
    const staticProps = ['className', 'size', 'color', 'strokeWidth']
      .filter((p) => island.props?.[p] !== undefined)
      .map((p) => `${p}:${String(island.props?.[p])}`)
      .join(',');
    const cacheKey = `${island.componentId}:${staticProps}`;

    let svgElement = svgInstanceCache.get(cacheKey);
    if (!svgElement) {
      svgElement = Icon(island.props ?? {});
      if (svgElement) svgInstanceCache.set(cacheKey, svgElement);
    }

    if (svgElement) {
      // Clone cached instance (elements can't be in multiple places)
      const clone = svgElement.cloneNode(true) as Element;
      placeholder.replaceWith(clone);
    }
  }

  // Component islands (existing logic)
  for (const island of (ir.islands ?? []).filter(
    (i: any) =>
      (registry[i.componentId] as any)?.__plecSvgIcon !== true,
  )) {
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
  if (!input?.shape) return [];
  return [
    producer.subscribe(() => {
      const start = performance.now();
      const metrics = runtime.apply_input_snapshot(
        inputId,
        producer.getSnapshot(),
      );
      const update: CompiledQueryUpdate = {
        ...metrics,
        adapterMs: performance.now() - start,
        reconciliationMs: (metrics.reconciliationUs ?? 0) / 1000,
      };
      onQueryUpdate?.(update);
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
  total.wasmDomUs += next.wasmDomUs;
}

function apply(
  runtime: WasmRuntimeInstance,
  delta: RuntimeDelta,
): CompiledUpdateMetrics {
  return runtime.apply_delta(delta);
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
