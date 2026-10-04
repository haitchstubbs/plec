import { describe, expect, it, vi } from 'vitest';
import {
  boundedResponseBytes,
  emitPlecDiagnostic,
  plecRuntimeDiagnostic,
  markPlecTiming,
  adaptLiveCollection,
  registerPlecProviders,
  startPlecRouter,
  validateSsrRouteChain,
  wireRoutedInputs,
  type LiveCollectionChange,
} from './index';

describe('bounded streamed response decoding', () => {
  const encoder = new TextEncoder();

  function chunkedResponse(
    chunks: (string | Uint8Array)[],
    headers?: Record<string, string>,
  ): Response {
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        for (const chunk of chunks) {
          controller.enqueue(
            typeof chunk === 'string' ? encoder.encode(chunk) : chunk,
          );
        }
        controller.close();
      },
    });
    return new Response(stream, { headers });
  }

  it('assembles chunked bodies without a declared length', async () => {
    const bytes = await boundedResponseBytes(
      chunkedResponse(['{"ok":', 'true}']),
      1024,
      'graph',
    );
    expect(new TextDecoder().decode(bytes)).toBe('{"ok":true}');
  });

  it('rejects forged declared lengths before streaming', async () => {
    await expect(
      boundedResponseBytes(
        chunkedResponse(['tiny'], { 'content-length': '99999999' }),
        1024,
        'graph',
      ),
    ).rejects.toThrow('graph exceeds byte limit');
  });

  it('cancels chunked bodies that pass the ceiling mid-stream', async () => {
    const chunk = new Uint8Array(700).fill(65);
    await expect(
      boundedResponseBytes(
        chunkedResponse([chunk, chunk]),
        1024,
        'graph',
      ),
    ).rejects.toThrow('graph exceeds byte limit');
  });

  it('accepts bodies exactly at the ceiling', async () => {
    const bytes = await boundedResponseBytes(
      chunkedResponse([new Uint8Array(1024).fill(65)]),
      1024,
      'graph',
    );
    expect(bytes.byteLength).toBe(1024);
  });

  it('rejects bodies that cannot stream', async () => {
    await expect(
      boundedResponseBytes(
        new Response(null, { status: 204 }),
        1024,
        'graph',
      ),
    ).rejects.toThrow('graph body is unreadable');
  });
});

describe('ssr route chain gate', () => {
  const manifestRoutes = [
    { id: 'root#layout', path: '' },
    { id: 'routes/home.tsx#Route', path: '' },
    { id: 'routes/project.tsx#Route', path: 'projects/$id' },
    { id: 'routes/about.tsx#Route', path: 'about' },
    { id: 'routes/not-found.tsx#Route', path: '*' },
  ];

  it('accepts flat, parameterized, catch-all, and nested chains', () => {
    expect(
      validateSsrRouteChain(manifestRoutes, [
        {
          routeId: 'routes/home.tsx#Route',
          params: {},
          phase: 'active',
        },
      ]),
    ).toBeNull();
    expect(
      validateSsrRouteChain(manifestRoutes, [
        {
          routeId: 'routes/project.tsx#Route',
          params: { id: 'a b' },
          phase: 'active',
        },
      ]),
    ).toBeNull();
    expect(
      validateSsrRouteChain(manifestRoutes, [
        { routeId: 'routes/not-found.tsx#Route', params: {} },
      ]),
    ).toBeNull();
    // A nested layout chain is shape-checked only; URL agreement is the WASM
    // runtime's cross-validation decision.
    expect(
      validateSsrRouteChain(manifestRoutes, [
        { routeId: 'root#layout', params: {} },
        { routeId: 'routes/home.tsx#Route', params: {} },
      ]),
    ).toBeNull();
  });

  it('rejects empty, malformed, and unknown chains with chain details', () => {
    expect(validateSsrRouteChain(manifestRoutes, [])).toBe('empty');
    expect(validateSsrRouteChain(manifestRoutes, undefined)).toBe(
      'empty',
    );
    expect(
      validateSsrRouteChain(manifestRoutes, [
        { routeId: 'ghost#Route' },
      ]),
    ).toBe('unknown-route:ghost#Route');
    expect(
      validateSsrRouteChain(manifestRoutes, [{ params: {} }]),
    ).toBe('instance:0');
    expect(
      validateSsrRouteChain(manifestRoutes, [
        { routeId: 'routes/project.tsx#Route', params: { id: 7 } },
      ]),
    ).toBe('params:0');
    expect(
      validateSsrRouteChain(manifestRoutes, [
        { routeId: 'routes/home.tsx#Route', phase: 'paused' },
      ]),
    ).toBe('phase:0');
  });
});

describe('compiled browser adapter', () => {
  it('classifies a runtime provider-resolution failure with actionable identity', () => {
    const error = new Error('unknown host component: lucide/Missing');
    const diagnostic = plecRuntimeDiagnostic(error, 'routes/home');
    expect(diagnostic).toMatchObject({
      code: 'PLEC-PROVIDER-RESOLUTION',
      phase: 'provider',
      message: expect.stringContaining('lucide/Missing'),
      graphId: 'routes/home',
      suggestion: expect.stringContaining('provider registration'),
    });
    expect(diagnostic).not.toHaveProperty('detail');
    expect(
      plecRuntimeDiagnostic(error, 'routes/home', true).detail,
    ).toBe(error.message);
    const dispatchEvent = vi.fn();
    vi.stubGlobal('window', { dispatchEvent });
    emitPlecDiagnostic({}, diagnostic);
    expect(dispatchEvent).toHaveBeenCalledWith(
      expect.objectContaining({
        type: 'plec:diagnostic',
        detail: expect.objectContaining({
          code: 'PLEC-PROVIDER-RESOLUTION',
        }),
      }),
    );
    vi.unstubAllGlobals();
  });

  it('classifies a missing host provider registry as a provider failure', () => {
    expect(
      plecRuntimeDiagnostic(
        new Error('host component registry is not installed'),
      ),
    ).toMatchObject({
      code: 'PLEC-PROVIDER-RESOLUTION',
      phase: 'provider',
      suggestion: expect.stringContaining(
        'Install a provider registry',
      ),
    });
  });

  it('classifies known protocol failures but leaves generic errors as runtime failures', () => {
    expect(
      plecRuntimeDiagnostic(new Error('mismatch:ssr-snapshot-version')),
    ).toMatchObject({
      code: 'PLEC-PROTOCOL-COMPATIBILITY',
      phase: 'protocol',
    });
    expect(
      plecRuntimeDiagnostic(new Error('application operation failed')),
    ).toMatchObject({
      code: 'PLEC-BROWSER-RUNTIME',
      phase: 'browser-runtime',
    });
  });

  it('reports a failed WASM mount once and preserves the original rejection', async () => {
    const failure = new Error('unknown host component: lucide/Missing');
    const dispatchEvent = vi.fn();
    const onDiagnostic = vi.fn();
    vi.stubGlobal('document', { querySelector: () => null });
    vi.stubGlobal('window', {
      location: { pathname: '/', search: '', hash: '' },
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent,
    });
    const runtimeModule = `
      export default async function() {}
      export class PlecRuntime {
        set_cookie_policy() {}
        set_fetch_policy() {}
        set_host_registry() {}
        set_host_inputs() {}
        register_graph() {}
        start() { throw new Error('unknown host component: lucide/Missing'); }
        dispose() {}
      }
    `;
    const RuntimeURL = class extends URL {};
    Object.assign(RuntimeURL, {
      createObjectURL: () =>
        `data:text/javascript;charset=utf-8,${encodeURIComponent(runtimeModule)}`,
      revokeObjectURL: vi.fn(),
    });
    vi.stubGlobal('URL', RuntimeURL);
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string) => {
        if (url === '/route-manifest.json')
          return Response.json({ rootGraphId: 'root', routes: [] });
        if (url === '/graphs/root.json') return Response.json({});
        return new Response(runtimeModule);
      }),
    );
    const root = {
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      contains: vi.fn(() => false),
    } as unknown as Element;

    await expect(
      startPlecRouter({ root, onDiagnostic }),
    ).rejects.toThrow(failure.message);
    expect(onDiagnostic).toHaveBeenCalledTimes(1);
    expect(onDiagnostic).toHaveBeenCalledWith(
      expect.objectContaining({
        code: 'PLEC-PROVIDER-RESOLUTION',
        phase: 'provider',
      }),
    );
    expect(onDiagnostic.mock.calls[0]?.[0]).not.toHaveProperty(
      'detail',
    );
    expect(dispatchEvent).toHaveBeenCalledTimes(1);
    vi.unstubAllGlobals();
  });

  it('keeps adoption fallback reporting separate from common diagnostics', async () => {
    const dispatchEvent = vi.fn();
    const onDiagnostic = vi.fn();
    const onAdoptionDiagnostic = vi.fn();
    const bootstrap = {
      version: 2,
      snapshot: {
        revision: 'revision-1',
        routes: [{ routeId: 'home', params: {}, phase: 'active' }],
      },
    };
    vi.stubGlobal('document', {
      querySelector: () => ({ textContent: JSON.stringify(bootstrap) }),
    });
    vi.stubGlobal('window', {
      location: { pathname: '/', search: '', hash: '' },
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent,
    });
    const runtimeModule = `
      export default async function() {}
      export class PlecRuntime {
        set_cookie_policy() {}
        set_fetch_policy() {}
        set_host_registry() {}
        set_host_inputs() {}
        register_graph() {}
        start_adopt_snapshot() { throw new Error('mismatch:ssr-snapshot-version'); }
        abandon_adoption() {}
        start() {}
        dispose() {}
      }
    `;
    const RuntimeURL = class extends URL {};
    Object.assign(RuntimeURL, {
      createObjectURL: () =>
        `data:text/javascript;charset=utf-8,${encodeURIComponent(runtimeModule)}`,
      revokeObjectURL: vi.fn(),
    });
    vi.stubGlobal('URL', RuntimeURL);
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string) => {
        if (url === '/route-manifest.json')
          return Response.json({
            rootGraphId: 'root',
            revision: 'revision-1',
            routes: [{ id: 'home', graphId: 'root' }],
          });
        if (url === '/graphs/root.json') return Response.json({});
        return new Response(runtimeModule);
      }),
    );
    const root = {
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      contains: vi.fn(() => false),
    } as unknown as Element;

    const controller = await startPlecRouter({
      root,
      development: true,
      onDiagnostic,
      onAdoptionDiagnostic,
    });
    expect(onAdoptionDiagnostic).toHaveBeenCalledTimes(1);
    expect(onAdoptionDiagnostic).toHaveBeenCalledWith(
      expect.objectContaining({
        outcome: 'fallback',
        mismatchCodes: ['mismatch:ssr-snapshot-version'],
      }),
    );
    expect(onDiagnostic).toHaveBeenCalledTimes(1);
    expect(onDiagnostic).toHaveBeenCalledWith(
      expect.objectContaining({
        code: 'PLEC-SSR-ADOPTION',
        phase: 'adoption',
        detail: 'mismatch:ssr-snapshot-version',
      }),
    );
    expect(dispatchEvent).toHaveBeenCalledTimes(2);
    expect(
      dispatchEvent.mock.calls.map(([event]) => event.type),
    ).toEqual(['plec:diagnostic', 'plec:adoption']);
    controller.dispose();
    vi.unstubAllGlobals();
  });

  it('surfaces structured browser diagnostics through the callback and DOM event', () => {
    const onDiagnostic = vi.fn();
    vi.stubGlobal('window', { dispatchEvent: vi.fn() });
    emitPlecDiagnostic(
      { onDiagnostic },
      {
        code: 'PLEC-ARTIFACT-LOAD',
        phase: 'artifact',
        message: 'Failed to load graph home',
        detail: 'private transport detail',
      },
    );
    expect(onDiagnostic).toHaveBeenCalledWith(
      expect.objectContaining({
        code: 'PLEC-ARTIFACT-LOAD',
        phase: 'artifact',
      }),
    );
    expect((window.dispatchEvent as any).mock.calls[0][0].type).toBe(
      'plec:diagnostic',
    );
    vi.unstubAllGlobals();
  });

  it('rejects provider modules outside the build-owned asset boundary', async () => {
    vi.stubGlobal('window', {
      location: { origin: 'http://plec.test' },
    });
    vi.stubGlobal(
      'fetch',
      vi.fn(
        async () =>
          new Response(
            JSON.stringify({
              version: 2,
              revision: 'revision-1',
              providers: [
                {
                  id: 'lucide',
                  module: 'https://attacker.test/provider.js',
                  components: ['House'],
                  ssr: false,
                },
              ],
            }),
          ),
      ),
    );

    await expect(registerPlecProviders()).rejects.toThrow(
      'Invalid host provider module URL for lucide',
    );
    vi.unstubAllGlobals();
  });

  it('emits named Plec mount boundaries through User Timing', () => {
    const mark = vi.fn();
    vi.stubGlobal('performance', { mark });
    markPlecTiming('plec:artifact-fetch-start');
    markPlecTiming('plec:mount-end');
    markPlecTiming('plec:mount-error');
    expect(mark.mock.calls).toEqual([
      ['plec:artifact-fetch-start'],
      ['plec:mount-end'],
      ['plec:mount-error'],
    ]);
    vi.unstubAllGlobals();
  });

  it('adapts live collection changes into runtime deltas', () => {
    let notify!: (
      changes: LiveCollectionChange<{ id: string; title: string }>[],
    ) => void;
    const input = adaptLiveCollection('todos', {
      toArray: [{ id: 'a', title: 'A' }],
      subscribeChanges: (listener) => {
        notify = listener;
        return { unsubscribe: vi.fn() };
      },
    });
    const receive = vi.fn();
    input.subscribeDeltas(receive);

    notify([
      {
        type: 'insert',
        key: 'b',
        value: { id: 'b', title: 'B' },
        beforeKey: null,
      },
      {
        type: 'update',
        key: 'a',
        value: { id: 'a', title: 'A2' },
        previousValue: { id: 'a', title: 'A' },
      },
      { type: 'move', key: 'b', beforeKey: 'a' },
      {
        type: 'delete',
        key: 'a',
        previousValue: { id: 'a', title: 'A2' },
      },
    ]);

    expect(input.getSnapshot()).toEqual([{ id: 'a', title: 'A' }]);
    expect(receive).toHaveBeenCalledWith([
      {
        type: 'insert',
        inputId: 'todos',
        rowKey: 'b',
        row: { id: 'b', title: 'B' },
        beforeRowKey: null,
      },
      {
        type: 'update',
        inputId: 'todos',
        rowKey: 'a',
        changes: { title: 'A2' },
      },
      {
        type: 'move',
        inputId: 'todos',
        rowKey: 'b',
        beforeRowKey: 'a',
      },
      { type: 'remove', inputId: 'todos', rowKey: 'a' },
    ]);
  });

  it('hydrates routed collection inputs, forwards deltas, and disposes subscriptions', () => {
    let notify!: (deltas: any[]) => void;
    const unsubscribe = vi.fn();
    const runtime = {
      initialize_input: vi.fn(() => ({})),
      apply_deltas: vi.fn(() => ({
        domOperations: 3,
        nodesTouched: 2,
        bindingsTouched: 2,
        wasmDomUs: 12,
      })),
    };
    const onQueryUpdate = vi.fn();
    const bridge = wireRoutedInputs(
      runtime as any,
      {
        instruments: {
          getSnapshot: () => [{ id: 'PX0001', last: 101 }],
          subscribeDeltas: (listener) => {
            notify = listener;
            return unsubscribe;
          },
        },
      },
      onQueryUpdate,
    );

    bridge.hydrate();
    expect(runtime.initialize_input).toHaveBeenCalledWith(
      'instruments',
      [{ id: 'PX0001', last: 101 }],
    );

    notify([
      {
        type: 'update',
        inputId: 'instruments',
        rowKey: 'PX0001',
        changes: { last: 101.2 },
      },
    ]);
    expect(runtime.apply_deltas).toHaveBeenCalledWith([
      {
        type: 'update',
        inputId: 'instruments',
        rowKey: 'PX0001',
        changes: { last: 101.2 },
      },
    ]);
    expect(onQueryUpdate).toHaveBeenCalledWith(
      expect.objectContaining({ deltaCount: 1, domOperations: 3 }),
    );

    bridge.hydrate();
    expect(runtime.initialize_input).toHaveBeenCalledTimes(2);
    bridge.dispose();
    expect(unsubscribe).toHaveBeenCalledOnce();
  });
});
