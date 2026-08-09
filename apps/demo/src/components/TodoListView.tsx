import { Badge } from '@wasm-runtime/ui/atoms/badge'
import { Card, CardContent, CardHeader, CardTitle } from '@wasm-runtime/ui/atoms/card'
import { Checkbox } from '@wasm-runtime/ui/atoms/checkbox'
import { Input } from '@wasm-runtime/ui/atoms/input'
import type { Todo } from '../compiler-fixtures/todo-data'

export function TodoListView({ todos, onUpdate }: { todos: Todo[]; onUpdate: (id: string, changes: Partial<Todo>) => void }) {
  return <main className="page-wrap px-4 py-10"><Card><CardHeader><p className="island-kicker">Todo workload</p><CardTitle>Renderer parity Todo list</CardTitle></CardHeader><CardContent><ul id="todos-root" className="space-y-3">{todos.map((todo) => <li key={todo.id} data-todo-id={todo.id} data-done={todo.done}><Card size="sm"><CardContent className="flex items-center gap-3"><Checkbox checked={todo.done} onClick={() => onUpdate(todo.id, { done: !todo.done })} aria-label={`Toggle ${todo.title}`} /><Input value={todo.title} onChange={(event) => onUpdate(todo.id, { title: event.currentTarget.value })} /><Badge variant={todo.done ? 'secondary' : 'outline'}>{todo.done ? 'Completed' : 'Open'}</Badge></CardContent></Card></li>)}</ul></CardContent></Card></main>
}
