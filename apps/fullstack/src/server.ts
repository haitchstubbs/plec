import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createPlecServer, serve } from 'plec-server';
import { createTodoApi, createTodoApiHandler, type Todo } from './api';

export type { Todo };
export { createTodoApi };

export function createAppServer(publicDir: string, api = createTodoApi()) {
  return createPlecServer({
    publicDir,
    artifactPath: path.join(publicDir, 'route-artifact.json'),
    clientScript: '/assets/client.js',
    stylesHref: '/assets/styles.css',
    document: { title: 'Plec fullstack playground', description: 'Plec fullstack runtime experiment.' },
    handleAppRequest: createTodoApiHandler(api),
    development: process.env.NODE_ENV !== 'production',
  });
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const server = createAppServer(path.resolve('dist/public'));
  serve(server);
  server.on('listening', () => console.log(`Plec fullstack playground on http://localhost:${process.env.PORT ?? 3100}`));
}
