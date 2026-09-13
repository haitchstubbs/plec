import { api } from '../_todos';
export function POST() {
  if (process.env.PLEC_ACCEPTANCE_CONTROL === '1') api.failNext();
  return new Response(null, { status: 204 });
}
