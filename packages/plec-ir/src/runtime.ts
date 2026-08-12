/** Runtime-safe v0.6 IR contract. This module intentionally has no Zod (or
 * validator) dependency: it is the only IR entry point browser executors may
 * import. Full schema validation belongs to compiler/build tooling. */
export const COMPILED_SUBTREE_VERSION = '0.6' as const;

export type CompiledSubtreeExpression =
  | { kind: 'literal'; value: unknown }
  | { kind: 'state'; stateSlotId: string }
  | { kind: 'derived'; derivedId: string }
  | { kind: 'row'; field: string }
  | { kind: 'event'; field: 'value' | 'checked' | 'rowKey' }
  | {
      kind: 'member';
      object: CompiledSubtreeExpression;
      property: string;
    }
  | {
      kind: 'unary';
      op: '!' | '+' | '-' | 'floor';
      argument: CompiledSubtreeExpression;
    }
  | {
      kind: 'index';
      object: CompiledSubtreeExpression;
      index: CompiledSubtreeExpression;
    }
  | {
      kind: 'binary';
      op:
        '+' | '-' | '*' | '/' | '===' | '!==' | '>' | '>=' | '<' | '<=';
      left: CompiledSubtreeExpression;
      right: CompiledSubtreeExpression;
    }
  | {
      kind: 'conditional';
      test: CompiledSubtreeExpression;
      consequent: CompiledSubtreeExpression;
      alternate: CompiledSubtreeExpression;
    }
  | {
      kind: 'template';
      parts: Array<string | CompiledSubtreeExpression>;
    }
  | { kind: 'array'; items: CompiledSubtreeExpression[] }
  | {
      kind: 'object';
      properties: Array<{
        name: string;
        value: CompiledSubtreeExpression;
      }>;
    };
export type CompiledSubtreeNode =
  | {
      kind: 'element';
      id: string;
      tag: string;
      parentId: string | null;
      attributes: Array<{ name: string; value: string }>;
    }
  | { kind: 'text'; id: string; parentId: string | null; value: string }
  | {
      kind: 'component';
      id: string;
      parentId: string | null;
      templateId: string;
    }
  | {
      kind: 'conditional';
      id: string;
      parentId: string | null;
      testExpressionId: string;
      consequentTemplateId: string;
      alternateTemplateId?: string;
    }
  | {
      kind: 'loop';
      id: string;
      parentId: string | null;
      sourceExpressionId: string;
      keyExpressionId: string;
      rowTemplateId: string;
    };
export interface CompiledSubtreeIr {
  version: typeof COMPILED_SUBTREE_VERSION;
  rootTemplateId: string;
  templates: Array<{
    id: string;
    nodes: CompiledSubtreeNode[];
    rootNodeIds: string[];
  }>;
  expressions: Array<{
    id: string;
    expression: CompiledSubtreeExpression;
  }>;
  stateSlots: Array<{
    id: string;
    templateId: string;
    slot: number;
    initialExpressionId: string;
  }>;
  derived: Array<{
    id: string;
    expressionId: string;
    dependencies: string[];
  }>;
  bindings: Array<{
    id: string;
    templateId: string;
    targetId: string;
    sink: 'text' | 'property' | 'attribute' | 'class';
    name?: string;
    expressionId: string;
  }>;
  transitions: Array<{
    id: string;
    eventType: string;
    targetId: string;
    op?:
      | 'set'
      | 'toggle'
      | 'increment'
      | 'decrement'
      | 'updateField'
      | 'keyedUpdateField'
      | 'keyedInsert'
      | 'keyedRemove'
      | 'keyedMove';
    stateSlotId?: string;
    field?: string;
    keyExpressionId?: string;
    valueExpressionId?: string;
    operations?: Array<
      | {
          kind: 'keyedInsertBatch';
          stateSlotId: string;
          valueExpressionId: string;
        }
      | {
          kind: 'shuffle';
          stateSlotId: string;
          randomSource: { kind: 'seeded'; stateSlotId: string };
        }
      | {
          kind: 'reindexField';
          stateSlotId: string;
          field: string;
          start: number;
        }
    >;
  }>;
  dependencyEdges: Array<{
    fromId: string;
    toId: string;
    kind: string;
  }>;
}

/** Cheap untrusted-artifact guard, not schema validation. It prevents the
 * executor from dereferencing malformed top-level graphs; build tooling runs
 * the complete Zod validation before artifacts are written. */
export function assertRuntimeCompiledSubtreeIr(
  value: unknown,
): CompiledSubtreeIr {
  if (!value || typeof value !== 'object')
    throw new Error('INVALID_COMPILED_SUBTREE_IR');
  const ir = value as Partial<CompiledSubtreeIr>;
  if (
    ir.version !== COMPILED_SUBTREE_VERSION ||
    typeof ir.rootTemplateId !== 'string' ||
    !Array.isArray(ir.templates) ||
    !Array.isArray(ir.expressions)
  )
    throw new Error('INVALID_COMPILED_SUBTREE_IR');
  if (
    !ir.templates.some(
      (template) => template && template.id === ir.rootTemplateId,
    ) ||
    ir.templates.some(
      (template) =>
        !template ||
        typeof template.id !== 'string' ||
        !Array.isArray(template.nodes) ||
        !Array.isArray(template.rootNodeIds),
    )
  )
    throw new Error('INVALID_COMPILED_SUBTREE_IR');
  return {
    ...ir,
    stateSlots: ir.stateSlots ?? [],
    derived: ir.derived ?? [],
    bindings: ir.bindings ?? [],
    transitions: ir.transitions ?? [],
    dependencyEdges: ir.dependencyEdges ?? [],
  } as CompiledSubtreeIr;
}
