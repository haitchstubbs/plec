import type { PlecNode } from '../jsx';
import { rerender } from './rerender';
import type { PlecController, RootState } from './root-state';

const roots = new WeakMap<Element, RootState>();

export function createRoot(root: Element) {
  const state: RootState = {
    root,
    state: new Map(),
    hookCursor: 0,
    disposed: false,
    mounts: 0,
    domOperations: 0,
    stateUpdates: 0,
    cleanup: [],
    renderCleanup: [],
  };
  roots.set(root, state);
  return {
    async render(app: PlecNode): Promise<PlecController> {
      state.app = app;
      rerender(state);
      return {
        dispose: () => {
          if (state.disposed) return;
          state.disposed = true;
          state.renderCleanup.splice(0).forEach((cleanup) => cleanup());
          state.cleanup.splice(0).forEach((cleanup) => cleanup());
          state.root.replaceChildren();
        },
        diagnostics: () => ({
          mounts: state.mounts,
          domOperations: state.domOperations,
          stateUpdates: state.stateUpdates,
        }),
      };
    },
  };
}
