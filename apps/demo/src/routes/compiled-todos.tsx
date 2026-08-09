import { useEffect, useMemo, useRef, useState } from 'react'
import { createFileRoute, useNavigate, useRouterState } from '@tanstack/react-router'
import { mountCompiledApplication, type CompiledQueryUpdate } from '@wasm-runtime/browser'
import { createTanStackAdapter } from '@wasm-runtime/tanstack-adapter'
import { createTodoCollection, type Todo } from '../compiler-fixtures/todo-data'
import { CompiledApplicationShell } from '../components/CompiledApplicationShell'
import application from 'virtual:wasm-runtime/compiled-todos-ir'
import { completeMeasurementIntegrity, evaluateCompiledIntegrity, resetMeasurementIntegrity } from '../benchmark-integrity'
import { nextAnimationFrame, waitForDom } from '../benchmark-timing'

const benchmarkRowId = '1'
const benchmarkRowLabel = 'Todo 1'
type BenchmarkOperation = 'retitle' | 'toggle' | 'insert' | 'remove' | 'move'
type ExpectedDom = { kind: BenchmarkOperation; id: string; title?: string; done?: boolean; edge?: 'first' | 'last' }
type LatencySample = { mutationApiMs: number; frameworkPropagationMs: null; reconciliationMs: null; domCommitMs: number; adapterPropagationMs: number | null; wasmDomMs: number | null; nextAnimationFrameMs: number }
type ActiveMeasurement = { apiEnd: number; compiledUpdate: CompiledQueryUpdate | null }

export const Route = createFileRoute('/compiled-todos')({ component: CompiledTodosRoute })

function CompiledTodosRoute() {
  const search = typeof window === 'undefined' ? new URLSearchParams() : new URLSearchParams(window.location.search)
  const benchmark = search.has('bench')
  const requestedSize = Number(search.get('size'))
  const size = benchmark && [10, 100, 1000, 10000, 50000].includes(requestedSize) ? requestedSize : 50000
  const todosCollection = useMemo(() => createTodoCollection(size), [size])
  const [tanstackAdapter, setTanstackAdapter] = useState<ReturnType<typeof createTanStackAdapter<Todo, string>> | null>(null)
  const [status, setStatus] = useState<'idle' | 'mounting' | 'mounted' | 'error'>('idle')
  const sequence = useRef(0)
  const rowCount = useRef(size)
  const benchmarkIds = useRef(new Set<string>())
  const activeMeasurement = useRef<ActiveMeasurement | null>(null)
  const controllerRef = useRef<Awaited<ReturnType<typeof mountCompiledApplication>> | null>(null)
  const reactCommits = useRef(0)
  const reactCommitBaseline = useRef<number | null>(null)
  const lastMeasurementIntegrity = useRef(resetMeasurementIntegrity())
  const unmanagedMutations = useRef(0)
  const navigate = useNavigate()
  const pathname = useRouterState({ select: (state) => state.location.pathname })
  // Profiler callbacks are development-oriented; this direct counter remains
  // meaningful in production benchmark builds as well.
  reactCommits.current += 1

  // `createTanStackAdapter` subscribes to raw collection changes. Do not create
  // it in useMemo: React Strict Mode may invoke a memo initializer twice and
  // discard one value without giving it a cleanup opportunity.
  useEffect(() => {
    const adapter = createTanStackAdapter<Todo, string>({
      collection: todosCollection,
      getKey: (todo) => todo.id,
      // TanStack DB's backing collection is key ordered. The React workload
      // explicitly presents by sortOrder, so seed the compiled graph in that
      // same user-visible order before applying targeted move deltas.
      getInitialRows: () => [...todosCollection.toArray].sort(compareTodos),
      onUnmanagedChanges: import.meta.env.DEV || benchmark ? (changes) => { unmanagedMutations.current += changes.length; console.warn('Compiled runtime ignored a TanStack mutation that bypassed its adapter.', changes) } : undefined,
    })
    setTanstackAdapter(adapter)
    return () => adapter.dispose()
  }, [todosCollection])

  function onCompiledQueryUpdate(update: CompiledQueryUpdate) {
    if (activeMeasurement.current) activeMeasurement.current.compiledUpdate = update
  }

  useEffect(() => {
    const runtimeRoot = document.querySelector<HTMLDivElement>('#compiled-todos-root')
    if (!runtimeRoot) return
    let cancelled = false
    let controller: Awaited<ReturnType<typeof mountCompiledApplication>> | null = null
    setStatus('mounting')
    if (!tanstackAdapter) return
    void mountCompiledApplication({ root: runtimeRoot, queries: { i1: tanstackAdapter.liveCollection }, actions: { updateTodo }, hostValues: { currentYear: new Date().getFullYear(), location: { pathname } }, onNavigate: ({ href, replace }: { href: string; replace?: boolean }) => { void navigate({ to: href as any, replace }) }, onQueryUpdate: onCompiledQueryUpdate })
      .then((mounted) => {
        // React development/Strict Mode may clean up this effect before the
        // async WASM mount resolves. Dispose that late controller immediately
        // so its delegated DOM listeners cannot issue duplicate mutations.
        if (cancelled) { mounted.dispose(); return }
        controller = mounted
        controllerRef.current = mounted
        setStatus('mounted')
        // `setStatus` and the mount-metrics callback both schedule React work.
        // Establish the benchmark baseline only after their commit opportunity
        // and one subsequent frame have passed.
        requestAnimationFrame(() => requestAnimationFrame(() => { reactCommitBaseline.current = reactCommits.current }))
      })
      .catch(() => { if (!cancelled) setStatus('error') })
    return () => { cancelled = true; controller?.dispose(); if (controllerRef.current === controller) controllerRef.current = null }
  }, [tanstackAdapter, navigate])

  useEffect(() => { controllerRef.current?.applyHostValues({ location: { pathname } }) }, [pathname])

  function updateTodo(id: string, changes: Partial<Todo>) {
    if (!tanstackAdapter) return
    tanstackAdapter.update(id, changes)
  }

  function requireAdapter() {
    if (!tanstackAdapter) throw new Error('Compiled runtime adapter is not ready.')
    return tanstackAdapter
  }

  function planOperation(operation: BenchmarkOperation) {
    if (operation === 'retitle') {
      const title = `${benchmarkRowLabel} benchmark ${++sequence.current}`
      return { expected: { kind: operation, id: benchmarkRowId, title } as ExpectedDom, run: () => updateTodo(benchmarkRowId, { title }) }
    }
    if (operation === 'toggle') {
      const done = !todosCollection.get(benchmarkRowId)?.done
      return { expected: { kind: operation, id: benchmarkRowId, done } as ExpectedDom, run: () => updateTodo(benchmarkRowId, { done }) }
    }
    if (operation === 'insert') {
      const id = `bench-${++sequence.current}`
      return { expected: { kind: operation, id } as ExpectedDom, run: () => { requireAdapter().insert({ id, title: `Benchmark ${sequence.current}`, done: false, sortOrder: rowCount.current + sequence.current }, { beforeKey: null }); rowCount.current += 1; benchmarkIds.current.add(id) } }
    }
    if (operation === 'remove') {
      const id = benchmarkIds.current.values().next().value as string | undefined
      if (!id) throw new Error('Remove requires a prepared benchmark row.')
      return { expected: { kind: operation, id } as ExpectedDom, run: () => { requireAdapter().remove(id); rowCount.current -= 1; benchmarkIds.current.delete(id) } }
    }
    const edge: 'first' | 'last' = ++sequence.current % 2 === 0 ? 'first' : 'last'
    const sortOrder = edge === 'first' ? -sequence.current : rowCount.current + sequence.current
    const beforeKey = edge === 'first' ? firstRenderedRowKey() : null
    return { expected: { kind: operation, id: benchmarkRowId, edge } as ExpectedDom, run: () => requireAdapter().move(benchmarkRowId, { beforeKey, changes: { sortOrder } }) }
  }

  function prepare(operation: BenchmarkOperation) {
    if (operation !== 'remove' || benchmarkIds.current.size > 0) return
    const id = `bench-${++sequence.current}`
    requireAdapter().insert({ id, title: `Benchmark ${sequence.current}`, done: false, sortOrder: rowCount.current + sequence.current }, { beforeKey: null })
    rowCount.current += 1
    benchmarkIds.current.add(id)
  }

  async function measure(operation: BenchmarkOperation): Promise<LatencySample> {
    if (status !== 'mounted') throw new Error('Compiled runtime is not mounted.')
    const root = document.querySelector('#compiled-todos-root')
    if (!root) throw new Error('Missing compiled benchmark root.')
    const plan = planOperation(operation)
    const settled = waitForDom(root, () => matchesExpectedDom(root, plan.expected))
    const start = performance.now()
    lastMeasurementIntegrity.current = resetMeasurementIntegrity()
    const reactCommitsAtStart = reactCommits.current
    const active: ActiveMeasurement = { apiEnd: start, compiledUpdate: null }
    activeMeasurement.current = active
    plan.run()
    active.apiEnd = performance.now()
    await settled
    const domCommit = performance.now()
    await nextAnimationFrame()
    lastMeasurementIntegrity.current = completeMeasurementIntegrity(reactCommitsAtStart, reactCommits.current)
    const integrityResult = integrity()
    if (!integrityResult.ok) throw new Error(`Compiled benchmark integrity failure: ${JSON.stringify(integrityResult)}`)
    const nextFrame = performance.now()
    activeMeasurement.current = null
    return { mutationApiMs: active.apiEnd - start, frameworkPropagationMs: null, reconciliationMs: null, domCommitMs: domCommit - start, adapterPropagationMs: active.compiledUpdate?.adapterMs ?? null, wasmDomMs: active.compiledUpdate ? active.compiledUpdate.wasmDomUs / 1000 : null, nextAnimationFrameMs: nextFrame - start }
  }

  useEffect(() => {
    if (!benchmark) return
    const api = {
      prepare,
      measure,
      retitle: () => planOperation('retitle').run(), toggle: () => planOperation('toggle').run(), insert: () => planOperation('insert').run(), remove: () => planOperation('remove').run(), move: () => planOperation('move').run(),
      validate: () => { const root = document.querySelector('#compiled-todos-root'); return !!root && validateFullDom(root, todosCollection.toArray as Todo[]) },
      snapshot: () => ({ collectionRows: todosCollection.toArray.length, title: todosCollection.get(benchmarkRowId)?.title, done: todosCollection.get(benchmarkRowId)?.done, compiledRows: document.querySelectorAll('#compiled-todos-root [data-todo-id]').length }),
      integrity,
    }
    ;(window as any).__wasmRuntimeBenchmark = api
    return () => { delete (window as any).__wasmRuntimeBenchmark }
  }, [benchmark, status, todosCollection])

  function integrity() {
    const adapter = tanstackAdapter?.diagnostics()
    const controller = controllerRef.current?.diagnostics()
    const reactRendersSinceMount = reactCommitBaseline.current == null ? null : reactCommits.current - reactCommitBaseline.current
    return evaluateCompiledIntegrity({
      adapter,
      controller,
      reactRendersSinceMount,
      measurement: lastMeasurementIntegrity.current,
      unmanagedMutations: unmanagedMutations.current,
      compiledRouteReactVisualNodesOutsideIslands: 0,
    })
  }

  return <CompiledApplicationShell id="compiled-todos-root" ir={application} pathname={pathname} />
}

function matchesExpectedDom(root: Element, expected: ExpectedDom) {
  const row = root.querySelector<HTMLElement>(`[data-todo-id="${expected.id}"]`)
  if (expected.kind === 'remove') return row === null
  if (!row) return false
  if (expected.kind === 'retitle') return (row.querySelector<HTMLInputElement>('input:not([type="checkbox"])')?.value ?? row.querySelector('span')?.textContent) === expected.title
  if (expected.kind === 'toggle') {
    const status = row.querySelector('[data-variant]')?.textContent
    return (row.dataset.done === 'true') === expected.done && status === (expected.done ? 'Completed' : 'Open')
  }
  const list = row.parentElement
  return expected.edge === 'first' ? list?.firstElementChild === row : list?.lastElementChild === row
}

function firstRenderedRowKey() {
  const row = document.querySelector<HTMLElement>(`#compiled-todos-root [data-todo-id]:not([data-todo-id="${benchmarkRowId}"])`)
  if (!row?.dataset.todoId) throw new Error('Move requires a non-focus Todo row.')
  return row.dataset.todoId
}

function validateFullDom(root: Element, todos: Todo[]) {
  const actual = [...root.querySelectorAll<HTMLElement>('[data-todo-id]')].map((row) => row.dataset.todoId)
  return actual.length === todos.length && actual.every((id, index) => id === todos[index]?.id)
}

function compareTodos(left: Todo, right: Todo) {
  return left.sortOrder - right.sortOrder || left.id.localeCompare(right.id)
}
