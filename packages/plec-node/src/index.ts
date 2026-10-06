import { readFile, realpath } from 'node:fs/promises';
import path from 'node:path';
import { classifyPlecPath, canonicalRequestTarget } from './request-target.js';
import { loadApplication, type NativeApplicationOptions, type PlecApplication } from './native.js';

export interface PlecNodeOptions {
  dir: string;
  development?: boolean;
  trustProxy?: boolean;
}

export interface PlecHandler {
  fetch(request: Request): Promise<Response>;
  close(): Promise<void>;
}

interface ServerManifest {
  publicDir?: string;
  artifact?: string;
  clientScript?: string;
  clientStyles?: string[];
  stylesHref?: string;
  preloads?: string[];
  customElements?: string[];
  document?: { title?: string; description?: string };
}

export async function createPlecHandler(options: PlecNodeOptions): Promise<PlecHandler> {
  const dir = await realpath(path.resolve(options.dir));
  const manifestPath = await realpath(path.join(dir, 'plec-server.json'));
  if (!inside(dir, manifestPath)) throw new Error('Plec manifest escapes the distribution directory');
  const manifest = JSON.parse(await readFile(manifestPath, 'utf8')) as ServerManifest;
  const publicRoot = await realpath(path.resolve(dir, manifest.publicDir ?? 'public'));
  const artifactPath = await realpath(path.resolve(dir, manifest.artifact ?? 'server/route-artifact.json'));
  if (!inside(dir, publicRoot) || !inside(dir, artifactPath)) {
    throw new Error('Plec manifest path escapes the distribution directory');
  }
  const nativeOptions: NativeApplicationOptions = {
    artifactPath,
    clientScript: manifest.clientScript,
    clientStyles: manifest.clientStyles,
    stylesHref: manifest.stylesHref,
    preloads: manifest.preloads,
    customElements: manifest.customElements,
    title: manifest.document?.title,
    description: manifest.document?.description,
    development: options.development ?? false,
  };
  const application: PlecApplication = await loadApplication(nativeOptions);
  let closed = false;

  return {
    async fetch(request: Request): Promise<Response> {
      if (closed) throw new Error('PLEC_APPLICATION_CLOSED');
      let pathname: string;
      try {
        pathname = canonicalRequestTarget(request.url).path;
      } catch {
        return new Response('Bad Request', { status: 400 });
      }
      if (classifyPlecPath(pathname) !== 'document') {
        return new Response(JSON.stringify({ error: 'not found' }), {
          status: 404,
          headers: { 'content-type': 'application/json; charset=utf-8', 'cache-control': 'no-store' },
        });
      }
      if (request.method !== 'GET' && request.method !== 'HEAD') {
        return new Response(null, { status: 405, headers: { allow: 'GET, HEAD' } });
      }
      const headers: { name: string; value: string }[] = [];
      request.headers.forEach((value, name) => {
        if (name !== 'host' && name !== 'x-forwarded-proto') headers.push({ name, value });
      });
      const requestUrl = new URL(request.url);
      headers.push({ name: 'host', value: requestUrl.host });
      const forwarded = options.trustProxy
        ? request.headers.get('x-forwarded-proto')?.split(',')[0]?.trim().toLowerCase()
        : undefined;
      const scheme = forwarded === 'http' || forwarded === 'https' ? forwarded : requestUrl.protocol.slice(0, -1);
      headers.push({ name: 'x-forwarded-proto', value: scheme });
      try {
        const nativeResponse = await application.handleDocument({
          method: request.method,
          url: request.url,
          headers,
        });
        const responseHeaders = new Headers();
        for (const entry of nativeResponse.headers) responseHeaders.append(entry.name, entry.value);
        const body = request.method === 'HEAD' || nativeResponse.status === 204 || nativeResponse.status === 304
          ? null
          : nativeResponse.body();
        return new Response(body, { status: nativeResponse.status, headers: responseHeaders });
      } catch (error) {
        if (error instanceof Error && error.message.includes('PLEC_APPLICATION_CLOSED')) throw error;
        const diagnostic = error instanceof Error ? error.message : String(error);
        try {
          const shell = await readFile(path.join(publicRoot, 'index.html'));
          const responseHeaders = new Headers({
            'content-type': 'text/html; charset=utf-8',
            'cache-control': 'no-cache',
          });
          if (options.development) responseHeaders.set('x-plec-ssr-fallback', diagnostic);
          return new Response(shell, { status: 200, headers: responseHeaders });
        } catch {
          return new Response(JSON.stringify({ error: 'Internal Server Error' }), {
            status: 500,
            headers: { 'content-type': 'application/json; charset=utf-8', 'cache-control': 'no-store' },
          });
        }
      }
    },
    async close(): Promise<void> {
      if (closed) return;
      closed = true;
      await application.close();
    },
  };
}

function inside(root: string, candidate: string): boolean {
  const relative = path.relative(root, candidate);
  return relative === '' || (!relative.startsWith(`..${path.sep}`) && relative !== '..' && !path.isAbsolute(relative));
}
