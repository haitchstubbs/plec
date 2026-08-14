import { describe, expect, it, vi } from 'vitest';
import {
  markPlecTiming,
  reconcileInputSnapshot,
  type CollectionProjection,
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
});
