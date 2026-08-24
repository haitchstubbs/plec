import { z } from 'zod';

/** Serializable, row-local expressions.  They are shared by all binding kinds. */
export const ExpressionSchema: z.ZodType<any> = z.lazy(() =>
  z.discriminatedUnion('kind', [
    z.object({ kind: z.literal('literal'), value: z.unknown() }),
    z.object({ kind: z.literal('identifier'), name: z.string() }),
    z.object({
      kind: z.literal('member'),
      object: ExpressionSchema,
      property: z.string(),
    }),
    z.object({
      kind: z.literal('binary'),
      op: z.string(),
      left: ExpressionSchema,
      right: ExpressionSchema,
    }),
    z.object({
      kind: z.literal('logical'),
      op: z.enum(['&&', '||', '??']),
      left: ExpressionSchema,
      right: ExpressionSchema,
    }),
    z.object({
      kind: z.literal('conditional'),
      test: ExpressionSchema,
      consequent: ExpressionSchema,
      alternate: ExpressionSchema,
    }),
    z.object({
      kind: z.literal('unary'),
      op: z.enum(['!', '+', '-']),
      argument: ExpressionSchema,
    }),
    z.object({
      kind: z.literal('template'),
      parts: z.array(z.union([z.string(), ExpressionSchema])),
    }),
    z.object({
      kind: z.literal('array'),
      items: z.array(
        z.union([
          ExpressionSchema,
          z.object({
            kind: z.literal('spread'),
            value: ExpressionSchema,
          }),
        ]),
      ),
    }),
    z.object({
      kind: z.literal('object'),
      properties: z
        .array(
          z.discriminatedUnion('kind', [
            z.object({
              kind: z.literal('entry'),
              key: z.string(),
              value: ExpressionSchema,
            }),
            z.object({
              kind: z.literal('spread'),
              value: ExpressionSchema,
            }),
          ]),
        )
        .optional(),
      entries: z
        .array(z.object({ key: z.string(), value: ExpressionSchema }))
        .optional(),
    }),
    z.object({
      kind: z.literal('intrinsic'),
      name: z.enum(['clsx', 'classnames', 'encodeURIComponent']),
      args: z.array(ExpressionSchema),
    }),
    z.object({
      kind: z.literal('method'),
      receiver: ExpressionSchema,
      name: z.enum(['trim', 'toLowerCase', 'toUpperCase', 'includes']),
      args: z.array(ExpressionSchema).default([]),
    }),
    z.object({
      kind: z.literal('collection'),
      op: z.enum(['filter', 'map']),
      source: ExpressionSchema,
      itemName: z.string().min(1),
      indexName: z.string().min(1).optional(),
      expression: ExpressionSchema,
    }),
    z.object({
      kind: z.literal('host'),
      name: z.enum(['currentYear', 'media-query']),
      query: z.string().optional(),
    }),
    z.object({ kind: z.literal('context'), contextId: z.string() }),
    z.object({
      kind: z.literal('host-element-read'),
      refId: z.string(),
      capability: z.discriminatedUnion('kind', [
        z.object({
          kind: z.literal('property'),
          name: z.enum([
            'value',
            'name',
            'disabled',
            'checked',
            'tagName',
          ]),
        }),
        z.object({ kind: z.literal('is-active') }),
        z.object({
          kind: z.literal('closest'),
          selector: z.string().regex(/^[a-z][a-z0-9-]*$/),
        }),
      ]),
    }),
  ]),
);

export const ExpressionNodeSchema = z.object({
  id: z.string(),
  expression: ExpressionSchema,
});
export const BindingSchema = z.object({
  id: z.string(),
  kind: z.enum(['text', 'attribute', 'property']),
  targetId: z.string(),
  attributeName: z.string().optional(),
  expressionId: z.string().optional(),
  expression: z.string().optional(),
});
export const PropWriteSchema = z.object({
  name: z.string(),
  staticValue: z.string().optional(),
  expressionId: z.string().optional(),
  kind: z
    .enum(['attribute', 'property', 'event', 'ref', 'spread'])
    .default('attribute'),
});
export const PropProgramSchema = z.object({
  id: z.string(),
  targetId: z.string(),
  writes: z.array(PropWriteSchema),
});
export const EventSchema = z.object({
  id: z.string(),
  type: z.string().regex(/^[a-z][a-z0-9-]*$/),
  targetId: z.string(),
  actionId: z.string(),
  args: z.array(z.string()).default([]),
  field: z.string().optional(),
  /** Row events retain their loop identity so WASM can reconstruct the
   * item scope from the keyed mounted row without a JavaScript closure. */
  loopId: z.string().optional(),
  navigate: z
    .object({ href: z.string(), replace: z.boolean().optional() })
    .optional(),
  stopPropagation: z.boolean().optional(),
  preventDefault: z.boolean().optional(),
});
export const RefBindingSchema = z.object({
  id: z.string(),
  targetId: z.string(),
  refId: z.string(),
  kind: z.enum(['callback', 'object']),
});
/** Opaque runtime-owned references to rendered host elements. Browser code
 * owns Elements; IR can only name finite capabilities over these handles. */
export const HostElementRefSchema = z.object({
  id: z.string(),
  targetId: z.string(),
  attachments: z.array(z.string()).default([]),
});
export const HostElementReadSchema = z.object({
  id: z.string(),
  refId: z.string(),
  capability: z.discriminatedUnion('kind', [
    z.object({
      kind: z.literal('property'),
      name: z.enum(['value', 'name', 'disabled', 'checked', 'tagName']),
    }),
    z.object({ kind: z.literal('is-active') }),
    z.object({
      kind: z.literal('closest'),
      selector: z.string().regex(/^[a-z][a-z0-9-]*$/),
    }),
  ]),
});
export const ResourceOperationSchema = z.object({
  kind: z.enum(['upsert', 'remove']),
  controllerId: z.string(),
  sourceId: z.string(),
  valueExpressionId: z.string().optional(),
});
/** The fetch boundary is deliberately data-only. The runtime serializes a JSON
 * body itself and exposes only the decoded result/error records to programs. */
export const FetchRequestSchema = z.object({
  url: ExpressionSchema,
  method: z.enum(['GET', 'POST', 'PATCH', 'DELETE']).default('GET'),
  headers: z.record(z.string(), ExpressionSchema).default({}),
  jsonBody: ExpressionSchema.optional(),
  decode: z.enum(['json', 'text', 'empty']).default('json'),
  requireOk: z.boolean().default(true),
});
/** Action programs are executable data, never serialized JavaScript.  A
 * capability request describes an environment service; renderer work remains
 * in lifecycle/effect operations. */
export const ActionOperationSchema: z.ZodTypeAny = z.lazy(() =>
  z.discriminatedUnion('kind', [
    z.object({
      kind: z.literal('set-state'),
      stateSlotId: z.string(),
      value: ExpressionSchema,
    }),
    z.object({
      kind: z.literal('if'),
      test: ExpressionSchema,
      consequent: z.array(ActionOperationSchema),
      alternate: z.array(ActionOperationSchema),
    }),
    z.object({ kind: z.literal('prevent-default') }),
    z.object({
      kind: z.literal('return'),
      value: ExpressionSchema.optional(),
    }),
    z.object({
      kind: z.literal('collection'),
      operation: z.enum(['append', 'keyed-replace', 'keyed-remove']),
      inputId: z.string(),
      key: ExpressionSchema.optional(),
      value: ExpressionSchema.optional(),
    }),
    z.object({
      kind: z.literal('capability-request'),
      capability: z.literal('network.fetch'),
      request: FetchRequestSchema,
      continuationId: z.string(),
      /** Names made available only while the matching continuation runs. */
      successResultName: z.string().default('result'),
      failureErrorName: z.string().default('error'),
      success: z.array(ActionOperationSchema).default([]),
      failure: z.array(ActionOperationSchema).default([]),
      finally: z.array(ActionOperationSchema).default([]),
    }),
    // A graph boundary carries invocation contract only. Captures stay private
    // to the action and graph instance which define them.
    z.object({
      kind: z.literal('invoke-action-ref'),
      actionId: z.string(),
      parameters: z.array(ExpressionSchema).default([]),
      result: z.object({ type: z.string() }).optional(),
    }),
  ]),
);
/** Values permitted at an action boundary. `event` is the finite record
 * extracted by the renderer, never a DOM Event or JavaScript closure. */
export const ActionParameterSchema = z.object({
  name: z.string().min(1),
  type: z.enum(['event', 'string', 'boolean', 'number', 'json']),
});
/** Serializable callable semantics used for callbacks carried through props,
 * context, and ref attachment. Captures are local expression ids and are
 * never exposed by a cross-graph invocation contract. */
export const CompiledActionSchema = z.object({
  id: z.string(),
  parameters: z.array(ActionParameterSchema).default([]),
  captures: z.array(z.string()).default([]),
  operations: z.array(ActionOperationSchema).default([]),
  result: z
    .object({
      type: z.enum(['void', 'string', 'boolean', 'number', 'json']),
    })
    .default({ type: 'void' }),
});
/** A provider is a virtual node: it scopes its value over `children` without
 * producing an extra DOM element. Context ids are declaration identities, not
 * component or package names. */
export const ContextScopeSchema = z.object({
  id: z.string(),
  contextId: z.string().default('legacy'),
  parentId: z.string().nullable(),
  valueExpressionId: z.string().optional(),
  values: z
    .array(
      z.object({
        name: z.string(),
        expressionId: z.string().optional(),
        staticValue: z.string().optional(),
      }),
    )
    .default([]),
  children: z.array(z.string()).default([]),
});
export const ContextDefinitionSchema = z.object({
  id: z.string(),
  defaultExpressionId: z.string(),
});
export const ConditionalSchema = z.object({
  id: z.string(),
  parentId: z.string(),
  expressionId: z.string(),
  consequent: z.array(z.string()).default([]),
  alternate: z.array(z.string()).default([]),
});
export const IslandSchema = z.object({
  islandInstanceId: z.string(),
  componentId: z.string(),
  placeholderNodeId: z.string(),
  moduleId: z.string(),
  exportName: z.string(),
  props: z.record(z.string(), z.unknown()).default({}),
});
/**
 * Values enter the compiled graph through inputs.  Their producer is outside
 * the IR: a React hook controller, observable, WebSocket, or direct delta
 * adapter can all drive the same view projection.
 */
export const CompiledInputSchema = z.object({
  id: z.string(),
  name: z.string(),
  shape: z.discriminatedUnion('kind', [
    z.object({ kind: z.literal('scalar') }),
    z.object({
      kind: z.literal('object'),
      observedPaths: z.array(z.array(z.string())).default([]),
    }),
    z.object({
      kind: z.literal('collection'),
      keyExpression: z.string(),
      orderSensitive: z.boolean().default(true),
      observedRowPaths: z.array(z.array(z.string())).default([]),
    }),
  ]),
});
/** @deprecated Kept readable for existing emitted IR while inputs replace it. */
export const QuerySchema = z.object({
  id: z.string(),
  source: z.string(),
  resultSymbol: z.string().optional(),
});
export const DependencyEdgeSchema = z.object({
  fromId: z.string(),
  toId: z.string(),
  kind: z.enum([
    'input-to-loop',
    'query-to-loop',
    'row-field-to-binding',
    'input-to-binding',
    'local-state-to-binding',
    'host-value-to-binding',
  ]),
});
export const LocalStateSlotSchema = z.object({
  id: z.string(),
  name: z.string(),
  initialValue: z.string(),
  values: z.array(z.string()),
});
export const HostValueSchema = z.object({
  id: z.string(),
  kind: z.literal('media-query'),
  query: z.string(),
});
/** A persistent compiled application shell may expose one or more named roots
 * for independently compiled route graphs. The shell remains runtime-owned. */
export const RouteOutletSchema = z.object({
  id: z.string(),
  elementId: z.string(),
});
export const LayoutMetadataSchema = z.object({
  routeOutlets: z.array(RouteOutletSchema).default([]),
});
export const LifecycleOperationSchema = z.object({
  kind: z.enum([
    'set-property',
    'set-attribute',
    'register',
    'unregister',
  ]),
  targetId: z.string().optional(),
  name: z.string().optional(),
  expressionId: z.string().optional(),
  staticValue: z.string().optional(),
  registryId: z.string().optional(),
});
export const LifecycleEffectSchema = z.object({
  id: z.string(),
  phase: z.enum(['layout', 'effect']).default('effect'),
  trigger: z.enum(['mount', 'unmount', 'state-change']),
  dependencies: z.array(z.string()).default([]),
  stateSlotId: z.string().optional(),
  operations: z
    .array(z.union([LifecycleOperationSchema, ResourceOperationSchema]))
    .default([]),
});
export const StateTransitionSchema = z.object({
  id: z.string(),
  eventId: z.string(),
  stateSlotId: z.string(),
  kind: z.enum(['set', 'toggle']),
  expressionId: z.string().optional(),
});
export const LoopRowSchema = z.object({
  id: z.string(),
  rootElementId: z.string(),
  keyValue: z.string().optional(),
});
export const LoopSchema = z.object({
  id: z.string(),
  parentId: z.string(),
  source: z.string(),
  itemName: z.string(),
  indexName: z.string().optional(),
  rows: z.array(LoopRowSchema).default([]),
  inputId: z.string().optional(),
  queryId: z.string().optional(),
  rowTemplateRootElementId: z.string().optional(),
  keyExpression: z.string().optional(),
});
export const RuntimeDeltaSchema = z.discriminatedUnion('type', [
  z.object({
    type: z.literal('update'),
    inputId: z.string(),
    rowKey: z.string(),
    changes: z.record(z.string(), z.unknown()),
  }),
  z.object({
    type: z.literal('insert'),
    inputId: z.string(),
    rowKey: z.string(),
    row: z.record(z.string(), z.unknown()),
    beforeRowKey: z.string().nullable().optional(),
  }),
  z.object({
    type: z.literal('remove'),
    inputId: z.string(),
    rowKey: z.string(),
  }),
  z.object({
    type: z.literal('move'),
    inputId: z.string(),
    rowKey: z.string(),
    beforeRowKey: z.string().nullable().optional(),
  }),
]);
export const AttributeSchema = z.object({
  name: z.string(),
  staticValue: z.string().optional(),
  bindingId: z.string().optional(),
});
export const ElementSchema = z.object({
  id: z.string(),
  tag: z.string(),
  parentId: z.string().nullable(),
  keyValue: z.string().optional(),
  attributes: z.array(AttributeSchema).default([]),
  children: z.array(z.string()).default([]),
});
export const TextNodeSchema = z.object({
  id: z.string(),
  parentId: z.string(),
  staticValue: z.string().optional(),
});
export const ComponentMetadataSchema = z.object({
  name: z.string(),
  moduleId: z.string(),
  elementIds: z.array(z.string()).default([]),
  bindingIds: z.array(z.string()).default([]),
  eventIds: z.array(z.string()).default([]),
});
export const ApplicationSchema = z.object({
  version: z.literal('0.8'),
  revision: z.string().optional(),
  rootElementId: z.string(),
  elements: z.array(ElementSchema),
  texts: z.array(TextNodeSchema),
  inputs: z.array(CompiledInputSchema).default([]),
  queries: z.array(QuerySchema).default([]),
  dependencyEdges: z.array(DependencyEdgeSchema).default([]),
  loops: z.array(LoopSchema).default([]),
  bindings: z.array(BindingSchema),
  expressions: z.array(ExpressionNodeSchema).default([]),
  propPrograms: z.array(PropProgramSchema).default([]),
  events: z.array(EventSchema).default([]),
  refs: z.array(RefBindingSchema).default([]),
  hostElementRefs: z.array(HostElementRefSchema).default([]),
  hostElementReads: z.array(HostElementReadSchema).default([]),
  actions: z.array(CompiledActionSchema).default([]),
  contexts: z.array(ContextScopeSchema).default([]),
  contextDefinitions: z.array(ContextDefinitionSchema).default([]),
  conditionals: z.array(ConditionalSchema).default([]),
  localStates: z.array(LocalStateSlotSchema).default([]),
  hostValues: z.array(HostValueSchema).default([]),
  lifecycleEffects: z.array(LifecycleEffectSchema).default([]),
  stateTransitions: z.array(StateTransitionSchema).default([]),
  islands: z.array(IslandSchema).default([]),
  components: z.array(ComponentMetadataSchema).default([]),
  layout: LayoutMetadataSchema.optional(),
});
export type Expression = z.infer<typeof ExpressionSchema>;
export type Binding = z.infer<typeof BindingSchema>;
export type EventNode = z.infer<typeof EventSchema>;
export type ApplicationIr = z.infer<typeof ApplicationSchema>;
export type RuntimeDelta = z.infer<typeof RuntimeDeltaSchema>;
export function validateApplicationIr(ir: unknown): ApplicationIr {
  return ApplicationSchema.parse(ir);
}

/** A compiled, immutable component definition. It carries no mount identity;
 * GraphInstance identity is assigned by PlecRuntime from topology at mount. */
export const ComponentGraphSchema = ApplicationSchema.extend({
  graphId: z.string(),
  componentId: z.string(),
  componentName: z.string(),
  moduleId: z.string(),
  capabilities: z
    .array(
      z.object({
        id: z.string(),
        kind: z.enum([
          'network',
          'persistence',
          'authentication',
          'history',
        ]),
        requestSchema: z.unknown(),
        responseSchema: z.unknown(),
        failureSchema: z.unknown(),
      }),
    )
    .default([]),
});
export type ComponentGraph = z.infer<typeof ComponentGraphSchema>;

export const RouteManifestEntrySchema = z.object({
  id: z.string().optional(),
  parentId: z.string().optional(),
  path: z.string(),
  graphId: z.string(),
  pendingGraphId: z.string().optional(),
  pendingMode: z.enum(['replace', 'retain']).default('replace'),
  errorGraphId: z.string().optional(),
  /** Index into this route graph's executable action table. */
  loaderAction: z.number().int().nonnegative().optional(),
  outletId: z.string().default('main'),
});
export const PlecRouteManifestSchema = z.object({
  version: z.literal(3),
  revision: z.string(),
  rootGraphId: z.string(),
  routes: z.array(RouteManifestEntrySchema),
});
export type PlecRouteManifest = z.infer<typeof PlecRouteManifestSchema>;

export function validateComponentGraph(value: unknown): ComponentGraph {
  return ComponentGraphSchema.parse(value);
}

/**
 * Version 0.6 is the deliberately small execution language for compiled Plec
 * subtrees.  It is separate from the legacy application graph above so that
 * adding persistent state does not accidentally turn the baseline renderer
 * into a reconciler.
 */
export const CompiledSubtreeExpressionSchema: z.ZodType<any> = z.lazy(
  () =>
    z.discriminatedUnion('kind', [
      z.object({ kind: z.literal('literal'), value: z.unknown() }),
      z.object({ kind: z.literal('state'), stateSlotId: z.string() }),
      z.object({ kind: z.literal('derived'), derivedId: z.string() }),
      z.object({ kind: z.literal('row'), field: z.string() }),
      z.object({
        kind: z.literal('event'),
        field: z.enum(['value', 'checked', 'rowKey']),
      }),
      z.object({
        kind: z.literal('member'),
        object: CompiledSubtreeExpressionSchema,
        property: z.string(),
      }),
      z.object({
        kind: z.literal('unary'),
        op: z.enum(['!', '+', '-', 'floor']),
        argument: CompiledSubtreeExpressionSchema,
      }),
      z.object({
        kind: z.literal('index'),
        object: CompiledSubtreeExpressionSchema,
        index: CompiledSubtreeExpressionSchema,
      }),
      z.object({
        kind: z.literal('binary'),
        op: z.enum([
          '+',
          '-',
          '*',
          '/',
          '===',
          '!==',
          '>',
          '>=',
          '<',
          '<=',
        ]),
        left: CompiledSubtreeExpressionSchema,
        right: CompiledSubtreeExpressionSchema,
      }),
      z.object({
        kind: z.literal('conditional'),
        test: CompiledSubtreeExpressionSchema,
        consequent: CompiledSubtreeExpressionSchema,
        alternate: CompiledSubtreeExpressionSchema,
      }),
      z.object({
        kind: z.literal('template'),
        parts: z.array(
          z.union([z.string(), CompiledSubtreeExpressionSchema]),
        ),
      }),
      z.object({
        kind: z.literal('array'),
        items: z.array(CompiledSubtreeExpressionSchema),
      }),
      z.object({
        kind: z.literal('object'),
        properties: z.array(
          z.object({
            name: z.string(),
            value: CompiledSubtreeExpressionSchema,
          }),
        ),
      }),
    ]),
);
export const CompiledSubtreeNodeSchema = z.discriminatedUnion('kind', [
  z.object({
    kind: z.literal('element'),
    id: z.string(),
    tag: z.string(),
    parentId: z.string().nullable(),
    attributes: z
      .array(z.object({ name: z.string(), value: z.string() }))
      .default([]),
  }),
  z.object({
    kind: z.literal('text'),
    id: z.string(),
    parentId: z.string().nullable(),
    value: z.string().default(''),
  }),
  z.object({
    kind: z.literal('component'),
    id: z.string(),
    parentId: z.string().nullable(),
    templateId: z.string(),
  }),
  z.object({
    kind: z.literal('conditional'),
    id: z.string(),
    parentId: z.string().nullable(),
    testExpressionId: z.string(),
    consequentTemplateId: z.string(),
    alternateTemplateId: z.string().optional(),
  }),
  z.object({
    kind: z.literal('loop'),
    id: z.string(),
    parentId: z.string().nullable(),
    sourceExpressionId: z.string(),
    keyExpressionId: z.string(),
    rowTemplateId: z.string(),
  }),
]);
export const CompiledSubtreeTemplateSchema = z.object({
  id: z.string(),
  nodes: z.array(CompiledSubtreeNodeSchema),
  rootNodeIds: z.array(z.string()),
});
export const CompiledSubtreeStateSlotSchema = z.object({
  id: z.string(),
  templateId: z.string(),
  slot: z.number().int().nonnegative(),
  initialExpressionId: z.string(),
});
export const CompiledSubtreeDerivedSchema = z.object({
  id: z.string(),
  expressionId: z.string(),
  dependencies: z.array(z.string()),
});
export const CompiledSubtreeBindingSchema = z.object({
  id: z.string(),
  templateId: z.string(),
  targetId: z.string(),
  sink: z.enum(['text', 'property', 'attribute', 'class']),
  name: z.string().optional(),
  expressionId: z.string(),
});
export const CompiledSubtreeTransactionOperationSchema =
  z.discriminatedUnion('kind', [
    z.object({
      kind: z.literal('keyedInsertBatch'),
      stateSlotId: z.string(),
      valueExpressionId: z.string(),
    }),
    z.object({
      kind: z.literal('shuffle'),
      stateSlotId: z.string(),
      randomSource: z.object({
        kind: z.literal('seeded'),
        stateSlotId: z.string(),
      }),
    }),
    z.object({
      kind: z.literal('reindexField'),
      stateSlotId: z.string(),
      field: z.string(),
      start: z.number().int(),
    }),
  ]);
export const CompiledSubtreeTransitionSchema = z
  .object({
    id: z.string(),
    eventType: z.string(),
    targetId: z.string(),
    op: z
      .enum([
        'set',
        'toggle',
        'increment',
        'decrement',
        'updateField',
        'keyedUpdateField',
        'keyedInsert',
        'keyedRemove',
        'keyedMove',
      ])
      .optional(),
    stateSlotId: z.string().optional(),
    field: z.string().optional(),
    keyExpressionId: z.string().optional(),
    valueExpressionId: z.string().optional(),
    operations: z
      .array(CompiledSubtreeTransactionOperationSchema)
      .min(1)
      .optional(),
  })
  .refine(
    (transition) =>
      Boolean(transition.op) !== Boolean(transition.operations),
    'A transition must define either op or operations.',
  );
export const CompiledSubtreeDependencyEdgeSchema = z.object({
  fromId: z.string(),
  toId: z.string(),
  kind: z.enum([
    'state-to-derived',
    'derived-to-derived',
    'state-to-binding',
    'derived-to-binding',
    'state-to-region',
    'derived-to-region',
  ]),
});
export const CompiledSubtreeIrSchema = z.object({
  version: z.literal('0.6'),
  rootTemplateId: z.string(),
  templates: z.array(CompiledSubtreeTemplateSchema),
  expressions: z.array(
    z.object({
      id: z.string(),
      expression: CompiledSubtreeExpressionSchema,
    }),
  ),
  stateSlots: z.array(CompiledSubtreeStateSlotSchema).default([]),
  derived: z.array(CompiledSubtreeDerivedSchema).default([]),
  bindings: z.array(CompiledSubtreeBindingSchema).default([]),
  transitions: z.array(CompiledSubtreeTransitionSchema).default([]),
  dependencyEdges: z
    .array(CompiledSubtreeDependencyEdgeSchema)
    .default([]),
});
export type CompiledSubtreeIr = z.infer<typeof CompiledSubtreeIrSchema>;
export function validateCompiledSubtreeIr(
  ir: unknown,
): CompiledSubtreeIr {
  const parsed = CompiledSubtreeIrSchema.parse(ir);
  const derived = new Map(
    parsed.derived.map((node) => [node.id, node]),
  );
  const visiting = new Set<string>();
  const visited = new Set<string>();
  const visit = (id: string, path: string[]) => {
    if (visited.has(id)) return;
    if (visiting.has(id))
      throw new Error(
        `DERIVED_DEPENDENCY_CYCLE: ${[...path, id].join(' -> ')}`,
      );
    const node = derived.get(id);
    if (!node) throw new Error(`MISSING_DERIVED: ${id}`);
    visiting.add(id);
    for (const dependency of node.dependencies)
      if (derived.has(dependency)) visit(dependency, [...path, id]);
    visiting.delete(id);
    visited.add(id);
  };
  for (const node of parsed.derived) visit(node.id, []);
  return parsed;
}

/**
 * Public, serializable boundaries between independently mounted PLEC graphs.
 * This deliberately describes values, not runtime objects: graph instances
 * cannot pass DOM nodes, closures, refs, or reactive handles to one another.
 */
export const PlecValueTypeSchema: z.ZodType<any> = z.lazy(() =>
  z.discriminatedUnion('kind', [
    z.object({ kind: z.literal('null') }),
    z.object({ kind: z.literal('boolean') }),
    z.object({ kind: z.literal('number') }),
    z.object({ kind: z.literal('string') }),
    z.object({ kind: z.literal('array'), item: PlecValueTypeSchema }),
    z.object({
      kind: z.literal('object'),
      fields: z.record(z.string(), PlecValueTypeSchema),
    }),
  ]),
);
export type PlecValueType = z.infer<typeof PlecValueTypeSchema>;

export const GraphInputSchema = z.object({
  id: z.string().min(1),
  type: PlecValueTypeSchema,
  required: z.boolean(),
  default: z.unknown().optional(),
});
export const GraphOutputSchema = z.object({
  id: z.string().min(1),
  payloadType: PlecValueTypeSchema,
});
export const GraphCommandSchema = z.object({
  id: z.string().min(1),
  capability: z.string().min(1),
  payloadType: PlecValueTypeSchema,
});
export const OutletAcceptedInputSchema = z.object({
  id: z.string().min(1),
  type: PlecValueTypeSchema,
});
export const OutletChildContractSchema = z.object({
  inputs: z.array(OutletAcceptedInputSchema).default([]),
  outputs: z.array(GraphOutputSchema).default([]),
  commands: z.array(GraphCommandSchema).default([]),
});
export const GraphOutletSchema = z.object({
  id: z.string().min(1),
  regionId: z.string().min(1),
  accepts: OutletChildContractSchema,
});
export const GraphInterfaceSchema = z.object({
  inputs: z.array(GraphInputSchema).default([]),
  outputs: z.array(GraphOutputSchema).default([]),
  commands: z.array(GraphCommandSchema).default([]),
  outlets: z.array(GraphOutletSchema).default([]),
});
export const PlecGraphArtifactSchema = z.object({
  graphId: z.string().min(1),
  revision: z.string().min(1),
  interface: GraphInterfaceSchema,
  ir: z.unknown(),
});
export type GraphInput = z.infer<typeof GraphInputSchema>;
export type GraphOutput = z.infer<typeof GraphOutputSchema>;
export type GraphCommand = z.infer<typeof GraphCommandSchema>;
export type OutletAcceptedInput = z.infer<
  typeof OutletAcceptedInputSchema
>;
export type OutletChildContract = z.infer<
  typeof OutletChildContractSchema
>;
export type GraphInterface = z.infer<typeof GraphInterfaceSchema>;
export type PlecGraphArtifact = z.infer<typeof PlecGraphArtifactSchema>;

export function plecValueTypeEquals(
  left: PlecValueType,
  right: PlecValueType,
): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}

export function isPlecValue(
  value: unknown,
  type: PlecValueType,
): boolean {
  if (type.kind === 'null') return value === null;
  if (
    type.kind === 'boolean' ||
    type.kind === 'number' ||
    type.kind === 'string'
  )
    return typeof value === type.kind;
  if (type.kind === 'array')
    return (
      Array.isArray(value) &&
      value.every((item) => isPlecValue(item, type.item))
    );
  if (
    value === null ||
    typeof value !== 'object' ||
    Array.isArray(value) ||
    Object.getPrototypeOf(value) !== Object.prototype
  )
    return false;
  const object = value as Record<string, unknown>;
  return (
    Object.keys(object).length === Object.keys(type.fields).length &&
    Object.entries(type.fields).every(
      ([key, field]) =>
        Object.hasOwn(object, key) && isPlecValue(object[key], field),
    )
  );
}

/** Parse the artifact and verify its public boundary is internally sound. */
export function validatePlecGraphArtifact(
  value: unknown,
): PlecGraphArtifact {
  const artifact = PlecGraphArtifactSchema.parse(value);
  const unique = (kind: string, values: Array<{ id: string }>) => {
    const seen = new Set<string>();
    for (const entry of values) {
      if (seen.has(entry.id))
        throw new Error(
          `DUPLICATE_GRAPH_INTERFACE_ID: ${kind}:${entry.id}`,
        );
      seen.add(entry.id);
    }
  };
  unique('input', artifact.interface.inputs);
  unique('output', artifact.interface.outputs);
  unique('command', artifact.interface.commands);
  unique('outlet', artifact.interface.outlets);
  for (const input of artifact.interface.inputs)
    if (
      input.default !== undefined &&
      !isPlecValue(input.default, input.type)
    )
      throw new Error(`INVALID_INPUT_DEFAULT: ${input.id}`);
  return artifact;
}

/** Values that are known while rendering the static, server-side application shell. */
export interface StaticRenderHostValues {
  currentYear?: number | string;
  location?: { pathname: string };
}

/**
 * Render the non-query portion of an application graph without depending on a
 * DOM or React. Query loops deliberately render as empty containers: their
 * rows are owned by the fresh client-side runtime after hydration.
 */
export function renderStaticApplication(
  ir: ApplicationIr,
  host: StaticRenderHostValues = {},
): string {
  const elements = new Map(ir.elements.map((node) => [node.id, node]));
  const texts = new Map(ir.texts.map((node) => [node.id, node]));
  const expressions = new Map(
    ir.expressions.map((node) => [node.id, node.expression]),
  );
  const bindings = new Map(
    ir.bindings.map((node) => [node.targetId, node]),
  );
  const events = new Map(
    ir.events.map((node) => [node.targetId, node]),
  );
  const loops = new Map(ir.loops.map((node) => [node.id, node]));
  const voidTags = new Set([
    'area',
    'base',
    'br',
    'col',
    'embed',
    'hr',
    'img',
    'input',
    'link',
    'meta',
    'param',
    'source',
    'track',
    'wbr',
  ]);

  const evaluate = (expression: any): unknown => {
    if (!expression) return undefined;
    if (expression.kind === 'literal') return expression.value;
    if (expression.kind === 'identifier')
      return expression.name === 'host' ? host : undefined;
    if (expression.kind === 'member') {
      const object = evaluate(expression.object) as
        Record<string, unknown> | undefined;
      return object?.[expression.property];
    }
    if (expression.kind === 'binary') {
      const left = evaluate(expression.left);
      const right = evaluate(expression.right);
      return expression.op === '==='
        ? left === right
        : expression.op === '!=='
          ? left !== right
          : undefined;
    }
    if (expression.kind === 'logical') {
      const left = evaluate(expression.left);
      return expression.op === '&&'
        ? left && evaluate(expression.right)
        : expression.op === '||'
          ? left || evaluate(expression.right)
          : (left ?? evaluate(expression.right));
    }
    if (expression.kind === 'conditional')
      return evaluate(expression.test)
        ? evaluate(expression.consequent)
        : evaluate(expression.alternate);
    if (expression.kind === 'template')
      return expression.parts
        .map((part: any) =>
          typeof part === 'string'
            ? part
            : String(evaluate(part) ?? ''),
        )
        .join('');
    if (expression.kind === 'intrinsic')
      return expression.args
        .map((argument: any) => evaluate(argument))
        .filter(Boolean)
        .join(' ');
    return undefined;
  };
  const escapeText = (value: unknown) =>
    String(value ?? '')
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;');
  const escapeAttribute = (value: unknown) =>
    escapeText(value).replace(/"/g, '&quot;').replace(/'/g, '&#39;');

  const renderNode = (id: string): string => {
    const loop = loops.get(id);
    if (loop)
      return loop.queryId
        ? ''
        : loop.rows
            .map((row) => renderNode(row.rootElementId))
            .join('');
    const text = texts.get(id);
    if (text) {
      const binding = bindings.get(id);
      const value = binding?.expressionId
        ? evaluate(expressions.get(binding.expressionId))
        : text.staticValue;
      return `<!--runtime-text:${text.id}-->${escapeText(value === undefined ? text.staticValue : value)}`;
    }
    const element = elements.get(id);
    if (!element) throw new Error(`IR references missing node ${id}`);
    const attributes = new Map(
      element.attributes
        .filter((attribute) => attribute.staticValue !== undefined)
        .map((attribute) => [
          attribute.name === 'className' ? 'class' : attribute.name,
          attribute.staticValue!,
        ]),
    );
    attributes.set('data-runtime-node', element.id);
    const binding = bindings.get(id);
    if (
      binding?.kind === 'attribute' &&
      binding.attributeName &&
      binding.expressionId
    ) {
      const value = evaluate(expressions.get(binding.expressionId));
      if (value !== undefined)
        attributes.set(
          binding.attributeName === 'className'
            ? 'class'
            : binding.attributeName,
          String(value),
        );
    }
    const event = events.get(id);
    if (event) {
      attributes.set('data-runtime-action', event.actionId);
      attributes.set('data-runtime-event', event.type);
      if (event.field)
        attributes.set('data-runtime-field', event.field);
    }
    const renderedAttributes = [...attributes]
      .map(([name, value]) => ` ${name}="${escapeAttribute(value)}"`)
      .join('');
    if (voidTags.has(element.tag))
      return `<${element.tag}${renderedAttributes}>`;
    return `<${element.tag}${renderedAttributes}>${element.children.map(renderNode).join('')}</${element.tag}>`;
  };

  return renderNode(ir.rootElementId);
}
