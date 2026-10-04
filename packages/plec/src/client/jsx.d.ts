export type PlecChild = PlecNode | string | number | boolean | null | undefined | PlecChild[];
declare global {
    namespace JSX {
        interface IntrinsicElements {
            [elementName: string]: any;
        }
    }
}
export declare namespace JSX {
    interface IntrinsicElements {
        [elementName: string]: any;
    }
}
export interface PlecNode {
    type: string | PlecComponent;
    props: Record<string, unknown>;
}
export type PlecComponent = (props: Record<string, any>) => PlecChild;
export declare function jsx(type: string | PlecComponent, props: Record<string, unknown> | null, ...children: PlecChild[]): PlecNode;
export declare const jsxs: typeof jsx;
export declare const Fragment: ({ children, }: {
    children?: PlecChild;
}) => PlecChild;
