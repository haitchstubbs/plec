import { createCollection, localOnlyCollectionOptions } from '@tanstack/db'

export interface Todo { id: string; title: string; done: boolean; sortOrder: number }

export function createTodoSeed(size: number): Todo[] {
  return Array.from({ length: size }, (_, index) => ({
  id: `${index + 1}`,
  title: `Todo ${index + 1}`,
  done: index % 2 === 0,
  sortOrder: index,
  }))
}

export function createTodoCollection(size = 50000) {
  return createCollection(localOnlyCollectionOptions<Todo, string>({ getKey: (todo) => todo.id, initialData: createTodoSeed(size) }))
}

// Compiler fixture import: the route itself always constructs an isolated collection.
export const todosCollection = createTodoCollection()
