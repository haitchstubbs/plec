import { rerender } from '../root/rerender';
import type { RootState } from '../root/root-state';

export function installNavigation(state: RootState): void {
  const onClick = (event: MouseEvent) => {
    const target =
      event.target instanceof Element ? event.target : null;
    const anchor = target?.closest<HTMLAnchorElement>('a[href]');
    if (
      !anchor ||
      anchor.origin !== window.location.origin ||
      event.button !== 0 ||
      event.metaKey ||
      event.ctrlKey ||
      event.shiftKey ||
      event.altKey
    )
      return;
    if (!state.router || !state.router.hasRoute(anchor.pathname))
      return;
    event.preventDefault();
    state.router.navigate(
      `${anchor.pathname}${anchor.search}${anchor.hash}`,
    );
  };
  const onPopState = () => state.router?.reload();
  state.root.addEventListener('click', onClick as EventListener);
  window.addEventListener('popstate', onPopState);
  state.cleanup.push(
    () =>
      state.root.removeEventListener('click', onClick as EventListener),
    () => window.removeEventListener('popstate', onPopState),
  );
}
