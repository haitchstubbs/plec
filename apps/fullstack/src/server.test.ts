import { describe, expect, it } from 'vitest';
import { GET as list, POST as create } from '../api/todos';
import { DELETE, PATCH } from '../api/todos/[id]';
import { GET as memory } from '../api/dev/memory';

const request = (init?: RequestInit) =>
  new Request('http://plec.test/api/todos', init);

describe('file-based API handlers', () => {
  it('handles Todo CRUD', async () => {
    expect(await (await list()).json()).toHaveLength(1);
    const created = await (
      await create(
        request({
          method: 'POST',
          body: JSON.stringify({ title: 'Ship Plec' }),
        }),
      )
    ).json();
    const context = { params: { id: created.id } };
    expect(
      (
        await (
          await PATCH(
            request({
              method: 'PATCH',
              body: JSON.stringify({ completed: true }),
            }),
            context,
          )
        ).json()
      ).completed,
    ).toBe(true);
    expect(
      (await DELETE(request({ method: 'DELETE' }), context)).status,
    ).toBe(204);
  });

  it('rejects invalid mutations', async () => {
    expect(
      (await create(request({ method: 'POST', body: '{}' }))).status,
    ).toBe(400);
    expect(
      (
        await PATCH(
          request({
            method: 'PATCH',
            body: JSON.stringify({ completed: 'yes' }),
          }),
          { params: { id: 'nope' } },
        )
      ).status,
    ).toBe(404);
  });

  it('reports Node memory diagnostics', async () => {
    expect((await memory()).status).toBe(200);
    expect(await (await memory()).json()).toMatchObject({
      pid: expect.any(Number),
      heapUsedBytes: expect.any(Number),
      activeResources: expect.any(Array),
    });
  });
});
