import { createRoute, notFound } from '@plec/core';
import { ProjectPage } from '../components/project';
import { Route as rootRoute } from './index';
import { ProjectMissing } from '../components/project-missing';

type Project = { missing: boolean; name: string | null };

export const Route = createRoute<Project>({
  getParentRoute: () => rootRoute,
  path: 'projects/$id',
  loader: async ({ params }) => {
    const project = (await fetch(
      `/api/projects/${params.id}`,
    )) as Project;
    if (project.missing) throw notFound();
    return project;
  },
  notFoundComponent: ProjectMissing,
  component: ProjectPage,
  meta: {
    title: 'Project',
    description: 'A parameterized Plec route.',
  },
});
