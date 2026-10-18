import { Outlet, useLocation, useMutation, useState } from '@plec/core';
import { PageFrame, PageKicker } from './page-primitives';
import { Route } from '../routes/project.route';
import { renameProject } from '../server-actions';

export function ProjectPage() {
  const location = useLocation();
  const project = Route.useLoaderData() as { missing: boolean; name: string | null };
  const params = Route.useParams();
  const rename = useMutation(async (name: string) => {
    return await renameProject(params.id, name);
  });
  const [detailed, setDetailed] = useState(false);
  return (
    <PageFrame>
      <PageKicker>Parameterized route</PageKicker>
      <h1 className="m-0 text-4xl font-bold tracking-tight sm:text-5xl">
        Project route resolved by the server-published chain.
      </h1>
      <p
        id="ssr-param"
        className="m-0 max-w-2xl text-lg leading-8 text-muted-foreground"
      >
        Project route matched with path {location.pathname}.
      </p>
      <p id="project-name">{project.name}</p>
      <button type="button" id="rename-project" onClick={() => rename.run('Project renamed by action')}>
        Rename project
      </button>
      <Outlet />
      {detailed ? (
        <p
          id="ssr-branch-open"
          className="m-0 max-w-2xl text-lg leading-8 text-muted-foreground"
        >
          Project detail expanded: this branch was adopted from the
          server render and flipped in place.
        </p>
      ) : (
        <p
          id="ssr-branch-closed"
          className="m-0 max-w-2xl text-lg leading-8 text-muted-foreground"
        >
          Project detail is collapsed.
        </p>
      )}
      <button
        type="button"
        id="ssr-branch-toggle"
        onClick={() => setDetailed(!detailed)}
        className="w-fit rounded-md border bg-card px-3 py-1.5 text-sm font-medium shadow-sm hover:bg-muted"
      >
        Toggle project detail
      </button>
    </PageFrame>
  );
}
