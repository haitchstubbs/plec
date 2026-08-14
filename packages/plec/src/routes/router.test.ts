import { afterEach, describe, expect, it } from 'vitest';
import {
  createRootRoute,
  createRoute,
  createRouter,
  matchRoutes,
} from './router';

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
});
