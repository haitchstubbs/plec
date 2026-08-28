import { createRouter } from 'plec';
import { Route as rootRoute } from './routes/index';
import { Route as homeRoute } from './routes/home.route';
import { Route as aboutRoute } from './routes/about.route';
import { Route as todosRoute } from './routes/todos';
import { Route as stressRoute } from './routes/stress.route';
import { Route as notFoundRoute } from './routes/not-found.route';

export const router = createRouter({
  routeTree: rootRoute.addChildren([
    homeRoute,
    aboutRoute,
    todosRoute,
    stressRoute,
    notFoundRoute,
  ]),
});

export type Router = typeof router;

export default router;
