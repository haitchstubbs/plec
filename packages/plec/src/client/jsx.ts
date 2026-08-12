export type PlecChild =
  PlecNode | string | number | boolean | null | undefined | PlecChild[];

declare global {
  namespace JSX {
    interface IntrinsicElements {
      [elementName: string]: any;
    }
  }
}

export interface PlecNode {
  type: string | PlecComponent;
  props: Record<string, unknown>;
}

export type PlecComponent = (props: Record<string, any>) => PlecChild;

export function jsx(
  type: string | PlecComponent,
  props: Record<string, unknown> | null,
  ...children: PlecChild[]
): PlecNode {
  const next = { ...(props ?? {}) } as Record<string, unknown>;
  if (children.length)
    next.children = children.length === 1 ? children[0] : children;
  return { type, props: next };
}

export const jsxs = jsx;
export const Fragment = ({
  children,
}: {
  children?: PlecChild;
}): PlecChild => children;
