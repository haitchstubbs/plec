import { mountPlecApplication } from 'plec-browser';
import { installPlecPerformance } from './performance';
import { installDevelopmentMemoryHud } from 'plec/client/effects/development-memory-hud';

installPlecPerformance();
const root = document.querySelector<HTMLElement>('#app');
if (!root) throw new Error('The Plec application root is missing.');
const appRoot = root;
const revision = new URL(import.meta.url).searchParams.get('v') ?? '';
const assetUrl = (path: string) =>
  revision ? `${path}?v=${revision}` : path;
const disposeMemoryHud = installDevelopmentMemoryHud(assetUrl);

type RouteManifest = {
  rootGraphId: string;
  routes: Array<{
    path: string;
    graphId: string;
    outletId: string;
  }>;
};
const manifest = (await fetch(assetUrl('/route-manifest.json')).then((response) => {
  if (!response.ok) throw new Error(`Failed to load route manifest: ${response.status}`);
  return response.json();
})) as RouteManifest;
const graphUrl = (id: string) => assetUrl(`/graphs/${id}.json`);
let pageController: Awaited<ReturnType<typeof mountPlecApplication>> | undefined;
const navigate = async (href: string, replace = false) => {
  const pathname = new URL(href, window.location.href).pathname;
  const route =
    manifest.routes.find((candidate) => candidate.path === pathname.slice(1)) ??
    manifest.routes.find((candidate) => candidate.path === '*');
  if (!route) throw new Error(`No compiled route for ${pathname}.`);
  if (replace) window.history.replaceState({}, '', pathname);
  else window.history.pushState({}, '', pathname);
  pageController?.dispose();
  const outlet = shell.outlet(route.outletId);
  if (!outlet) throw new Error(`Compiled route outlet is missing: ${route.outletId}.`);
  pageController = await mountPlecApplication({
    root: outlet,
    irUrl: graphUrl(route.graphId),
    onNavigate: ({ href, replace }) => void navigate(href, replace),
    hostValues: { location: { pathname } },
  });
};
const shell = await mountPlecApplication({
  root: appRoot,
  irUrl: graphUrl(manifest.rootGraphId),
  onNavigate: ({ href, replace }) => void navigate(href, replace),
  hostValues: { location: { pathname: window.location.pathname } },
});
document.addEventListener('click', (event) => {
  if (
    event.defaultPrevented ||
    event.button !== 0 ||
    event.metaKey ||
    event.ctrlKey ||
    event.shiftKey ||
    event.altKey
  )
    return;
  const anchor = (event.target as Element | null)?.closest(
    'a[href]',
  ) as HTMLAnchorElement | null;
  if (!anchor || anchor.target || anchor.hasAttribute('download')) return;
  const href = new URL(anchor.href, window.location.href);
  if (href.origin !== window.location.origin) return;
  event.preventDefault();
  void navigate(`${href.pathname}${href.search}${href.hash}`);
});
window.addEventListener('popstate', () =>
  void navigate(window.location.pathname, true),
);
await navigate(window.location.pathname, true);
void disposeMemoryHud;
