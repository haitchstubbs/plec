import type { ApiMiddleware } from 'plec/server';

export const middleware: ApiMiddleware = async (
  request,
  _context,
  next,
) => {
  request.headers.set('x-plec-middleware-request-order', 'outer');

  if (request.headers.get('x-plec-middleware-short-circuit') === '1') {
    return new Response('short-circuit', {
      status: 418,
      headers: { 'x-plec-middleware': 'outer-short-circuit' },
    });
  }

  const response = await next();
  const order = response.headers.get('x-plec-middleware') ?? '';
  response.headers.set(
    'x-plec-middleware',
    order ? `${order},outer` : 'outer',
  );
  return response;
};
