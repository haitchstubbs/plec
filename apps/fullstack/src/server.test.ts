import { describe, expect, it } from 'vitest';
import { handleRequest } from './server';

const request = async (path: string, init?: RequestInit) => {
  const url = `http://plec.test${path}`;
  const response = await handleRequest(new Request(url, init), {
    url,
    pathname: path,
    method: init?.method ?? 'GET',
    headers: {},
    cookies: {},
    params: {},
    query: {},
  });
  if (!response) throw new Error(`unhandled request ${path}`);
  return response;
};

describe('Plec application server', () => {
  it('handles the CRUD Todo API through the sidecar contract', async () => {
    const initial = await (await request('/api/todos')).json();
    expect(initial).toHaveLength(1);
    const created = await (
      await request('/api/todos', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ title: 'Ship Plec' }),
      })
    ).json();
    expect(created.title).toBe('Ship Plec');
    const updated = await (
      await request(`/api/todos/${created.id}`, {
        method: 'PATCH',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ completed: true }),
      })
    ).json();
    expect(updated.completed).toBe(true);
    expect(
      (
        await request(`/api/todos/${created.id}`, {
          method: 'DELETE',
        })
      ).status,
    ).toBe(204);
  });

  it('rejects invalid mutations and unknown API routes', async () => {
    expect(
      (
        await request('/api/todos', {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: '{}',
        })
      ).status,
    ).toBe(400);
    expect(
      (
        await request('/api/todos/nope', {
          method: 'PATCH',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify({ completed: 'yes' }),
        })
      ).status,
    ).toBe(404);
    expect(
      await handleRequest(new Request('http://plec.test/api/nope'), {
        url: 'http://plec.test/api/nope',
        pathname: '/api/nope',
        method: 'GET',
        headers: {},
        cookies: {},
        params: {},
        query: {},
      }),
    ).toBeUndefined();
  });

  it('reports Node memory diagnostics for the cross-browser development HUD', async () => {
    const response = await request('/api/dev/memory');
    expect(response.status).toBe(200);
    expect(await response.json()).toMatchObject({
      pid: expect.any(Number),
      rssBytes: expect.any(Number),
      heapUsedBytes: expect.any(Number),
      externalBytes: expect.any(Number),
      arrayBuffersBytes: expect.any(Number),
      heapLimitBytes: expect.any(Number),
      activeResources: expect.any(Array),
    });
  });
});
