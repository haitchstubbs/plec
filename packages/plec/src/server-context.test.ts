import { describe, expect, it } from 'vitest';
import { invalidate, requestContext, withActionInvalidationCollection, withRequestContext } from './server-context';
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

  it('isolates, deduplicates, and commits action invalidations only on success', async () => {
    const successful = await withActionInvalidationCollection(async () => {
      invalidate('project:42');
      invalidate('project:42');
      await Promise.resolve();
      invalidate('todos');
      return 'unchanged-result';
    });
    expect(successful).toEqual({ value: 'unchanged-result', invalidations: ['project:42', 'todos'] });
    await expect(withActionInvalidationCollection(async () => {
      invalidate('discard-me');
      throw new Error('failed');
    })).rejects.toThrow('failed');
    expect(() => invalidate('outside')).toThrow('active server action');
  });

  it('fails explicitly when resource identity or metadata budgets are exceeded', async () => {
    await expect(withActionInvalidationCollection(() => invalidate('x'.repeat(257)))).rejects.toThrow('256 UTF-8 bytes');
    await expect(withActionInvalidationCollection(() => {
      for (let index = 0; index < 33; index++) invalidate(`resource:${index}`);
    })).rejects.toThrow('32 invalidation limit');
    await expect(withActionInvalidationCollection(() => {
      for (let index = 0; index < 17; index++) invalidate(`${index}${'x'.repeat(240)}`);
    })).rejects.toThrow('4 KiB metadata limit');
  });
});
