import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
// Vitest needs the source module rather than the checked-in JavaScript artefact.
// @ts-expect-error TypeScript's Node resolution intentionally disallows this suffix.
import { compile } from "./index.ts";

describe("compile", () => {
  it("compiles Hello component to deterministic IR", () => {
    const source = `
      function Hello({ name }) {
        return <div>Hello {name}</div>
      }
    `;

    const result = compile(source, { mode: "lenient" });

    expect(result.diagnostics).toEqual([]);
    expect(result.ir).toMatchObject({ version: "0.5", rootElementId: "e1" });
    expect(result.ir.bindings[0]).toMatchObject({ id: "b1", expressionId: "x1", expression: "name" });
    expect(result.ir.expressions).toEqual([{ id: "x1", expression: { kind: "identifier", name: "name" } }]);
  });

  it("reports unsupported fragments in lenient mode", () => {
    const source = `
      function Example() {
        return <><span>Hi</span></>
      }
    `;

    const result = compile(source, { mode: "lenient" });

    expect(result.diagnostics).toMatchInlineSnapshot(`
      [
        {
          "code": "UNSUPPORTED_JSX_NODE",
          "message": "Unsupported JSX node type: JSXFragment.",
          "severity": "error",
        },
        {
          "code": "INVALID_ROOT",
          "message": "The root JSX node could not be lowered to an intrinsic element.",
          "severity": "error",
        },
      ]
    `);
  });

  it("fails in strict mode when unsupported syntax is present", () => {
    const source = `
      function Example() {
        return <><span>Hi</span></>
      }
    `;

    expect(() => compile(source, { mode: "strict" })).toThrowError(
      /Strict compilation failed/
    );
  });

  it("compiles a simple list map into explicit loop ir", () => {
    const source = `
      function FeatureList() {
        const cards = [
          ['Type-Safe Routing', 'Routes and links stay in sync across every page.'],
          ['Server Functions', 'Call server code from your UI without creating API boilerplate.']
        ]

        return (
          <section>
            {cards.map(([title, description]) => (
              <article key={title}>
                <h2>{title}</h2>
                <p>{description}</p>
              </article>
            ))}
          </section>
        )
      }
    `;

    const result = compile(source, { mode: "lenient" });

    expect(result.diagnostics).toEqual([]);
    expect({
      rootElementId: result.ir.rootElementId,
      loopCount: result.ir.loops.length,
      rowCount: result.ir.loops[0]?.rows.length,
      elementCount: result.ir.elements.length,
      textCount: result.ir.texts.length,
      bindingCount: result.ir.bindings.length
    }).toMatchInlineSnapshot(`
      {
        "bindingCount": 0,
        "elementCount": 7,
        "loopCount": 1,
        "rootElementId": "e1",
        "rowCount": 2,
        "textCount": 4,
      }
    `);
  });

  it("normalizes published React jsx factory calls without package adapters", () => {
    const result = compile(`function App() { return _jsxs("label", { className: "field", children: [_jsx("input", { type: "checkbox", checked: true }), _jsx("span", { children: "Ready" })] }); }`, { rootComponent: "App" });
    expect(result.diagnostics).toEqual([]);
    expect(result.ir.elements).toEqual(expect.arrayContaining([
      expect.objectContaining({ tag: "label" }),
      expect.objectContaining({ tag: "input", attributes: expect.arrayContaining([expect.objectContaining({ name: "type", staticValue: "checkbox" })]) }),
      expect.objectContaining({ tag: "span" }),
    ]));
    expect(result.ir.texts).toEqual(expect.arrayContaining([expect.objectContaining({ staticValue: "Ready" })]));
  });

  it("normalizes statically tagged createElement calls", () => {
    const result = compile(`function App() { return React.createElement('button', { type: 'button' }, 'Save') }`, { rootComponent: "App" });
    expect(result.diagnostics).toEqual([]);
    expect(result.ir.elements[0]).toMatchObject({ tag: "button", attributes: [{ name: "type", staticValue: "button" }] });
    expect(result.ir.texts[0]).toMatchObject({ staticValue: "Save" });
  });

  it("expands nested object spreads and ordinary object merges before lowering props", () => {
    const result = compile(`
      function Surface({ children, ...rest }) {
        const base = { role: 'status', className: cn('surface', rest.className) }
        const forwarded = { ...base, ...Object.assign({}, rest), children }
        return <span {...forwarded} />
      }
      function App({ label }) { return <Surface className="caller" aria-label={label}>Ready</Surface> }
    `, { rootComponent: "App" });
    expect(result.diagnostics).toEqual([]);
    expect(result.ir.elements[0]).toMatchObject({ tag: "span", attributes: expect.arrayContaining([
      expect.objectContaining({ name: "role", staticValue: "status" }),
      expect.objectContaining({ name: "className", staticValue: "surface" }),
      expect.objectContaining({ name: "className", staticValue: "caller" }),
      expect.objectContaining({ name: "aria-label", bindingId: expect.any(String) }),
    ]) });
  });

  it("lowers optional members, unary operators, and memoized expression values", () => {
    const result = compile(`
      function App({ todo }) {
        const label = useMemo(() => todo?.title ?? 'Untitled', [todo])
        return <output data-open={!todo.done} data-rank={-todo.rank}>{label}</output>
      }
    `, { rootComponent: "App" });
    expect(result.diagnostics).toEqual([]);
    expect(result.ir.expressions.map((entry) => entry.expression)).toEqual(expect.arrayContaining([
      expect.objectContaining({ kind: "unary", op: "!" }),
      expect.objectContaining({ kind: "unary", op: "-" }),
      expect.objectContaining({ kind: "logical", op: "??" }),
    ]));
  });

  it("inlines reachable expression-only custom hooks without symbol adapters", () => {
    const result = compile(`
      function useLabel(todo) { const suffix = todo.done ? 'done' : 'open'; return todo.title + ' (' + suffix + ')' }
      function App({ todo }) { return <output>{useLabel(todo)}</output> }
    `, { rootComponent: "App" });
    expect(result.diagnostics).toEqual([]);
    expect(result.ir.expressions[0]?.expression).toMatchObject({ kind: "binary", op: "+" });
  });

  it("lowers lexical createContext providers and custom-hook reads without component identities", () => {
    const result = compile(`
      const Theme = createContext({ tone: 'default' });
      function useTheme() { return useContext(Theme) }
      function Label() { const theme = useTheme(); return <span>{theme.tone}</span> }
      function App() { return <Theme.Provider value={{ tone: 'provided' }}><div><Label /></div></Theme.Provider> }
    `, { rootComponent: "App" });
    expect(result.diagnostics).toEqual([]);
    expect(result.ir.rootElementId).toBe("c1");
    expect(result.ir.contexts[0]).toMatchObject({ contextId: "ctx1", children: ["e1"] });
    expect(JSON.stringify(result.ir.expressions)).toContain('"context"');
  });

  it("lowers an ordinary element-returning helper through its source body", () => {
    const result = compile(`
      function renderLabel(name) { return <strong data-name={name}>Hello {name}</strong> }
      function App() { return <div>{renderLabel('Ada')}</div> }
    `, { rootComponent: "App" });
    expect(result.diagnostics).toEqual([]);
    expect(result.ir.elements.map((element) => element.tag)).toEqual(["div", "strong"]);
    expect(result.ir.texts).toEqual(expect.arrayContaining([expect.objectContaining({ staticValue: "Hello" }), expect.objectContaining({ staticValue: "Ada" })]));
  });

  it("keeps a dynamic record spread as an ordered runtime prop write", () => {
    const result = compile(`function App({ todo }) { return <input {...todo} /> }`, { rootComponent: "App" });
    expect(result.diagnostics).toEqual([]);
    expect(result.ir.propPrograms[0]?.writes).toEqual([expect.objectContaining({ kind: "spread", expressionId: expect.any(String) })]);
  });

  it("lowers generic ordered prop composition without recognizing its importer", () => {
    const result = compile(`function App({ base, override }) { return <div {...mergeProps(base, override)} /> }`, { rootComponent: "App" });
    expect(result.diagnostics).toEqual([]);
    const expression = result.ir.expressions.find((entry) => (entry.expression as any).kind === "object");
    expect((expression?.expression as any).properties).toEqual([
      { kind: "spread", value: { kind: "identifier", name: "base" } },
      { kind: "spread", value: { kind: "identifier", name: "override" } },
    ]);
  });

  it("describes scalar and object inputs by the paths the view reads", () => {
    const result = compile(`function Profile({ selectedId, user }) { return <div data-selected={selectedId}>{user.name}</div> }`);
    expect(result.ir.inputs).toEqual([
      { id: "i1", name: "selectedId", shape: { kind: "scalar" } },
      { id: "i2", name: "user", shape: { kind: "object", observedPaths: [["name"]] } }
    ]);
    expect(result.ir.dependencyEdges.filter((edge) => edge.kind === "input-to-binding")).toHaveLength(2);
  });

  it("keeps an opaque hook result as one input-backed row template", () => {
    const result = compile(`
      function Todos() {
        const { data: todos } = useLiveQuery((q) => q.from({ todos: todoCollection }))
        return <ul>{todos.map((todo) => <li key={todo.id} data-done={todo.done}><span>{todo.title}</span></li>)}</ul>
      }
    `);
    expect(result.diagnostics).toEqual([]);
    expect(result.ir.loops[0]).toMatchObject({ inputId: "i1", rowTemplateRootElementId: "e2", keyExpression: "todo.id", rows: [] });
    expect(result.ir.inputs).toEqual([{ id: "i1", name: "todos", shape: { kind: "collection", keyExpression: "todo.id", orderSensitive: true, observedRowPaths: [["done"], ["id"], ["title"]] } }]);
    expect(result.ir.elements).toHaveLength(3);
  });

  it("evaluates Array.from seeds for large benchmark fixtures", () => {
    const source = `
      function BenchmarkTodos() {
        const todos = Array.from({ length: 3 }, (_, index) => ({
          id: \`todo-\${index + 1}\`,
          title: \`Todo \${index + 1}\`,
          done: index % 2 === 0,
        }))

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
    `;

    const result = compile(source, { mode: "lenient" });

    expect(result.diagnostics).toEqual([]);
    expect({
      rowCount: result.ir.loops[0]?.rows.length,
      firstRowId: result.ir.loops[0]?.rows[0]?.id,
      elementCount: result.ir.elements.length,
      textCount: result.ir.texts.length
    }).toMatchInlineSnapshot(`
      {
        "elementCount": 7,
        "firstRowId": "r1",
        "rowCount": 3,
        "textCount": 3,
      }
    `);
  });

  it("expands imported components with provenance, a neutral Link, and an island", () => {
    const source = `import Header from './Header'; import Footer from './Footer'; function App() { return <div><Header /><Footer /></div> }`;
    const result = compile(source, {
      moduleId: 'src/App.tsx',
      modules: [
        { id: 'src/App.tsx', source },
        { id: 'src/Header.tsx', source: `import { Link } from '@tanstack/react-router'; import ThemeToggle from './ThemeToggle'; export default function Header() { return <header><Link to="/about">About</Link><ThemeToggle /></header> }` },
        { id: 'src/Footer.tsx', source: `export default function Footer() { const year = new Date().getFullYear(); return <footer>{year}</footer> }` },
        { id: 'src/ThemeToggle.tsx', source: `export default function ThemeToggle() { return <button>Theme</button> }` },
      ],
      islandComponents: ['ThemeToggle'],
    });
    expect(result.diagnostics).toEqual([]);
    expect(result.ir.components).toEqual(expect.arrayContaining([
      expect.objectContaining({ name: 'Header', moduleId: 'src/Header.tsx' }),
      expect.objectContaining({ name: 'Footer', moduleId: 'src/Footer.tsx' }),
    ]));
    expect(result.ir.events[0]).toMatchObject({ navigate: { href: '/about' } });
    expect(result.ir.islands[0]).toMatchObject({ islandInstanceId: 'i1', componentId: 'ThemeToggle', moduleId: 'src/ThemeToggle.tsx' });
    expect((result.ir.expressions[0] as any).expression).toEqual({ kind: 'host', name: 'currentYear' });
  });

  it("expands nested components with isolated props and expression bindings", () => {
    const result = compile(`
      function TodoStatus({ todo }) { return <p className={todo.done ? 'done' : 'open'}>{todo.done ? 'Done' : 'Open'}</p> }
      function TodoItem({ todo, onUpdate }) { return <article data-todo-id={todo.id}><input checked={todo.done} onChange={(event) => onUpdate(todo.id, { done: event.currentTarget.checked })} /><TodoStatus todo={todo} /></article> }
      function TodoList() { const { data: todos } = useLiveQuery(source); function updateTodo() {} return <ul>{todos.map((todo) => <TodoItem key={todo.id} todo={todo} onUpdate={updateTodo} />)}</ul> }
    `, { rootComponent: "TodoList" });
    expect(result.diagnostics).toEqual([]);
    expect(result.ir.components.map((component) => component.name)).toEqual(["TodoItem", "TodoStatus"]);
    expect(result.ir.expressions.some((expression) => (expression.expression as any).kind === "conditional")).toBe(true);
    expect((result.ir.events[0] as any).callbackName).toBe("updateTodo");
    expect(result.ir.loops[0]).toMatchObject({ inputId: "i1", rowTemplateRootElementId: "e2" });
  });

  it.skip("lowers the real ThemeToggle into O1 state and host primitives without an island", async () => {
    const themeSource = await readFile(path.resolve(__dirname, "../../../apps/demo/src/components/ThemeToggle.tsx"), "utf8");
    const buttonSource = await readFile(path.resolve(__dirname, "../../ui/src/components/button.tsx"), "utf8");
    const source = `import Header from './Header'; function App() { return <div><Header /></div> }`;
    const headerSource = await readFile(path.resolve(__dirname, "../../../apps/demo/src/components/Header.tsx"), "utf8");
    const result = compile(source, { moduleId: "src/App.tsx", modules: [
      { id: "src/App.tsx", source },
      { id: "src/Header.tsx", source: headerSource },
      { id: "src/ThemeToggle.tsx", source: themeSource },
      { id: "@wasm-runtime/ui/atoms/button", source: buttonSource },
    ] });
    expect(result.diagnostics).toEqual([]);
    expect(result.ir.islands).toHaveLength(0);
    expect(result.ir.localStates).toEqual([{ id: "s1", name: "mode", initialValue: "auto", values: ["light", "dark", "auto"] }]);
    expect(result.ir.hostValues).toEqual([{ id: "h1", kind: "media-query", query: "(prefers-color-scheme: dark)" }]);
    expect(result.ir.lifecycleEffects).toHaveLength(2);
    expect(result.ir.stateTransitions).toEqual([expect.objectContaining({ stateSlotId: "s1", kind: "theme-cycle" })]);
    expect(result.ir.dependencyEdges.filter((edge) => edge.kind === "local-state-to-binding").length).toBeGreaterThanOrEqual(2);
    const transition = result.ir.stateTransitions[0]!;
    const event = result.ir.events.find((candidate) => candidate.id === transition.eventId);
    const button = result.ir.elements.find((candidate) => candidate.id === event?.targetId);
    expect(button).toMatchObject({ tag: "button" });
    expect(button?.attributes).toContainEqual(expect.objectContaining({ name: "data-slot", staticValue: "button" }));
    expect(button?.attributes).toContainEqual(expect.objectContaining({ name: "className", staticValue: expect.stringContaining("bg-primary") }));
  });

  it.skip("hard-errors unsupported hooks in ThemeToggle", async () => {
    const themeSource = (await readFile(path.resolve(__dirname, "../../../apps/demo/src/components/ThemeToggle.tsx"), "utf8"))
      .replace("const [mode, setMode] = useState<ThemeMode>('auto')", "useMemo(() => mode, [])\n  const [mode, setMode] = useState<ThemeMode>('auto')");
    const source = `import Header from './Header'; function App() { return <div><Header /></div> }`;
    const headerSource = await readFile(path.resolve(__dirname, "../../../apps/demo/src/components/Header.tsx"), "utf8");
    const buttonSource = await readFile(path.resolve(__dirname, "../../ui/src/components/button.tsx"), "utf8");
    expect(() => compile(source, { mode: "strict", moduleId: "src/App.tsx", modules: [{ id: "src/App.tsx", source }, { id: "src/Header.tsx", source: headerSource }, { id: "src/ThemeToggle.tsx", source: themeSource }, { id: "@wasm-runtime/ui/atoms/button", source: buttonSource }] })).toThrow(/UNSUPPORTED_HOOK/);
  });

  it("normalizes defaults, rest props, classes, children, and forwarded events", () => {
    const result = compile(`
      function Frame({ className, label = 'default', children, ...rest }) {
        return <section className={cn('frame', className)} data-label={label} {...rest}>{children}</section>
      }
      function App() { function save() {} return <Frame className="caller" onClick={() => save()} data-x="x">Hello</Frame> }
    `, { rootComponent: "App" });
    expect(result.diagnostics).toEqual([]);
    expect(result.ir.elements[0]).toMatchObject({ tag: "section" });
    expect(result.ir.elements[0]?.attributes).toEqual(expect.arrayContaining([
      expect.objectContaining({ name: "className", staticValue: "frame caller" }),
      expect.objectContaining({ name: "data-label" }),
      expect.objectContaining({ name: "data-x", staticValue: "x" }),
    ]));
    expect(result.ir.events[0]).toMatchObject({ type: "click" });
    expect(result.ir.texts[0]).toMatchObject({ staticValue: "Hello" });
  });

  it("preserves children forwarded only through a rest-props spread", () => {
    const result = compile(`
      function Frame({ ...props }) { return <section {...props} /> }
      function App() { return <Frame><strong>Kept</strong></Frame> }
    `, { rootComponent: "App" });
    expect(result.diagnostics).toEqual([]);
    expect(result.ir.elements).toEqual(expect.arrayContaining([
      expect.objectContaining({ tag: "section", children: ["e2"] }),
      expect.objectContaining({ id: "e2", tag: "strong" }),
    ]));
    expect(result.ir.texts).toEqual([expect.objectContaining({ staticValue: "Kept" })]);
  });

  it("allows a component inside content forwarded through an ancestor", () => {
    const result = compile(`
      function Card({ ...props }) { return <div {...props} /> }
      function App() { return <Card><Card>Nested</Card></Card> }
    `, { rootComponent: "App" });
    expect(result.diagnostics).toEqual([]);
    expect(result.ir.elements.filter((element) => element.tag === "div")).toHaveLength(2);
    expect(result.ir.texts).toEqual([expect.objectContaining({ staticValue: "Nested" })]);
  });

  it("materializes shadcn-style base classes when optional props are absent", () => {
    const result = compile(`
      function Card({ className, size = 'default', ...props }) { return <div className={cn('card-base', className)} data-size={size} {...props} /> }
      function App() { return <Card>Body</Card> }
    `, { rootComponent: "App" });
    const card = result.ir.elements.find((element) => element.tag === "div");
    expect(card?.attributes).toEqual(expect.arrayContaining([
      expect.objectContaining({ name: "className", staticValue: "card-base" }),
      expect.objectContaining({ name: "data-size", staticValue: "default" }),
    ]));
  });

  it.skip("adapts an aliased Base UI Checkbox through a local component", () => {
    const result = compile(`import { Checkbox as CheckboxPrimitive } from '@base-ui/react/checkbox'; function Checkbox() { return <CheckboxPrimitive.Root /> } function TodoListView() { return <Checkbox /> }`, { rootComponent: "TodoListView" });
    expect(result.diagnostics).toEqual([]);
    expect((result.ir as any).toggles).toHaveLength(1);
  });

  it.skip("preserves UI atom classes and makes Badge variants reactive", () => {
    const result = compile(`
      import { Checkbox } from '@wasm-runtime/ui/atoms/checkbox'
      import { Input } from '@wasm-runtime/ui/atoms/input'
      import { Badge } from '@wasm-runtime/ui/atoms/badge'
      function App({ todo }) { return <div><Checkbox checked={todo.done} /><Input value={todo.title} /><Badge variant={todo.done ? 'secondary' : 'outline'}>{todo.done ? 'Completed' : 'Open'}</Badge></div> }
    `, { rootComponent: 'App' })
    expect(result.diagnostics).toEqual([])
    expect(result.ir.elements).toEqual(expect.arrayContaining([
      expect.objectContaining({ tag: 'input', attributes: expect.arrayContaining([expect.objectContaining({ name: 'className' })]) }),
      expect.objectContaining({ tag: 'span', attributes: expect.arrayContaining([expect.objectContaining({ name: 'className', staticValue: expect.stringContaining('data-[variant=secondary]') }), expect.objectContaining({ name: 'data-variant', bindingId: expect.any(String) })]) }),
    ]))
  })

  it.skip("lowers imported UI atoms inside an expanded row template", () => {
    const result = compile(`import { TodoListView } from './TodoListView'; function App() { return <TodoListView /> }`, {
      rootComponent: 'App',
      moduleId: 'src/App.tsx',
      modules: [
        { id: 'src/App.tsx', source: `import { TodoListView } from './TodoListView'; function App() { return <TodoListView /> }` },
        { id: 'src/TodoListView.tsx', source: `import { Checkbox } from '@wasm-runtime/ui/atoms/checkbox'; import { Input } from '@wasm-runtime/ui/atoms/input'; import { Badge } from '@wasm-runtime/ui/atoms/badge'; export function TodoListView() { const todos = [{ id: '1', title: 'Todo 1', done: false }]; return <ul>{todos.map(todo => <li key={todo.id}><Checkbox checked={todo.done} /><Input value={todo.title} /><Badge variant={todo.done ? 'secondary' : 'outline'}>{todo.done ? 'Completed' : 'Open'}</Badge></li>)}</ul> }` },
        { id: '@wasm-runtime/ui/atoms/checkbox', source: `export function Checkbox() { return null }` },
        { id: '@wasm-runtime/ui/atoms/input', source: `export function Input() { return null }` },
        { id: '@wasm-runtime/ui/atoms/badge', source: `export function Badge() { return null }` },
      ],
    })
    expect(result.diagnostics).toEqual([])
    expect(result.ir.elements.map((element) => element.tag)).toEqual(expect.arrayContaining(['input', 'span']))
  })

  it.skip("lowers the internal Toggle fixture into native parts and semantic metadata", async () => {
    const source = await readFile(path.resolve(__dirname, "../fixtures/toggle.tsx"), "utf8");
    const result = compile(source, { rootComponent: "ControlledTodoToggle" });
    expect(result.diagnostics).toEqual([]);
    expect((result.ir as any).toggles).toEqual([expect.objectContaining({ id: "t1", rootElementId: "e1", inputElementId: "e2", indicatorElementId: "e3", stateSlotId: "s1" })]);
    expect(result.ir.elements).toEqual(expect.arrayContaining([
      expect.objectContaining({ id: "e1", tag: "label" }),
      expect.objectContaining({ id: "e2", tag: "input", attributes: expect.arrayContaining([expect.objectContaining({ name: "type", staticValue: "checkbox" })]) }),
      expect.objectContaining({ id: "e3", tag: "span" }),
    ]));
    expect(result.ir.localStates[0]).toMatchObject({ values: ["false", "true", "mixed"] });
  });

  it.skip("diagnoses invalid Toggle child composition", () => {
    const result = compile(`import { Toggle } from '@wasm-runtime/internal-toggle'; function App() { return <Toggle.Root><span>nope</span><Toggle.Indicator /><Toggle.Indicator /></Toggle.Root> }`);
    expect(result.diagnostics.map((diagnostic) => diagnostic.code)).toEqual(["UNSUPPORTED_TOGGLE_CHILD", "DUPLICATE_TOGGLE_INDICATOR"]);
  });

  it.skip("adapts Base UI Checkbox parts to Toggle without compiling package hooks", () => {
    const result = compile(`
      import { Checkbox } from '@base-ui/react/checkbox'
      function App() { return <Checkbox.Root defaultChecked className="shadcn-checkbox"><Checkbox.Indicator className="indicator">✓</Checkbox.Indicator></Checkbox.Root> }
    `);
    expect(result.diagnostics).toEqual([]);
    expect((result.ir as any).toggles[0]).toMatchObject({ id: "t1", rootElementId: "e1", inputElementId: "e2", indicatorElementId: "e3" });
  });

  it.skip("lowers the Hugeicons checkbox mark to intrinsic SVG", () => {
    const result = compile(`import { HugeiconsIcon } from '@hugeicons/react'; function Icon() { return <HugeiconsIcon icon={Tick02Icon} strokeWidth={2} /> }`);
    expect(result.diagnostics).toEqual([]);
    expect(result.ir.elements).toEqual(expect.arrayContaining([expect.objectContaining({ tag: "svg" }), expect.objectContaining({ tag: "path" })]));
  });

  it("emits generic ordered props, refs, and mouse event bindings", () => {
    const result = compile(`function Part({ className, onMouseDown, ref, ...rest }) { return <button className="authored" {...rest} className={className} ref={ref} onMouseDown={onMouseDown}>ok</button> } function App({ onMouseDown, rootRef }) { return <Part className="caller" data-testid="part" ref={rootRef} onMouseDown={onMouseDown} /> }`, { rootComponent: "App" });
    expect(result.diagnostics).toEqual([]);
    expect(result.ir.propPrograms[0]?.writes.map((write) => write.name)).toEqual(["className", "data-testid", "className", "ref", "onMouseDown"]);
    expect(result.ir.events[0]).toMatchObject({ type: "mousedown", callbackName: "onMouseDown" });
    expect(result.ir.refs).toHaveLength(1);
  });

  it("resolves namespace members through re-exported dependency modules", () => {
    const result = compile(`import * as Primitive from 'pkg'; function App({ onMouseDown }) { return <Primitive.Root data-testid="root" onMouseDown={onMouseDown}>ok</Primitive.Root> }`, {
      rootComponent: "App", moduleId: "src/App.tsx", modules: [
        { id: "src/App.tsx", source: `import * as Primitive from 'pkg'; function App({ onMouseDown }) { return <Primitive.Root data-testid="root" onMouseDown={onMouseDown}>ok</Primitive.Root> }` },
        { id: "pkg", source: `export * as Primitive from './parts.mjs'` },
        { id: "pkg/parts.mjs", source: `export { PrimitiveRoot as Root } from './root.mjs'` },
        { id: "pkg/root.mjs", source: `export const PrimitiveRoot = forwardRef(function PrimitiveRoot({ children, ...props }, ref) { return <button {...props} ref={ref}>{children}</button> })` }
      ]
    });
    expect(result.diagnostics).toEqual([]);
    expect(result.ir.elements[0]).toMatchObject({ tag: "button" });
    expect(result.ir.events[0]).toMatchObject({ type: "mousedown" });
  });
});
