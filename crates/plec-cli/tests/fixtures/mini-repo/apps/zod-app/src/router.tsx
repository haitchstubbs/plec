import { createRouter } from 'plec';
import { Route as rootRoute } from './routes/index';

export const router = createRouter({
  routeTree: rootRoute,
});

export type Router = typeof router;

export default router;
