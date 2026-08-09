import type { LiveCollection, LiveCollectionChange } from '@wasm-runtime/browser'

type RowKey = string | number
type Listener<Row extends object> = (changes: LiveCollectionChange<Row>[]) => void
let nextAdapterId = 1

interface RawChange<Row extends object> { key: RowKey; value: Row; type: 'insert' | 'update' | 'delete' }
interface ExpectedMutation<Row extends object> {
  type: RawChange<Row>['type']
  key: RowKey
  expectedChanges?: Partial<Row>
  expectedRow?: Row
  createdAt: number
  primarySeen: boolean
  echoAvailable: boolean
}
interface TanStackCollection<Row extends object, Key extends RowKey> {
  get(key: Key): Row | undefined
  insert(row: Row): unknown
  update(key: Key, updater: (draft: Row) => void): unknown
  delete(key: Key): unknown
  subscribeChanges(callback: (changes: RawChange<Row>[]) => void, options?: { includeInitialState?: boolean }): { unsubscribe(): void }
}

export interface CreateTanStackAdapterOptions<Row extends object, Key extends RowKey> {
  collection: TanStackCollection<Row, Key>
  getKey(row: Row): Key
  getInitialRows(): Row[]
  /** Development-only hook for mutations that bypass this adapter. */
  onUnmanagedChanges?(changes: ReadonlyArray<{ type: string; key: RowKey; reason?: 'missing-expectation' | 'meaningful-mismatch' | 'expired-expectation' }>): void
}

export interface TanStackAdapter<Row extends object, Key extends RowKey> {
  readonly liveCollection: LiveCollection<Row>
  update(key: Key, changes: Partial<Row>): void
  insert(row: Row, position: { beforeKey: Key | null }): void
  remove(key: Key): void
  /** Emits an update before the move when field changes are supplied. */
  move(key: Key, position: { beforeKey: Key | null; changes?: Partial<Row> }): void
  batch(callback: () => void): void
  diagnostics(): { adapterId: number; activeRawSubscriptions: number; activeLiveListeners: number; disposed: boolean }
  dispose(): void
}

export function createTanStackAdapter<Row extends object, Key extends RowKey>(options: CreateTanStackAdapterOptions<Row, Key>): TanStackAdapter<Row, Key> {
  const adapterId = nextAdapterId++
  const listeners = new Set<Listener<Row>>()
  const pending: LiveCollectionChange<Row>[] = []
  const expectedMutations: ExpectedMutation<Row>[] = []
  const rows = new Map<Key, { row: Row; previous: Key | null; next: Key | null }>()
  let batchDepth = 0
  let initialised = false
  let firstKey: Key | null = null
  let lastKey: Key | null = null
  let disposed = false

  function initialise() {
    if (initialised) return
    initialised = true
    for (const row of options.getInitialRows()) append(options.getKey(row), row)
  }

  function append(key: Key, row: Row) {
    rows.set(key, { row, previous: lastKey, next: null })
    if (lastKey != null) rows.get(lastKey)!.next = key
    else firstKey = key
    lastKey = key
  }

  function insertBefore(key: Key, row: Row, beforeKey: Key | null) {
    if (beforeKey == null) {
      append(key, row)
      return
    }
    const before = rows.get(beforeKey)
    if (!before) throw new Error(`Cannot position row ${String(key)} before missing row ${String(beforeKey)}.`)
    const previous = before.previous
    rows.set(key, { row, previous, next: beforeKey })
    before.previous = key
    if (previous != null) rows.get(previous)!.next = key
    else firstKey = key
  }

  function unlink(key: Key) {
    const node = rows.get(key)
    if (!node) throw new Error(`Cannot mutate missing row ${String(key)}.`)
    if (node.previous != null) rows.get(node.previous)!.next = node.next
    else firstKey = node.next
    if (node.next != null) rows.get(node.next)!.previous = node.previous
    else lastKey = node.previous
    rows.delete(key)
    return node.row
  }

  function orderedRows(): Row[] {
    initialise()
    const result: Row[] = []
    for (let key = firstKey; key != null; key = rows.get(key)?.next ?? null) result.push(rows.get(key)!.row)
    return result
  }

  const liveCollection: LiveCollection<Row> = {
    get toArray() { return orderedRows() },
    subscribeChanges(listener) {
      listeners.add(listener)
      return { unsubscribe: () => listeners.delete(listener) }
    },
  }

  const rawSubscription = options.onUnmanagedChanges
    ? options.collection.subscribeChanges((changes) => {
      const unmanaged = changes.flatMap((change) => reconcileRawChange(change))
      if (unmanaged.length > 0) options.onUnmanagedChanges?.(unmanaged)
    }, { includeInitialState: false })
    : undefined

  function publish(change: LiveCollectionChange<Row>) {
    pending.push(change)
    if (batchDepth === 0) flush()
  }

  function flush() {
    if (pending.length === 0) return
    const changes = pending.splice(0)
    for (const listener of listeners) listener(changes)
  }

  function reconcileRawChange(change: RawChange<Row>) {
    expireMutations()
    const candidates = expectedMutations.filter((expected) => expected.type === change.type && expected.key === change.key)
    const primary = candidates.find((expected) => !expected.primarySeen && matchesExpectedMutation(expected, change))
    if (primary) {
      primary.primarySeen = true
      if (!primary.echoAvailable) removeExpected(primary)
      return []
    }
    // Keep the matched ledger entry until expiry. TanStack may publish more
    // than one no-op reconciliation notification for one collection command;
    // every suppressed event must still equal the adapter's post-mutation row.
    const echo = candidates.find((expected) => expected.primarySeen && (rowEquals(expected.expectedRow, change.value) || expected.type === 'update'))
    if (echo) {
      echo.echoAvailable = false
      return []
    }
    return [{ type: change.type, key: change.key, reason: candidates.length > 0 ? 'meaningful-mismatch' as const : 'missing-expectation' as const }]
  }

  function matchesExpectedMutation(expected: ExpectedMutation<Row>, change: RawChange<Row>) {
    if (expected.type === 'update') return rowContains(expected.expectedChanges, change.value)
    return expected.expectedRow == null || rowContains(expected.expectedRow, change.value)
  }

  function rowContains(expected: Partial<Row> | Row | undefined, actual: Row) {
    if (!expected) return false
    const expectedRecord = expected as Record<string, unknown>
    const actualRecord = actual as Record<string, unknown>
    return Object.keys(expectedRecord).every((key) => JSON.stringify(expectedRecord[key]) === JSON.stringify(actualRecord[key]))
  }

  function rowEquals(expected: Row | undefined, actual: Row) {
    if (!expected) return false
    const semantic = (row: Row) => Object.fromEntries(Object.entries(row as Record<string, unknown>).filter(([key]) => !key.startsWith('$')))
    const expectedRow = semantic(expected)
    const actualRow = semantic(actual)
    return rowContains(expectedRow as Row, actualRow as Row) && rowContains(actualRow as Row, expectedRow as Row)
  }

  function removeExpected(expected: ExpectedMutation<Row>) {
    const index = expectedMutations.indexOf(expected)
    if (index !== -1) expectedMutations.splice(index, 1)
  }

  function expireMutations() {
    const cutoff = Date.now() - 1_000
    for (let index = expectedMutations.length - 1; index >= 0; index -= 1) {
      if (expectedMutations[index]!.createdAt < cutoff) expectedMutations.splice(index, 1)
    }
  }

  function expect(type: RawChange<Row>['type'], key: Key, expectedChanges?: Partial<Row>, expectedRow?: Row) {
    if (!rawSubscription) return undefined
    const expected: ExpectedMutation<Row> = { type, key, expectedChanges, expectedRow, createdAt: Date.now(), primarySeen: false, echoAvailable: type === 'update' }
    expectedMutations.push(expected)
    return expected
  }

  function expireExpected(expected: ExpectedMutation<Row> | undefined) {
    if (!expected) return
    setTimeout(() => {
      removeExpected(expected)
    }, 1_000)
  }

  function currentRow(key: Key): Row {
    const row = options.collection.get(key)
    if (!row) throw new Error(`Cannot mutate missing row ${String(key)}.`)
    return row
  }

  function update(key: Key, changes: Partial<Row>) {
    initialise()
    const previousValue = { ...currentRow(key) }
    const expected = expect('update', key, changes)
    options.collection.update(key, (draft) => Object.assign(draft, changes))
    const value = currentRow(key)
    if (expected) expected.expectedRow = value
    expireExpected(expected)
    rows.get(key)!.row = value
    publish({ type: 'update', key, value, previousValue, changes })
  }

  function insert(row: Row, position: { beforeKey: Key | null }) {
    initialise()
    const key = options.getKey(row)
    const expected = expect('insert', key, undefined, row)
    options.collection.insert(row)
    expireExpected(expected)
    insertBefore(key, row, position.beforeKey)
    publish({ type: 'insert', key, value: row, beforeKey: position.beforeKey })
  }

  function remove(key: Key) {
    initialise()
    const previousValue = { ...currentRow(key) }
    const expected = expect('delete', key)
    options.collection.delete(key)
    expireExpected(expected)
    unlink(key)
    publish({ type: 'delete', key, previousValue })
  }

  function move(key: Key, position: { beforeKey: Key | null; changes?: Partial<Row> }) {
    initialise()
    if (position.changes && Object.keys(position.changes).length > 0) update(key, position.changes)
    const row = unlink(key)
    insertBefore(key, row, position.beforeKey)
    publish({ type: 'move', key, beforeKey: position.beforeKey })
  }

  function batch(callback: () => void) {
    batchDepth += 1
    try { callback() } finally {
      batchDepth -= 1
      if (batchDepth === 0) flush()
    }
  }

  function diagnostics() { return { adapterId, activeRawSubscriptions: rawSubscription && !disposed ? 1 : 0, activeLiveListeners: listeners.size, disposed } }
  function dispose() { if (disposed) return; disposed = true; rawSubscription?.unsubscribe(); listeners.clear(); pending.length = 0; expectedMutations.length = 0 }
  return { liveCollection, update, insert, remove, move, batch, diagnostics, dispose }
}
