import { AsyncLocalStorage } from 'node:async_hooks';
import type { RequestContext } from './server';

type ActionInvalidationCollector = { identities: string[]; seen: Set<string> };
type ServerContextStorage = {
  requestContexts: AsyncLocalStorage<RequestContext>;
  actionInvalidations: AsyncLocalStorage<ActionInvalidationCollector>;
};
// The compiler may bundle the server entry and generated action registry via
// different package subpaths. A process-global storage *instance* keeps those
// module copies on the same async-local contexts; the collected identities
// themselves remain per invocation.
const storageKey = Symbol.for('plec.server-context.storage');
const globalStorage = globalThis as typeof globalThis & { [key: symbol]: unknown };
const storage = (globalStorage[storageKey] as ServerContextStorage | undefined) ?? (() => {
  const created: ServerContextStorage = {
    requestContexts: new AsyncLocalStorage<RequestContext>(),
    actionInvalidations: new AsyncLocalStorage<ActionInvalidationCollector>(),
  };
  globalStorage[storageKey] = created;
  return created;
})();
const MAX_IDENTITY_BYTES = 256;
const MAX_ACTION_INVALIDATIONS = 32;
const MAX_INVALIDATIONS_JSON_BYTES = 4 * 1024;

/** Declare that a successful active server action changed a resource. */
export function invalidate(identity: string): void {
  const collector = storage.actionInvalidations.getStore();
  if (!collector) throw new Error('invalidate() requires an active server action');
  if (typeof identity !== 'string' || Buffer.byteLength(identity, 'utf8') > MAX_IDENTITY_BYTES) {
    throw new Error('resource identity must be a string no longer than 256 UTF-8 bytes');
  }
  if (collector.seen.has(identity)) return;
  const next = [...collector.identities, identity];
  if (next.length > MAX_ACTION_INVALIDATIONS) {
    throw new Error('server action exceeds the 32 invalidation limit');
  }
  if (Buffer.byteLength(JSON.stringify(next), 'utf8') > MAX_INVALIDATIONS_JSON_BYTES) {
    throw new Error('server action invalidations exceed the 4 KiB metadata limit');
  }
  collector.seen.add(identity);
  collector.identities.push(identity);
}

/** @internal Runs one generated action in an isolated, success-committed collector. */
export async function withActionInvalidationCollection<T>(
  run: () => T | Promise<T>,
): Promise<{ value: T; invalidations: string[] }> {
  const collector: ActionInvalidationCollector = { identities: [], seen: new Set() };
  const value = await storage.actionInvalidations.run(collector, run);
  return { value, invalidations: collector.identities };
}

export function requestContext(): RequestContext {
  const context = storage.requestContexts.getStore();
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
  return storage.requestContexts.run(context, run);
}
