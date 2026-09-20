import { expect, test } from '@playwright/test';

test('scoped API middleware composes around handlers and generated methods', async ({
  request,
}) => {
  const normal = await request.get('/api/admin/projects');
  expect(normal.status()).toBe(200);
  expect(await normal.text()).toBe('projects');
  expect(normal.headers()['x-plec-middleware-request-order']).toBe(
    'outer,inner',
  );
  expect(normal.headers()['x-plec-middleware']).toBe('inner,outer');

  const options = await request.fetch('/api/admin/projects', {
    method: 'OPTIONS',
  });
  expect(options.status()).toBe(204);
  expect(options.headers()['x-plec-middleware']).toBe('inner,outer');

  const methodNotAllowed = await request.post('/api/admin/projects');
  expect(methodNotAllowed.status()).toBe(405);
  expect(methodNotAllowed.headers()['x-plec-middleware']).toBe(
    'inner,outer',
  );
});

test('scoped API middleware can short-circuit and misses stay outside scope', async ({
  request,
}) => {
  const shortCircuit = await request.get('/api/admin/projects', {
    headers: { 'x-plec-middleware-short-circuit': '1' },
  });
  expect(shortCircuit.status()).toBe(418);
  expect(await shortCircuit.text()).toBe('short-circuit');
  expect(shortCircuit.headers()['x-plec-middleware']).toBe(
    'outer-short-circuit',
  );

  const unmatched = await request.get('/api/admin/missing');
  expect(unmatched.status()).toBe(404);
  expect(unmatched.headers()['x-plec-middleware']).toBeUndefined();
});
