import {
  validateExecutableApplication,
  type ExecutableApplication,
  type ExecutableValue,
} from 'plec-ir';

type LegacyIr = any;

/**
 * Lowers the compiler's private syntax facts into the closed execution graph.
 * The input is intentionally not exported: 0.8 was an implementation detail
 * of the old compiler, never a compatibility format for this path.
 */
export function lowerCompilerFacts(
  ir: LegacyIr,
): ExecutableApplication {
  const strings: string[] = [];
  const constants: ExecutableValue[] = [];
  const string = (value: string) => {
    const existing = strings.indexOf(value);
    if (existing >= 0) return existing;
    strings.push(value);
    return strings.length - 1;
  };
  const constant = (value: ExecutableValue) => {
    const encoded = JSON.stringify(value);
    const existing = constants.findIndex(
      (item) => JSON.stringify(item) === encoded,
    );
    if (existing >= 0) return existing;
    constants.push(value);
    return constants.length - 1;
  };
  const stateByName = new Map(
    (ir.localStates ?? []).map((slot: any, index: number) => [
      slot.name,
      index,
    ]),
  );
  const expressionById = new Map(
    (ir.expressions ?? []).map((entry: any, index: number) => [
      entry.id,
      index,
    ]),
  );
  const required = <T>(value: T | undefined, description: string): T => {
    if (value === undefined)
      throw new Error(`EXECUTABLE_REFERENCE_MISSING:${description}`);
    return value;
  };
  const hostSlots: any[] = [];
  const host = (value: any) => {
    const key = `${value.name}:${value.query ?? ''}`;
    const existing = hostSlots.findIndex(
      (entry) => `${entry.kind}:${entry.query ?? ''}` === key,
    );
    if (existing >= 0) return existing;
    hostSlots.push(
      value.name === 'media-query'
        ? { kind: 'mediaQuery', query: string(value.query) }
        : {
            kind:
              value.name === 'location' ? 'location' : 'currentYear',
          },
    );
    return hostSlots.length - 1;
  };
  const rowNames = new Set(
    (ir.loops ?? []).map((loop: any) => loop.itemName),
  );
  const program = (
    expression: any,
    frame: { item?: string; index?: string; slots?: Map<string, number>; eventFields?: Map<string, number> } = {},
  ) => {
    const instructions: any[] = [];
    const emit = (value: any): void => {
      if (!value || typeof value !== 'object') {
        instructions.push({ op: 'constant', constant: constant(null) });
        return;
      }
      if (value.kind === 'literal') {
        instructions.push({
          op: 'constant',
          constant: constant(value.value ?? null),
        });
        return;
      }
      if (value.kind === 'identifier') {
        if (stateByName.has(value.name))
          instructions.push({
            op: 'loadState',
            state: stateByName.get(value.name),
          });
        else if (value.name === frame.item || rowNames.has(value.name))
          instructions.push({ op: 'loadRowField', field: string('') });
        else if (frame.slots?.has(value.name))
          instructions.push({ op: 'loadFrame', slot: frame.slots.get(value.name) });
        else if (value.name === 'host')
          instructions.push({
            op: 'loadHost',
            host: host({ name: 'location' }),
          });
        else
          instructions.push({
            op: 'constant',
            constant: constant(null),
          });
        return;
      }
      if (value.kind === 'host') {
        instructions.push({ op: 'loadHost', host: host(value) });
        return;
      }
      if (value.kind === 'member') {
        const eventField = eventMember(value);
        if (eventField && frame.eventFields?.has(eventField)) {
          instructions.push({ op: 'loadEventField', field: frame.eventFields.get(eventField) });
          return;
        }
        emit(value.object);
        instructions.push({
          op: 'field',
          field: string(value.property),
        });
        return;
      }
      if (value.kind === 'unary') {
        emit(value.argument);
        instructions.push({
          op: 'unary',
          kind:
            value.op === '!'
              ? 'not'
              : value.op === '-'
                ? 'minus'
                : 'plus',
        });
        return;
      }
      if (value.kind === 'binary' || value.kind === 'logical') {
        emit(value.left);
        emit(value.right);
        const kinds: Record<string, string> = {
          '+': 'add',
          '-': 'subtract',
          '*': 'multiply',
          '/': 'divide',
          '%': 'modulo',
          '===': 'equal',
          '!==': 'notEqual',
          '>': 'greater',
          '>=': 'greaterEqual',
          '<': 'less',
          '<=': 'lessEqual',
          '&&': 'and',
          '||': 'or',
          '??': 'coalesce',
        };
        instructions.push({
          op: 'binary',
          kind: kinds[value.op] ?? 'equal',
        });
        return;
      }
      if (value.kind === 'conditional') {
        emit(value.test);
        const branch = instructions.length;
        instructions.push({ op: 'jumpIfFalse', target: 0 });
        emit(value.consequent);
        const done = instructions.length;
        instructions.push({ op: 'jump', target: 0 });
        instructions[branch].target = instructions.length;
        emit(value.alternate);
        instructions[done].target = instructions.length;
        return;
      }
      if (value.kind === 'template') {
        for (const part of value.parts ?? [])
          emit(
            typeof part === 'string'
              ? { kind: 'literal', value: part }
              : part,
          );
        instructions.push({
          op: 'string',
          kind: 'concat',
          count: (value.parts ?? []).length,
        });
        return;
      }
      if (value.kind === 'array') {
        for (const item of value.items ?? []) emit(item.value ?? item);
        instructions.push({
          op: 'makeArray',
          count: (value.items ?? []).length,
        });
        return;
      }
      if (value.kind === 'object') {
        const entries = value.properties ?? value.entries ?? [];
        for (const entry of entries) emit(entry.value);
        instructions.push({
          op: 'makeRecord',
          fields: entries.map((entry: any) => string(entry.key ?? '')),
        });
        return;
      }
      if (value.kind === 'intrinsic' || value.kind === 'method') {
        emit(value.receiver ?? { kind: 'literal', value: '' });
        for (const arg of value.args ?? []) emit(arg);
        const names: Record<string, string> = {
          trim: 'trim',
          toLowerCase: 'lower',
          toUpperCase: 'upper',
          includes: 'includes',
          encodeURIComponent: 'encodeUriComponent',
          cn: 'concat',
          clsx: 'concat',
          classnames: 'concat',
        };
        instructions.push({
          op: 'string',
          kind: names[value.name] ?? 'concat',
          count: (value.args?.length ?? 0) + (value.receiver ? 1 : 0),
        });
        return;
      }
      if (value.kind === 'collection') {
        emit(value.source);
        const callback =
          expressions.push(
            program(value.expression, {
              item: value.itemName,
              index: value.indexName,
            }),
          ) - 1;
        instructions.push({
          op: value.op,
          [value.op === 'filter' ? 'predicate' : 'mapper']: callback,
          itemSlot: 0,
          ...(value.indexName ? { indexSlot: 1 } : {}),
        });
        return;
      }
      instructions.push({ op: 'constant', constant: constant(null) });
    };
    emit(expression);
    instructions.push({ op: 'return' });
    return {
      instructions,
      frameSlots: frame.slots?.size ?? 0,
    };
  };
  const expressions: any[] = [];
  for (const entry of ir.expressions ?? [])
    expressions.push(program(entry.expression));
  const expression = (value: any, frame: Parameters<typeof program>[1] = {}) =>
    expressions.push(program(value, frame)) - 1;
  const nodeIndex = new Map<string, number>();
  const nodes: any[] = [];
  for (const element of ir.elements ?? []) {
    nodeIndex.set(element.id, nodes.length);
    nodes.push({
      op: 'element',
      tag: string(element.tag),
      parent: null,
      children: [],
    });
  }
  const texts: any[] = [];
  for (const text of ir.texts ?? []) {
    nodeIndex.set(text.id, nodes.length);
    const textIndex = texts.length;
    texts.push({
      ...(text.staticValue === undefined
        ? {}
        : { value: text.staticValue }),
    });
    nodes.push({ op: 'text', text: textIndex, parent: null });
  }
  const loops = (ir.loops ?? []).map((loop: any, loopIndex: number) => {
    nodeIndex.set(loop.id, nodes.length);
    nodes.push({ op: 'loop', loop: loopIndex, parent: null });
    const sourceExpression = loop.sourceExpressionId
      ? (expressionById.get(loop.sourceExpressionId) ??
        expressions.push(program({ kind: 'literal', value: null })) - 1)
      : expressions.push(program({ kind: 'literal', value: null })) - 1;
    const keyExpression = loop.keyExpressionId
      ? (expressionById.get(loop.keyExpressionId) ??
        expressions.push(program({ kind: 'literal', value: null })) - 1)
      : expressions.push(program({ kind: 'literal', value: null })) - 1;
    return {
      sourceExpression,
      keyExpression,
      itemSlot: 0,
      ...(loop.indexName ? { indexSlot: 1 } : {}),
      rowTemplate: required(
        nodeIndex.get(loop.rowTemplateRootElementId),
        `loop ${loop.id} row template ${loop.rowTemplateRootElementId}`,
      ),
      dependencySlots: (ir.localStates ?? []).flatMap(
        (slot: any, index: number) =>
          (loop.dependencyStateNames ?? []).includes(slot.name) ||
          String(loop.source ?? '').includes(slot.name)
            ? [index]
            : [],
      ),
      ...(loop.inputId
        ? {
            input: required(
              (ir.inputs ?? []).findIndex(
                (input: any) => input.id === loop.inputId,
              ) >= 0
                ? (ir.inputs ?? []).findIndex((input: any) => input.id === loop.inputId)
                : undefined,
              `loop ${loop.id} input ${loop.inputId}`,
            ),
          }
        : {}),
    };
  });
  for (const element of ir.elements ?? []) {
    const node = nodes[nodeIndex.get(element.id)!];
    node.parent = element.parentId
      ? (nodeIndex.get(element.parentId) ?? null)
      : null;
    node.children = (element.children ?? [])
      .map((id: string) => nodeIndex.get(id))
      .filter(
        (id: number | undefined): id is number => id !== undefined,
      );
  }
  for (const text of ir.texts ?? [])
    nodes[nodeIndex.get(text.id)!].parent =
      nodeIndex.get(text.parentId) ?? null;
  const bindings = (ir.bindings ?? []).map((binding: any) => ({
    target: required(nodeIndex.get(binding.targetId), `binding ${binding.id} target ${binding.targetId}`),
    sink: binding.kind,
    ...(binding.attributeName
      ? { name: string(binding.attributeName) }
      : {}),
    expression: required(expressionById.get(binding.expressionId), `binding ${binding.id} expression ${binding.expressionId}`),
  }));
  for (const [index, binding] of (ir.bindings ?? []).entries())
    if (binding.kind === 'text')
      texts[
        nodes[nodeIndex.get(binding.targetId) ?? 0]?.text ?? 0
      ].binding = index;
  const propPrograms = (ir.propPrograms ?? []).map((entry: any) => ({
    target: nodeIndex.get(entry.targetId) ?? 0,
    writes: entry.writes
      .filter(
        (write: any) => write.kind !== 'event' && write.kind !== 'ref',
      )
      .map((write: any) => ({
        name: string(write.name),
        kind: write.kind,
        ...(write.expressionId
          ? { expression: required(expressionById.get(write.expressionId), `prop ${entry.id} expression ${write.expressionId}`) }
          : { constant: constant(write.staticValue ?? '') }),
      })),
  }));
  const stateSlots = (ir.localStates ?? []).map(
    (slot: any, index: number) => ({
      initialExpression:
        expressions.push(
          program({
            kind: 'literal',
            // `undefined` is a valid scalar initial value in authored Plec
            // code but is not JSON text. It crosses the executable boundary
            // as JSON null.
            value: parseInitialValue(slot.initialValue),
          }),
        ) - 1,
      frameSlot: index,
    }),
  );
  const rootNode = required(nodeIndex.get(ir.rootElementId), `root node ${ir.rootElementId}`);
  const actionById = new Map<string, number>(
    (ir.actions ?? []).map((action: any, index: number) => [action.id, index]),
  );
  const events: any[] = [];
  const actions = (ir.actions ?? []).map((action: any) => {
    const fields = eventFields(action.operations ?? []);
    const fieldSlots = new Map(fields.map((field, index) => [field, index]));
    const slots = new Map<string, number>();
    let nextSlot = 0;
    const slot = (name: string) => {
      const found = slots.get(name);
      if (found !== undefined) return found;
      slots.set(name, nextSlot);
      return nextSlot++;
    };
    const instructions: any[] = [];
    const emit = (operations: any[]) => {
      for (const operation of operations ?? []) {
        switch (operation?.kind) {
          case 'set-state':
            instructions.push({ op: 'evaluate', expression: expression(operation.value, { slots, eventFields: fieldSlots }) });
            instructions.push({ op: 'storeState', state: required(stateByName.get((ir.localStates ?? []).find((state: any) => state.id === operation.stateSlotId)?.name), `action ${action.id} state ${operation.stateSlotId}`) });
            break;
          case 'prevent-default': instructions.push({ op: 'preventDefault' }); break;
          case 'return': instructions.push({ op: 'return' }); break;
          case 'invoke-action-ref':
            instructions.push({ op: 'call', action: required(actionById.get(operation.actionId), `action ${action.id} call ${operation.actionId}`), arguments: (operation.parameters ?? []).map((value: any) => expression(value, { slots, eventFields: fieldSlots })) });
            break;
          case 'collection': {
            const input = (ir.inputs ?? []).findIndex((entry: any) => entry.id === operation.inputId);
            if (input < 0) throw new Error(`EXECUTABLE_REFERENCE_MISSING:action ${action.id} collection input ${operation.inputId}`);
            if (!operation.key) throw new Error(`EXECUTABLE_COLLECTION_KEY_MISSING:action ${action.id}`);
            if (operation.operation !== 'keyed-remove' && !operation.value)
              throw new Error(`EXECUTABLE_COLLECTION_VALUE_MISSING:action ${action.id}`);
            instructions.push({
              op: 'collectionMutation', input,
              kind: operation.operation === 'keyed-replace' ? 'keyedReplace' : operation.operation === 'keyed-remove' ? 'keyedRemove' : 'append',
              key: expression(operation.key, { slots, eventFields: fieldSlots }),
              ...(operation.value ? { value: expression(operation.value, { slots, eventFields: fieldSlots }) } : {}),
            });
            break;
          }
          case 'if': {
            instructions.push({ op: 'evaluate', expression: expression(operation.test, { slots, eventFields: fieldSlots }) });
            const branch = instructions.length;
            instructions.push({ op: 'jumpIfFalse', target: 0 });
            emit(operation.consequent);
            const done = instructions.length;
            instructions.push({ op: 'jump', target: 0 });
            instructions[branch].target = instructions.length;
            emit(operation.alternate);
            instructions[done].target = instructions.length;
            break;
          }
          case 'capability-request': {
            const request = operation.request ?? {};
            const resultSlot = slot(operation.successResultName ?? 'result');
            const errorSlot = slot(operation.failureErrorName ?? 'error');
            const requestInstruction: any = {
              op: 'capabilityRequest', capability: 'fetch',
              request: {
                url: expression(request.url, { slots, eventFields: fieldSlots }),
                method: request.method ?? 'GET', decode: request.decode ?? 'json', requireOk: request.requireOk !== false,
                headers: Object.entries(request.headers ?? {}).map(([name, value]) => ({ name: string(name), value: expression(value, { slots, eventFields: fieldSlots }) })),
                ...(request.jsonBody ? { body: expression(request.jsonBody, { slots, eventFields: fieldSlots }) } : {}),
              }, successPc: 0, failurePc: 0, finallyPc: undefined, resultSlot, errorSlot,
            };
            instructions.push(requestInstruction);
            requestInstruction.successPc = instructions.length;
            emit(operation.success);
            const afterSuccess = instructions.length;
            if (operation.finally?.length) { requestInstruction.finallyPc = afterSuccess; emit(operation.finally); }
            instructions.push({ op: 'return' });
            requestInstruction.failurePc = instructions.length;
            emit(operation.failure);
            if (operation.finally?.length) emit(operation.finally);
            instructions.push({ op: 'return' });
            break;
          }
        }
      }
    };
    emit(action.operations ?? []);
    if (!instructions.length || instructions.at(-1)?.op !== 'return') instructions.push({ op: 'return' });
    return { instructions, frameSlots: nextSlot, parameterSlots: [] };
  });
  for (const event of ir.events ?? []) {
    const actionIndex: number = required(actionById.get(event.actionId), `event ${event.id} action ${event.actionId}`);
    const fields = eventFields((ir.actions ?? [])[actionIndex]?.operations ?? []);
    const loop = event.loopId ? (ir.loops ?? []).findIndex((entry: any) => entry.id === event.loopId) : undefined;
    events.push({ target: required(nodeIndex.get(event.targetId), `event ${event.id} target ${event.targetId}`), type: string(event.type), action: actionIndex, fields: fields.map((field) => string(field)), ...(loop === undefined ? {} : { loop: required(loop >= 0 ? loop : undefined, `event ${event.id} loop ${event.loopId}`) }) });
  }
  const dependencyEdges: any[] = [];
  const loopIndexById = new Map(
    (ir.loops ?? []).map((loop: any, index: number) => [
      loop.id,
      index,
    ]),
  );
  for (const [loopIndex, loop] of (ir.loops ?? []).entries()) {
    for (const slot of (ir.localStates ?? []).flatMap(
      (state: any, index: number) =>
        (loop.dependencyStateNames ?? []).includes(state.name) ||
        String(loop.source ?? '').includes(state.name)
          ? [index]
          : [],
    ))
      dependencyEdges.push({
        source: { kind: 'state', handle: slot },
        target: { kind: 'loop', handle: loopIndex },
      });
    if (loop.inputId) {
      const input = (ir.inputs ?? []).findIndex(
        (entry: any) => entry.id === loop.inputId,
      );
      if (input >= 0)
        dependencyEdges.push({
          source: { kind: 'input', handle: input },
          target: { kind: 'loop', handle: loopIndex },
        });
    }
  }
  const rowFields = (
    expressionId: string | undefined,
    loopId: string | undefined,
  ) => {
    if (!expressionId || !loopId) return [];
    const expression = (ir.expressions ?? []).find(
      (entry: any) => entry.id === expressionId,
    )?.expression;
    const loop = (ir.loops ?? []).find(
      (entry: any) => entry.id === loopId,
    );
    const fields = new Set<string>();
    const visit = (value: any) => {
      if (!value || typeof value !== 'object') return;
      if (
        value.kind === 'member' &&
        value.object?.kind === 'identifier' &&
        value.object.name === loop?.itemName
      )
        fields.add(value.property);
      Object.values(value).forEach((child: any) =>
        Array.isArray(child) ? child.forEach(visit) : visit(child),
      );
    };
    visit(expression);
    return [...fields];
  };
  for (const [index, binding] of (ir.bindings ?? []).entries())
    for (const field of rowFields(binding.expressionId, binding.loopId))
      dependencyEdges.push({
        source: {
          kind: 'rowField',
          handle: string(field),
          loop: loopIndexById.get(binding.loopId),
        },
        target: { kind: 'binding', handle: index },
      });
  for (const [index, prop] of (ir.propPrograms ?? []).entries())
    for (const write of prop.writes ?? [])
      for (const field of rowFields(write.expressionId, prop.loopId))
        dependencyEdges.push({
          source: {
            kind: 'rowField',
            handle: string(field),
            loop: loopIndexById.get(prop.loopId),
          },
          target: { kind: 'propProgram', handle: index },
        });
  return lowerExecutableApplication({
    strings,
    constants,
    nodes,
    texts,
    bindings,
    propPrograms,
    events,
    actions,
    stateSlots,
    expressions,
    loops,
    rootNode,
    inputs: (ir.inputs ?? []).map((input: any) => ({
      name: string(input.name),
      kind: input.shape.kind,
    })),
    hostSlots,
    dependencyEdges,
  });

  function expressionUsesState(
    expressionId: string | undefined,
    name: string,
  ) {
    const expression = (ir.expressions ?? []).find(
      (entry: any) => entry.id === expressionId,
    )?.expression;
    const visit = (value: any): boolean =>
      !value || typeof value !== 'object'
        ? false
        : (value.kind === 'identifier' && value.name === name) ||
          Object.values(value).some((child: any) =>
            Array.isArray(child) ? child.some(visit) : visit(child),
          );
    return visit(expression);
  }

  function eventMember(value: any): string | undefined {
    const property = value?.property;
    const object = value?.object;
    if (object?.kind === 'identifier' && object.name === 'event') return property;
    if (object?.kind === 'member' && object.object?.kind === 'identifier' && object.object.name === 'event' && object.property === 'currentTarget') return property;
    return undefined;
  }
  function parseInitialValue(value: string | undefined) {
    if (!value || value === 'undefined') return null;
    try { return JSON.parse(value); } catch { return null; }
  }
  function eventFields(operations: any[]): string[] {
    const found = new Set<string>();
    const visit = (value: any): void => {
      if (!value || typeof value !== 'object') return;
      const field = eventMember(value);
      if (field && ['value', 'checked', 'key', 'rowKey', 'button', 'metaKey', 'ctrlKey', 'shiftKey', 'altKey', 'type'].includes(field)) found.add(field);
      Object.values(value).forEach((child: any) => Array.isArray(child) ? child.forEach(visit) : visit(child));
    };
    visit(operations);
    return [...found];
  }
}

/**
 * Compiler-owned facts, deliberately separate from the readable 0.8 JSON
 * artifact. Later phases populate these directly while lowering SWC syntax.
 */
export interface ExecutableApplicationFacts {
  strings: string[];
  constants?: ExecutableValue[];
  nodes: ExecutableApplication['nodes'];
  texts?: ExecutableApplication['texts'];
  bindings?: ExecutableApplication['bindings'];
  events?: ExecutableApplication['events'];
  propPrograms?: ExecutableApplication['propPrograms'];
  inputs?: ExecutableApplication['inputs'];
  stateSlots?: ExecutableApplication['stateSlots'];
  expressions?: ExecutableApplication['expressions'];
  actions?: ExecutableApplication['actions'];
  loops?: ExecutableApplication['loops'];
  contexts?: ExecutableApplication['contexts'];
  hostSlots?: ExecutableApplication['hostSlots'];
  dependencyEdges?: ExecutableApplication['dependencyEdges'];
  rootNode?: number;
}

/**
 * The Phase 1 seam: it receives compiler facts, assigns only table-order
 * handles, and validates the executable contract. It never reads a 0.8 IR.
 */
export function lowerExecutableApplication(
  facts: ExecutableApplicationFacts,
): ExecutableApplication {
  return validateExecutableApplication({
    version: '0.9',
    rootNode: facts.rootNode ?? 0,
    strings: facts.strings,
    constants: facts.constants ?? [],
    nodes: facts.nodes,
    texts: facts.texts ?? [],
    bindings: facts.bindings ?? [],
    events: facts.events ?? [],
    propPrograms: facts.propPrograms ?? [],
    inputs: facts.inputs ?? [],
    stateSlots: facts.stateSlots ?? [],
    expressions: facts.expressions ?? [],
    actions: facts.actions ?? [],
    loops: facts.loops ?? [],
    contexts: facts.contexts ?? [],
    hostSlots: facts.hostSlots ?? [],
    dependencyEdges: facts.dependencyEdges ?? [],
  });
}
