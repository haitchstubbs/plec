export interface CompilerDiagnostic {
    code: string;
    message: string;
    severity: "error" | "warning";
}
interface BindingNode {
    id: string;
    kind: "text" | "attribute";
    targetId: string;
    attributeName?: string;
    expression: string;
    expressionId?: string;
}
interface ExpressionNode {
    id: string;
    expression: unknown;
}
interface EventNode {
    id: string;
    type: "change" | "click";
    targetId: string;
    actionId: string;
    args: string[];
    field?: string;
    navigate?: {
        href: string;
        replace?: boolean;
    };
}
interface IslandNode {
    islandInstanceId: string;
    componentId: string;
    placeholderNodeId: string;
    moduleId: string;
    exportName: string;
    props: Record<string, unknown>;
}
interface ComponentMetadata {
    name: string;
    moduleId: string;
    elementIds: string[];
    bindingIds: string[];
    eventIds: string[];
}
interface QueryNode {
    id: string;
    source: string;
    resultSymbol?: string;
}
interface CompiledInputNode {
    id: string;
    name: string;
    shape: {
        kind: "scalar";
    } | {
        kind: "object";
        observedPaths: string[][];
    } | {
        kind: "collection";
        keyExpression: string;
        orderSensitive: boolean;
        observedRowPaths: string[][];
    };
}
interface DependencyEdge {
    fromId: string;
    toId: string;
    kind: "input-to-loop" | "query-to-loop" | "row-field-to-binding" | "input-to-binding" | "local-state-to-binding" | "host-value-to-binding";
}
interface LoopRowNode {
    id: string;
    rootElementId: string;
    keyValue?: string;
}
interface LoopNode {
    id: string;
    parentId: string;
    source: string;
    itemName: string;
    indexName?: string;
    rows: LoopRowNode[];
    inputId?: string;
    queryId?: string;
    rowTemplateRootElementId?: string;
    keyExpression?: string;
}
interface AttributeNode {
    name: string;
    staticValue?: string;
    bindingId?: string;
}
interface ElementNode {
    id: string;
    tag: string;
    parentId: string | null;
    keyValue?: string;
    attributes: AttributeNode[];
    children: string[];
}
interface TextNode {
    id: string;
    parentId: string;
    staticValue?: string;
}
interface ApplicationIr {
    version: "0.4";
    revision?: string;
    rootElementId: string;
    elements: ElementNode[];
    texts: TextNode[];
    inputs: CompiledInputNode[];
    queries: QueryNode[];
    dependencyEdges: DependencyEdge[];
    loops: LoopNode[];
    bindings: BindingNode[];
    expressions: ExpressionNode[];
    events: EventNode[];
    localStates: Array<{
        id: string;
        name: string;
        initialValue: string;
        values: string[];
    }>;
    hostValues: Array<{
        id: string;
        kind: "media-query";
        query: string;
    }>;
    lifecycleEffects: Array<any>;
    stateTransitions: Array<any>;
    toggles: Array<any>;
    islands: IslandNode[];
    components: ComponentMetadata[];
}
export interface CompileOptions {
    mode?: "strict" | "lenient";
    rootComponent?: string;
    moduleId?: string;
    applicationRevision?: string;
    modules?: Array<{
        id: string;
        source: string;
    }>;
    islandComponents?: string[];
}
export interface CompileResult {
    ir: ApplicationIr;
    diagnostics: CompilerDiagnostic[];
}
export declare function compile(source: string, options?: CompileOptions): CompileResult;
export {};
//# sourceMappingURL=index.d.ts.map