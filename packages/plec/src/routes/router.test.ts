import { afterEach, describe, expect, it } from 'vitest';
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
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: {
      location: {
        pathname,
        search: '',
        hash: '',
        href: `http://localhost${pathname}`,
        origin: 'http://localhost',
      },
      history: { pushState() {}, replaceState() {} },
    },
  });
}

describe('code-first routes', () => {
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
      { route: root, params: {}, status: 'ready' },
      { route: todos, params: {}, status: 'ready', data: 'initial' },
      { route: other, params: {}, status: 'ready', data: 'initial' },
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
