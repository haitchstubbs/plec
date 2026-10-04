export function jsx(type, props, ...children) {
    const next = { ...(props ?? {}) };
    if (children.length)
        next.children = children.length === 1 ? children[0] : children;
    return { type, props: next };
}
export const jsxs = jsx;
export const Fragment = ({ children, }) => children;
