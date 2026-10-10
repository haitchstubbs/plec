import { readFile, realpath, stat } from 'node:fs/promises';
import { isAbsolute, relative, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const MAX_PROVIDER_MANIFEST_BYTES = 1024 * 1024;
const MAX_HOST_RENDER_BYTES = 1024 * 1024;

interface ProviderManifest {
  version: 2;
  revision: string;
  providers: Array<{
    id: string;
    module: string;
    components: string[];
    ssr: boolean;
  }>;
}

interface ProviderComponent {
  render?: (props: Record<string, unknown>) => string | Promise<string>;
}

type ProviderFactory = () => Record<string, ProviderComponent>;

export type RenderHostRequest = {
  provider: string;
  component: string;
  props: Record<string, unknown>;
};

export async function loadSsrProviders(
  distRoot: string,
  manifestPath: string,
): Promise<(requestJson: string) => Promise<string>> {
  let canonicalManifest: string;
  try {
    canonicalManifest = await realpath(manifestPath);
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') {
      throw new Error('host provider manifest is missing');
    }
    throw error;
  }
  if (!isWithin(distRoot, canonicalManifest)) {
    throw new Error(
      'host provider manifest escapes the distribution directory',
    );
  }
  const metadata = await stat(canonicalManifest);
  if (metadata.size > MAX_PROVIDER_MANIFEST_BYTES) {
    throw new Error('host provider manifest exceeds limit');
  }
  const bytes = await readFile(canonicalManifest);
  const manifest = JSON.parse(bytes.toString('utf8')) as unknown;
  if (!isProviderManifest(manifest))
    throw new Error('invalid host provider manifest');

  const clientRoot = resolve(canonicalManifest, '..');
  const assetsRoot = await realpath(resolve(clientRoot, 'assets'));
  const providers = new Map<
    string,
    {
      components: Set<string>;
      registry: Record<string, ProviderComponent>;
    }
  >();
  for (const entry of manifest.providers) {
    if (!entry.ssr) continue;
    const assetPath = providerAssetPath(entry.id, entry.module);
    const modulePath = resolve(clientRoot, assetPath);
    if (!isWithin(assetsRoot, modulePath))
      throw invalidModule(entry.id);
    let canonicalModule: string;
    try {
      canonicalModule = await realpath(modulePath);
    } catch {
      throw invalidModule(entry.id);
    }
    if (!isWithin(assetsRoot, canonicalModule))
      throw invalidModule(entry.id);
    const imported = (await import(
      pathToFileURL(canonicalModule).href
    )) as { default?: unknown };
    if (typeof imported.default !== 'function') {
      throw new Error(
        `host provider ${entry.id} has no default factory`,
      );
    }
    const registry = (imported.default as ProviderFactory)();
    if (
      !registry ||
      typeof registry !== 'object' ||
      Array.isArray(registry)
    ) {
      throw new Error(
        `host provider ${entry.id} has an invalid default factory`,
      );
    }
    providers.set(entry.id, {
      components: new Set(entry.components),
      registry,
    });
  }

  return async (requestJson: string): Promise<string> => {
    const request = JSON.parse(
      requestJson,
    ) as Partial<RenderHostRequest>;
    if (
      typeof request.provider !== 'string' ||
      typeof request.component !== 'string' ||
      !request.props ||
      typeof request.props !== 'object' ||
      Array.isArray(request.props)
    )
      throw new Error('invalid host render request');
    const provider = providers.get(request.provider);
    const render = provider?.components.has(request.component)
      ? provider.registry[request.component]?.render
      : undefined;
    if (!render) return '';
    const html = await render(request.props);
    if (
      typeof html !== 'string' ||
      Buffer.byteLength(html) > MAX_HOST_RENDER_BYTES
    ) {
      throw new Error('invalid host render response');
    }
    return html;
  };
}

function providerAssetPath(id: string, module: string): string {
  let parsed: URL;
  try {
    parsed = new URL(module, 'http://plec.internal');
  } catch {
    throw invalidModule(id);
  }
  if (
    parsed.origin !== 'http://plec.internal' ||
    parsed.search ||
    parsed.hash ||
    !parsed.pathname.startsWith('/_plec/assets/')
  ) {
    throw invalidModule(id);
  }
  let assetPath: string;
  try {
    assetPath = decodeURIComponent(
      parsed.pathname.slice('/_plec/'.length),
    );
  } catch {
    throw invalidModule(id);
  }
  if (!assetPath.startsWith('assets/') || assetPath.includes('\\'))
    throw invalidModule(id);
  return assetPath;
}

function isProviderManifest(value: unknown): value is ProviderManifest {
  if (!value || typeof value !== 'object') return false;
  const candidate = value as Partial<ProviderManifest>;
  return (
    candidate.version === 2 &&
    typeof candidate.revision === 'string' &&
    Array.isArray(candidate.providers) &&
    candidate.providers.every(
      (entry) =>
        !!entry &&
        typeof entry.id === 'string' &&
        entry.id.length > 0 &&
        typeof entry.module === 'string' &&
        Array.isArray(entry.components) &&
        entry.components.every(
          (component) => typeof component === 'string',
        ) &&
        typeof entry.ssr === 'boolean',
    )
  );
}

function isWithin(root: string, candidate: string): boolean {
  const rel = relative(root, candidate);
  return (
    rel === '' ||
    (!isAbsolute(rel) &&
      rel !== '..' &&
      !rel.startsWith(`..${process.platform === 'win32' ? '\\' : '/'}`))
  );
}

function invalidModule(id: string): Error {
  return new Error(`invalid host provider module for ${id}`);
}
