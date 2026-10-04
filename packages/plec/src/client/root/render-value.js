import { Outlet, RouterProvider } from '../../routes/router';
import { renderOutlet, renderRouteView, RouteView, } from '../../routes/render-outlet';
export function flatten(children) {
    return children.flatMap((child) => Array.isArray(child) ? flatten(child) : [child]);
}
export function applyProps(element, props) {
    for (const [name, raw] of Object.entries(props)) {
        if (name === 'children' ||
            name === 'ref' ||
            raw === undefined ||
            raw === null ||
            raw === false)
            continue;
        if (name === 'className') {
            element.setAttribute('class', String(raw));
            continue;
        }
        if (name === 'style' && typeof raw === 'object') {
            Object.assign(element.style, raw);
            continue;
        }
        if (name.startsWith('on') && typeof raw === 'function') {
            element.addEventListener(name.slice(2).toLowerCase(), raw);
            continue;
        }
        if (name in element && typeof raw === 'boolean') {
            element[name] = raw;
            continue;
        }
        element.setAttribute(name, String(raw));
    }
    const ref = props.ref;
    if (typeof ref === 'function')
        ref(element);
    else if (ref)
        ref.current = element;
}
export function renderValue(value, state, document) {
    if (value === null ||
        value === undefined ||
        value === false ||
        value === true)
        return [];
    if (Array.isArray(value))
        return flatten(value).flatMap((child) => renderValue(child, state, document));
    if (typeof value === 'string' || typeof value === 'number')
        return [document.createTextNode(String(value))];
    if (typeof value.type === 'function') {
        if (value.type === RouterProvider) {
            const router = value.props.router;
            if (!router)
                throw new Error('RouterProvider requires a router.');
            state.router = router;
            router.start();
            return renderRouteView(router.matches[0], state, document);
        }
        if (value.type === Outlet)
            return renderOutlet(value.props, state, document);
        if (value.type === RouteView)
            return [];
        const previousComponent = state.component;
        const previousCursor = state.hookCursor;
        state.component = value.type.name || 'anonymous';
        state.hookCursor = 0;
        const result = value.type(value.props);
        state.component = previousComponent;
        state.hookCursor = previousCursor;
        return renderValue(result, state, document);
    }
    const element = value.type === 'svg' ||
        value.type === 'path' ||
        value.type === 'circle' ||
        value.type === 'rect' ||
        value.type === 'line' ||
        value.type === 'polyline' ||
        value.type === 'polygon'
        ? document.createElementNS('http://www.w3.org/2000/svg', value.type)
        : document.createElement(value.type);
    applyProps(element, value.props);
    for (const child of renderValue(value.props.children, state, document))
        element.appendChild(child);
    return [element];
}
