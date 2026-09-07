import type { AppRequestHandler } from 'plec/server';
import { createTodoApi, createTodoApiHandler, type Todo } from './api';

export type { Todo };
export { createTodoApi };

/** The application's own API. Plec knows nothing about it. */
const api = createTodoApi();

/**
 * The server-bundle contract: the only symbol the framework consumes.
 * The native host's Node sidecar imports the bundle directly.
 */
export const handleRequest: AppRequestHandler = withAcceptanceFixture(
  createTodoApiHandler(api),
);

/** Acceptance-only fixture: SSR executes route loaders on the server, so the
 * harness cannot induce the loader-error phase by intercepting browser
 * requests. With PLEC_ACCEPTANCE_CONTROL=1 the harness can arm a one-shot
 * failure for the next GET /api/todos — exactly the request the compiled SSR
 * loader performs — leaving production loader semantics untouched. */
function withAcceptanceFixture(
  handleAppRequest: AppRequestHandler,
): AppRequestHandler {
  if (process.env.PLEC_ACCEPTANCE_CONTROL !== '1')
    return handleAppRequest;
  let failNextTodoGet = false;
  return async (request, context) => {
    const url = new URL(request.url);
    if (
      url.pathname === '/api/acceptance/todo-loader-failure' &&
      request.method === 'POST'
    ) {
      failNextTodoGet = true;
      return new Response(null, { status: 204 });
    }
    if (
      failNextTodoGet &&
      url.pathname === '/api/todos' &&
      request.method === 'GET'
    ) {
      failNextTodoGet = false;
      return new Response('{}', {
        status: 500,
        headers: { 'content-type': 'application/json' },
      });
    }
    return handleAppRequest(request, context);
  };
}
