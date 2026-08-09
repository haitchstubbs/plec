import type { Todo } from '../compiler-fixtures/todo-data'
import { Checkbox } from '@wasm-runtime/ui/atoms/checkbox'
import { Input } from '@wasm-runtime/ui/atoms/input'

export function TodoStatus({ todo }: { todo: Todo }) {
  return <p className={todo.done ? 'text-emerald-700 font-medium' : 'text-amber-700 font-medium'} data-todo-status>{todo.done ? 'Completed' : 'Open'}</p>
}

/** Ordinary controlled React component. The compiler expands it as static JSX. */
export function TodoItem({ todo, onUpdate }: { todo: Todo; onUpdate: (id: string, changes: Partial<Todo>) => void }) {
  const rowClasses = todo.done ? 'rounded-xl border border-emerald-200 bg-emerald-50 p-3' : 'rounded-xl border border-amber-200 bg-amber-50 p-3'
  return <article className={rowClasses} data-todo-id={todo.id} data-done={todo.done}>
    <label className="flex items-center gap-3">
      <Checkbox checked={todo.done} onCheckedChange={(checked) => onUpdate(todo.id, { done: checked })} aria-label={`Toggle ${todo.title}`} />
      <Input className="min-w-0 flex-1 rounded border px-2 py-1" value={todo.title} onChange={(event) => onUpdate(todo.id, { title: event.currentTarget.value })} />
    </label>
    <TodoStatus todo={todo} />
  </article>
}
