import {
  registerPlecProviders,
  startPlecRouter,
} from '@plec/core/browser';
import { installPlecPerformance } from './performance';
import { installDevelopmentMemoryHud } from '@plec/core/client/effects/development-memory-hud';
import { createRuntimeStressFeed } from './stress-feed';
import { installDuckTracking } from './duck-tracking';
import './styles.css';

installPlecPerformance();
await registerPlecProviders();
const root = document.querySelector<HTMLElement>('#app');
if (!root) throw new Error('The Plec application root is missing.');
const assetUrl = (path: string) => path;
const disposeMemoryHud = installDevelopmentMemoryHud(assetUrl);

// The browser only retrieves immutable artifacts. The runtime owns matching,
// history, link interception, outlet replacement and instance disposal.
// Capabilities declared by artifacts are requests only; the grants below are
// the host-owned authority for cookies and fetch in this application.
const origin = window.location.origin;
const stressFeed = createRuntimeStressFeed();
const app = await startPlecRouter({
  root,
  manifestUrl: assetUrl('/_plec/route-manifest.json'),
  graphUrl: (graphId) =>
    assetUrl(
      `/_plec/graphs/${graphId.replace(/[\/\\]/g, '--').replace('#', '--')}.json`,
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
const disposeDuckTracking = installDuckTracking(root);
stressFeed.start();
window.addEventListener(
  'pagehide',
  () => {
    disposeDuckTracking();
    stressFeed.stop();
    app.dispose();
  },
  { once: true },
);
void disposeMemoryHud;
