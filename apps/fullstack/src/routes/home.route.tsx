import { createRoute } from 'plec';
import { HomePage } from '../components/home';
import { Route as rootRoute } from './index';

export const Route = createRoute({
  getParentRoute: () => rootRoute,
  path: '',
  component: HomePage,
  meta: {
    title: 'Plec runtime control room',
    description: 'Plec compiler and runtime control room.',
  },
});
