import { installNavigation } from '../effects/navigation';
import { withRendering } from './render-context';
import { renderValue } from './render-value';
export function rerender(state) {
    if (state.disposed || !state.app)
        return;
    const focused = focusSnapshot(state);
    state.renderCleanup.splice(0).forEach((cleanup) => cleanup());
    withRendering(state, () => state.root.replaceChildren(...renderValue(state.app, state, state.root.ownerDocument ?? document)));
    restoreFocus(state, focused);
    if (state.mounts === 0) {
        installNavigation(state);
        if (state.router && !state.routerSubscribed) {
            state.routerSubscribed = true;
            state.cleanup.push(state.router.subscribe(() => rerender(state)));
        }
    }
    state.mounts++;
    state.domOperations++;
    state.root.dispatchEvent(new CustomEvent('plec:render'));
}
function focusSnapshot(state) {
    const document = state.root.ownerDocument ?? window.document;
    const active = document.activeElement;
    if (!active?.id || !state.root.contains(active))
        return undefined;
    return {
        id: active.id,
        selectionStart: active.selectionStart,
        selectionEnd: active.selectionEnd,
    };
}
function restoreFocus(state, focused) {
    if (!focused)
        return;
    const document = state.root.ownerDocument ?? window.document;
    const input = document.getElementById(focused.id);
    if (!input || !state.root.contains(input))
        return;
    input.focus();
    if (focused.selectionStart !== null && focused.selectionEnd !== null)
        input.setSelectionRange(focused.selectionStart, focused.selectionEnd);
}
