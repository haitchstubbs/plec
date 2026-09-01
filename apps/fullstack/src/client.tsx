import { startPlecRouter } from 'plec-browser';
import { installPlecPerformance } from './performance';
import { installDevelopmentMemoryHud } from 'plec/client/effects/development-memory-hud';
import { createRuntimeStressFeed } from './stress-feed';

installPlecPerformance();
const root = document.querySelector<HTMLElement>('#app');
if (!root) throw new Error('The Plec application root is missing.');
const revision = new URL(import.meta.url).searchParams.get('v') ?? '';
const assetUrl = (path: string) =>
  revision ? `${path}?v=${revision}` : path;
const disposeMemoryHud = installDevelopmentMemoryHud(assetUrl);

// The browser only retrieves immutable artifacts. The runtime owns matching,
// history, link interception, outlet replacement and instance disposal.
const stressFeed = createRuntimeStressFeed();
const app = await startPlecRouter({
  root,
  manifestUrl: assetUrl('/route-manifest.json'),
  graphUrl: (graphId) =>
    assetUrl(
      `/graphs/${graphId.replace(/[\/\\]/g, '--').replace('#', '--')}.json`,
    ),
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
