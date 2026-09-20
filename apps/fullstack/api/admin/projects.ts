export function GET(request: Request) {
  return new Response('projects', {
    headers: {
      'x-plec-middleware-request-order':
        request.headers.get('x-plec-middleware-request-order') ?? '',
    },
  });
}
