import { useLiveQuery } from '@tanstack/react-db'
import { TodoListView } from '../components/TodoListView'

/** Visual-only route input; the React route controller owns data wiring. */
export default function CompiledTodosRouteApplication({ todosCollection }: { todosCollection: any }) {
  const { data: todos } = useLiveQuery(todosCollection)
  function updateTodo() {}
  return <div data-compiled-route-root><TodoListView todos={todos as any} onUpdate={updateTodo} /></div>
}
