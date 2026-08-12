import { createRoute } from 'plec';
import { NotFoundPage } from './not-found';
import { Route as rootRoute } from './index';

export const Route = createRoute({
  getParentRoute: () => rootRoute,
  path: '*',
  component: NotFoundPage,
});
