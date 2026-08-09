import { z } from "zod";

/** Serializable, row-local expressions.  They are shared by all binding kinds. */
export const ExpressionSchema: z.ZodType<any> = z.lazy(() => z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("literal"), value: z.unknown() }),
  z.object({ kind: z.literal("identifier"), name: z.string() }),
  z.object({ kind: z.literal("member"), object: ExpressionSchema, property: z.string() }),
  z.object({ kind: z.literal("binary"), op: z.string(), left: ExpressionSchema, right: ExpressionSchema }),
  z.object({ kind: z.literal("logical"), op: z.enum(["&&", "||", "??"]), left: ExpressionSchema, right: ExpressionSchema }),
  z.object({ kind: z.literal("conditional"), test: ExpressionSchema, consequent: ExpressionSchema, alternate: ExpressionSchema }),
  z.object({ kind: z.literal("unary"), op: z.enum(["!", "+", "-"]), argument: ExpressionSchema }),
  z.object({ kind: z.literal("template"), parts: z.array(z.union([z.string(), ExpressionSchema])) }),
  z.object({ kind: z.literal("array"), items: z.array(ExpressionSchema) }),
  z.object({ kind: z.literal("object"), entries: z.array(z.object({ key: z.string(), value: ExpressionSchema })) }),
  z.object({ kind: z.literal("intrinsic"), name: z.enum(["clsx", "classnames"]), args: z.array(ExpressionSchema) }),
  z.object({ kind: z.literal("host"), name: z.literal("currentYear") })
]));

export const ExpressionNodeSchema = z.object({ id: z.string(), expression: ExpressionSchema });
export const BindingSchema = z.object({ id: z.string(), kind: z.enum(["text", "attribute", "property"]), targetId: z.string(), attributeName: z.string().optional(), expressionId: z.string().optional(), expression: z.string().optional() });
export const PropWriteSchema = z.object({ name: z.string(), staticValue: z.string().optional(), expressionId: z.string().optional(), kind: z.enum(["attribute", "property", "event", "ref"]).default("attribute") });
export const PropProgramSchema = z.object({ id: z.string(), targetId: z.string(), writes: z.array(PropWriteSchema) });
export const EventSchema = z.object({ id: z.string(), type: z.string().regex(/^[a-z][a-z0-9-]*$/), targetId: z.string(), actionId: z.string(), args: z.array(z.string()).default([]), field: z.string().optional(), callbackName: z.string().optional(), navigate: z.object({ href: z.string(), replace: z.boolean().optional() }).optional(), stopPropagation: z.boolean().optional(), preventDefault: z.boolean().optional() });
export const RefBindingSchema = z.object({ id: z.string(), targetId: z.string(), refId: z.string(), kind: z.enum(["callback", "object"]) });
export const ContextScopeSchema = z.object({ id: z.string(), parentId: z.string().nullable(), values: z.array(z.object({ name: z.string(), expressionId: z.string().optional(), staticValue: z.string().optional() })) });
export const ConditionalSchema = z.object({ id: z.string(), parentId: z.string(), expressionId: z.string(), children: z.array(z.string()) });
export const IslandSchema = z.object({ islandInstanceId: z.string(), componentId: z.string(), placeholderNodeId: z.string(), moduleId: z.string(), exportName: z.string(), props: z.record(z.string(), z.unknown()).default({}) });
/**
 * Values enter the compiled graph through inputs.  Their producer is outside
 * the IR: a React hook controller, observable, WebSocket, or direct delta
 * adapter can all drive the same view projection.
 */
export const CompiledInputSchema = z.object({
  id: z.string(),
  name: z.string(),
  shape: z.discriminatedUnion("kind", [
    z.object({ kind: z.literal("scalar") }),
    z.object({ kind: z.literal("object"), observedPaths: z.array(z.array(z.string())).default([]) }),
    z.object({ kind: z.literal("collection"), keyExpression: z.string(), orderSensitive: z.boolean().default(true), observedRowPaths: z.array(z.array(z.string())).default([]) })
  ])
});
/** @deprecated Kept readable for existing emitted IR while inputs replace it. */
export const QuerySchema = z.object({ id: z.string(), source: z.string(), resultSymbol: z.string().optional() });
export const DependencyEdgeSchema = z.object({ fromId: z.string(), toId: z.string(), kind: z.enum(["input-to-loop", "query-to-loop", "row-field-to-binding", "input-to-binding", "local-state-to-binding", "host-value-to-binding"]) });
export const LocalStateSlotSchema = z.object({ id: z.string(), name: z.string(), initialValue: z.string(), values: z.array(z.string()) });
export const HostValueSchema = z.object({ id: z.string(), kind: z.literal("media-query"), query: z.string() });
export const LifecycleOperationSchema = z.object({ kind: z.enum(["set-property", "set-attribute", "register", "unregister"]), targetId: z.string().optional(), name: z.string().optional(), expressionId: z.string().optional(), staticValue: z.string().optional(), registryId: z.string().optional() });
export const LifecycleEffectSchema = z.object({ id: z.string(), trigger: z.enum(["mount", "unmount", "state-change"]), stateSlotId: z.string().optional(), operations: z.array(LifecycleOperationSchema).default([]) });
export const StateTransitionSchema = z.object({ id: z.string(), eventId: z.string(), stateSlotId: z.string(), kind: z.enum(["set", "toggle"]), expressionId: z.string().optional() });
export const LoopRowSchema = z.object({ id: z.string(), rootElementId: z.string(), keyValue: z.string().optional() });
export const LoopSchema = z.object({ id: z.string(), parentId: z.string(), source: z.string(), itemName: z.string(), indexName: z.string().optional(), rows: z.array(LoopRowSchema).default([]), inputId: z.string().optional(), queryId: z.string().optional(), rowTemplateRootElementId: z.string().optional(), keyExpression: z.string().optional() });
export const RuntimeDeltaSchema = z.discriminatedUnion("type", [z.object({ type: z.literal("update"), inputId: z.string(), rowKey: z.string(), changes: z.record(z.string(), z.unknown()) }), z.object({ type: z.literal("insert"), inputId: z.string(), rowKey: z.string(), row: z.record(z.string(), z.unknown()), beforeRowKey: z.string().nullable().optional() }), z.object({ type: z.literal("remove"), inputId: z.string(), rowKey: z.string() }), z.object({ type: z.literal("move"), inputId: z.string(), rowKey: z.string(), beforeRowKey: z.string().nullable().optional() })]);
export const AttributeSchema = z.object({ name: z.string(), staticValue: z.string().optional(), bindingId: z.string().optional() });
export const ElementSchema = z.object({ id: z.string(), tag: z.string(), parentId: z.string().nullable(), keyValue: z.string().optional(), attributes: z.array(AttributeSchema).default([]), children: z.array(z.string()).default([]) });
export const TextNodeSchema = z.object({ id: z.string(), parentId: z.string(), staticValue: z.string().optional() });
export const ComponentMetadataSchema = z.object({ name: z.string(), moduleId: z.string(), elementIds: z.array(z.string()).default([]), bindingIds: z.array(z.string()).default([]), eventIds: z.array(z.string()).default([]) });
export const ApplicationSchema = z.object({ version: z.literal("0.5"), revision: z.string().optional(), rootElementId: z.string(), elements: z.array(ElementSchema), texts: z.array(TextNodeSchema), inputs: z.array(CompiledInputSchema).default([]), queries: z.array(QuerySchema).default([]), dependencyEdges: z.array(DependencyEdgeSchema).default([]), loops: z.array(LoopSchema).default([]), bindings: z.array(BindingSchema), expressions: z.array(ExpressionNodeSchema).default([]), propPrograms: z.array(PropProgramSchema).default([]), events: z.array(EventSchema).default([]), refs: z.array(RefBindingSchema).default([]), contexts: z.array(ContextScopeSchema).default([]), conditionals: z.array(ConditionalSchema).default([]), localStates: z.array(LocalStateSlotSchema).default([]), hostValues: z.array(HostValueSchema).default([]), lifecycleEffects: z.array(LifecycleEffectSchema).default([]), stateTransitions: z.array(StateTransitionSchema).default([]), islands: z.array(IslandSchema).default([]), components: z.array(ComponentMetadataSchema).default([]) });
export type Expression = z.infer<typeof ExpressionSchema>; export type Binding = z.infer<typeof BindingSchema>; export type EventNode = z.infer<typeof EventSchema>; export type ApplicationIr = z.infer<typeof ApplicationSchema>; export type RuntimeDelta = z.infer<typeof RuntimeDeltaSchema>;
export function validateApplicationIr(ir: unknown): ApplicationIr { return ApplicationSchema.parse(ir); }

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
export function renderStaticApplication(ir: ApplicationIr, host: StaticRenderHostValues = {}): string {
  const elements = new Map(ir.elements.map((node) => [node.id, node]));
  const texts = new Map(ir.texts.map((node) => [node.id, node]));
  const expressions = new Map(ir.expressions.map((node) => [node.id, node.expression]));
  const bindings = new Map(ir.bindings.map((node) => [node.targetId, node]));
  const events = new Map(ir.events.map((node) => [node.targetId, node]));
  const loops = new Map(ir.loops.map((node) => [node.id, node]));
  const voidTags = new Set(["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"]);

  const evaluate = (expression: any): unknown => {
    if (!expression) return undefined;
    if (expression.kind === "literal") return expression.value;
    if (expression.kind === "identifier") return expression.name === "host" ? host : undefined;
    if (expression.kind === "member") {
      const object = evaluate(expression.object) as Record<string, unknown> | undefined;
      return object?.[expression.property];
    }
    if (expression.kind === "binary") {
      const left = evaluate(expression.left);
      const right = evaluate(expression.right);
      return expression.op === "===" ? left === right : expression.op === "!==" ? left !== right : undefined;
    }
    if (expression.kind === "logical") {
      const left = evaluate(expression.left);
      return expression.op === "&&" ? left && evaluate(expression.right) : expression.op === "||" ? left || evaluate(expression.right) : left ?? evaluate(expression.right);
    }
    if (expression.kind === "conditional") return evaluate(expression.test) ? evaluate(expression.consequent) : evaluate(expression.alternate);
    if (expression.kind === "template") return expression.parts.map((part: any) => typeof part === "string" ? part : String(evaluate(part) ?? "")).join("");
    if (expression.kind === "intrinsic") return expression.args.map((argument: any) => evaluate(argument)).filter(Boolean).join(" ");
    return undefined;
  };
  const escapeText = (value: unknown) => String(value ?? "").replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  const escapeAttribute = (value: unknown) => escapeText(value).replace(/"/g, "&quot;").replace(/'/g, "&#39;");

  const renderNode = (id: string): string => {
    const loop = loops.get(id);
    if (loop) return loop.queryId ? "" : loop.rows.map((row) => renderNode(row.rootElementId)).join("");
    const text = texts.get(id);
    if (text) {
      const binding = bindings.get(id);
      const value = binding?.expressionId ? evaluate(expressions.get(binding.expressionId)) : text.staticValue;
      return `<!--runtime-text:${text.id}-->${escapeText(value === undefined ? text.staticValue : value)}`;
    }
    const element = elements.get(id);
    if (!element) throw new Error(`IR references missing node ${id}`);
    const attributes = new Map(element.attributes.filter((attribute) => attribute.staticValue !== undefined).map((attribute) => [attribute.name === "className" ? "class" : attribute.name, attribute.staticValue!]));
    attributes.set("data-runtime-node", element.id);
    const binding = bindings.get(id);
    if (binding?.kind === "attribute" && binding.attributeName && binding.expressionId) {
      const value = evaluate(expressions.get(binding.expressionId));
      if (value !== undefined) attributes.set(binding.attributeName === "className" ? "class" : binding.attributeName, String(value));
    }
    const event = events.get(id);
    if (event) {
      attributes.set("data-runtime-action", event.actionId);
      attributes.set("data-runtime-event", event.type);
      if (event.field) attributes.set("data-runtime-field", event.field);
    }
    const renderedAttributes = [...attributes].map(([name, value]) => ` ${name}="${escapeAttribute(value)}"`).join("");
    if (voidTags.has(element.tag)) return `<${element.tag}${renderedAttributes}>`;
    return `<${element.tag}${renderedAttributes}>${element.children.map(renderNode).join("")}</${element.tag}>`;
  };

  return renderNode(ir.rootElementId);
}
