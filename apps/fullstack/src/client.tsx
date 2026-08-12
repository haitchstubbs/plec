import { startPlecRouter } from 'plec-browser';
import { installPlecPerformance } from './performance';
import { installDevelopmentMemoryHud } from 'plec/client/effects/development-memory-hud';

installPlecPerformance();
const root = document.querySelector<HTMLElement>('#app');
if (!root) throw new Error('The Plec application root is missing.');
const revision = new URL(import.meta.url).searchParams.get('v') ?? '';
const assetUrl = (path: string) => revision ? `${path}?v=${revision}` : path;
const disposeMemoryHud = installDevelopmentMemoryHud(assetUrl);

// The browser only retrieves immutable artifacts. The runtime owns matching,
// history, link interception, outlet replacement and instance disposal.
const app = await startPlecRouter({
  root,
  manifestUrl: assetUrl('/route-manifest.json'),
  graphUrl: (graphId) => assetUrl(`/graphs/${graphId}.json`),
});
window.addEventListener('pagehide', () => app.dispose(), { once: true });
void disposeMemoryHud;
