import { currentRendering } from '../root/render-context';
export function useRef(initial) {
    const owner = currentRendering();
    if (!owner)
        throw new Error('Plec.useRef can only run while rendering a Plec component.');
    const key = `ref:${owner.component ?? 'root'}:${owner.hookCursor++}`;
    if (!owner.state.has(key))
        owner.state.set(key, { current: initial });
    return owner.state.get(key);
}
/** A ref whose value is owned by host-node mount/disposal rather than user code. */
export function useHostRef() {
    return useRef(null);
}
