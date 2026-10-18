import { json } from '../_todos';
import { getProjectName } from '../../src/project-data';

export async function GET(
  _request: Request,
  context: { params: { id: string } },
) {
  const id = decodeURIComponent(context.params.id);
  const name = getProjectName(id);
  // A missing project is a loader decision, not a transport failure: the API
  // answers 200 with `missing` so the route loader can resolve notFound().
  return json({ missing: name === undefined, name: name ?? null });
}
