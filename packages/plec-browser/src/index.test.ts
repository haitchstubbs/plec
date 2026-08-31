import { describe, expect, it, vi } from 'vitest';
import {
  markPlecTiming,
  adaptLiveCollection,
  validateSsrRouteChain,
  wireRoutedInputs,
  type LiveCollectionChange,
} from './index';

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
        { routeId: 'routes/home.tsx#Route', params: {}, phase: 'active' },
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
    expect(validateSsrRouteChain(manifestRoutes, undefined)).toBe('empty');
    expect(
      validateSsrRouteChain(manifestRoutes, [{ routeId: 'ghost#Route' }]),
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
