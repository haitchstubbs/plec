import { createRoute } from '@plec/core';
import { AboutPage } from '../components/about';
import { Route as rootRoute } from './index';

export const Route = createRoute({
  getParentRoute: () => rootRoute,
  path: 'about',
  component: AboutPage,
  meta: {
    title: 'About Plec',
    description: 'About the Plec experimental runtime.',
  },
});
