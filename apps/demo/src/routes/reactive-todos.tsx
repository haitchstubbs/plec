import { Profiler, useEffect, useMemo, useRef, type ProfilerOnRenderCallback } from 'react'
import { useLiveQuery } from '@tanstack/react-db'
import { createFileRoute } from '@tanstack/react-router'
import { createTodoCollection, type Todo } from '../compiler-fixtures/todo-data'
import { TodoListView } from '../components/TodoListView'
import { nextAnimationFrame, waitForDom } from '../benchmark-timing'

const benchmarkRowId = '1'
const benchmarkRowLabel = 'Todo 1'
type BenchmarkOperation = 'retitle' | 'toggle' | 'insert' | 'remove' | 'move'
type ExpectedDom = { kind: BenchmarkOperation; id: string; title?: string; done?: boolean; edge?: 'first' | 'last' }
type ReactProfile = { actualDuration: number; startTime: number }
type ActiveMeasurement = { apiEnd: number; profile: ReactProfile | null }

export const Route = createFileRoute('/reactive-todos')({ ssr: false, component: ReactiveTodosRoute })

function ReactiveTodosRoute() {
  const search = typeof window === 'undefined' ? new URLSearchParams() : new URLSearchParams(window.location.search)
  const benchmark = search.has('bench')
  const requestedSize = Number(search.get('size'))
  const size = benchmark && [10, 100, 1000, 10000, 50000].includes(requestedSize) ? requestedSize : 50000
  const todosCollection = useMemo(() => createTodoCollection(size), [size])
  const { data: todos } = useLiveQuery(todosCollection as any) as unknown as { data: Todo[] }
  const orderedTodos = useMemo(() => [...todos].sort((left, right) => left.sortOrder - right.sortOrder || left.id.localeCompare(right.id)), [todos])
  const sequence = useRef(0)
  const activeMeasurement = useRef<ActiveMeasurement | null>(null)

  function updateTodo(id: string, changes: Partial<Todo>) { todosCollection.update(id, (draft) => Object.assign(draft, changes)) }
  function planOperation(operation: BenchmarkOperation) {
    if (operation === 'retitle') { const title = `${benchmarkRowLabel} benchmark ${++sequence.current}`; return { expected: { kind: operation, id: benchmarkRowId, title } as ExpectedDom, run: () => updateTodo(benchmarkRowId, { title }) } }
    if (operation === 'toggle') { const done = !todosCollection.get(benchmarkRowId)?.done; return { expected: { kind: operation, id: benchmarkRowId, done } as ExpectedDom, run: () => updateTodo(benchmarkRowId, { done }) } }
    if (operation === 'insert') { const id = `bench-${++sequence.current}`; return { expected: { kind: operation, id } as ExpectedDom, run: () => todosCollection.insert({ id, title: `Benchmark ${sequence.current}`, done: false, sortOrder: todosCollection.toArray.length + sequence.current }) } }
    if (operation === 'remove') { const id = todosCollection.toArray.find((todo) => todo.id.startsWith('bench-'))?.id; if (!id) throw new Error('Remove requires a prepared benchmark row.'); return { expected: { kind: operation, id } as ExpectedDom, run: () => todosCollection.delete(id) } }
    const edge: 'first' | 'last' = ++sequence.current % 2 === 0 ? 'first' : 'last'
    const sortOrder = edge === 'first' ? -sequence.current : todosCollection.toArray.length + sequence.current
    return { expected: { kind: operation, id: benchmarkRowId, edge } as ExpectedDom, run: () => updateTodo(benchmarkRowId, { sortOrder }) }
  }
  function prepare(operation: BenchmarkOperation) { if (operation !== 'remove' || todosCollection.toArray.some((todo) => todo.id.startsWith('bench-'))) return; const id = `bench-${++sequence.current}`; todosCollection.insert({ id, title: `Benchmark ${sequence.current}`, done: false, sortOrder: todosCollection.toArray.length + sequence.current }) }
  const onRender: ProfilerOnRenderCallback = (_id, _phase, actualDuration, _baseDuration, startTime) => { if (activeMeasurement.current) activeMeasurement.current.profile = { actualDuration, startTime } }
  async function measure(operation: BenchmarkOperation) {
    const root = document.querySelector('#reactive-todos-root')
    if (!root) throw new Error('Missing React benchmark root.')
    const plan = planOperation(operation)
    const settled = waitForDom(root, () => matchesExpectedDom(root, plan.expected))
    const start = performance.now(); const active: ActiveMeasurement = { apiEnd: start, profile: null }; activeMeasurement.current = active
    plan.run(); active.apiEnd = performance.now(); await settled; const domCommit = performance.now(); await nextAnimationFrame(); const nextFrame = performance.now(); activeMeasurement.current = null
    return { mutationApiMs: active.apiEnd - start, frameworkPropagationMs: active.profile ? Math.max(0, active.profile.startTime - active.apiEnd) : null, reconciliationMs: active.profile?.actualDuration ?? null, domCommitMs: domCommit - start, adapterPropagationMs: null, wasmDomMs: null, nextAnimationFrameMs: nextFrame - start }
  }
  useEffect(() => {
    if (!benchmark) return
    const api = { prepare, measure, retitle: () => planOperation('retitle').run(), toggle: () => planOperation('toggle').run(), insert: () => planOperation('insert').run(), remove: () => planOperation('remove').run(), move: () => planOperation('move').run(), validate: () => { const root = document.querySelector('#reactive-todos-root'); return !!root && validateFullDom(root, orderedTodos) }, snapshot: () => ({ collectionRows: todosCollection.toArray.length, title: todosCollection.get(benchmarkRowId)?.title, done: todosCollection.get(benchmarkRowId)?.done, compiledRows: 0 }) }
    ;(window as any).__wasmRuntimeBenchmark = api
    return () => { delete (window as any).__wasmRuntimeBenchmark }
  }, [benchmark, orderedTodos, todosCollection])
  return <Profiler id="react-todos" onRender={onRender}><div id="reactive-todos-root"><TodoListView todos={orderedTodos} onUpdate={updateTodo} /></div></Profiler>
}

function matchesExpectedDom(root: Element, expected: ExpectedDom) { const row = root.querySelector<HTMLElement>(`[data-todo-id="${expected.id}"]`); if (expected.kind === 'remove') return row === null; if (!row) return false; if (expected.kind === 'retitle') return row.querySelector<HTMLInputElement>('input:not([type="checkbox"])')?.value === expected.title; if (expected.kind === 'toggle') return (row.dataset.done === 'true') === expected.done; const list = row.parentElement; return expected.edge === 'first' ? list?.firstElementChild === row : list?.lastElementChild === row }
function validateFullDom(root: Element, todos: Todo[]) { const actual = [...root.querySelectorAll<HTMLElement>('[data-todo-id]')].map((row) => row.dataset.todoId); return actual.length === todos.length && actual.every((id, index) => id === todos[index]?.id) }
