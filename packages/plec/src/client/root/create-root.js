import { rerender } from './rerender';
const roots = new WeakMap();
export function createRoot(root) {
    const state = {
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
        async render(app) {
            state.app = app;
            rerender(state);
            return {
                dispose: () => {
                    if (state.disposed)
                        return;
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
