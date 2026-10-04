export function installNavigation(state) {
    const onClick = (event) => {
        const target = event.target instanceof Element ? event.target : null;
        const anchor = target?.closest('a[href]');
        if (!anchor ||
            anchor.origin !== window.location.origin ||
            event.button !== 0 ||
            event.metaKey ||
            event.ctrlKey ||
            event.shiftKey ||
            event.altKey)
            return;
        if (!state.router || !state.router.hasRoute(anchor.pathname))
            return;
        event.preventDefault();
        state.router.navigate(`${anchor.pathname}${anchor.search}${anchor.hash}`);
    };
    const onPopState = () => state.router?.reload();
    state.root.addEventListener('click', onClick);
    window.addEventListener('popstate', onPopState);
    state.cleanup.push(() => state.root.removeEventListener('click', onClick), () => window.removeEventListener('popstate', onPopState));
}
