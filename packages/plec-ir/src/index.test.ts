import { describe, expect, it } from 'vitest';
import {
  isPlecValue,
  renderStaticApplication,
  validateApplicationIr,
  validateCompiledSubtreeIr,
  validatePlecGraphArtifact,
} from './index.ts';

const base = {
  version: '0.8',
  rootElementId: 'e1',
  elements: [
    { id: 'e1', tag: 'label', parentId: null, children: ['e2'] },
    { id: 'e2', tag: 'input', parentId: 'e1', children: [] },
  ],
  texts: [],
  bindings: [],
  localStates: [
    {
      id: 's1',
      name: 'toggle-1',
      initialValue: 'false',
      values: ['false', 'true', 'mixed'],
    },
  ],
  events: [
    { id: 'ev1', type: 'change', targetId: 'e2', actionId: 'a1' },
  ],
};

describe('generic structural IR', () => {
  it('validates graph interfaces as serializable public boundaries', () => {
    const artifact = validatePlecGraphArtifact({
      graphId: 'layout',
      revision: 'r1',
      interface: {
        inputs: [
          { id: 'title', type: { kind: 'string' }, required: true },
        ],
        outputs: [],
        commands: [],
        outlets: [
          {
            id: 'main',
            regionId: 'region-main',
            accepts: {
              inputs: [{ id: 'title', type: { kind: 'string' } }],
              outputs: [],
              commands: [],
            },
          },
        ],
      },
      ir: {},
    });
    expect(artifact.interface.outlets[0]!.accepts.inputs[0]!.id).toBe(
      'title',
    );
    expect(
      isPlecValue(
        { title: 'safe' },
        { kind: 'object', fields: { title: { kind: 'string' } } },
      ),
    ).toBe(true);
  });
  it('validates the closure-free v0.6 compiled subtree language', () => {
    const ir = validateCompiledSubtreeIr({
      version: '0.6',
      rootTemplateId: 't1',
      templates: [
        {
          id: 't1',
          rootNodeIds: ['e1'],
          nodes: [
            {
              kind: 'element',
              id: 'e1',
              tag: 'button',
              parentId: null,
              attributes: [],
            },
            { kind: 'text', id: 'n1', parentId: 'e1', value: '' },
          ],
        },
      ],
      expressions: [
        { id: 'x0', expression: { kind: 'literal', value: 0 } },
        { id: 'x1', expression: { kind: 'state', stateSlotId: 's1' } },
      ],
      stateSlots: [
        {
          id: 's1',
          templateId: 't1',
          slot: 0,
          initialExpressionId: 'x0',
        },
      ],
      bindings: [
        {
          id: 'b1',
          templateId: 't1',
          targetId: 'n1',
          sink: 'text',
          expressionId: 'x1',
        },
      ],
      transitions: [
        {
          id: 'tr1',
          eventType: 'click',
          targetId: 'e1',
          op: 'increment',
          stateSlotId: 's1',
        },
      ],
      dependencyEdges: [
        { fromId: 's1', toId: 'b1', kind: 'state-to-binding' },
      ],
    });
    expect(ir.version).toBe('0.6');
  });

  it('rejects derived dependency cycles deterministically', () => {
    expect(() =>
      validateCompiledSubtreeIr({
        version: '0.6',
        rootTemplateId: 't',
        templates: [{ id: 't', nodes: [], rootNodeIds: [] }],
        expressions: [],
        derived: [
          { id: 'd1', expressionId: 'x', dependencies: ['d2'] },
          { id: 'd2', expressionId: 'x', dependencies: ['d1'] },
        ],
      }),
    ).toThrow('DERIVED_DEPENDENCY_CYCLE: d1 -> d2 -> d1');
  });

  it('accepts prop programs, refs, contexts, and generic events', () => {
    const ir = validateApplicationIr({
      ...base,
      propPrograms: [
        {
          id: 'p1',
          targetId: 'e1',
          writes: [{ name: 'data-testid', staticValue: 'root' }],
        },
      ],
      refs: [
        { id: 'r1', targetId: 'e1', refId: 'root', kind: 'callback' },
      ],
      contexts: [
        {
          id: 'c1',
          parentId: null,
          values: [{ name: 'checked', staticValue: 'false' }],
        },
      ],
      events: [
        {
          id: 'ev1',
          type: 'mousedown',
          targetId: 'e1',
          actionId: 'a1',
        },
      ],
    });
    expect(ir.propPrograms).toHaveLength(1);
  });

  it('accepts only executable typed action operations', () => {
    const ir = validateApplicationIr({
      ...base,
      actions: [
        {
          id: 'a1',
          parameters: [{ name: 'event', type: 'event' }],
          captures: ['s1'],
          operations: [
            {
              kind: 'if',
              test: { kind: 'literal', value: true },
              consequent: [
                {
                  kind: 'set-state',
                  stateSlotId: 's1',
                  value: { kind: 'literal', value: true },
                },
              ],
              alternate: [],
            },
          ],
        },
      ],
    });
    expect(ir.actions[0]?.operations[0]?.kind).toBe('if');
    expect(() =>
      validateApplicationIr({
        ...base,
        actions: [
          {
            id: 'a1',
            operations: [{ kind: 'javascript', source: '() => {}' }],
          },
        ],
      }),
    ).toThrow();
  });
});

describe('static application renderer', () => {
  it('accepts 0.10 component graphs without legacy rootElementId fields', () => {
    const ir = validateApplicationIr({
      version: '0.10',
      rootComponent: 0,
      components: [
        {
          id: 'src/routes/todos.tsx#TodosPage',
          rootNode: 0,
          strings: ['hello'],
          constants: [],
          nodes: [
            { op: 'element', tag: 0, parent: null, children: [] },
          ],
        },
      ],
    });
    expect(ir.version).toBe('0.10');
    expect(renderStaticApplication(ir as any)).toBe('');
  });

  it('renders static nodes and host bindings while omitting query rows', () => {
    const ir = validateApplicationIr({
      version: '0.8',
      revision: 'revision-1',
      rootElementId: 'e1',
      elements: [
        {
          id: 'e1',
          tag: 'main',
          parentId: null,
          attributes: [],
          children: ['t1', 'l1', 'l2'],
        },
        {
          id: 'e2',
          tag: 'p',
          parentId: 'e1',
          attributes: [],
          children: ['t2'],
        },
      ],
      texts: [
        { id: 't1', parentId: 'e1', staticValue: 'Hello <world>' },
        { id: 't2', parentId: 'e2', staticValue: 'static row' },
      ],
      bindings: [],
      expressions: [],
      events: [],
      loops: [
        {
          id: 'l1',
          parentId: 'e1',
          source: 'static',
          itemName: 'item',
          rows: [{ id: 'r1', rootElementId: 'e2' }],
        },
        {
          id: 'l2',
          parentId: 'e1',
          source: 'todos',
          itemName: 'todo',
          queryId: 'q1',
          rowTemplateRootElementId: 'e2',
          rows: [],
        },
      ],
    });
    expect(renderStaticApplication(ir)).toBe(
      '<main data-runtime-node="e1"><!--runtime-text:t1-->Hello &lt;world&gt;<p data-runtime-node="e2"><!--runtime-text:t2-->static row</p></main>',
    );
  });

  it('evaluates pathname bindings deterministically', () => {
    const ir = validateApplicationIr({
      version: '0.8',
      rootElementId: 'e1',
      elements: [
        {
          id: 'e1',
          tag: 'a',
          parentId: null,
          attributes: [{ name: 'className', staticValue: 'link' }],
          children: [],
        },
      ],
      texts: [],
      bindings: [
        {
          id: 'b1',
          kind: 'attribute',
          targetId: 'e1',
          attributeName: 'className',
          expressionId: 'x1',
        },
      ],
      expressions: [
        {
          id: 'x1',
          expression: {
            kind: 'conditional',
            test: {
              kind: 'binary',
              op: '===',
              left: {
                kind: 'member',
                object: {
                  kind: 'member',
                  object: { kind: 'identifier', name: 'host' },
                  property: 'location',
                },
                property: 'pathname',
              },
              right: { kind: 'literal', value: '/active' },
            },
            consequent: { kind: 'literal', value: 'active' },
            alternate: { kind: 'literal', value: 'link' },
          },
        },
      ],
      events: [],
    });
    expect(
      renderStaticApplication(ir, {
        location: { pathname: '/active' },
      }),
    ).toContain('class="active"');
  });
});
