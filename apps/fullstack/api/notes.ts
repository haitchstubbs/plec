import { json } from './_todos';
export function GET() {
  return json({
    headline: 'Notes transferred from the server',
    detail:
      'This loader ran during SSR; the browser resumed the outcome instead of refetching.',
  });
}
