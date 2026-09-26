import { json } from '../_todos';

const PROJECTS: Record<string, string> = {
  plec: 'Plec compiler and runtime',
  p42: 'Project 42',
};

export async function GET(
  _request: Request,
  context: { params: { id: string } },
) {
  const id = decodeURIComponent(context.params.id);
  const name = PROJECTS[id];
  // A missing project is a loader decision, not a transport failure: the API
  // answers 200 with `missing` so the route loader can resolve notFound().
  return json({ missing: name === undefined, name: name ?? null });
}
