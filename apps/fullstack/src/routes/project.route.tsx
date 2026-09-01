import { createRoute } from 'plec';
import { ProjectPage } from '../components/project';
import { Route as rootRoute } from './index';

export const Route = createRoute({
  getParentRoute: () => rootRoute,
  path: 'projects/$id',
  component: ProjectPage,
  meta: {
    title: 'Project',
    description: 'A parameterized Plec route.',
  },
});
