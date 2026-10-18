import { createRoute } from '@plec/core';
import { Route as projectRoute } from './project.route';

type Project = { missing: boolean; name: string | null };

export function ProjectDetailsPage() {
  const project = Route.useLoaderData();
  return <p id="project-details-name">{project.name}</p>;
}

export const Route = createRoute<Project>({
  getParentRoute: () => projectRoute,
  path: 'details',
  dependsOn: ({ params }) => [`project:${params.id}`],
  loader: async ({ params }) => {
    const project = (await fetch(
      `/api/projects/${params.id}`,
    )) as Project;
    return project;
  },
  component: ProjectDetailsPage,
});
