import { describe, expect, it, vi } from 'vitest';
import {
  markPlecTiming,
  adaptLiveCollection,
  reconcileInputSnapshot,
  type CollectionProjection,
  type LiveCollectionChange,
} from './index';

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

  it('reconciles only observed fields for a snapshot collection', () => {
    const previous: CollectionProjection = {
      kind: 'collection',
      keys: ['a', 'b'],
      rows: new Map([
        ['a', { title: 'A', done: false }],
        ['b', { title: 'B', done: false }],
      ]),
    };
    const next: CollectionProjection = {
      kind: 'collection',
      keys: ['a', 'b'],
      rows: new Map([
        ['a', { title: 'A', done: true }],
        ['b', { title: 'B', done: false }],
      ]),
    };
    expect(
      reconcileInputSnapshot('todos', previous, next, {
        kind: 'collection',
        orderSensitive: true,
      }),
    ).toEqual([
      {
        type: 'update',
        inputId: 'todos',
        rowKey: 'a',
        changes: { done: true },
      },
    ]);
  });

  it('adapts live collection changes into runtime deltas', () => {
    let notify!: (changes: LiveCollectionChange<{ id: string; title: string }>[]) => void;
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
      { type: 'insert', key: 'b', value: { id: 'b', title: 'B' }, beforeKey: null },
      { type: 'update', key: 'a', value: { id: 'a', title: 'A2' }, previousValue: { id: 'a', title: 'A' } },
      { type: 'move', key: 'b', beforeKey: 'a' },
      { type: 'delete', key: 'a', previousValue: { id: 'a', title: 'A2' } },
    ]);

    expect(input.getSnapshot()).toEqual([{ id: 'a', title: 'A' }]);
    expect(receive).toHaveBeenCalledWith([
      { type: 'insert', inputId: 'todos', rowKey: 'b', row: { id: 'b', title: 'B' }, beforeRowKey: null },
      { type: 'update', inputId: 'todos', rowKey: 'a', changes: { title: 'A2' } },
      { type: 'move', inputId: 'todos', rowKey: 'b', beforeRowKey: 'a' },
      { type: 'remove', inputId: 'todos', rowKey: 'a' },
    ]);
  });
});
