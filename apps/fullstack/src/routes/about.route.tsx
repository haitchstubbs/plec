import { createRoute } from 'plec';
import { AboutPage } from '../components/about';
import { Route as rootRoute } from './index';

export const Route = createRoute({
  getParentRoute: () => rootRoute,
  path: 'about',
  component: AboutPage,
});
