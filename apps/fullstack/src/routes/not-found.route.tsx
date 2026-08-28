import { createRoute } from 'plec';
import { NotFoundPage } from '../components/not-found';
import { Route as rootRoute } from './index';

export const Route = createRoute({
  getParentRoute: () => rootRoute,
  path: '*',
  component: NotFoundPage,
  meta: { title: 'Page not found', description: 'The requested Plec route does not exist.' },
});
