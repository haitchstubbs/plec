import { currentRendering } from '../root/render-context';
import { rerender } from '../root/rerender';

export function useState<T>(
  initial: T,
): [T, (next: T | ((current: T) => T)) => void] {
  const owner = currentRendering();
  if (!owner)
    throw new Error(
      'Plec.useState can only run while rendering a Plec component.',
    );
  const key = `${owner.component ?? 'root'}:${owner.hookCursor++}`;
  if (!owner.state.has(key)) owner.state.set(key, initial);
  const set = (next: T | ((current: T) => T)) => {
    const current = owner.state.get(key) as T;
    owner.state.set(
      key,
      typeof next === 'function'
        ? (next as (value: T) => T)(current)
        : next,
    );
    owner.stateUpdates++;
    rerender(owner);
  };
  return [owner.state.get(key) as T, set];
}
