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

export type RouteOptions<TData = unknown> = {
  path?: string;
  /** Child graphs mount here in the matching parent graph. */
  outletId?: string;
  component: PlecComponent;
  loader?: (context: LoaderContext) => TData | Promise<TData>;
  pendingComponent?: PlecComponent;
  pendingMode?: PendingMode;
  errorComponent?: PlecComponent;
};

export type RouteDefinition<TData = unknown> = RouteOptions<TData> & {
  children: RouteDefinition[];
  parent?: RouteDefinition;
  addChildren(children: RouteDefinition[]): RouteDefinition<TData>;
  useLoaderData(): TData;
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
  };
  return route;
}

export class PlecRouter {
  private listeners = new Set<() => void>();
  private controller?: AbortController;
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
    void this.load(locationFromWindow(), false);
  }

  private async load(location: PlecLocation, retainCommitted: boolean) {
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
        const data = await match.route.loader({
          params: match.params,
          location,
          signal: controller.signal,
        });
        if (controller.signal.aborted || this.controller !== controller)
          return;
        match.data = data;
        match.status = 'ready';
        this.emit();
      }
      if (this.controller === controller) {
        this.committedMatches = this.matches.map((match) => ({
          ...match,
        }));
        this.emit();
      }
    } catch (error) {
      if (controller.signal.aborted || this.controller !== controller)
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
