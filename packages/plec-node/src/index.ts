import { readFile, realpath, stat } from 'node:fs/promises';
import { createReadStream } from 'node:fs';
import { Readable } from 'node:stream';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { classifyPlecPath, canonicalRequestTarget } from './request-target.js';
import { loadApplication, MAX_REQUEST_BODY_BYTES, type NativeApplicationOptions, type PlecApplication } from './native.js';
import { loadSsrProviders } from './providers.js';

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
  clientDir?: string;
  artifact?: string;
  clientScript?: string;
  clientStyles?: string[];
  stylesHref?: string;
  preloads?: string[];
  customElements?: string[];
  document?: { title?: string; description?: string };
  server?: { entry?: string };
}

interface GeneratedApplication {
  handleRequest(request: Request, context: ApiRequestContext): Response | null | undefined | Promise<Response | null | undefined>;
  hasAction(id: string): boolean;
  invokeAction(id: string, args: unknown[], context: unknown): Promise<unknown>;
}

interface ApiRequestContext {
  url: string;
  pathname: string;
  method: string;
  headers: Record<string, string>;
  cookies: Record<string, string>;
  params: Record<string, string>;
  query: Record<string, string | string[]>;
}

const MAX_ACTIVE_REQUESTS = 256;
const MAX_AGGREGATE_API_PREREAD_BYTES = 32 * 1024 * 1024;

export async function createPlecHandler(options: PlecNodeOptions): Promise<PlecHandler> {
  const dir = await realpath(path.resolve(options.dir));
  const manifestPath = await realpath(path.join(dir, 'plec-server.json'));
  if (!inside(dir, manifestPath)) throw new Error('Plec manifest escapes the distribution directory');
  const manifest = JSON.parse(await readFile(manifestPath, 'utf8')) as ServerManifest;
  const publicRoot = await realpath(path.resolve(dir, manifest.publicDir ?? 'public'));
  const clientRoot = await realpath(path.resolve(dir, manifest.clientDir ?? 'client'));
  const providerManifestPath = path.resolve(dir, manifest.clientDir ?? 'client', 'host-providers.json');
  const artifactPath = await realpath(path.resolve(dir, manifest.artifact ?? 'server/route-artifact.json'));
  if (typeof manifest.server?.entry !== 'string' || manifest.server.entry.length === 0) {
    throw new Error('Plec server entry is missing');
  }
  const appEntry = await realpath(path.resolve(dir, manifest.server.entry));
  if (!inside(dir, publicRoot) || !inside(dir, artifactPath)) {
    throw new Error('Plec manifest path escapes the distribution directory');
  }
  if (!inside(dir, appEntry)) {
    throw new Error('Plec server entry escapes the distribution directory');
  }
  const generated = await import(pathToFileURL(appEntry).href) as GeneratedApplication;
  if (typeof generated.handleRequest !== 'function' || typeof generated.hasAction !== 'function' || typeof generated.invokeAction !== 'function') {
    throw new Error('generated server application has an invalid server interface');
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
  const renderHost = await loadSsrProviders(dir, providerManifestPath);
  const invokeAction = async (payload: string): Promise<string> => {
    const { id, arguments: args, context } = JSON.parse(payload) as {
      id: string; arguments: unknown[]; context: unknown;
    };
    if (!generated?.hasAction(id)) return JSON.stringify({ found: false });
    const value = await generated.invokeAction(id, args, context);
    return JSON.stringify({ found: true, value: value ?? null });
  };
  const application: PlecApplication = await loadApplication(nativeOptions, renderHost, invokeAction);
  let closed = false;
  let activeApiRequests = 0;
  let reservedApiBytes = 0;

  return {
    async fetch(request: Request): Promise<Response> {
      if (closed) throw new Error('PLEC_APPLICATION_CLOSED');
      let pathname: string;
      let rawQuery: string;
      try {
        const target = canonicalRequestTarget(request.url);
        pathname = target.path;
        rawQuery = target.query;
      } catch {
        return new Response('Bad Request', { status: 400 });
      }
      const routeClass = classifyPlecPath(pathname);
      if (routeClass === 'api') {
        if (activeApiRequests >= MAX_ACTIVE_REQUESTS) return apiError(503, 'service unavailable');
        activeApiRequests += 1;
        let reserved = 0;
        let releaseOnReturn = true;
        let released = false;
        const releaseAdmission = (): void => {
          if (released) return;
          released = true;
          reservedApiBytes -= reserved;
          activeApiRequests -= 1;
        };
        try {
          if (request.signal.aborted) throw request.signal.reason ?? new DOMException('The request was aborted', 'AbortError');
          let apiRequest: Request;
          if (request.method === 'GET' || request.method === 'HEAD') {
            apiRequest = new Request(request.url, {
              method: request.method,
              headers: request.headers,
              signal: request.signal,
            });
          } else {
            const declared = request.headers.get('content-length');
            const declaredLength = declared !== null && /^\d+$/u.test(declared) ? Number(declared) : undefined;
            if (declaredLength !== undefined && declaredLength > MAX_REQUEST_BODY_BYTES) {
              return apiError(413, 'request body exceeds byte limit');
            }
            if (declaredLength !== undefined) {
              reserved = Math.min(declaredLength, MAX_REQUEST_BODY_BYTES);
              if (reservedApiBytes + reserved > MAX_AGGREGATE_API_PREREAD_BYTES) {
                reserved = 0;
                return apiError(503, 'service unavailable');
              }
              reservedApiBytes += reserved;
            }
            const chunks = await readApiBody(request, reserved, (additional) => {
              if (reservedApiBytes + additional > MAX_AGGREGATE_API_PREREAD_BYTES) return false;
              reservedApiBytes += additional;
              reserved += additional;
              return true;
            });
            apiRequest = new Request(request.url, {
              method: request.method,
              headers: request.headers,
              body: chunks.length === 0 ? null : chunksAsStream(chunks),
              signal: request.signal,
              ...(chunks.length === 0 ? {} : { duplex: 'half' } as RequestInit),
            });
          }
          if (request.signal.aborted) throw request.signal.reason ?? new DOMException('The request was aborted', 'AbortError');
          const context = buildApiContext(apiRequest, pathname, rawQuery, options.trustProxy ?? false);
          let response: Response | null | undefined;
          let handlerPromise: Promise<Response | null | undefined>;
          try {
            handlerPromise = Promise.resolve(generated.handleRequest(apiRequest, context));
          } catch (error) {
            console.error('[PLEC-API] handler failed', error);
            return apiError(500, 'Internal Server Error');
          }
          try {
            response = await waitForApiHandler(handlerPromise, request.signal, () => {
              // JS handlers cannot be preempted. Keep capacity until their promise settles.
              releaseOnReturn = false;
              void handlerPromise.then(releaseAdmission, releaseAdmission);
            });
          } catch (error) {
            if (request.signal.aborted) throw error;
            console.error('[PLEC-API] handler failed', error);
            return apiError(500, 'Internal Server Error');
          }
          if (response == null) return apiError(404, 'endpoint not found');
          if (!(response instanceof Response)) {
            console.error('[PLEC-API] handler returned an invalid response');
            return apiError(500, 'Internal Server Error');
          }
          if (apiRequest.method === 'HEAD' || response.status === 204 || response.status === 304) {
            return new Response(null, { status: response.status, statusText: response.statusText, headers: response.headers });
          }
          if (response.body) {
            releaseOnReturn = false;
            return new Response(trackApiResponseBody(response.body, releaseAdmission), {
              status: response.status,
              statusText: response.statusText,
              headers: response.headers,
            });
          }
          return response;
        } catch (error) {
          if (error instanceof ApiBodyTooLarge) return apiError(413, 'request body exceeds byte limit');
          if (error instanceof ApiBodyOverloaded) return apiError(503, 'service unavailable');
          if (error instanceof ApiBodyTimedOut) return apiError(408, 'request body timed out');
          if (request.signal.aborted) throw request.signal.reason ?? error;
          if (error instanceof Error && error.message.includes('PLEC_APPLICATION_CLOSED')) throw error;
          console.error('[PLEC-API] request failed', error);
          return apiError(400, 'invalid request body');
        } finally {
          if (releaseOnReturn) releaseAdmission();
        }
      }
      if (routeClass === 'action') {
        const id = pathname.slice('/_plec/actions/'.length);
        if (!id || !/^[A-Za-z0-9_-]+$/u.test(id)) {
          return actionError(404, 'unknown server action');
        }
        let reader: ReadableStreamDefaultReader<Uint8Array> | undefined;
        const body = request.body
          ? new ReadableStream<Uint8Array>({
              start() { reader = request.body!.getReader(); },
              async pull(controller) {
                try {
                  const result = await reader!.read();
                  if (result.done) controller.close();
                  else controller.enqueue(result.value);
                } catch (error) {
                  controller.error(error);
                }
              },
              async cancel(reason) { await reader?.cancel(reason); },
            }, { highWaterMark: 0 })
          : new ReadableStream<Uint8Array>({ start(controller) { controller.close(); } });
        const headers: { name: string; value: string }[] = [];
        request.headers.forEach((value, name) => {
          if (name !== 'host' && name !== 'x-forwarded-proto') headers.push({ name, value });
        });
        const requestUrl = new URL(request.url);
        headers.push({ name: 'host', value: requestUrl.host });
        const forwarded = options.trustProxy
          ? request.headers.get('x-forwarded-proto')?.split(',')[0]?.trim().toLowerCase()
          : undefined;
        const scheme = forwarded === 'http' || forwarded === 'https'
          ? forwarded
          : requestUrl.protocol.slice(0, -1);
        headers.push({ name: 'x-forwarded-proto', value: scheme });
        const response = await application.handleAction(
          { method: request.method, url: request.url, headers },
          body,
          async () => { await reader?.cancel('native action stream stopped'); },
        );
        const responseHeaders = new Headers();
        for (const entry of response.headers) responseHeaders.append(entry.name, entry.value);
        return new Response(response.body(), { status: response.status, headers: responseHeaders });
      }
      if (routeClass === 'static') return serveStatic(request, pathname, publicRoot, clientRoot);
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

async function serveStatic(request: Request, pathname: string, publicRoot: string, clientRoot: string): Promise<Response> {
  if (request.method !== 'GET' && request.method !== 'HEAD') return new Response(null, { status: 405, headers: { allow: 'GET, HEAD' } });
  if (pathname === '/_plec') return assetNotFound();
  const clientAsset = pathname.startsWith('/_plec/');
  const root = clientAsset ? clientRoot : publicRoot;
  const relativeEncoded = clientAsset ? pathname.slice('/_plec/'.length) : pathname.slice(1);
  let segments: string[];
  try {
    segments = relativeEncoded.split('/').map((part) => decodeURIComponent(part));
  } catch { return invalidAssetPath(); }
  if (segments.some((part) => part === '' || part === '.' || part === '..' || part.includes('/') || part.includes('\\'))) {
    return invalidAssetPath();
  }
  const relative = path.join(...segments);
  try {
    const rootReal = await realpath(root);
    let candidate = await realpath(path.join(rootReal, relative));
    if (!inside(rootReal, candidate)) return invalidAssetPath();
    const mediaType = contentType(candidate);
    const variants: Partial<Record<'br' | 'gzip', string>> = {};
    for (const [coding, suffix] of [['br', '.br'], ['gzip', '.gz']] as const) {
      try {
        const sidecar = await realpath(`${candidate}${suffix}`);
        if (inside(rootReal, sidecar) && (await stat(sidecar)).isFile()) variants[coding] = sidecar;
      } catch { /* absent sidecars are not available representations */ }
    }
    const qualities = parseAcceptEncoding(request.headers.get('accept-encoding') ?? '');
    const available: ('br' | 'gzip' | 'identity')[] = ['identity'];
    if (variants.br) available.push('br');
    if (variants.gzip) available.push('gzip');
    const selected = available
      .filter((coding) => encodingQuality(qualities, coding) > 0)
      .sort((a, b) => encodingQuality(qualities, b) - encodingQuality(qualities, a)
        || (a === 'br' ? -1 : b === 'br' ? 1 : a === 'gzip' ? -1 : b === 'gzip' ? 1 : 0))[0];
    if (!selected) return new Response(null, { status: 406, headers: { vary: 'Accept-Encoding' } });
    const encoding = selected === 'identity' ? undefined : selected;
    if (encoding) candidate = variants[encoding]!;
    const info = await stat(candidate);
    if (!info.isFile()) return assetNotFound();
    const headers = new Headers({ 'content-type': mediaType, 'cache-control': 'no-cache' });
    if (Object.keys(variants).length > 0) headers.set('vary', 'Accept-Encoding');
    if (encoding) headers.set('content-encoding', encoding);
    const range = request.headers.get('range');
    const size = info.size;
    let start = 0;
    let end = size - 1;
    let status = 200;
    if (range && !encoding) {
      const parsedRange = parseSingleByteRange(range, size);
      if (parsedRange?.kind === 'satisfiable') {
        ({ start, end } = parsedRange);
          status = 206;
          headers.set('accept-ranges', 'bytes');
          headers.set('content-range', `bytes ${start}-${end}/${size}`);
      } else if (parsedRange?.kind === 'unsatisfiable') {
        headers.set('accept-ranges', 'bytes');
        headers.set('content-range', `bytes */${size}`);
        return new Response(null, { status: 416, headers });
      }
    }
    headers.set('content-length', String(status === 206 ? end - start + 1 : size));
    if (request.method === 'HEAD') return new Response(null, { status, headers });
    const stream = createReadStream(candidate, status === 206 ? { start, end } : undefined);
    return new Response(Readable.toWeb(stream) as ReadableStream<Uint8Array>, { status, headers });
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT' || (error as NodeJS.ErrnoException).code === 'ENOTDIR') return assetNotFound();
    return invalidAssetPath();
  }
}

function assetNotFound(): Response { return new Response(JSON.stringify({ error: 'asset not found' }), { status: 404, headers: { 'content-type': 'application/json; charset=utf-8' } }); }
function invalidAssetPath(): Response { return new Response(JSON.stringify({ error: 'invalid asset path' }), { status: 400, headers: { 'content-type': 'application/json; charset=utf-8' } }); }
function contentType(file: string): string {
  const ext = path.extname(file).toLowerCase();
  return ({
    '.avif': 'image/avif', '.css': 'text/css; charset=utf-8', '.csv': 'text/csv; charset=utf-8',
    '.gif': 'image/gif', '.htm': 'text/html; charset=utf-8', '.html': 'text/html; charset=utf-8',
    '.ico': 'image/x-icon', '.jpeg': 'image/jpeg', '.jpg': 'image/jpeg', '.js': 'text/javascript; charset=utf-8',
    '.json': 'application/json; charset=utf-8', '.mjs': 'text/javascript; charset=utf-8', '.mp3': 'audio/mpeg',
    '.mp4': 'video/mp4', '.otf': 'font/otf', '.pdf': 'application/pdf', '.png': 'image/png',
    '.svg': 'image/svg+xml', '.txt': 'text/plain; charset=utf-8', '.wasm': 'application/wasm',
    '.webmanifest': 'application/manifest+json', '.webp': 'image/webp', '.woff': 'font/woff', '.woff2': 'font/woff2',
    '.xml': 'application/xml; charset=utf-8',
  } as Record<string, string>)[ext] ?? 'application/octet-stream';
}

type EncodingQualities = Map<string, number>;
function parseAcceptEncoding(header: string): EncodingQualities {
  const result: EncodingQualities = new Map();
  for (const part of header.split(',')) {
    const [rawToken, ...parameters] = part.trim().toLowerCase().split(';');
    if (!rawToken || !/^[!#$%&'*+.^_`|~0-9a-z-]+$/u.test(rawToken)) continue;
    let quality = 1;
    let valid = true;
    for (const parameter of parameters) {
      const match = /^\s*q\s*=\s*(0(?:\.\d{0,3})?|1(?:\.0{0,3})?)\s*$/u.exec(parameter);
      if (!match) { valid = false; break; }
      quality = Number(match[1]);
    }
    if (valid) result.set(rawToken, quality);
  }
  return result;
}

function encodingQuality(qualities: EncodingQualities, coding: string): number {
  if (qualities.has(coding)) return qualities.get(coding)!;
  if (coding === 'identity') return 1;
  return qualities.get('*') ?? 0;
}

type ParsedRange = { kind: 'satisfiable'; start: number; end: number } | { kind: 'unsatisfiable' } | undefined;
function parseSingleByteRange(header: string, size: number): ParsedRange {
  const match = /^bytes=(\d*)-(\d*)$/iu.exec(header.trim());
  if (!match || (!match[1] && !match[2])) return undefined;
  const first = match[1] ? Number(match[1]) : undefined;
  const last = match[2] ? Number(match[2]) : undefined;
  if ((first !== undefined && !Number.isSafeInteger(first)) || (last !== undefined && !Number.isSafeInteger(last))) return undefined;
  if (size === 0 || (first === undefined && last === 0) || (first !== undefined && first >= size) || (first !== undefined && last !== undefined && last < first)) return { kind: 'unsatisfiable' };
  const start = first ?? Math.max(0, size - last!);
  const end = first === undefined ? size - 1 : Math.min(last ?? size - 1, size - 1);
  return { kind: 'satisfiable', start, end };
}

function actionError(status: number, message: string): Response {
  return new Response(JSON.stringify({ error: message }), {
    status,
    headers: { 'content-type': 'application/json; charset=utf-8', 'cache-control': 'no-store' },
  });
}

class ApiBodyTooLarge extends Error {}
class ApiBodyOverloaded extends Error {}
class ApiBodyTimedOut extends Error {}

async function readApiBody(
  request: Request,
  initialReservation: number,
  reserveAdditional: (bytes: number) => boolean,
): Promise<Uint8Array[]> {
  if (!request.body) return [];
  const reader = request.body.getReader();
  const chunks: Uint8Array[] = [];
  let total = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let timedOut = false;
  let abortListener: (() => void) | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(() => {
      timedOut = true;
      void reader.cancel('API request body timed out');
      reject(new ApiBodyTimedOut());
    }, 30_000);
  });
  const aborted = new Promise<never>((_, reject) => {
    abortListener = () => {
      void reader.cancel(request.signal.reason);
      reject(request.signal.reason ?? new DOMException('The request was aborted', 'AbortError'));
    };
    if (request.signal.aborted) abortListener();
    else request.signal.addEventListener('abort', abortListener, { once: true });
  });
  try {
    const read = async (): Promise<Uint8Array[]> => {
      for (;;) {
        const result = await reader.read();
        if (result.done) return chunks;
        const size = result.value.byteLength;
        if (total + size > MAX_REQUEST_BODY_BYTES) {
          await reader.cancel('API request body exceeds limit');
          throw new ApiBodyTooLarge();
        }
        const excessReservation = Math.max(0, total + size - initialReservation) - Math.max(0, total - initialReservation);
        if (excessReservation > 0 && !reserveAdditional(excessReservation)) {
          await reader.cancel('API request admission limit reached');
          throw new ApiBodyOverloaded();
        }
        total += size;
        chunks.push(result.value);
      }
    };
    return await Promise.race([read(), timeout, aborted]);
  } catch (error) {
    if (timedOut) throw new ApiBodyTimedOut();
    throw error;
  } finally {
    if (timer) clearTimeout(timer);
    if (abortListener) request.signal.removeEventListener('abort', abortListener);
    reader.releaseLock();
  }
}

function chunksAsStream(chunks: Uint8Array[]): ReadableStream<Uint8Array> {
  let index = 0;
  return new ReadableStream<Uint8Array>({
    pull(controller) {
      if (index === chunks.length) controller.close();
      else controller.enqueue(chunks[index++]!);
    },
  }, { highWaterMark: 0 });
}

async function waitForApiHandler<T>(
  handler: Promise<T>,
  signal: AbortSignal,
  detached: () => void,
): Promise<T> {
  if (signal.aborted) {
    detached();
    throw signal.reason ?? new DOMException('The request was aborted', 'AbortError');
  }
  let onAbort: (() => void) | undefined;
  const aborted = new Promise<never>((_, reject) => {
    onAbort = () => {
      detached();
      reject(signal.reason ?? new DOMException('The request was aborted', 'AbortError'));
    };
    signal.addEventListener('abort', onAbort, { once: true });
  });
  try {
    return await Promise.race([handler, aborted]);
  } finally {
    if (onAbort) signal.removeEventListener('abort', onAbort);
  }
}

function buildApiContext(request: Request, pathname: string, rawQuery: string, trustProxy: boolean): ApiRequestContext {
  const headers: Record<string, string> = {};
  request.headers.forEach((value, name) => { headers[name] = value; });
  const url = new URL(request.url);
  const forwarded = trustProxy
    ? request.headers.get('x-forwarded-proto')?.split(',')[0]?.trim().toLowerCase()
    : undefined;
  const scheme = forwarded === 'http' || forwarded === 'https' ? forwarded : url.protocol.slice(0, -1);
  url.protocol = `${scheme}:`;
  const cookies: Record<string, string> = {};
  for (const part of (headers.cookie ?? '').split(';')) {
    const index = part.indexOf('=');
    if (index < 0) continue;
    cookies[part.slice(0, index).trim()] = decodeURIComponent(part.slice(index + 1).trim());
  }
  const query: Record<string, string | string[]> = {};
  for (const pair of rawQuery.split('&')) {
    if (!pair) continue;
    const index = pair.indexOf('=');
    const key = decodeFormComponent(index < 0 ? pair : pair.slice(0, index));
    const value = decodeFormComponent(index < 0 ? '' : pair.slice(index + 1));
    const existing = query[key];
    if (existing === undefined) query[key] = value;
    else if (Array.isArray(existing)) existing.push(value);
    else query[key] = [existing, value];
  }
  return {
    url: url.href,
    pathname,
    method: request.method,
    headers,
    cookies,
    params: {},
    query,
  };
}

function decodeFormComponent(value: string): string {
  return decodeURIComponent(value.replaceAll('+', ' '));
}

function trackApiResponseBody(
  body: ReadableStream<Uint8Array>,
  release: () => void,
): ReadableStream<Uint8Array> {
  const reader = body.getReader();
  return new ReadableStream<Uint8Array>({
    async pull(controller) {
      try {
        const result = await reader.read();
        if (result.done) {
          controller.close();
          release();
        } else {
          controller.enqueue(result.value);
        }
      } catch (error) {
        controller.error(error);
        release();
      }
    },
    async cancel(reason) {
      try { await reader.cancel(reason); }
      finally { release(); }
    },
  });
}

function apiError(status: number, message: string): Response {
  return new Response(JSON.stringify({ error: message }), {
    status,
    headers: { 'content-type': 'application/json; charset=utf-8', 'cache-control': 'no-store' },
  });
}

function inside(root: string, candidate: string): boolean {
  const relative = path.relative(root, candidate);
  return relative === '' || (!relative.startsWith(`..${path.sep}`) && relative !== '..' && !path.isAbsolute(relative));
}
