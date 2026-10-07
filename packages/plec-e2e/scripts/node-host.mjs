import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { serve } from '../../plec-node/src/index.ts';

const workspaceRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../../..',
);
const dir = process.env.PLEC_E2E_DIST
  ? path.resolve(process.env.PLEC_E2E_DIST)
  : path.join(workspaceRoot, 'apps/fullstack/dist');
const port = Number(process.env.PORT ?? 3000);

await serve({
  dir,
  host: '127.0.0.1',
  port,
  // Match `plec serve`: E2E runs in development unless explicitly marked
  // production, including the SSR-gating diagnostic headers.
  development: process.env.NODE_ENV !== 'production',
});
