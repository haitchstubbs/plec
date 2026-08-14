import { jsx, type PlecComponent } from '../client/jsx';
import { applyProps, renderValue } from '../client/root/render-value';
import type { RootState } from '../client/root/root-state';
import type { RouteMatch } from './router';

export const RouteView = () => null;

export function renderRouteView(
  match: RouteMatch,
  state: RootState,
  document: Document,
): Node[] {
  const previous = state.activeRouteMatch;
  state.activeRouteMatch = match;
  try {
    return renderValue(jsx(match.route.component, {}), state, document);
  } finally {
    state.activeRouteMatch = previous;
  }
}

export function renderOutlet(
  props: Record<string, unknown>,
  state: RootState,
  document: Document,
): Node[] {
  const outlet = document.createElement('main');
  applyProps(outlet, props);
  outlet.dataset.plecRouteOutlet = String(props.id ?? 'main');
  const router = state.router;
  const parent = state.activeRouteMatch;
  if (!router || !parent) return [outlet];
  const index = router.matches.indexOf(parent);
  const next = router.matches[index + 1];
  if (!next) return [outlet];
  let match = next;
  if (next.status === 'pending' && next.route.pendingMode === 'retain')
    match = router.committedMatches[index + 1] ?? next;
  if (next.status === 'pending' && match === next) {
    const Pending = next.route.pendingComponent ?? DefaultPending;
    outlet.append(...renderValue(jsx(Pending, {}), state, document));
  } else if (next.status === 'error') {
    const ErrorView =
      nearestErrorComponent(router.matches, index + 1) ?? DefaultError;
    outlet.append(
      ...renderValue(
        jsx(ErrorView, {
          error: next.error,
          retry: () => router.reload(),
        }),
        state,
        document,
      ),
    );
  } else outlet.append(...renderRouteView(match, state, document));
  return [outlet];
}

function nearestErrorComponent(matches: RouteMatch[], from: number) {
  for (let index = from; index >= 0; index -= 1) {
    const component = matches[index]?.route.errorComponent;
    if (component) return component;
  }
}

export function DefaultPending() {
  return jsx('p', {
    role: 'status',
    'aria-busy': true,
    children: 'Loading…',
  });
}

export const DefaultError: PlecComponent = ({ retry }) => {
  const retryPage = retry as () => void;
  return jsx('section', {
    role: 'alert',
    children: [
      jsx('p', { children: 'Could not load this page.' }),
      jsx('button', {
        type: 'button',
        onClick: retryPage,
        children: 'Try again',
      }),
    ],
  });
};
