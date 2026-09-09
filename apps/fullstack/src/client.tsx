import {
  registerPlecHostProvider,
  startPlecRouter,
} from 'plec-browser';
import { createLucideHostProvider } from '@wasm-runtime/lucide-plec';
import { installPlecPerformance } from './performance';
import { installDevelopmentMemoryHud } from 'plec/client/effects/development-memory-hud';
import { createRuntimeStressFeed } from './stress-feed';

installPlecPerformance();
registerPlecHostProvider('lucide', createLucideHostProvider());
const root = document.querySelector<HTMLElement>('#app');
if (!root) throw new Error('The Plec application root is missing.');
const revision = new URL(import.meta.url).searchParams.get('v') ?? '';
const assetUrl = (path: string) =>
  revision ? `${path}?v=${revision}` : path;
const disposeMemoryHud = installDevelopmentMemoryHud(assetUrl);

// The browser only retrieves immutable artifacts. The runtime owns matching,
// history, link interception, outlet replacement and instance disposal.
// Capabilities declared by artifacts are requests only; the grants below are
// the host-owned authority for cookies and fetch in this application.
const origin = window.location.origin;
const stressFeed = createRuntimeStressFeed();
const app = await startPlecRouter({
  root,
  manifestUrl: assetUrl('/route-manifest.json'),
  graphUrl: (graphId) =>
    assetUrl(
      `/graphs/${graphId.replace(/[\/\\]/g, '--').replace('#', '--')}.json`,
    ),
  cookiePolicy: {
    sidebar_state: { operations: ['getSync', 'set'] },
  },
  fetchPolicy: [
    {
      origin,
      methods: ['GET', 'POST', 'PATCH', 'DELETE'],
      headers: ['content-type'],
      credentials: true,
    },
  ],
  inputs: stressFeed.inputs,
  onQueryUpdate: stressFeed.recordRuntimeUpdate,
});
stressFeed.start();
window.addEventListener(
  'pagehide',
  () => {
    stressFeed.stop();
    app.dispose();
  },
  { once: true },
);
void disposeMemoryHud;
