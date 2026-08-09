import { z } from "zod";
export declare const BindingSchema: z.ZodObject<{
    id: z.ZodString;
    kind: z.ZodEnum<{
        text: "text";
        attribute: "attribute";
    }>;
    targetId: z.ZodString;
    attributeName: z.ZodOptional<z.ZodString>;
    expression: z.ZodString;
}, z.core.$strip>;
export declare const QuerySchema: z.ZodObject<{
    id: z.ZodString;
    source: z.ZodString;
    resultSymbol: z.ZodOptional<z.ZodString>;
}, z.core.$strip>;
export declare const DependencyEdgeSchema: z.ZodObject<{
    fromId: z.ZodString;
    toId: z.ZodString;
    kind: z.ZodEnum<{
        "query-to-loop": "query-to-loop";
    }>;
}, z.core.$strip>;
export declare const LoopRowSchema: z.ZodObject<{
    id: z.ZodString;
    rootElementId: z.ZodString;
    keyValue: z.ZodOptional<z.ZodString>;
}, z.core.$strip>;
export declare const LoopSchema: z.ZodObject<{
    id: z.ZodString;
    parentId: z.ZodString;
    source: z.ZodString;
    itemName: z.ZodString;
    indexName: z.ZodOptional<z.ZodString>;
    rows: z.ZodDefault<z.ZodArray<z.ZodObject<{
        id: z.ZodString;
        rootElementId: z.ZodString;
        keyValue: z.ZodOptional<z.ZodString>;
    }, z.core.$strip>>>;
}, z.core.$strip>;
export declare const AttributeSchema: z.ZodObject<{
    name: z.ZodString;
    staticValue: z.ZodOptional<z.ZodString>;
    bindingId: z.ZodOptional<z.ZodString>;
}, z.core.$strip>;
export declare const ElementSchema: z.ZodObject<{
    id: z.ZodString;
    tag: z.ZodString;
    parentId: z.ZodNullable<z.ZodString>;
    keyValue: z.ZodOptional<z.ZodString>;
    attributes: z.ZodDefault<z.ZodArray<z.ZodObject<{
        name: z.ZodString;
        staticValue: z.ZodOptional<z.ZodString>;
        bindingId: z.ZodOptional<z.ZodString>;
    }, z.core.$strip>>>;
    children: z.ZodDefault<z.ZodArray<z.ZodString>>;
}, z.core.$strip>;
export declare const TextNodeSchema: z.ZodObject<{
    id: z.ZodString;
    parentId: z.ZodString;
    staticValue: z.ZodOptional<z.ZodString>;
}, z.core.$strip>;
export declare const ApplicationSchema: z.ZodObject<{
    version: z.ZodLiteral<"0.1">;
    rootElementId: z.ZodString;
    elements: z.ZodArray<z.ZodObject<{
        id: z.ZodString;
        tag: z.ZodString;
        parentId: z.ZodNullable<z.ZodString>;
        keyValue: z.ZodOptional<z.ZodString>;
        attributes: z.ZodDefault<z.ZodArray<z.ZodObject<{
            name: z.ZodString;
            staticValue: z.ZodOptional<z.ZodString>;
            bindingId: z.ZodOptional<z.ZodString>;
        }, z.core.$strip>>>;
        children: z.ZodDefault<z.ZodArray<z.ZodString>>;
    }, z.core.$strip>>;
    texts: z.ZodArray<z.ZodObject<{
        id: z.ZodString;
        parentId: z.ZodString;
        staticValue: z.ZodOptional<z.ZodString>;
    }, z.core.$strip>>;
    queries: z.ZodDefault<z.ZodArray<z.ZodObject<{
        id: z.ZodString;
        source: z.ZodString;
        resultSymbol: z.ZodOptional<z.ZodString>;
    }, z.core.$strip>>>;
    dependencyEdges: z.ZodDefault<z.ZodArray<z.ZodObject<{
        fromId: z.ZodString;
        toId: z.ZodString;
        kind: z.ZodEnum<{
            "query-to-loop": "query-to-loop";
        }>;
    }, z.core.$strip>>>;
    loops: z.ZodDefault<z.ZodArray<z.ZodObject<{
        id: z.ZodString;
        parentId: z.ZodString;
        source: z.ZodString;
        itemName: z.ZodString;
        indexName: z.ZodOptional<z.ZodString>;
        rows: z.ZodDefault<z.ZodArray<z.ZodObject<{
            id: z.ZodString;
            rootElementId: z.ZodString;
            keyValue: z.ZodOptional<z.ZodString>;
        }, z.core.$strip>>>;
    }, z.core.$strip>>>;
    bindings: z.ZodArray<z.ZodObject<{
        id: z.ZodString;
        kind: z.ZodEnum<{
            text: "text";
            attribute: "attribute";
        }>;
        targetId: z.ZodString;
        attributeName: z.ZodOptional<z.ZodString>;
        expression: z.ZodString;
    }, z.core.$strip>>;
}, z.core.$strip>;
export type Binding = z.infer<typeof BindingSchema>;
export type QueryNode = z.infer<typeof QuerySchema>;
export type DependencyEdge = z.infer<typeof DependencyEdgeSchema>;
export type Attribute = z.infer<typeof AttributeSchema>;
export type ElementNode = z.infer<typeof ElementSchema>;
export type LoopRow = z.infer<typeof LoopRowSchema>;
export type LoopNode = z.infer<typeof LoopSchema>;
export type TextNode = z.infer<typeof TextNodeSchema>;
export type ApplicationIr = z.infer<typeof ApplicationSchema>;
export declare function validateApplicationIr(ir: unknown): ApplicationIr;
//# sourceMappingURL=index.d.ts.map