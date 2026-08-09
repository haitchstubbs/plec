import { z } from "zod";
export const BindingSchema = z.object({
    id: z.string(),
    kind: z.enum(["text", "attribute"]),
    targetId: z.string(),
    attributeName: z.string().optional(),
    expression: z.string()
});
export const QuerySchema = z.object({
    id: z.string(),
    source: z.string(),
    resultSymbol: z.string().optional()
});
export const DependencyEdgeSchema = z.object({
    fromId: z.string(),
    toId: z.string(),
    kind: z.enum(["query-to-loop"])
});
export const LoopRowSchema = z.object({
    id: z.string(),
    rootElementId: z.string(),
    keyValue: z.string().optional()
});
export const LoopSchema = z.object({
    id: z.string(),
    parentId: z.string(),
    source: z.string(),
    itemName: z.string(),
    indexName: z.string().optional(),
    rows: z.array(LoopRowSchema).default([])
});
export const AttributeSchema = z.object({
    name: z.string(),
    staticValue: z.string().optional(),
    bindingId: z.string().optional()
});
export const ElementSchema = z.object({
    id: z.string(),
    tag: z.string(),
    parentId: z.string().nullable(),
    keyValue: z.string().optional(),
    attributes: z.array(AttributeSchema).default([]),
    children: z.array(z.string()).default([])
});
export const TextNodeSchema = z.object({
    id: z.string(),
    parentId: z.string(),
    staticValue: z.string().optional()
});
export const ApplicationSchema = z.object({
    version: z.literal("0.1"),
    rootElementId: z.string(),
    elements: z.array(ElementSchema),
    texts: z.array(TextNodeSchema),
    queries: z.array(QuerySchema).default([]),
    dependencyEdges: z.array(DependencyEdgeSchema).default([]),
    loops: z.array(LoopSchema).default([]),
    bindings: z.array(BindingSchema)
});
export function validateApplicationIr(ir) {
    return ApplicationSchema.parse(ir);
}
//# sourceMappingURL=index.js.map