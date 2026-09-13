import { createRoute } from 'plec';
import { RuntimeStressPage } from '../components/runtime-stress';
import { Route as rootRoute } from './index';

export const Route = createRoute({
  getParentRoute: () => rootRoute,
  path: 'stress',
  component: RuntimeStressPage,
  meta: {
    title: 'Plec runtime stress',
    description: 'Plec keyed DOM update stress fixture.',
  },
});
