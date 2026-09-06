import path from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  createPlecServer,
  serve,
  type AppRequestHandler,
} from 'plec-server';
import { createTodoApi, createTodoApiHandler, type Todo } from './api';

export type { Todo };
export { createTodoApi };

/** The application's own API. Plec knows nothing about it. */
const api = createTodoApi();

/**
 * The server-bundle contract: the only symbol the framework consumes.
 * Both hosts execute this — the Node sidecar imports the bundle directly,
 * and the TS host wires it as its `handleAppRequest` below.
 */
export const handleRequest: AppRequestHandler =
  withAcceptanceFixture(createTodoApiHandler(api));

export function createAppServer(
  publicDir: string,
  api = createTodoApi(),
) {
  return createPlecServer({
    publicDir,
    artifactPath: path.join(publicDir, 'route-artifact.json'),
    clientScript: '/assets/client.js',
    stylesHref: '/assets/styles.css',
    // Self-hosted variable fonts: without preloading they are discovered only
    // after the stylesheet finishes parsing, so first paint uses fallback
    // metrics and reflows when the woff2 lands (font swap flash).
    preloads: [
      '/assets/files/outfit-latin-wght-normal.woff2',
      '/assets/files/raleway-latin-wght-normal.woff2',
    ],
    document: {
      title: 'Plec fullstack playground',
      description: 'Plec fullstack runtime experiment.',
    },
    // Transitional: while the two hosts coexist, the TS host duplicates the
    // host configuration that `plec.toml` carries for the generated manifest.
    handleAppRequest: handleRequest,
    development: process.env.NODE_ENV !== 'production',
  });
}

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

if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  const server = createAppServer(path.resolve('dist/public'));
  serve(server);
  server.on('listening', () =>
    console.log(
      `Plec fullstack playground on http://localhost:${process.env.PORT ?? 3000}`,
    ),
  );
}
