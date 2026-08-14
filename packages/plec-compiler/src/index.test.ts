import { describe, expect, it } from 'vitest';
import { compile } from './index.ts';

describe('compile executable application', () => {
  it('lowers event state updates into numeric action and event tables', () => {
    const result = compile(`function App() { const [title, setTitle] = useState(''); return <input value={title} onInput={(event) => setTitle(event.currentTarget.value)} /> }`, { mode: 'strict' });
    expect(result.ir.events).toHaveLength(1);
    expect(result.ir.actions).toHaveLength(1);
    expect(result.ir.events[0]).toMatchObject({ action: 0, fields: expect.any(Array) });
    expect(result.ir.actions[0]?.instructions.some((instruction: any) => instruction.op === 'storeState')).toBe(true);
  });
  it('emits deterministic dense 0.9 tables for static and bound text', () => {
    const source = `function Hello({ name }) { return <div title={name}>Hello {name}</div> }`;
    const first = compile(source, { mode: 'strict' });
    expect(first).toEqual(compile(source, { mode: 'strict' }));
    expect(first.ir).toMatchObject({ version: '0.9', rootNode: 0 });
    expect(first.ir.strings).toEqual(['div', 'title', 'name']);
    expect(first.ir.nodes).toHaveLength(3);
    expect(first.ir.bindings).toHaveLength(2);
    expect(
      first.ir.expressions.every(
        (entry) => entry.instructions.at(-1)?.op === 'return',
      ),
    ).toBe(true);
  });

  it('lowers local state, conditionals, and host reads into typed programs', () => {
    const result = compile(
      `function App() { const [open, setOpen] = useState(true); const year = new Date().getFullYear(); return <div title={year}>{open ? 'open' : 'closed'}</div> }`,
      { mode: 'strict' },
    );
    expect(result.ir.stateSlots).toHaveLength(1);
    expect(result.ir.hostSlots).toEqual([{ kind: 'currentYear' }]);
    expect(
      result.ir.expressions.every((entry) => !('expression' in entry)),
    ).toBe(true);
    expect(
      result.ir.expressions.some((entry) =>
        entry.instructions.some(
          (instruction) => instruction.op === 'jumpIfFalse',
        ),
      ),
    ).toBe(true);
  });

  it('keeps a mapped collection as a typed loop table entry', () => {
    const result = compile(
      `function App({ todos }) { return <ul>{todos.map((todo) => <li key={todo.id}>{todo.title}</li>)}</ul> }`,
      { mode: 'strict' },
    );
    expect(result.ir.loops).toHaveLength(1);
    expect(result.ir.nodes.some((node) => node.op === 'loop')).toBe(
      true,
    );
  });

  it('compiles local derived keyed loops with slot and row-field edges', () => {
    const result = compile(
      `function App() {
      const [todos, setTodos] = useState([{ id: 'a', title: 'A', done: false }]);
      const [search, setSearch] = useState('');
      return <ul>{todos.filter((todo) => todo.title.toLowerCase().includes(search)).map((todo, index) => <li key={todo.id}>{index}: {todo.title}</li>)}</ul>;
    }`,
      { mode: 'strict' },
    );
    const loop = result.ir.loops[0]!;
    expect(loop.input).toBeUndefined();
    expect(loop.dependencySlots).toEqual([0, 1]);
    expect(result.ir.dependencyEdges).toContainEqual({
      source: { kind: 'state', handle: 0 },
      target: { kind: 'loop', handle: 0 },
    });
    expect(result.ir.dependencyEdges).toContainEqual(
      expect.objectContaining({
        source: {
          kind: 'rowField',
          handle: result.ir.strings.indexOf('title'),
          loop: 0,
        },
        target: expect.objectContaining({ kind: 'binding' }),
      }),
    );
  });

  it('continues to reject unsupported source in strict mode', () => {
    expect(() =>
      compile(`function App() { return <><span /></> }`, {
        mode: 'strict',
      }),
    ).toThrow(/Strict compilation failed/);
  });
});
