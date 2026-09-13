import { api, json } from '../_todos';

export async function PATCH(
  request: Request,
  context: { params: { id: string } },
) {
  const updated = api.update(
    context.params.id,
    await request.json().catch(() => undefined),
  );
  return updated === null
    ? json(
        {
          error:
            'title must be non-empty and completed must be boolean',
        },
        400,
      )
    : updated
      ? json(updated)
      : json({ error: 'todo not found' }, 404);
}
export async function DELETE(
  _request: Request,
  context: { params: { id: string } },
) {
  return api.remove(context.params.id)
    ? new Response(null, { status: 204 })
    : json({ error: 'todo not found' }, 404);
}
