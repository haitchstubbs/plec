import { createRouter } from 'plec';
import { Route as rootRoute } from './routes/index';
import { Route as listRoute } from './routes/list.route';
import { Route as aboutRoute } from './routes/about.route';

export const router = createRouter({
  routeTree: rootRoute.addChildren([listRoute, aboutRoute]),
});

export type Router = typeof router;

export default router;
