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
    await Promise.resolve();
    await Promise.resolve();

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
});
