import { describe, expect, it } from 'vitest';
import { compile, compileComponentGraph } from './index.ts';

describe('compile executable application', () => {
  it('writes static router link attributes', () => {
    const result = compile(
      `import { Link } from '@tanstack/react-router'; function App() { return <Link to="/about" className="nav">About</Link> }`,
      { mode: 'strict' },
    );
    expect(result.ir.propPrograms).toContainEqual(
      expect.objectContaining({
        writes: expect.arrayContaining([
          expect.objectContaining({ name: result.ir.strings.indexOf('href') }),
          expect.objectContaining({ name: result.ir.strings.indexOf('className') }),
        ]),
      }),
    );
  });

  it('lowers event state updates into numeric action and event tables', () => {
    const result = compile(`function App() { const [title, setTitle] = useState(''); return <input value={title} onInput={(event) => setTitle(event.currentTarget.value)} /> }`, { mode: 'strict' });
    expect(result.ir.events).toHaveLength(1);
    expect(result.ir.actions).toHaveLength(1);
    expect(result.ir.events[0]).toMatchObject({ action: 0, fields: expect.any(Array) });
    expect(result.ir.actions[0]?.instructions.some((instruction: any) => instruction.op === 'storeState')).toBe(true);
  });

  it('preserves array spreads in typed action expressions', () => {
    const result = compile(
      `function App() { const [items, setItems] = useState([]); return <button onClick={() => setItems((current) => [...current, 'next'])} /> }`,
      { mode: 'strict' },
    );
    const expression = result.ir.actions[0]?.instructions
      .filter((instruction: any) => instruction.op === 'evaluate')
      .map((instruction: any) => result.ir.expressions[instruction.expression])
      .find((entry) => entry.instructions.some((instruction: any) => instruction.op === 'makeArray'));
    expect(expression?.instructions).toContainEqual({
      op: 'makeArray', count: 2, spreads: [true, false],
    });
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

  it('declares cookie authority and separates sync input from async writes', () => {
    const result = compile(`import { cookie, useState } from 'plec'; function App() { const [open, setOpen] = useState(cookie.getSync('sidebar') === 'open'); return <button onClick={() => { void cookie.set('sidebar', open ? 'closed' : 'open', { path: '/', maxAge: 1 }); setOpen(!open); }} /> }`, { mode: 'strict' });
    expect(result.ir.hostSlots).toEqual([{ kind: 'cookie', name: expect.any(Number) }]);
    expect(result.ir.capabilities).toEqual([{ kind: 'cookie', name: 'sidebar', operations: ['getSync', 'set'], path: '/', expiryModes: ['session', 'maxAge'] }]);
    expect(result.ir.actions[0]?.instructions[0]).toMatchObject({ op: 'capabilityRequest', capability: 'cookie' });
  });

  it('keeps component graph cookie authority', () => {
    const graph = compileComponentGraph(
      `import { cookie } from 'plec'; function App() { return <button onClick={() => cookie.set('sidebar', 'closed')} /> }`,
      { mode: 'strict' },
    ).graph;
    expect(graph.capabilities).toEqual([
      expect.objectContaining({ name: 'sidebar', operations: ['set'] }),
    ]);
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

  it('keeps a conditional loop parent and its state dependency typed', () => {
    const result = compile(
      `function App() { const [items, setItems] = useState([]); return <main>{items.length ? <ul>{items.map((item) => <li key={item.id}>{item.id}</li>)}</ul> : <p>Empty</p>}</main> }`,
      { mode: 'strict' },
    );
    const conditional = result.ir.nodes.findIndex((node) => node.op === 'conditional');
    const loop = result.ir.nodes.findIndex((node) => node.op === 'loop');
    expect(conditional).toBeGreaterThanOrEqual(0);
    expect(result.ir.nodes[loop]).toMatchObject({ parent: expect.any(Number) });
    expect(result.ir.dependencyEdges).toContainEqual({
      source: { kind: 'state', handle: 0 },
      target: { kind: 'conditional', handle: conditional },
    });
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

  it('keeps derived collection transforms out of typed action programs', () => {
    const result = compile(
      `function App() {
        const [todos, setTodos] = useState([{ id: 'a', title: 'Alpha' }]);
        const [search, setSearch] = useState('');
        const filtered = todos.filter((todo) => todo.title.includes(search));
        return <div><input onInput={(event) => setSearch(event.currentTarget.value)} />{filtered.map((todo) => <span key={todo.id}>{todo.title}</span>)}</div>;
      }`,
      { mode: 'strict' },
    );
    expect(result.ir.loops).toHaveLength(1);
    expect(result.ir.dependencyEdges).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ source: { kind: 'state', handle: 0 } }),
        expect.objectContaining({ source: { kind: 'state', handle: 1 } }),
      ]),
    );
    const actionText = JSON.stringify(result.ir.actions);
    expect(actionText).not.toMatch(/todos|search|filter|map/);
    expect(result.ir.actions[0]?.instructions).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ op: 'storeState', state: 1 }),
      ]),
    );
  });

  it('emits flattened typed action control flow and omits unsupported lenient handlers', () => {
    const typed = compile(
      `function App() { const [open, setOpen] = useState(false); return <button onClick={() => { if (open) setOpen(false); else setOpen(true); }} /> }`,
      { mode: 'strict' },
    );
    const instructions = typed.ir.actions[0]!.instructions as any[];
    expect(instructions.map((instruction) => instruction.op)).toEqual(
      expect.arrayContaining(['evaluate', 'jumpIfFalse', 'jump', 'storeState']),
    );
    expect(instructions.every((instruction) => !('kind' in instruction))).toBe(true);

    const lenient = compile(
      `function App() { return <button onClick={() => missingAction} /> }`,
      { mode: 'lenient' },
    );
    expect(lenient.diagnostics.map((diagnostic) => diagnostic.code)).toContain(
      'UNSUPPORTED_ACTION_EXPRESSION',
    );
    expect(lenient.ir.events).toEqual([]);
    expect(lenient.ir.actions).toEqual([]);
  });

  it('assigns deterministic continuation slots and PCs for supported fetch helpers', () => {
    const result = compile(
      `function App() {
        const [pending, setPending] = useState(false);
        const [error, setError] = useState('');
        async function request(action) {
          setPending(true);
          try { await action(); }
          catch (reason) { setError('failed'); }
          finally { setPending(false); }
        }
        async function save() { await request(() => fetch('/api/save', { method: 'POST' })); }
        return <button onClick={save} />;
      }`,
      { mode: 'strict' },
    );
    const action = result.ir.actions[0]!;
    const request = action.instructions.find(
      (instruction: any) => instruction.op === 'capabilityRequest',
    ) as any;
    expect(request).toMatchObject({ capability: 'fetch', resultSlot: 0, errorSlot: 1 });
    expect(request.successPc).toBeGreaterThan(0);
    expect(request.failurePc).toBeGreaterThan(request.successPc);
    expect(request.finallyPc).toBeGreaterThan(request.failurePc);
    expect(action.frameSlots).toBe(2);
  });

  it('binds a decoded fetch result after a request helper guard', () => {
    const result = compile(`function App() {
      const [todos, setTodos] = useState([]);
      async function request(action) {
        try {
          const response = await action();
          if (!response.ok) throw new Error('failed');
          return response;
        } catch { return undefined; }
      }
      async function save() {
        const response = await request(() => fetch('/api/save'));
        if (!response) return;
        const todo = await response.json();
        setTodos((items) => items.map((item) => item.id === todo.id ? todo : item));
      }
      return <div><button onClick={save} /><ul>{todos.map((todo) => <li key={todo.id}>{todo.title}</li>)}</ul></div>;
    }`, { mode: 'strict' });
    const action: any = result.ir.actions[0];
    const request = action.instructions.findIndex(
      (instruction: any) => instruction.op === 'capabilityRequest',
    );
    const update = action.instructions
      .slice(request + 1)
      .find((instruction: any) => instruction.op === 'evaluate');
    const expression: any = result.ir.expressions[update.expression];
    const mapper: any = result.ir.expressions[
      expression.instructions.find((instruction: any) => instruction.op === 'map').mapper
    ];
    expect(mapper.instructions.filter((instruction: any) => instruction.op === 'loadFrame')).toHaveLength(2);
  });

  it('keeps a successful empty fetch continuation after a response guard', () => {
    const result = compile(`function App() {
      const [todos, setTodos] = useState([]);
      async function request(action) {
        try { const response = await action(); if (!response.ok) throw new Error('failed'); return response; }
        catch { return undefined; }
      }
      async function remove() {
        const response = await request(() => fetch('/api/todos/a', { method: 'DELETE' }));
        if (response) setTodos((items) => items.filter((item) => item.id !== 'a'));
      }
      return <button onClick={remove} />;
    }`, { mode: 'strict' });
    const action: any = result.ir.actions[0];
    const request = action.instructions.find(
      (instruction: any) => instruction.op === 'capabilityRequest',
    );
    expect(action.instructions[request.successPc + 1]).toMatchObject({ op: 'storeState' });
  });

  it('keeps an HTTP rejection message when the catch variable shadows state', () => {
    const result = compile(`function App() {
      const [error, setError] = useState();
      async function request(action) {
        try { const response = await action(); if (!response.ok) throw new Error('rejected'); }
        catch (error) { setError(error.message); }
      }
      async function save() { await request(() => fetch('/api/save')); }
      return <button onClick={save} />;
    }`, { mode: 'strict' });
    const text = JSON.stringify(result.ir);
    expect(text).toContain('rejected');
  });

  it('selects typed fetch decoders from the response method', () => {
    const result = compile(`function App() {
      async function text() { const response = await fetch('/text'); const value = await response.text(); }
      async function empty() { await fetch('/empty'); }
      return <div><button onClick={text} /><button onClick={empty} /></div>;
    }`, { mode: 'lenient' });
    const requests = result.ir.actions.flatMap((action: any) => action.instructions)
      .filter((instruction: any) => instruction.op === 'capabilityRequest');
    expect(requests.map((request: any) => request.request.decode)).toEqual(['text', 'json']);
  });

  it('reserves loader params and location frame slots while leaving signal runtime-owned', () => {
    const result = compile(`function App() {
      async function load({ params, location, signal }) {
        await fetch('/api/' + params.id + location.search, { signal });
      }
      return <button onClick={load} />;
    }`, { mode: 'strict' });
    const action = result.ir.actions[0]!;
    expect(action).toMatchObject({ parameterSlots: [0, 1] });
    expect(action.frameSlots).toBeGreaterThanOrEqual(2);
    expect(JSON.stringify(action.instructions)).not.toContain('signal');
  });

  it('continues to reject unsupported source in strict mode', () => {
    expect(() =>
      compile(`function App() { return <><span /></> }`, {
        mode: 'strict',
      }),
    ).toThrow(/Strict compilation failed/);
  });
});
