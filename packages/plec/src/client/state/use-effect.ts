import { currentRendering } from '../root/render-context';

/** DOM-only lifecycle hook. The compiler accepts only its documented declarative subset. */
export function useEffect(
  effect: () => void | (() => void),
  _dependencies: readonly unknown[] = [],
): void {
  const owner = currentRendering();
  if (!owner)
    throw new Error(
      'Plec.useEffect can only run while rendering a Plec component.',
    );
  let active = true;
  let cleanup: void | (() => void);
  queueMicrotask(() => {
    if (active) cleanup = effect();
  });
  owner.renderCleanup.push(() => {
    active = false;
    if (typeof cleanup === 'function') cleanup();
  });
  owner.hookCursor++;
}
