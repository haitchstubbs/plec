import { createRoute } from 'plec';
import { Route as rootRoute } from './index';
import { ListPage } from '../list';

export const Route = createRoute({
  getParentRoute: () => rootRoute,
  path: 'list',
  component: ListPage,
});
