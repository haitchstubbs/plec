import { createRoute } from 'plec';
import { HomePage } from './home';
import { Route as rootRoute } from './index';

export const Route = createRoute({
  getParentRoute: () => rootRoute,
  path: '',
  component: HomePage,
});
