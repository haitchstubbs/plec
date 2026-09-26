import { json } from './_todos';

export function GET() {
  return json({ admin: false });
}
