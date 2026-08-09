import { createCollection, localOnlyCollectionOptions } from '@tanstack/db'
import { describe, expect, it, vi } from 'vitest'
import { createTanStackAdapter } from './index'

interface Todo { id: string; title: string; done: boolean; sortOrder: number }

function makeAdapter(onUnmanagedChanges?: (changes: ReadonlyArray<{ type: string; key: string | number }>) => void) {
  const collection = createCollection(localOnlyCollectionOptions<Todo, string>({
    getKey: (todo) => todo.id,
    initialData: [
      { id: 'a', title: 'A', done: false, sortOrder: 0 },
      { id: 'b', title: 'B', done: false, sortOrder: 1 },
    ],
  }))
  let initialReads = 0
  const adapter = createTanStackAdapter<Todo, string>({
    collection,
    getKey: (todo) => todo.id,
    getInitialRows: () => {
      initialReads += 1
      return [...collection.toArray].sort((left, right) => left.sortOrder - right.sortOrder)
    },
    onUnmanagedChanges,
  })
  return { adapter, collection, getInitialReads: () => initialReads }
}

describe('createTanStackAdapter', () => {
  it('reads its initial rows once and never reads them in the mutation path', () => {
    const { adapter, getInitialReads } = makeAdapter()
    expect(adapter.liveCollection.toArray.map((todo) => todo.id)).toEqual(['a', 'b'])
    expect(adapter.liveCollection.toArray.map((todo) => todo.id)).toEqual(['a', 'b'])
    adapter.update('a', { title: 'Updated' })
    adapter.move('a', { beforeKey: 'b', changes: { sortOrder: 1 } })
    adapter.insert({ id: 'c', title: 'C', done: false, sortOrder: 2 }, { beforeKey: null })
    adapter.remove('c')
    expect(getInitialReads()).toBe(1)
  })

  it('emits field deltas and canonical update-then-move ordering', () => {
    const { adapter } = makeAdapter()
    const received: unknown[] = []
    adapter.liveCollection.subscribeChanges((changes) => received.push(...changes))

    adapter.update('a', { title: 'Updated' })
    adapter.move('a', { beforeKey: 'b', changes: { done: true } })

    expect(received).toMatchObject([
      { type: 'update', key: 'a', changes: { title: 'Updated' } },
      { type: 'update', key: 'a', changes: { done: true } },
      { type: 'move', key: 'a', beforeKey: 'b' },
    ])
  })

  it('preserves exact command order in a batch', () => {
    const { adapter } = makeAdapter()
    const listener = vi.fn()
    adapter.liveCollection.subscribeChanges(listener)

    adapter.batch(() => {
      adapter.update('a', { title: 'Updated' })
      adapter.move('a', { beforeKey: 'b' })
      adapter.insert({ id: 'c', title: 'C', done: false, sortOrder: 2 }, { beforeKey: null })
      adapter.remove('a')
    })

    expect(listener).toHaveBeenCalledTimes(1)
    expect(listener.mock.calls[0]?.[0].map((change: { type: string }) => change.type)).toEqual(['update', 'move', 'insert', 'delete'])
  })

  it('reports direct collection mutations through the unmanaged-change guard', () => {
    const unmanaged = vi.fn()
    const { adapter, collection } = makeAdapter(unmanaged)
    collection.update('a', (draft) => { draft.title = 'Bypassed' })
    expect(unmanaged).toHaveBeenCalledWith(expect.arrayContaining([expect.objectContaining({ type: 'update', key: 'a' })]))
    adapter.dispose()
  })

  it('does not classify adapter-owned commands as unmanaged after deferred delivery', async () => {
    const unmanaged = vi.fn()
    const { adapter } = makeAdapter(unmanaged)
    adapter.update('a', { done: true })
    adapter.insert({ id: 'c', title: 'C', done: false, sortOrder: 2 }, { beforeKey: null })
    adapter.remove('c')
    await Promise.resolve()
    expect(unmanaged).not.toHaveBeenCalled()
    adapter.dispose()
  })

  it('keeps the reconciliation allowance until a collection-scheduled deferred update arrives', async () => {
    const row: Todo = { id: 'a', title: 'A', done: false, sortOrder: 0 }
    const subscribers = new Set<(changes: Array<{ key: string; value: Todo; type: 'update' }>) => void>()
    const collection = {
      get: (key: string) => key === row.id ? row : undefined,
      insert: vi.fn(),
      delete: vi.fn(),
      update: (_key: string, updater: (draft: Todo) => void) => {
        updater(row)
        setTimeout(() => {
          for (const subscriber of subscribers) subscriber([{ key: row.id, type: 'update', value: row }])
        }, 0)
      },
      subscribeChanges: (callback: (changes: Array<{ key: string; value: Todo; type: 'update' }>) => void) => {
        subscribers.add(callback)
        return { unsubscribe: () => subscribers.delete(callback) }
      },
    }
    const unmanaged = vi.fn()
    const adapter = createTanStackAdapter<Todo, string>({ collection, getKey: (todo) => todo.id, getInitialRows: () => [row], onUnmanagedChanges: unmanaged })

    adapter.update('a', { done: true })
    await new Promise<void>((resolve) => setTimeout(resolve, 0))

    expect(unmanaged).not.toHaveBeenCalled()
    adapter.dispose()
  })

  it('reports owned subscription diagnostics and disposes idempotently', () => {
    const { adapter } = makeAdapter(() => {})
    expect(adapter.diagnostics()).toMatchObject({ activeRawSubscriptions: 1, activeLiveListeners: 0, disposed: false })
    const listener = adapter.liveCollection.subscribeChanges(() => {})
    expect(adapter.diagnostics().activeLiveListeners).toBe(1)
    listener.unsubscribe()
    adapter.dispose()
    adapter.dispose()
    expect(adapter.diagnostics()).toMatchObject({ activeRawSubscriptions: 0, activeLiveListeners: 0, disposed: true })
  })
})
