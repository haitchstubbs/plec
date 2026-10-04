import { type PlecComponent } from '../client/jsx';
export type PlecLocation = {
    pathname: string;
    search: string;
    hash: string;
};
export type LoaderContext = {
    params: Record<string, string>;
    location: PlecLocation;
    signal: AbortSignal;
};
export type PendingMode = 'replace' | 'retain';
export type RouteMetadata = {
    title?: string;
    description?: string;
};
type ParamNames<TPath extends string> = TPath extends `${string}$${infer Tail}` ? Tail extends `${infer Name}/${infer Rest}` ? Name | ParamNames<Rest> : Tail extends '' ? never : Tail : never;
export type RouteParams<TPath extends string> = string extends TPath ? Record<string, string> : {
    [Name in ParamNames<TPath>]: string;
};
export type RouteSearch = Record<string, string | string[] | undefined>;
export type RouteOptions<TData = unknown, TPath extends string = string> = {
    path?: TPath;
    /** Child graphs mount here in the matching parent graph. */
    outletId?: string;
    component: PlecComponent;
    loader?: (context: LoaderContext) => TData | Promise<TData>;
    pendingComponent?: PlecComponent;
    pendingMode?: PendingMode;
    errorComponent?: PlecComponent;
    /**
     * Renders when a loader in this route's subtree resolves as not found.
     * The deepest matched route owning a not-found boundary wins; without any
     * route boundary the root boundary applies. SSR answers 404.
     */
    notFoundComponent?: PlecComponent;
    /** Static document metadata rendered by the Plec server. */
    meta?: RouteMetadata;
};
export type RouteDefinition<TData = unknown, TPath extends string = string> = RouteOptions<TData, TPath> & {
    children: RouteDefinition[];
    parent?: RouteDefinition;
    addChildren(children: RouteDefinition[]): RouteDefinition<TData>;
    useLoaderData(): TData;
    useParams(): RouteParams<TPath>;
    useSearch(): RouteSearch;
    useReload(): () => Promise<void>;
};
export type RedirectOptions = {
    /** Replace the current history entry. Defaults to `true`. */
    replace?: boolean;
};
/** Terminal loader outcome: navigate to an application path. */
export declare function redirect(to: string, options?: RedirectOptions): never;
/** Terminal loader outcome: the matched route resolves as not found. */
export declare function notFound(): never;
export declare class PlecRedirectOutcome extends Error {
    readonly location: string;
    readonly replace: boolean;
    constructor(location: string, replace: boolean);
}
export declare class PlecNotFoundOutcome extends Error {
    constructor();
}
export type RouteMatch = {
    route: RouteDefinition;
    params: Record<string, string>;
    search: RouteSearch;
    data?: unknown;
    status: 'pending' | 'ready' | 'error' | 'notFound';
    error?: unknown;
};
type RouteMatchSeed = Pick<RouteMatch, 'route' | 'params'>;
export declare function createRootRoute<TData = unknown>(options: Omit<RouteOptions<TData>, 'path'>): RouteDefinition<TData>;
export declare function createRoute<TData = unknown, TPath extends string = string>(options: RouteOptions<TData, TPath> & {
    getParentRoute: () => RouteDefinition;
}): RouteDefinition<TData, TPath>;
export declare class PlecRouter {
    readonly routeTree: RouteDefinition;
    private listeners;
    private controller?;
    private requestVersion;
    private routeReloads;
    private started;
    private redirectHops;
    matches: RouteMatch[];
    committedMatches: RouteMatch[];
    constructor(routeTree: RouteDefinition);
    start(): void;
    subscribe(listener: () => void): () => boolean;
    hasRoute(pathname: string): boolean;
    navigate(to: string, options?: {
        replace?: boolean;
    }): void;
    reload(): Promise<void>;
    private followLoaderRedirect;
    reloadRoute(route: RouteDefinition): Promise<void>;
    private load;
    private loadMatch;
    /**
     * Truncate the active route chain at its nearest not-found boundary. If no
     * route declares one, the root match owns the built-in not-found view.
     */
    private resolveNotFound;
    private emit;
}
export declare function createRouter(options: {
    routeTree: RouteDefinition;
}): PlecRouter;
export declare const RouterProvider: PlecComponent;
export declare const Outlet: PlecComponent;
export declare const Link: PlecComponent;
export declare function useNavigate(): (options: {
    to: string;
    replace?: boolean;
}) => void;
export declare function matchRoutes(root: RouteDefinition, pathname: string): RouteMatchSeed[] | undefined;
/** Parse URL form query values; repeated keys retain order, malformed escapes throw. */
export declare function parseSearch(search: string): RouteSearch;
export {};
