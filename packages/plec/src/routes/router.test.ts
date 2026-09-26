import { afterEach, describe, expect, expectTypeOf, it } from 'vitest';
import {
  createRootRoute,
  createRoute,
  createRouter,
  matchRoutes,
} from './router';
import { withRendering } from '../client/root/render-context';

const View = () => null;
const originalWindow = globalThis.window;

afterEach(() => {
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: originalWindow,
  });
});

function setLocation(pathname: string) {
  const url = new URL(pathname, 'http://localhost');
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: {
      location: {
        pathname: url.pathname,
        search: url.search,
        hash: url.hash,
        href: url.href,
        origin: 'http://localhost',
      },
      history: { pushState() {}, replaceState() {} },
    },
  });
}

describe('code-first routes', () => {
  it('infers parameter names from literal route paths', () => {
    const root = createRootRoute({ component: View });
    const route = createRoute({
      getParentRoute: () => root,
      path: 'projects/$projectId/tasks/$taskId',
      component: View,
    });
    expectTypeOf<ReturnType<typeof route.useParams>>().toEqualTypeOf<{
      projectId: string;
      taskId: string;
    }>();
  });

  it('matches nested, parameter, index, and fallback routes', () => {
    const root = createRootRoute({ component: View });
    const index = createRoute({
      getParentRoute: () => root,
      component: View,
    });
    const projects = createRoute({
      getParentRoute: () => root,
      path: 'projects',
      component: View,
    });
    const project = createRoute({
      getParentRoute: () => projects,
      path: '$projectId',
      component: View,
    });
    const missing = createRoute({
      getParentRoute: () => root,
      path: '*',
      component: View,
    });
    root.addChildren([index, projects, missing]);
    projects.addChildren([project]);

    expect(matchRoutes(root, '/')).toEqual([
      { route: root, params: {} },
      { route: index, params: {} },
    ]);
    expect(matchRoutes(root, '/projects/42')).toEqual([
      { route: root, params: {} },
      { route: projects, params: {} },
      { route: project, params: { projectId: '42' } },
    ]);
    expect(matchRoutes(root, '/missing')).toEqual([
      { route: root, params: {} },
      { route: missing, params: {} },
    ]);
  });

  it('runs loaders before committing their data and exposes failures', async () => {
    setLocation('/todos');
    const root = createRootRoute({ component: View });
    const todos = createRoute({
      getParentRoute: () => root,
      path: 'todos',
      component: View,
      loader: async () => ['first'],
    });
    root.addChildren([todos]);
    const router = createRouter({ routeTree: root });
    router.start();
    expect(router.matches[1]?.status).toBe('pending');
    await Promise.resolve();
    await Promise.resolve();
    expect(router.matches[1]).toMatchObject({
      status: 'ready',
      data: ['first'],
    });
    expect(router.committedMatches[1]).toMatchObject({
      data: ['first'],
    });

    const failing = createRoute({
      getParentRoute: () => root,
      path: 'failure',
      component: View,
      loader: () => {
        throw new Error('unavailable');
      },
    });
    root.addChildren([failing]);
    setLocation('/failure');
    router.reload();
    await Promise.resolve();
    expect(router.matches[1]).toMatchObject({
      status: 'error',
      error: expect.any(Error),
    });
  });

  it('exposes decoded params from matching route', () => {
    const root = createRootRoute({ component: View });
    const project = createRoute({
      getParentRoute: () => root,
      path: '$projectId',
      component: View,
    });
    root.addChildren([project]);
    const router = createRouter({ routeTree: root });
    const state = {
      router,
      activeRouteMatch: {
        route: project,
        params: { projectId: 'a b' },
      },
    } as unknown as Parameters<typeof withRendering>[0];

    expect(withRendering(state, () => project.useParams())).toEqual({
      projectId: 'a b',
    });
  });

  it('parses search values with ordered repeated keys and strict decoding', async () => {
    const { parseSearch } = await import('./router');
    expect(parseSearch('?tag=one&tag=two+words&empty')).toEqual({
      tag: ['one', 'two words'],
      empty: '',
    });
    expect(() => parseSearch('?bad=%')).toThrow(URIError);
  });

  it('exposes search only from the matching route', () => {
    const root = createRootRoute({ component: View });
    const route = createRoute({
      getParentRoute: () => root,
      path: '$projectId',
      component: View,
    });
    root.addChildren([route]);
    const state = {
      activeRouteMatch: {
        route,
        params: { projectId: '42' },
        search: { tag: ['one', 'two'] },
      },
    } as unknown as Parameters<typeof withRendering>[0];
    expect(withRendering(state, () => route.useSearch())).toEqual({
      tag: ['one', 'two'],
    });
    expect(() => withRendering(state, () => root.useSearch())).toThrow(
      'Route.useSearch() can only run while rendering its matching route.',
    );
  });

  it('exposes accumulated nested params and rejects non-matching access', () => {
    const root = createRootRoute({ component: View });
    const organization = createRoute({
      getParentRoute: () => root,
      path: '$organizationId',
      component: View,
    });
    const project = createRoute({
      getParentRoute: () => organization,
      path: '$projectId',
      component: View,
    });
    root.addChildren([organization]);
    organization.addChildren([project]);
    const state = {
      activeRouteMatch: {
        route: project,
        params: { organizationId: 'acme', projectId: '42' },
      },
    } as unknown as Parameters<typeof withRendering>[0];

    expect(withRendering(state, () => project.useParams())).toEqual({
      organizationId: 'acme',
      projectId: '42',
    });
    expect(() =>
      withRendering(state, () => organization.useParams()),
    ).toThrow(
      'Route.useParams() can only run while rendering its matching route.',
    );
  });

  it('updates params after client navigation', async () => {
    setLocation('/projects/old?tag=old');
    const root = createRootRoute({ component: View });
    const projects = createRoute({
      getParentRoute: () => root,
      path: 'projects',
      component: View,
    });
    const project = createRoute({
      getParentRoute: () => projects,
      path: '$projectId',
      component: View,
    });
    root.addChildren([projects]);
    projects.addChildren([project]);
    const router = createRouter({ routeTree: root });
    router.start();
    await Promise.resolve();
    expect(router.matches.at(-1)?.params).toEqual({ projectId: 'old' });
    expect(router.matches.at(-1)?.search).toEqual({ tag: 'old' });

    setLocation('/projects/new?tag=first&tag=second');
    await router.reload();
    expect(router.matches.at(-1)?.params).toEqual({ projectId: 'new' });
    expect(router.matches.at(-1)?.search).toEqual({
      tag: ['first', 'second'],
    });
  });

  it('reloads only the selected route and rejects stale results', async () => {
    setLocation('/todos');
    let calls = 0;
    let resolveFirst!: (value: string[]) => void;
    let resolveSecond!: (value: string[]) => void;
    const root = createRootRoute({ component: View });
    const todos = createRoute({
      getParentRoute: () => root,
      path: 'todos',
      component: View,
      loader: () => {
        calls += 1;
        if (calls === 1) return ['initial'];
        return new Promise<string[]>((resolve) => {
          if (calls === 2) resolveFirst = resolve;
          else resolveSecond = resolve;
        });
      },
    });
    const other = createRoute({
      getParentRoute: () => root,
      path: 'other',
      component: View,
      loader: async () => ['other'],
    });
    root.addChildren([todos, other]);
    const router = createRouter({ routeTree: root });
    router.start();
    for (let index = 0; index < 6; index++) await Promise.resolve();

    const state = {
      router,
      activeRouteMatch: router.matches[1],
    } as Parameters<typeof withRendering>[0];
    const reload = withRendering(state, () => todos.useReload());
    const first = reload();
    const second = reload();
    resolveFirst(['stale']);
    resolveSecond(['fresh']);
    await Promise.all([first, second]);

    expect(router.matches[1]).toMatchObject({
      status: 'ready',
      data: ['fresh'],
    });
    expect(router.matches[0]?.status).toBe('ready');
    expect(router.matches[0]?.data).toBeUndefined();
  });

  it('commits a navigation only after the complete route chain resolves', async () => {
    setLocation('/parent/old');
    let resolveChild!: (value: string) => void;
    const root = createRootRoute({ component: View });
    const parent = createRoute({
      getParentRoute: () => root,
      path: 'parent',
      component: View,
      loader: async () => 'parent',
    });
    const old = createRoute({
      getParentRoute: () => parent,
      path: 'old',
      component: View,
      loader: async () => 'old',
    });
    const next = createRoute({
      getParentRoute: () => parent,
      path: 'next',
      component: View,
      loader: () =>
        new Promise<string>((resolve) => (resolveChild = resolve)),
    });
    root.addChildren([parent]);
    parent.addChildren([old, next]);
    const router = createRouter({ routeTree: root });
    router.start();
    for (let index = 0; index < 8; index++) await Promise.resolve();
    expect(router.committedMatches.at(-1)?.route).toBe(old);

    setLocation('/parent/next');
    router.reload();
    await Promise.resolve();
    await Promise.resolve();

    expect(router.matches.map((match) => match.route)).toEqual([
      root,
      parent,
      next,
    ]);
    expect(router.matches[1]?.status).toBe('ready');
    expect(router.matches[2]?.status).toBe('pending');
    expect(router.committedMatches.map((match) => match.route)).toEqual(
      [root, parent, old],
    );

    resolveChild('next');
    await Promise.resolve();
    await Promise.resolve();
    expect(router.committedMatches.map((match) => match.route)).toEqual(
      [root, parent, next],
    );
  });

  it('does not cancel revalidation of a different route', async () => {
    setLocation('/todos');
    let resolveTodos!: (value: string) => void;
    let resolveOther!: (value: string) => void;
    const root = createRootRoute({ component: View });
    const todos = createRoute({
      getParentRoute: () => root,
      path: 'todos',
      component: View,
      loader: () =>
        new Promise<string>((resolve) => (resolveTodos = resolve)),
    });
    const other = createRoute({
      getParentRoute: () => root,
      path: 'other',
      component: View,
      loader: () =>
        new Promise<string>((resolve) => (resolveOther = resolve)),
    });
    root.addChildren([todos, other]);
    const router = createRouter({ routeTree: root });
    router.matches = [
      { route: root, params: {}, search: {}, status: 'ready' },
      {
        route: todos,
        params: {},
        search: {},
        status: 'ready',
        data: 'initial',
      },
      {
        route: other,
        params: {},
        search: {},
        status: 'ready',
        data: 'initial',
      },
    ];
    setLocation('/todos');
    const first = router.reloadRoute(todos);
    setLocation('/other');
    const second = router.reloadRoute(other);
    resolveTodos('todos');
    resolveOther('other');
    await Promise.all([first, second]);
    expect(
      router.matches.find((match) => match.route === todos)?.data,
    ).toBe('todos');
    expect(
      router.matches.find((match) => match.route === other)?.data,
    ).toBe('other');
  });
});
