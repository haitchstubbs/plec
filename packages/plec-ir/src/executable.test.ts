import { describe, expect, it } from 'vitest';
import {
  validateExecutableApplication,
  type ExecutableApplication,
} from './executable.ts';

function application(): ExecutableApplication {
  return {
    version: '0.9',
    rootNode: 0,
    strings: ['div', 'title'],
    constants: ['initial'],
    nodes: [
      { op: 'element', tag: 0, parent: null, children: [1] },
      { op: 'text', text: 0, parent: 0 },
    ],
    texts: [{ binding: 0 }],
    bindings: [{ target: 1, sink: 'text', expression: 1 }],
    events: [{ target: 0, type: 1, action: 0, fields: [1] }],
    propPrograms: [],
    inputs: [],
    stateSlots: [{ initialExpression: 0, frameSlot: 0 }],
    expressions: [
      {
        instructions: [
          { op: 'constant', constant: 0 },
          { op: 'return' },
        ],
      },
      {
        instructions: [{ op: 'loadState', state: 0 }, { op: 'return' }],
      },
      {
        instructions: [
          { op: 'loadRowField', field: 1 },
          { op: 'return' },
        ],
      },
    ],
    actions: [
      {
        instructions: [
          {
            op: 'capabilityRequest',
            capability: 'fetch',
            request: { url: 0, method: 'GET', decode: 'json', requireOk: true },
            successPc: 1,
            failurePc: 2,
            finallyPc: 3,
            resultSlot: 0,
            errorSlot: 1,
          },
          { op: 'return' },
          { op: 'return' },
          { op: 'return' },
        ],
        frameSlots: 2,
      },
    ],
    loops: [
      {
        sourceExpression: 1,
        keyExpression: 2,
        itemSlot: 0,
        rowTemplate: 1,
        dependencySlots: [0],
      },
    ],
    contexts: [],
    hostSlots: [{ kind: 'currentYear' }],
    dependencyEdges: [
      {
        source: { kind: 'state', handle: 0 },
        target: { kind: 'loop', handle: 0 },
      },
    ],
  };
}

describe('ExecutableApplication 0.9', () => {
  it('accepts a representative typed graph', () => {
    const parsed = validateExecutableApplication(application());
    expect(parsed).toMatchObject(application());
    expect(
      parsed.expressions.every((program) => program.frameSlots === 0),
    ).toBe(true);
  });
  it('rejects an unknown opcode', () => {
    const value: any = application();
    value.expressions[0].instructions[0].op = 'evaluateJavaScript';
    expect(() => validateExecutableApplication(value)).toThrow();
  });
  it('rejects invalid runtime values', () => {
    const value: any = application();
    value.constants[0] = Infinity;
    expect(() => validateExecutableApplication(value)).toThrow();
  });
  it('rejects out-of-range handles and non-dense state slots', () => {
    const outOfRange: any = application();
    outOfRange.bindings[0].target = 99;
    expect(() => validateExecutableApplication(outOfRange)).toThrow(
      /HANDLE_OUT_OF_RANGE/,
    );
    const duplicateSlot: any = application();
    duplicateSlot.stateSlots[0].frameSlot = 1;
    expect(() => validateExecutableApplication(duplicateSlot)).toThrow(
      /STATE_SLOT_NOT_DENSE/,
    );
  });
  it('rejects invalid jump and continuation targets', () => {
    const jump: any = application();
    jump.expressions[0].instructions[0] = { op: 'jump', target: 9 };
    expect(() => validateExecutableApplication(jump)).toThrow(
      /INVALID_JUMP_TARGET/,
    );
    const continuation: any = application();
    continuation.actions[0].instructions[0].successPc = 9;
    expect(() => validateExecutableApplication(continuation)).toThrow(
      /INVALID_CONTINUATION_TARGET/,
    );
  });
  it('requires a route loader result destination', () => {
    const value: any = application();
    value.actions[0].routeLoader = true;
    expect(() => validateExecutableApplication(value)).toThrow(
      /MISSING_LOADER_RESULT_DESTINATION/,
    );
  });
  it('rejects frame accesses outside their declared layout', () => {
    const expression: any = application();
    expression.expressions[0] = {
      frameSlots: 0,
      instructions: [{ op: 'loadFrame', slot: 0 }, { op: 'return' }],
    };
    expect(() => validateExecutableApplication(expression)).toThrow(
      /FRAME_SLOT_OUT_OF_RANGE/,
    );
    const action: any = application();
    action.actions[0].frameSlots = 1;
    expect(() => validateExecutableApplication(action)).toThrow(
      /FRAME_SLOT_OUT_OF_RANGE/,
    );
  });
});
