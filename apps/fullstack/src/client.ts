import { mountCompiledApplication, type CompiledRuntimeController } from "@wasm-runtime/browser";
import { artifactForPath, createO1Router, shouldInterceptLink, type RouteArtifact } from "./router";
import { installO1Performance } from "./performance";

installO1Performance();

const root = document.querySelector<HTMLElement>("#app");
if (!root) throw new Error("The O1 application root is missing.");
const appRoot = root;
const assetRevision = new URL(import.meta.url).searchParams.get("v") ?? "";
const assetUrl = (path: string) => assetRevision ? `${path}?v=${assetRevision}` : path;

const router = createO1Router();
let controller: CompiledRuntimeController | undefined;
let activeArtifact: RouteArtifact | undefined;
let layoutController: CompiledRuntimeController | undefined;
let renderQueue = Promise.resolve();

async function renderLocation() {
  const artifact = artifactForPath(router, window.location.pathname);
  if (artifact === activeArtifact) return;
  const outlet = layoutController?.outlet();
  if (!outlet) throw new Error("The compiled O1 layout does not expose its main route outlet.");
  controller?.dispose();
  activeArtifact = artifact;
  outlet.setAttribute("aria-busy", "true");
  try {
    controller = await mountCompiledApplication({
      root: outlet,
      adopt: false,
      irUrl: assetUrl(`/o1/${artifact}.ir.json`),
      runtimeJsUrl: assetUrl("/o1/runtime/runtime.js"),
      runtimeWasmUrl: assetUrl("/o1/runtime/runtime_bg.wasm"),
    });
  } finally {
    outlet.removeAttribute("aria-busy");
  }
}

async function syncLocation() {
  layoutController?.applyHostValues({ location: { pathname: window.location.pathname } });
  // A WASM mount is asynchronous. Serialize route changes so a stale mount
  // cannot claim the outlet after a newer navigation has already started.
  renderQueue = renderQueue.catch(() => {}).then(renderLocation);
  await renderQueue;
}

document.addEventListener("click", (event) => {
  const target = event.target;
  if (!(target instanceof Element)) return;
  const anchor = target.closest<HTMLAnchorElement>("a[href]");
  if (!anchor || !shouldInterceptLink(event, anchor)) return;
  event.preventDefault();
  window.history.pushState({}, "", `${anchor.pathname}${anchor.search}${anchor.hash}`);
  void syncLocation();
});

window.addEventListener("popstate", () => { void syncLocation(); });
void (async () => {
  layoutController = await mountCompiledApplication({
    root: appRoot,
    irUrl: assetUrl("/o1/layout.ir.json"),
    runtimeJsUrl: assetUrl("/o1/runtime/runtime.js"),
    runtimeWasmUrl: assetUrl("/o1/runtime/runtime_bg.wasm"),
    hostValues: { location: { pathname: window.location.pathname } },
  });
  await syncLocation();
})();
