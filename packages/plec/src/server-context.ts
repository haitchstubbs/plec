import { AsyncLocalStorage } from 'node:async_hooks';
import type { RequestContext } from './server';

const requestContexts = new AsyncLocalStorage<RequestContext>();

export function requestContext(): RequestContext {
  const context = requestContexts.getStore();
  if (!context)
    throw new Error(
      'requestContext() requires an active server request',
    );
  return context;
}

/** @internal Used by the generated Node action registry. */
export function withRequestContext<T>(
  context: RequestContext,
  run: () => T,
): T {
  return requestContexts.run(context, run);
}
