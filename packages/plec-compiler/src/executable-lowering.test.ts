import { describe, expect, it } from 'vitest';
import { lowerCompilerFacts, lowerExecutableApplication } from './executable-lowering.ts';

describe('executable lowering seam', () => {
  it('assigns stable table handles from compiler fact order without reading 0.8 IR', () => {
    const facts = {
      strings: ['div'],
      constants: ['Hello'],
      nodes: [
        { op: 'element' as const, tag: 0, parent: null, children: [1] },
        { op: 'text' as const, text: 0, parent: 0 },
      ],
      texts: [{ binding: 0 }],
      bindings: [{ target: 1, sink: 'text' as const, expression: 0 }],
      events: [],
      expressions: [
        {
          instructions: [
            { op: 'constant' as const, constant: 0 },
            { op: 'return' as const },
          ],
        },
      ],
    };
    const first = lowerExecutableApplication(facts);
    const second = lowerExecutableApplication(facts);
    expect(first).toEqual(second);
    expect(first).toMatchInlineSnapshot(`
      {
        "actions": [],
        "bindings": [
          {
            "expression": 0,
            "sink": "text",
            "target": 1,
          },
        ],
        "constants": [
          "Hello",
        ],
        "contexts": [],
        "dependencyEdges": [],
        "events": [],
        "expressions": [
          {
            "frameSlots": 0,
            "instructions": [
              {
                "constant": 0,
                "op": "constant",
              },
              {
                "op": "return",
              },
            ],
          },
        ],
        "hostSlots": [],
        "inputs": [],
        "loops": [],
        "nodes": [
          {
            "children": [
              1,
            ],
            "op": "element",
            "parent": null,
            "tag": 0,
          },
          {
            "op": "text",
            "parent": 0,
            "text": 0,
          },
        ],
        "propPrograms": [],
        "rootNode": 0,
        "stateSlots": [],
        "strings": [
          "div",
        ],
        "texts": [
          {
            "binding": 0,
          },
        ],
        "version": "0.9",
      }
    `);
  });

  it('keeps future conditional, loop, host, event, and loader facts typed', () => {
    const application = lowerExecutableApplication({
      strings: ['section', 'input', 'value'],
      constants: [''],
      nodes: [
        { op: 'element', tag: 0, parent: null, children: [1, 2] },
        { op: 'text', text: 0, parent: 0 },
        { op: 'loop', loop: 0, parent: 0 },
      ],
      texts: [{ binding: 0 }],
      bindings: [{ target: 1, sink: 'text', expression: 1 }],
      events: [{ target: 0, type: 2, action: 0, fields: [2] }],
      stateSlots: [{ initialExpression: 0, frameSlot: 0 }],
      expressions: [
        {
          instructions: [
            { op: 'constant', constant: 0 },
            { op: 'return' },
          ],
        },
        {
          instructions: [
            { op: 'loadState', state: 0 },
            { op: 'return' },
          ],
        },
        {
          instructions: [{ op: 'loadHost', host: 0 }, { op: 'return' }],
        },
        {
          instructions: [
            { op: 'loadRowField', field: 2 },
            { op: 'return' },
          ],
        },
      ],
      actions: [
        {
          routeLoader: true,
          loaderResultState: 0,
          frameSlots: 2,
          instructions: [
            {
              op: 'capabilityRequest',
              capability: 'fetch',
              request: { url: 0, method: 'GET', decode: 'json', requireOk: true },
              successPc: 1,
              failurePc: 2,
              resultSlot: 0,
              errorSlot: 1,
            },
            { op: 'return' },
            { op: 'return' },
          ],
        },
      ],
      loops: [
        {
          sourceExpression: 1,
          keyExpression: 3,
          itemSlot: 0,
          rowTemplate: 1,
          dependencySlots: [0],
        },
      ],
      hostSlots: [{ kind: 'currentYear' }],
      dependencyEdges: [
        {
          source: { kind: 'host', handle: 0 },
          target: { kind: 'binding', handle: 0 },
        },
      ],
    });
    expect(application).toMatchObject({
      version: '0.9',
      events: [{ target: 0, action: 0 }],
      loops: [{ sourceExpression: 1, dependencySlots: [0] }],
      actions: [{ routeLoader: true, loaderResultState: 0 }],
    });
  });
  it('rejects unresolved action references instead of emitting handle zero', () => {
    expect(() =>
      lowerExecutableApplication({
        strings: ['div'],
        nodes: [{ op: 'element', tag: 0, parent: null, children: [] }],
        rootNode: 0,
        events: [{ target: 0, type: 0, action: 3, fields: [] }],
      }),
    ).toThrow(/HANDLE_OUT_OF_RANGE:actions:3/);
  });
  it('rejects unresolved legacy action references instead of substituting handle zero', () => {
    expect(() =>
      lowerCompilerFacts({
        rootElementId: 'root',
        elements: [{ id: 'root', tag: 'div' }],
        actions: [{ id: 'a1', operations: [{ kind: 'invoke-action-ref', actionId: 'missing' }] }],
      }),
    ).toThrow(/EXECUTABLE_REFERENCE_MISSING:action a1 call missing/);
  });
});
