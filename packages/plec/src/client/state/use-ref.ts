import { currentRendering } from '../root/render-context';

export interface PlecRef<T> {
  current: T;
}

export function useRef<T>(initial: T): PlecRef<T> {
  const owner = currentRendering();
  if (!owner)
    throw new Error(
      'Plec.useRef can only run while rendering a Plec component.',
    );
  const key = `ref:${owner.component ?? 'root'}:${owner.hookCursor++}`;
  if (!owner.state.has(key)) owner.state.set(key, { current: initial });
  return owner.state.get(key) as PlecRef<T>;
}

/** A ref whose value is owned by host-node mount/disposal rather than user code. */
export function useHostRef<T extends Element = Element>(): PlecRef<T | null> {
  return useRef<T | null>(null);
}
