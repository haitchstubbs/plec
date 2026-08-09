import { useLiveQuery } from '@tanstack/react-db'
import { todosCollection, type Todo } from './todo-data'
import { TodoItem } from '../components/TodoItem'

function CompiledTodosFixture() {
  // The fixture is compiled syntax-first; the cast keeps duplicated workspace
  // package identities from polluting the ordinary Todo shape.
  const { data: todos } = useLiveQuery(todosCollection as any) as unknown as { data: Todo[] }
  function updateTodo(id: string, changes: Partial<Todo>) { todosCollection.update(id, (draft) => Object.assign(draft, changes)) }

  return (
    <main className="todo-runtime-shell">
      <section className="todo-runtime-card">
        <p className="todo-runtime-kicker">Compiled Todo Runtime</p>
        <h1 className="todo-runtime-title">Direct delta updates on a todo list</h1>
        <p className="todo-runtime-copy">
          This fixture is the compiled workload for the runtime experiment. The route shell drives live TanStack DB mutations and sends targeted updates to the mounted DOM.
        </p>
        <ul className="todo-runtime-list">
          {todos.map((todo) => <TodoItem key={todo.id} todo={todo} onUpdate={updateTodo} />)}
        </ul>
      </section>
    </main>
  )
}

export default CompiledTodosFixture
