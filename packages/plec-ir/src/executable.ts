import { z } from 'zod';

/** The execution-only artifact introduced by the runtime reduction work. */
export const EXECUTABLE_APPLICATION_VERSION = '0.9' as const;

/** Every executable reference is an index into a table in the same artifact. */
export const ExecutableHandleSchema = z
  .number()
  .int()
  .min(0)
  .max(0xffff_ffff);
export type ExecutableHandle = z.infer<typeof ExecutableHandleSchema>;

export const ExecutableValueSchema: z.ZodType<any> = z.lazy(() =>
  z.union([
    z.null(),
    z.boolean(),
    z.number().finite(),
    z.string(),
    z.array(ExecutableValueSchema),
    z.record(z.string(), ExecutableValueSchema),
  ]),
);
export type ExecutableValue = z.infer<typeof ExecutableValueSchema>;

const unary = z.enum(['not', 'plus', 'minus', 'floor']);
const binary = z.enum([
  'add',
  'subtract',
  'multiply',
  'divide',
  'modulo',
  'equal',
  'notEqual',
  'greater',
  'greaterEqual',
  'less',
  'lessEqual',
  'and',
  'or',
  'coalesce',
]);
const stringOperation = z.enum([
  'concat',
  'trim',
  'lower',
  'upper',
  'includes',
  'encodeUriComponent',
]);

/** Flat stack-machine instructions. Program counters are instruction indexes. */
export const ExpressionInstructionSchema = z.discriminatedUnion('op', [
  z.object({
    op: z.literal('constant'),
    constant: ExecutableHandleSchema,
  }),
  z.object({
    op: z.literal('loadState'),
    state: ExecutableHandleSchema,
  }),
  z.object({
    op: z.literal('loadRowField'),
    field: ExecutableHandleSchema,
  }),
  z.object({
    op: z.literal('loadEventField'),
    field: ExecutableHandleSchema,
  }),
  z.object({ op: z.literal('loadFrame'), slot: ExecutableHandleSchema }),
  z.object({ op: z.literal('loadHost'), host: ExecutableHandleSchema }),
  z.object({ op: z.literal('field'), field: ExecutableHandleSchema }),
  z.object({ op: z.literal('index') }),
  z.object({ op: z.literal('unary'), kind: unary }),
  z.object({ op: z.literal('binary'), kind: binary }),
  z.object({
    op: z.literal('string'),
    kind: stringOperation,
    count: z.number().int().min(0).default(1),
  }),
  z.object({
    op: z.literal('makeArray'),
    count: z.number().int().min(0),
  }),
  z.object({
    op: z.literal('makeRecord'),
    fields: z.array(ExecutableHandleSchema),
  }),
  z.object({
    op: z.literal('filter'),
    predicate: ExecutableHandleSchema,
    itemSlot: ExecutableHandleSchema,
    indexSlot: ExecutableHandleSchema.optional(),
  }),
  z.object({
    op: z.literal('map'),
    mapper: ExecutableHandleSchema,
    itemSlot: ExecutableHandleSchema,
    indexSlot: ExecutableHandleSchema.optional(),
  }),
  z.object({ op: z.literal('jump'), target: ExecutableHandleSchema }),
  z.object({
    op: z.literal('jumpIfFalse'),
    target: ExecutableHandleSchema,
  }),
  z.object({
    op: z.literal('jumpIfTrue'),
    target: ExecutableHandleSchema,
  }),
  z.object({ op: z.literal('return') }),
]);
export type ExpressionInstruction = z.infer<
  typeof ExpressionInstructionSchema
>;

export const ExpressionProgramSchema = z.object({
  instructions: z.array(ExpressionInstructionSchema).min(1),
  frameSlots: z.number().int().min(0).default(0),
});
export type ExpressionProgram = z.infer<typeof ExpressionProgramSchema>;

export const ActionInstructionSchema = z.discriminatedUnion('op', [
  z.object({
    op: z.literal('evaluate'),
    expression: ExecutableHandleSchema,
  }),
  z.object({
    op: z.literal('storeState'),
    state: ExecutableHandleSchema,
  }),
  z.object({
    op: z.literal('collectionMutation'),
    input: ExecutableHandleSchema,
    kind: z.enum(['append', 'keyedReplace', 'keyedRemove']),
    /** The key is always explicit so collection identity never depends on row shape. */
    key: ExecutableHandleSchema,
    /** Remove is the only mutation that does not construct a replacement row. */
    value: ExecutableHandleSchema.optional(),
  }),
  z.object({ op: z.literal('preventDefault') }),
  z.object({
    op: z.literal('call'),
    action: ExecutableHandleSchema,
    arguments: z.array(ExecutableHandleSchema).default([]),
  }),
  z.object({ op: z.literal('jump'), target: ExecutableHandleSchema }),
  z.object({
    op: z.literal('jumpIfFalse'),
    target: ExecutableHandleSchema,
  }),
  z.object({
    op: z.literal('capabilityRequest'),
    capability: z.literal('fetch'),
    request: z.object({
      url: ExecutableHandleSchema,
      method: z.enum(['GET', 'POST', 'PATCH', 'PUT', 'DELETE']).default('GET'),
      headers: z.array(z.object({ name: ExecutableHandleSchema, value: ExecutableHandleSchema })).default([]),
      body: ExecutableHandleSchema.optional(),
      decode: z.enum(['empty', 'text', 'json']).default('json'),
      requireOk: z.boolean().default(true),
    }),
    successPc: ExecutableHandleSchema,
    failurePc: ExecutableHandleSchema,
    finallyPc: ExecutableHandleSchema.optional(),
    resultSlot: ExecutableHandleSchema,
    errorSlot: ExecutableHandleSchema,
  }),
  z.object({ op: z.literal('return') }),
]);
export type ActionInstruction = z.infer<typeof ActionInstructionSchema>;

export const ActionProgramSchema = z
  .object({
    instructions: z.array(ActionInstructionSchema).min(1),
    frameSlots: z.number().int().min(0).default(0),
    parameterSlots: z.array(ExecutableHandleSchema).default([]),
    /** A route loader writes here directly; no runtime operation rewriting. */
    loaderResultState: ExecutableHandleSchema.optional(),
    routeLoader: z.boolean().default(false),
  })
  .superRefine((program, context) => {
    if (program.routeLoader && program.loaderResultState === undefined)
      context.addIssue({
        code: 'custom',
        path: ['loaderResultState'],
        message: 'MISSING_LOADER_RESULT_DESTINATION',
      });
  });
export type ActionProgram = z.infer<typeof ActionProgramSchema>;

const NodeSchema = z.discriminatedUnion('op', [
  z.object({
    op: z.literal('element'),
    tag: ExecutableHandleSchema,
    parent: ExecutableHandleSchema.nullable(),
    children: z.array(ExecutableHandleSchema).default([]),
  }),
  z.object({
    op: z.literal('text'),
    text: ExecutableHandleSchema,
    parent: ExecutableHandleSchema.nullable(),
  }),
  z.object({
    op: z.literal('conditional'),
    test: ExecutableHandleSchema,
    parent: ExecutableHandleSchema.nullable(),
    consequent: ExecutableHandleSchema,
    alternate: ExecutableHandleSchema.optional(),
  }),
  z.object({
    op: z.literal('loop'),
    loop: ExecutableHandleSchema,
    parent: ExecutableHandleSchema.nullable(),
  }),
]);

export const ExecutableApplicationSchema = z.object({
  version: z.literal(EXECUTABLE_APPLICATION_VERSION),
  rootNode: ExecutableHandleSchema,
  strings: z.array(z.string()),
  constants: z.array(ExecutableValueSchema),
  nodes: z.array(NodeSchema),
  texts: z.array(
    z.object({
      value: z.string().optional(),
      binding: ExecutableHandleSchema.optional(),
    }),
  ),
  bindings: z.array(
    z.object({
      target: ExecutableHandleSchema,
      sink: z.enum(['text', 'attribute', 'property', 'class']),
      name: ExecutableHandleSchema.optional(),
      expression: ExecutableHandleSchema,
    }),
  ),
  events: z
    .array(
      z.object({
        target: ExecutableHandleSchema,
        type: ExecutableHandleSchema,
        action: ExecutableHandleSchema,
        fields: z.array(ExecutableHandleSchema).default([]),
        loop: ExecutableHandleSchema.optional(),
      }),
    )
    .default([]),
  propPrograms: z.array(
    z.object({
      target: ExecutableHandleSchema,
      writes: z.array(
        z.object({
          name: ExecutableHandleSchema,
          expression: ExecutableHandleSchema.optional(),
          constant: ExecutableHandleSchema.optional(),
          kind: z.enum([
            'attribute',
            'property',
            'event',
            'ref',
            'spread',
          ]),
        }),
      ),
    }),
  ),
  inputs: z
    .array(
      z.object({
        /** Public producer name. Runtime code only uses the table handle. */
        name: ExecutableHandleSchema,
        kind: z.enum(['scalar', 'object', 'collection']),
      }),
    )
    .default([]),
  stateSlots: z.array(
    z.object({
      initialExpression: ExecutableHandleSchema,
      frameSlot: ExecutableHandleSchema,
    }),
  ),
  expressions: z.array(ExpressionProgramSchema),
  actions: z.array(ActionProgramSchema),
  loops: z.array(
    z.object({
      sourceExpression: ExecutableHandleSchema,
      keyExpression: ExecutableHandleSchema,
      itemSlot: ExecutableHandleSchema,
      indexSlot: ExecutableHandleSchema.optional(),
      rowTemplate: ExecutableHandleSchema,
      dependencySlots: z.array(ExecutableHandleSchema).default([]),
      /** Present only for the direct external-collection delta path. */
      input: ExecutableHandleSchema.optional(),
    }),
  ),
  contexts: z.array(
    z.object({
      parent: ExecutableHandleSchema.nullable(),
      valueExpression: ExecutableHandleSchema.optional(),
    }),
  ),
  hostSlots: z.array(
    z.object({
      kind: z.enum(['currentYear', 'mediaQuery', 'location']),
      query: ExecutableHandleSchema.optional(),
    }),
  ),
  dependencyEdges: z
    .array(
      z.object({
        source: z.object({
          kind: z.enum(['state', 'input', 'rowField', 'host']),
          handle: ExecutableHandleSchema,
          /** Row fields are lexical: the same property in two loops is not
           * one dependency. */
          loop: ExecutableHandleSchema.optional(),
        }),
        target: z.object({
          kind: z.enum([
            'binding',
            'propProgram',
            'conditional',
            'loop',
          ]),
          handle: ExecutableHandleSchema,
        }),
      }),
    )
    .default([]),
});
export type ExecutableApplication = z.infer<
  typeof ExecutableApplicationSchema
>;

type Tables = z.infer<typeof ExecutableApplicationSchema>;
const tableLength = (app: Tables, table: keyof Tables) =>
  (app[table] as unknown[]).length;

/** Full validation is compiler/build tooling; runtimes may use a cheaper decoder. */
export function validateExecutableApplication(
  value: unknown,
): ExecutableApplication {
  const app = ExecutableApplicationSchema.parse(value);
  const issue = (path: Array<string | number>, message: string) => {
    throw new z.ZodError([{ code: 'custom', path, message }]);
  };
  const check = (
    value: number | undefined,
    table: keyof Tables,
    path: Array<string | number>,
  ) => {
    if (value !== undefined && value >= tableLength(app, table))
      issue(path, `HANDLE_OUT_OF_RANGE:${table}:${value}`);
  };
  check(app.rootNode, 'nodes', ['rootNode']);
  app.nodes.forEach((node, index) => {
    check(node.parent ?? undefined, 'nodes', [
      'nodes',
      index,
      'parent',
    ]);
    if (node.op === 'element') {
      check(node.tag, 'strings', ['nodes', index, 'tag']);
      node.children.forEach((child, childIndex) =>
        check(child, 'nodes', ['nodes', index, 'children', childIndex]),
      );
    }
    if (node.op === 'text')
      check(node.text, 'texts', ['nodes', index, 'text']);
    if (node.op === 'conditional') {
      check(node.test, 'expressions', ['nodes', index, 'test']);
      check(node.consequent, 'nodes', ['nodes', index, 'consequent']);
      check(node.alternate, 'nodes', ['nodes', index, 'alternate']);
    }
    if (node.op === 'loop')
      check(node.loop, 'loops', ['nodes', index, 'loop']);
  });
  app.texts.forEach((text, index) =>
    check(text.binding, 'bindings', ['texts', index, 'binding']),
  );
  app.bindings.forEach((binding, index) => {
    check(binding.target, 'nodes', ['bindings', index, 'target']);
    check(binding.name, 'strings', ['bindings', index, 'name']);
    check(binding.expression, 'expressions', [
      'bindings',
      index,
      'expression',
    ]);
  });
  app.inputs.forEach((input, index) =>
    check(input.name, 'strings', ['inputs', index, 'name']),
  );
  app.events.forEach((event, index) => {
    check(event.target, 'nodes', ['events', index, 'target']);
    check(event.type, 'strings', ['events', index, 'type']);
    check(event.action, 'actions', ['events', index, 'action']);
    event.fields.forEach((field, fieldIndex) =>
      check(field, 'strings', ['events', index, 'fields', fieldIndex]),
    );
    check(event.loop, 'loops', ['events', index, 'loop']);
  });
  app.propPrograms.forEach((program, index) => {
    check(program.target, 'nodes', ['propPrograms', index, 'target']);
    program.writes.forEach((write, writeIndex) => {
      check(write.name, 'strings', [
        'propPrograms',
        index,
        'writes',
        writeIndex,
        'name',
      ]);
      check(write.expression, 'expressions', [
        'propPrograms',
        index,
        'writes',
        writeIndex,
        'expression',
      ]);
      check(write.constant, 'constants', [
        'propPrograms',
        index,
        'writes',
        writeIndex,
        'constant',
      ]);
      if (
        write.expression === undefined &&
        write.constant === undefined
      )
        issue(
          ['propPrograms', index, 'writes', writeIndex],
          'MISSING_PROP_VALUE',
        );
    });
  });
  app.stateSlots.forEach((slot, index) => {
    check(slot.initialExpression, 'expressions', [
      'stateSlots',
      index,
      'initialExpression',
    ]);
    if (slot.frameSlot !== index)
      issue(['stateSlots', index, 'frameSlot'], 'STATE_SLOT_NOT_DENSE');
  });
  const expressionReference = (
    instruction: ExpressionInstruction,
    expressionIndex: number,
    instructionIndex: number,
  ) => {
    const path = [
      'expressions',
      expressionIndex,
      'instructions',
      instructionIndex,
    ];
    if ('constant' in instruction)
      check(instruction.constant, 'constants', [...path, 'constant']);
    if ('state' in instruction)
      check(instruction.state, 'stateSlots', [...path, 'state']);
    if ('field' in instruction)
      check(instruction.field, 'strings', [...path, 'field']);
    if ('host' in instruction)
      check(instruction.host, 'hostSlots', [...path, 'host']);
    if ('predicate' in instruction)
      check(instruction.predicate, 'expressions', [
        ...path,
        'predicate',
      ]);
    if ('mapper' in instruction)
      check(instruction.mapper, 'expressions', [...path, 'mapper']);
  };
  app.expressions.forEach((program, expressionIndex) =>
    program.instructions.forEach((instruction, instructionIndex) => {
      expressionReference(
        instruction,
        expressionIndex,
        instructionIndex,
      );
      if (
        'target' in instruction &&
        instruction.target >= program.instructions.length
      )
        issue(
          [
            'expressions',
            expressionIndex,
            'instructions',
            instructionIndex,
            'target',
          ],
          'INVALID_JUMP_TARGET',
        );
      if (
        instruction.op === 'loadFrame' &&
        instruction.slot >= program.frameSlots
      )
        issue(
          ['expressions', expressionIndex, 'instructions', instructionIndex, 'slot'],
          'FRAME_SLOT_OUT_OF_RANGE',
        );
    }),
  );
  app.actions.forEach((program, actionIndex) => {
    const parameterSlots = new Set<number>();
    program.parameterSlots.forEach((slot, parameterIndex) => {
      if (slot >= program.frameSlots)
        issue(['actions', actionIndex, 'parameterSlots', parameterIndex], 'FRAME_SLOT_OUT_OF_RANGE');
      if (parameterSlots.has(slot))
        issue(['actions', actionIndex, 'parameterSlots', parameterIndex], 'DUPLICATE_PARAMETER_SLOT');
      parameterSlots.add(slot);
    });
    program.instructions.forEach((instruction, instructionIndex) => {
      const path = [
        'actions',
        actionIndex,
        'instructions',
        instructionIndex,
      ];
      if ('expression' in instruction)
        check(instruction.expression, 'expressions', [
          ...path,
          'expression',
        ]);
      if ('state' in instruction)
        check(instruction.state, 'stateSlots', [...path, 'state']);
      if ('action' in instruction)
        check(instruction.action, 'actions', [...path, 'action']);
      if ('input' in instruction)
        check(instruction.input, 'inputs', [...path, 'input']);
      if (instruction.op === 'collectionMutation') {
        check(instruction.key, 'expressions', [...path, 'key']);
        check(instruction.value, 'expressions', [...path, 'value']);
        if (app.inputs[instruction.input]?.kind !== 'collection')
          issue([...path, 'input'], 'COLLECTION_MUTATION_REQUIRES_COLLECTION_INPUT');
        if (instruction.kind === 'keyedRemove' && instruction.value !== undefined)
          issue([...path, 'value'], 'COLLECTION_REMOVE_FORBIDS_VALUE');
        if (instruction.kind !== 'keyedRemove' && instruction.value === undefined)
          issue([...path, 'value'], 'COLLECTION_MUTATION_REQUIRES_VALUE');
      }
      if (
        'target' in instruction &&
        instruction.target >= program.instructions.length
      )
        issue([...path, 'target'], 'INVALID_JUMP_TARGET');
      if (instruction.op === 'capabilityRequest') {
        check(instruction.request.url, 'expressions', [...path, 'request', 'url']);
        check(instruction.request.body, 'expressions', [...path, 'request', 'body']);
        instruction.request.headers.forEach((header, headerIndex) => {
          check(header.name, 'strings', [...path, 'request', 'headers', headerIndex, 'name']);
          check(header.value, 'expressions', [...path, 'request', 'headers', headerIndex, 'value']);
        });
        for (const [name, pc] of [
          ['successPc', instruction.successPc],
          ['failurePc', instruction.failurePc],
          ['finallyPc', instruction.finallyPc],
        ] as const)
          if (pc !== undefined && pc >= program.instructions.length)
            issue([...path, name], 'INVALID_CONTINUATION_TARGET');
        if (instruction.resultSlot >= program.frameSlots)
          issue([...path, 'resultSlot'], 'FRAME_SLOT_OUT_OF_RANGE');
        if (instruction.errorSlot >= program.frameSlots)
          issue([...path, 'errorSlot'], 'FRAME_SLOT_OUT_OF_RANGE');
      }
      if (program.loaderResultState !== undefined)
        check(program.loaderResultState, 'stateSlots', [
          'actions',
          actionIndex,
          'loaderResultState',
        ]);
    });
  });
  app.loops.forEach((loop, index) => {
    check(loop.sourceExpression, 'expressions', [
      'loops',
      index,
      'sourceExpression',
    ]);
    check(loop.keyExpression, 'expressions', [
      'loops',
      index,
      'keyExpression',
    ]);
    check(loop.rowTemplate, 'nodes', ['loops', index, 'rowTemplate']);
    loop.dependencySlots.forEach((slot, slotIndex) =>
      check(slot, 'stateSlots', [
        'loops',
        index,
        'dependencySlots',
        slotIndex,
      ]),
    );
    check(loop.input, 'inputs', ['loops', index, 'input']);
  });
  app.contexts.forEach((context, index) => {
    check(context.parent ?? undefined, 'contexts', [
      'contexts',
      index,
      'parent',
    ]);
    check(context.valueExpression, 'expressions', [
      'contexts',
      index,
      'valueExpression',
    ]);
  });
  app.dependencyEdges.forEach((edge, index) => {
    check(
      edge.source.handle,
      edge.source.kind === 'state'
        ? 'stateSlots'
        : edge.source.kind === 'host'
          ? 'hostSlots'
          : edge.source.kind === 'input'
            ? 'inputs'
            : 'strings',
      ['dependencyEdges', index, 'source', 'handle'],
    );
    if (edge.source.kind === 'rowField')
      check(edge.source.loop, 'loops', [
        'dependencyEdges',
        index,
        'source',
        'loop',
      ]);
    check(
      edge.target.handle,
      edge.target.kind === 'binding'
        ? 'bindings'
        : edge.target.kind === 'propProgram'
          ? 'propPrograms'
          : edge.target.kind === 'conditional'
            ? 'nodes'
            : 'loops',
      ['dependencyEdges', index, 'target', 'handle'],
    );
  });
  return app;
}
