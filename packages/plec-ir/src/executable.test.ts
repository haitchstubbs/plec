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
  it('validates explicit, typed collection mutation operands', () => {
    const valid: any = application();
    valid.inputs = [{ name: 1, kind: 'collection' }];
    valid.actions[0] = {
      instructions: [
        { op: 'collectionMutation', input: 0, kind: 'append', key: 0, value: 0 },
        { op: 'collectionMutation', input: 0, kind: 'keyedReplace', key: 0, value: 0 },
        { op: 'collectionMutation', input: 0, kind: 'keyedRemove', key: 0 },
      ],
    };
    expect(() => validateExecutableApplication(valid)).not.toThrow();

    const missingValue: any = structuredClone(valid);
    delete missingValue.actions[0].instructions[0].value;
    expect(() => validateExecutableApplication(missingValue)).toThrow(
      /COLLECTION_MUTATION_REQUIRES_VALUE/,
    );
    const removeValue: any = structuredClone(valid);
    removeValue.actions[0].instructions[2].value = 0;
    expect(() => validateExecutableApplication(removeValue)).toThrow(
      /COLLECTION_REMOVE_FORBIDS_VALUE/,
    );
    const scalarInput: any = structuredClone(valid);
    scalarInput.inputs[0].kind = 'scalar';
    expect(() => validateExecutableApplication(scalarInput)).toThrow(
      /COLLECTION_MUTATION_REQUIRES_COLLECTION_INPUT/,
    );
  });
  it('rejects duplicate action parameter slots while allowing implicit returns', () => {
    const implicitReturn: any = application();
    implicitReturn.actions[0] = { instructions: [{ op: 'preventDefault' }] };
    expect(() => validateExecutableApplication(implicitReturn)).not.toThrow();
    const duplicate: any = structuredClone(implicitReturn);
    duplicate.actions[0] = {
      frameSlots: 1,
      parameterSlots: [0, 0],
      instructions: [{ op: 'return' }],
    };
    expect(() => validateExecutableApplication(duplicate)).toThrow(
      /DUPLICATE_PARAMETER_SLOT/,
    );
  });
});
