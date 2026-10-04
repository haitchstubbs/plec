import { currentRendering } from '../root/render-context';
/** Compiler-owned declaration. Runtime execution is supplied by compiled actions. */
export function useMutation(_callback) {
    if (!currentRendering())
        throw new Error('Plec.useMutation can only run while rendering a Plec component.');
    return undefined;
}
