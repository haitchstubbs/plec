import { jsx } from '../client/jsx';
import { currentRendering } from '../client/root/render-context';
// Keep aligned with crates/plec-ir/src/limits.rs::MAX_REDIRECT_HOPS.
const MAX_REDIRECT_HOPS = 5;
/** Terminal loader outcome: navigate to an application path. */
export function redirect(to, options = {}) {
    throw new PlecRedirectOutcome(to, options.replace ?? true);
}
/** Terminal loader outcome: the matched route resolves as not found. */
export function notFound() {
    throw new PlecNotFoundOutcome();
}
export class PlecRedirectOutcome extends Error {
    location;
    replace;
    constructor(location, replace) {
        super(`redirect to ${location}`);
        this.location = location;
        this.replace = replace;
        this.name = 'PlecRedirectOutcome';
    }
}
export class PlecNotFoundOutcome extends Error {
    constructor() {
        super('not found');
        this.name = 'PlecNotFoundOutcome';
    }
}
export function createRootRoute(options) {
    return makeRoute(options);
}
export function createRoute(options) {
    const route = makeRoute(options);
    route.parent = options.getParentRoute();
    return route;
}
function makeRoute(options) {
    const route = {
        ...options,
        children: [],
        addChildren(children) {
            route.children.push(...children);
            for (const child of children)
                child.parent = route;
            return route;
        },
        useLoaderData() {
            const state = currentRendering();
            const match = state?.activeRouteMatch;
            if (!state || !match || match.route !== route)
                throw new Error('Route.useLoaderData() can only run while rendering its matching route.');
            return match.data;
        },
        useParams() {
            const state = currentRendering();
            const match = state?.activeRouteMatch;
            if (!state || !match || match.route !== route)
                throw new Error('Route.useParams() can only run while rendering its matching route.');
            return match.params;
        },
        useSearch() {
            const state = currentRendering();
            const match = state?.activeRouteMatch;
            if (!state || !match || match.route !== route)
                throw new Error('Route.useSearch() can only run while rendering its matching route.');
            return match.search;
        },
        useReload() {
            const state = currentRendering();
            const match = state?.activeRouteMatch;
            if (!state || !match || match.route !== route || !state.router)
                throw new Error('Route.useReload() can only run while rendering its matching route.');
            return () => state.router.reloadRoute(route);
        },
    };
    return route;
}
export class PlecRouter {
    routeTree;
    listeners = new Set();
    controller;
    requestVersion = 0;
    routeReloads = new Map();
    started = false;
    redirectHops = 0;
    matches = [];
    committedMatches = [];
    constructor(routeTree) {
        this.routeTree = routeTree;
    }
    start() {
        if (this.started)
            return;
        this.started = true;
        this.redirectHops = 0;
        void this.load(locationFromWindow());
    }
    subscribe(listener) {
        this.listeners.add(listener);
        return () => this.listeners.delete(listener);
    }
    hasRoute(pathname) {
        return Boolean(matchRoutes(this.routeTree, pathname));
    }
    navigate(to, options = {}) {
        this.redirectHops = 0;
        const target = new URL(to, window.location.href);
        if (target.origin !== window.location.origin) {
            window.location.assign(target.href);
            return;
        }
        window.history[options.replace ? 'replaceState' : 'pushState']({}, '', `${target.pathname}${target.search}${target.hash}`);
        void this.load(locationFromWindow());
    }
    reload() {
        this.redirectHops = 0;
        return this.load(locationFromWindow());
    }
    followLoaderRedirect(location, replace) {
        try {
            if (!location.startsWith('/') ||
                location.startsWith('//') ||
                /[\\#\s\u0000-\u001f\u007f]/.test(location)) {
                throw new Error(`Invalid loader redirect target: ${JSON.stringify(location)}`);
            }
            const target = new URL(location, window.location.origin);
            if (target.origin !== window.location.origin)
                throw new Error(`Invalid loader redirect target: ${JSON.stringify(location)}`);
            if (++this.redirectHops > MAX_REDIRECT_HOPS)
                throw new Error(`Loader redirect loop exceeded ${MAX_REDIRECT_HOPS} hops`);
            window.history[replace ? 'replaceState' : 'pushState']({}, '', `${target.pathname}${target.search}`);
            void this.load(locationFromWindow());
        }
        catch (error) {
            const active = this.matches.find((match) => match.status === 'pending');
            if (active) {
                active.status = 'error';
                active.error = error;
            }
            this.emit();
        }
    }
    reloadRoute(route) {
        this.redirectHops = 0;
        const location = locationFromWindow();
        const current = matchRoutes(this.routeTree, location.pathname)?.find((candidate) => candidate.route === route);
        const match = this.matches.find((candidate) => candidate.route === route);
        if (!match || !current || !route.loader)
            return Promise.resolve();
        match.params = current.params;
        match.search = parseSearch(location.search);
        const previous = this.routeReloads.get(route);
        previous?.controller.abort();
        const request = {
            controller: new AbortController(),
            version: (previous?.version ?? 0) + 1,
        };
        this.routeReloads.set(route, request);
        return this.loadMatch(match, location, request.controller, request.version, true, route);
    }
    async load(location) {
        const version = ++this.requestVersion;
        this.controller?.abort();
        const controller = new AbortController();
        this.controller = controller;
        const matches = matchRoutes(this.routeTree, location.pathname) ?? [
            { route: this.routeTree, params: {}, status: 'ready' },
        ];
        const search = parseSearch(location.search);
        this.matches = matches.map((match) => ({
            ...match,
            search,
            status: match.route.loader ? 'pending' : 'ready',
        }));
        // A navigation is committed atomically after every active loader succeeds.
        // Keep the prior chain available while the replacement chain is pending.
        this.emit();
        try {
            for (const match of this.matches) {
                if (!match.route.loader)
                    continue;
                await this.loadMatch(match, location, controller, version, false);
                if (controller.signal.aborted ||
                    this.controller !== controller ||
                    this.requestVersion !== version)
                    return;
                if (match.status === 'error')
                    return;
                if (this.matches.some((candidate) => candidate.status === 'notFound')) {
                    this.committedMatches = this.matches.map((candidate) => ({
                        ...candidate,
                    }));
                    this.emit();
                    return;
                }
            }
            if (this.controller === controller &&
                this.requestVersion === version) {
                this.committedMatches = this.matches.map((match) => ({
                    ...match,
                }));
                this.emit();
            }
        }
        catch (error) {
            if (controller.signal.aborted ||
                this.controller !== controller ||
                this.requestVersion !== version)
                return;
            const failed = this.matches.find((match) => match.status === 'pending');
            if (failed) {
                failed.status = 'error';
                failed.error = error;
            }
            this.emit();
        }
    }
    async loadMatch(match, location, controller, version, commit, reloadRoute) {
        match.status = 'pending';
        match.error = undefined;
        this.emit();
        try {
            const data = await match.route.loader({
                params: match.params,
                location,
                signal: controller.signal,
            });
            if (controller.signal.aborted ||
                (reloadRoute
                    ? this.routeReloads.get(reloadRoute)?.version !== version
                    : this.controller !== controller ||
                        this.requestVersion !== version))
                return;
            match.data = data;
            match.status = 'ready';
            if (commit) {
                if (reloadRoute) {
                    const committed = this.committedMatches.find((candidate) => candidate.route === reloadRoute);
                    if (committed) {
                        Object.assign(committed, match);
                    }
                    else {
                        this.committedMatches = this.matches.map((candidate) => ({
                            ...candidate,
                        }));
                    }
                }
                else {
                    this.committedMatches = this.matches.map((candidate) => ({
                        ...candidate,
                    }));
                }
            }
            this.emit();
        }
        catch (error) {
            if (controller.signal.aborted ||
                (reloadRoute
                    ? this.routeReloads.get(reloadRoute)?.version !== version
                    : this.controller !== controller ||
                        this.requestVersion !== version))
                return;
            // Terminal loader outcomes do not commit the interrupted route.
            if (error instanceof PlecRedirectOutcome) {
                this.followLoaderRedirect(error.location, error.replace);
                return;
            }
            if (error instanceof PlecNotFoundOutcome) {
                this.resolveNotFound(match);
                if (commit) {
                    this.committedMatches = this.matches.map((candidate) => ({
                        ...candidate,
                    }));
                }
                this.emit();
                return;
            }
            match.status = 'error';
            match.error = error;
            this.emit();
        }
    }
    /**
     * Truncate the active route chain at its nearest not-found boundary. If no
     * route declares one, the root match owns the built-in not-found view.
     */
    resolveNotFound(origin) {
        const originIndex = this.matches.indexOf(origin);
        if (originIndex < 0)
            return;
        let ownerIndex = 0;
        for (let index = originIndex; index >= 0; index -= 1) {
            if (this.matches[index]?.route.notFoundComponent) {
                ownerIndex = index;
                break;
            }
        }
        this.matches.splice(ownerIndex + 1);
        const owner = this.matches[ownerIndex];
        if (owner) {
            owner.status = 'notFound';
            owner.error = undefined;
        }
    }
    emit() {
        this.listeners.forEach((listener) => listener());
    }
}
export function createRouter(options) {
    return new PlecRouter(options.routeTree);
}
export const RouterProvider = () => null;
export const Outlet = () => null;
export const Link = ({ to, children, ...props }) => jsx('a', {
    ...props,
    href: to,
    children: children,
});
export function useNavigate() {
    const state = currentRendering();
    if (!state?.router)
        throw new Error('Plec.useNavigate can only run inside RouterProvider.');
    return (options) => state.router.navigate(options.to, options);
}
function locationFromWindow() {
    return {
        pathname: window.location.pathname,
        search: window.location.search,
        hash: window.location.hash,
    };
}
export function matchRoutes(root, pathname) {
    const segments = pathname
        .replace(/^\/+|\/+$/g, '')
        .split('/')
        .filter(Boolean);
    const walk = (route, offset, params) => {
        if (offset === segments.length) {
            const index = route.children.find((child) => !child.path);
            return index
                ? [
                    { route, params },
                    { route: index, params },
                ]
                : [{ route, params }];
        }
        for (const child of route.children) {
            if (!child.path)
                continue;
            if (child.path === '*')
                return [
                    { route, params },
                    { route: child, params },
                ];
            const childSegments = child.path.split('/');
            const next = { ...params };
            if (childSegments.every((part, index) => {
                const value = segments[offset + index];
                if (!value)
                    return false;
                if (part.startsWith('$')) {
                    next[part.slice(1)] = decodeURIComponent(value);
                    return true;
                }
                return part === value;
            })) {
                const nested = walk(child, offset + childSegments.length, next);
                if (nested)
                    return [{ route, params }, ...nested];
            }
        }
        return undefined;
    };
    return walk(root, 0, {});
}
/** Parse URL form query values; repeated keys retain order, malformed escapes throw. */
export function parseSearch(search) {
    const result = {};
    const query = search.startsWith('?') ? search.slice(1) : search;
    for (const pair of query.split('&')) {
        if (!pair)
            continue;
        const separator = pair.indexOf('=');
        const rawKey = separator < 0 ? pair : pair.slice(0, separator);
        const rawValue = separator < 0 ? '' : pair.slice(separator + 1);
        const key = decodeURIComponent(rawKey.replace(/\+/g, ' '));
        const value = decodeURIComponent(rawValue.replace(/\+/g, ' '));
        const previous = result[key];
        if (!Object.prototype.hasOwnProperty.call(result, key)) {
            Object.defineProperty(result, key, {
                value,
                enumerable: true,
                configurable: true,
                writable: true,
            });
        }
        else if (Array.isArray(previous))
            previous.push(value);
        else
            result[key] = [previous, value];
    }
    return result;
}
