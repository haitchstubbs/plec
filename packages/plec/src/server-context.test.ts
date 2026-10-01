import { describe, expect, it } from 'vitest';
import { requestContext, withRequestContext } from './server-context';
import type { RequestContext } from './server';

const context = (session: string): RequestContext => ({
  url: 'https://example.test/action',
  pathname: '/action',
  method: 'POST',
  headers: { authorization: `Bearer ${session}` },
  cookies: { session },
  params: {},
  query: {},
});

describe('server action request context', () => {
  it('is unavailable outside a server request', () => {
    expect(() => requestContext()).toThrow(
      'requires an active server request',
    );
  });

  it('isolates concurrent request contexts across async work', async () => {
    const read = (value: string, delay: number) =>
      withRequestContext(context(value), async () => {
        await new Promise((resolve) => setTimeout(resolve, delay));
        return [
          requestContext().cookies.session,
          requestContext().headers.authorization,
        ];
      });
    await expect(
      Promise.all([read('first', 10), read('second', 0)]),
    ).resolves.toEqual([
      ['first', 'Bearer first'],
      ['second', 'Bearer second'],
    ]);
  });
});
