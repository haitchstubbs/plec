import { compile } from './packages/compiler/dist/index.js'

const source = `
const todoSeed = Array.from({ length: 3 }, (_, index) => ({
  id: 'todo-' + (index + 1),
  title: 'Todo ' + (index + 1),
  done: index % 2 === 0,
}))

function BenchmarkTodos() {
  const todos = todoSeed
  return (
    <ul>
      {todos.map((todo) => (
        <li key={todo.id} data-done={todo.done}>
          <span>{todo.title}</span>
        </li>
      ))}
    </ul>
  )
}
`

const result = compile(source, { mode: 'lenient' })
console.log(JSON.stringify({
  loops: result.ir.loops.length,
  bindings: result.ir.bindings.length,
  diagnostics: result.diagnostics.map((diagnostic) => diagnostic.code),
}, null, 2))
