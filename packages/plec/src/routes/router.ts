import { jsx, type PlecChild, type PlecComponent } from '../client/jsx';
import { currentRendering } from '../client/root/render-context';

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

export type RouteOptions<TData = unknown> = {
  path?: string;
  /** Child graphs mount here in the matching parent graph. */
  outletId?: string;
  component: PlecComponent;
  loader?: (context: LoaderContext) => TData | Promise<TData>;
  pendingComponent?: PlecComponent;
  pendingMode?: PendingMode;
  errorComponent?: PlecComponent;
  /** Static document metadata rendered by the Plec server. */
  meta?: RouteMetadata;
};

export type RouteDefinition<TData = unknown> = RouteOptions<TData> & {
  children: RouteDefinition[];
  parent?: RouteDefinition;
  addChildren(children: RouteDefinition[]): RouteDefinition<TData>;
  useLoaderData(): TData;
  useReload(): () => Promise<void>;
};

export type RouteMatch = {
  route: RouteDefinition;
  params: Record<string, string>;
  data?: unknown;
  status: 'pending' | 'ready' | 'error';
  error?: unknown;
};

type RouteMatchSeed = Pick<RouteMatch, 'route' | 'params'>;

export function createRootRoute<TData = unknown>(
  options: Omit<RouteOptions<TData>, 'path'>,
): RouteDefinition<TData> {
  return makeRoute(options);
}

export function createRoute<TData = unknown>(
  options: RouteOptions<TData> & {
    getParentRoute: () => RouteDefinition;
  },
): RouteDefinition<TData> {
  const route = makeRoute(options);
  route.parent = options.getParentRoute();
  return route;
}

function makeRoute<TData>(
  options: RouteOptions<TData>,
): RouteDefinition<TData> {
  const route: RouteDefinition<TData> = {
    ...options,
    children: [],
    addChildren(children) {
      route.children.push(...children);
      for (const child of children) child.parent = route;
      return route;
    },
    useLoaderData() {
      const state = currentRendering();
      const match = state?.activeRouteMatch;
      if (!state || !match || match.route !== route)
        throw new Error(
          'Route.useLoaderData() can only run while rendering its matching route.',
        );
      return match.data as TData;
    },
    useReload() {
      const state = currentRendering();
      const match = state?.activeRouteMatch;
      if (!state || !match || match.route !== route || !state.router)
        throw new Error(
          'Route.useReload() can only run while rendering its matching route.',
        );
      return () => state.router!.reloadRoute(route);
    },
  };
  return route;
}

export class PlecRouter {
  private listeners = new Set<() => void>();
  private controller?: AbortController;
  private requestVersion = 0;
  private started = false;
  matches: RouteMatch[] = [];
  committedMatches: RouteMatch[] = [];

  constructor(readonly routeTree: RouteDefinition) {}

  start() {
    if (this.started) return;
    this.started = true;
    void this.load(locationFromWindow(), false);
  }

  subscribe(listener: () => void) {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  hasRoute(pathname: string) {
    return Boolean(matchRoutes(this.routeTree, pathname));
  }

  navigate(to: string, options: { replace?: boolean } = {}) {
    const target = new URL(to, window.location.href);
    if (target.origin !== window.location.origin) {
      window.location.assign(target.href);
      return;
    }
    window.history[options.replace ? 'replaceState' : 'pushState'](
      {},
      '',
      `${target.pathname}${target.search}${target.hash}`,
    );
    void this.load(locationFromWindow(), true);
  }

  reload() {
    return this.load(locationFromWindow(), false);
  }

  reloadRoute(route: RouteDefinition) {
    const location = locationFromWindow();
    const current = matchRoutes(
      this.routeTree,
      location.pathname,
    )?.find((candidate) => candidate.route === route);
    const match = this.matches.find(
      (candidate) => candidate.route === route,
    );
    if (!match || !current || !route.loader) return Promise.resolve();
    match.params = current.params;
    return this.loadMatch(match, location);
  }

  private async load(location: PlecLocation, retainCommitted: boolean) {
    const version = ++this.requestVersion;
    this.controller?.abort();
    const controller = new AbortController();
    this.controller = controller;
    const matches = matchRoutes(this.routeTree, location.pathname) ?? [
      { route: this.routeTree, params: {}, status: 'ready' as const },
    ];
    this.matches = matches.map((match) => ({
      ...match,
      status: match.route.loader ? 'pending' : 'ready',
    }));
    if (!retainCommitted) this.committedMatches = [];
    this.emit();
    try {
      for (const match of this.matches) {
        if (!match.route.loader) continue;
        await this.loadMatch(match, location, controller, version);
        if (
          controller.signal.aborted ||
          this.controller !== controller ||
          this.requestVersion !== version
        )
          return;
        if (match.status === 'error') return;
      }
      if (
        this.controller === controller &&
        this.requestVersion === version
      ) {
        this.committedMatches = this.matches.map((match) => ({
          ...match,
        }));
        this.emit();
      }
    } catch (error) {
      if (
        controller.signal.aborted ||
        this.controller !== controller ||
        this.requestVersion !== version
      )
        return;
      const failed = this.matches.find(
        (match) => match.status === 'pending',
      );
      if (failed) {
        failed.status = 'error';
        failed.error = error;
      }
      this.emit();
    }
  }

  private async loadMatch(
    match: RouteMatch,
    location: PlecLocation,
    controller?: AbortController,
    version?: number,
  ) {
    if (!controller) {
      this.controller?.abort();
      controller = new AbortController();
      this.controller = controller;
      version = ++this.requestVersion;
    }
    const requestVersion = version!;
    match.status = 'pending';
    match.error = undefined;
    this.emit();
    try {
      const data = await match.route.loader!({
        params: match.params,
        location,
        signal: controller.signal,
      });
      if (
        controller.signal.aborted ||
        this.controller !== controller ||
        this.requestVersion !== requestVersion
      )
        return;
      match.data = data;
      match.status = 'ready';
      this.committedMatches = this.matches.map((candidate) => ({
        ...candidate,
      }));
      this.emit();
    } catch (error) {
      if (
        controller.signal.aborted ||
        this.controller !== controller ||
        this.requestVersion !== requestVersion
      )
        return;
      match.status = 'error';
      match.error = error;
      this.emit();
    }
  }

  private emit() {
    this.listeners.forEach((listener) => listener());
  }
}

export function createRouter(options: { routeTree: RouteDefinition }) {
  return new PlecRouter(options.routeTree);
}

export const RouterProvider: PlecComponent = () => null;
export const Outlet: PlecComponent = () => null;

export const Link: PlecComponent = ({ to, children, ...props }) =>
  jsx('a', {
    ...props,
    href: to as string,
    children: children as PlecChild,
  });

export function useNavigate() {
  const state = currentRendering();
  if (!state?.router)
    throw new Error(
      'Plec.useNavigate can only run inside RouterProvider.',
    );
  return (options: { to: string; replace?: boolean }) =>
    state.router!.navigate(options.to, options);
}

function locationFromWindow(): PlecLocation {
  return {
    pathname: window.location.pathname,
    search: window.location.search,
    hash: window.location.hash,
  };
}

export function matchRoutes(
  root: RouteDefinition,
  pathname: string,
): RouteMatchSeed[] | undefined {
  const segments = pathname
    .replace(/^\/+|\/+$/g, '')
    .split('/')
    .filter(Boolean);
  const walk = (
    route: RouteDefinition,
    offset: number,
    params: Record<string, string>,
  ): RouteMatchSeed[] | undefined => {
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
      if (!child.path) continue;
      if (child.path === '*')
        return [
          { route, params },
          { route: child, params },
        ];
      const childSegments = child.path.split('/');
      const next = { ...params };
      if (
        childSegments.every((part, index) => {
          const value = segments[offset + index];
          if (!value) return false;
          if (part.startsWith('$')) {
            next[part.slice(1)] = decodeURIComponent(value);
            return true;
          }
          return part === value;
        })
      ) {
        const nested = walk(child, offset + childSegments.length, next);
        if (nested) return [{ route, params }, ...nested];
      }
    }
    return undefined;
  };
  return walk(root, 0, {});
}
