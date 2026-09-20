import type { ApiMiddleware } from 'plec/server';

export const middleware: ApiMiddleware = async (
  request,
  _context,
  next,
) => {
  request.headers.set(
    'x-plec-middleware-request-order',
    `${request.headers.get('x-plec-middleware-request-order')},inner`,
  );
  const response = await next();
  const order = response.headers.get('x-plec-middleware') ?? '';
  response.headers.set(
    'x-plec-middleware',
    order ? `${order},inner` : 'inner',
  );
  return response;
};
