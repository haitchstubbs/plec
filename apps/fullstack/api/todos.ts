import { api, json } from './_todos';

export async function GET() {
  if (
    process.env.PLEC_ACCEPTANCE_CONTROL === '1' &&
    api.consumeFailure()
  )
    return json({}, 500);
  return json(api.list());
}
export async function POST(request: Request) {
  const body = (await request.json().catch(() => undefined)) as
    { title?: unknown } | undefined;
  const todo = api.create(body?.title);
  return todo
    ? json(todo, 201)
    : json({ error: 'title must be a non-empty string' }, 400);
}
