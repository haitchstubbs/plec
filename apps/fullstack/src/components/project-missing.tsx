import { PageFrame, PageKicker } from './page-primitives';

export function ProjectMissing() {
  return (
    <PageFrame>
      <PageKicker>Project</PageKicker>
      <section>
        <h1 role="alert">Project not found</h1>
        <p data-testid="project-not-found">
          This project does not exist, so its loader resolved as not
          found.
        </p>
      </section>
    </PageFrame>
  );
}
