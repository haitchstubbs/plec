import {
  BaseRootRoute,
  BaseRoute,
  RouterCore,
  createNonReactiveMutableStore,
  createNonReactiveReadonlyStore,
  type AnyRoute,
} from "@tanstack/router-core";

export type RouteArtifact = "home" | "about" | "not-found";

const rootRoute = new BaseRootRoute();
const homeRoute = new BaseRoute({ getParentRoute: () => rootRoute, path: "/" });
const aboutRoute = new BaseRoute({ getParentRoute: () => rootRoute, path: "about" });
const routeTree = rootRoute.addChildren([homeRoute, aboutRoute]);

/** The core router does not require a framework store; this adapter publishes
 * its results straight to the DOM renderer. */
export function createO1Router() {
  return new RouterCore({ routeTree: routeTree as AnyRoute }, () => ({
    createMutableStore: createNonReactiveMutableStore,
    createReadonlyStore: createNonReactiveReadonlyStore,
    batch: (fn) => fn(),
  }));
}

export function artifactForPath(router: ReturnType<typeof createO1Router>, pathname: string): RouteArtifact {
  const matches = router.matchRoutes(pathname);
  const leaf = matches.at(-1);
  if (leaf?.routeId === "/about") return "about";
  if (leaf?.routeId === "/") return "home";
  return "not-found";
}

export function shouldInterceptLink(event: MouseEvent, anchor: HTMLAnchorElement): boolean {
  if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return false;
  if (anchor.target && anchor.target !== "_self") return false;
  if (anchor.hasAttribute("download") || anchor.origin !== window.location.origin) return false;
  return anchor.pathname.startsWith("/");
}
