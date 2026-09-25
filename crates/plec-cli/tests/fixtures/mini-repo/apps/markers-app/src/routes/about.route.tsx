import { createRoute } from '@plec/core';
import { Route as rootRoute } from './index';
import { AboutPage } from '../about';

export const Route = createRoute({
  getParentRoute: () => rootRoute,
  path: 'about',
  component: AboutPage,
});
