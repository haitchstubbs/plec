import { Toggle } from "@wasm-runtime/internal-toggle";

// Compiler-only fixture. The import is deliberately synthetic: it proves the
// semantic adapter contract without creating a public authoring package.
export function ControlledTodoToggle({ todo, update }: any) {
  return (
    <Toggle.Root
      checked={todo.done}
      disabled={todo.locked}
      name="done"
      onCheckedChange={(next) => update(todo.id, next)}
      className="toggle"
    >
      <Toggle.Indicator className="indicator">✓</Toggle.Indicator>
    </Toggle.Root>
  );
}

export function MixedUncontrolledToggle() {
  return (
    <Toggle.Root defaultChecked indeterminate name="selection" required>
      <Toggle.Indicator>mixed</Toggle.Indicator>
    </Toggle.Root>
  );
}
