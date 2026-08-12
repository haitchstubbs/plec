type ServerMemory = {
  timestamp: string;
  pid: number;
  uptimeSeconds: number;
  rssBytes: number;
  heapUsedBytes: number;
  heapTotalBytes: number;
  externalBytes: number;
  arrayBuffers: number;
  heapLimitBytes: number;
  activeResources: string[];
};

type ChromiumMemory = {
  usedJSHeapSize: number;
  totalJSHeapSize: number;
  jsHeapSizeLimit: number;
};

const formatBytes = (bytes: number) =>
  `${(bytes / 1024 / 1024).toFixed(bytes >= 1024 * 1024 * 1024 ? 2 : 0)} MB`;
const browserMemory = () =>
  (performance as Performance & { memory?: ChromiumMemory }).memory;

/** Optional client-side development diagnostics, supplied with an app's API URL resolver. */
export function installDevelopmentMemoryHud(
  assetUrl: (path: string) => string,
): () => void {
  const button = document.createElement('button');
  button.type = 'button';
  button.className = 'plec-development-memory-toggle';
  button.textContent = 'Memory';
  button.setAttribute('aria-expanded', 'false');

  const panel = document.createElement('aside');
  panel.className = 'plec-development-memory-panel';
  panel.hidden = true;
  panel.setAttribute('aria-label', 'Development memory diagnostics');
  panel.innerHTML =
    '<strong>Development memory</strong><p>Loading Node diagnostics…</p>';

  const render = (server: ServerMemory) => {
    const tab = browserMemory();
    const browser = tab
      ? `Browser JS heap: ${formatBytes(tab.usedJSHeapSize)} / ${formatBytes(tab.jsHeapSizeLimit)}`
      : 'Browser JS heap: unavailable (Firefox/Safari do not expose tab heap usage to pages)';
    panel.innerHTML = [
      '<strong>Development memory</strong>',
      `<p>Node RSS: <b>${formatBytes(server.rssBytes)}</b></p>`,
      `<p>Node heap: <b>${formatBytes(server.heapUsedBytes)} / ${formatBytes(server.heapLimitBytes)}</b></p>`,
      `<p>Node external: ${formatBytes(server.externalBytes)} · Array buffers: ${formatBytes(server.arrayBuffers)}</p>`,
      `<p>${browser}</p>`,
      `<p class="plec-development-memory-meta">PID ${server.pid} · uptime ${server.uptimeSeconds}s · ${server.activeResources.join(', ') || 'no active resources'}</p>`,
    ].join('');
  };

  let timer: number | undefined;
  const close = () => {
    if (timer !== undefined) window.clearInterval(timer);
    timer = undefined;
    panel.hidden = true;
    button.setAttribute('aria-expanded', 'false');
  };
  const refresh = async () => {
    try {
      const response = await fetch(assetUrl('/api/dev/memory'), {
        cache: 'no-store',
      });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      render((await response.json()) as ServerMemory);
    } catch (error) {
      panel.innerHTML = `<strong>Development memory</strong><p>Unable to read Node diagnostics: ${error instanceof Error ? error.message : 'unknown error'}</p>`;
    }
  };
  const onClick = () => {
    if (!panel.hidden) return close();
    panel.hidden = false;
    button.setAttribute('aria-expanded', 'true');
    void refresh();
    timer = window.setInterval(() => void refresh(), 2_000);
  };
  button.addEventListener('click', onClick);
  document.body.append(button, panel);
  return () => {
    close();
    button.removeEventListener('click', onClick);
    button.remove();
    panel.remove();
  };
}
