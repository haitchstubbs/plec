import { readFile, realpath } from 'node:fs/promises';
import { createReadStream } from 'node:fs';
import { Readable } from 'node:stream';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import {
  classifyPlecPath,
  canonicalRequestTarget,
} from './request-target.js';
import {
  loadApplication,
  MAX_REQUEST_BODY_BYTES,
  type NativeApplicationOptions,
  type PlecApplication,
} from './native.js';
import { loadSsrProviders } from './providers.js';
import { serveStatic } from './static.js';
import type { PlecTransportContext } from './transport.js';

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
  version?: number;
  publicDir?: string;
  clientDir?: string;
  artifact?: string;
  clientScript?: string;
  clientStyles?: string[];
  stylesHref?: string;
  preloads?: string[];
  customElements?: string[];
  document?: { title?: string; description?: string };
  server?: { entry?: string; runtime?: string };
}

interface GeneratedApplication {
  handleRequest(
    request: Request,
    context: ApiRequestContext,
  ): Response | null | undefined | Promise<Response | null | undefined>;
  hasAction(id: string): boolean;
  invokeAction(
    id: string,
    args: unknown[],
    context: unknown,
  ): Promise<unknown>;
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

export async function createPlecHandler(
  options: PlecNodeOptions,
): Promise<PlecHandler> {
  const dir = await realpath(path.resolve(options.dir));
  const manifestPath = await realpath(
    path.join(dir, 'plec-server.json'),
  );
  if (!inside(dir, manifestPath))
    throw new Error('Plec manifest escapes the distribution directory');
  const manifest = JSON.parse(
    await readFile(manifestPath, 'utf8'),
  ) as ServerManifest;
  if ((manifest.version ?? 1) !== 1) {
    throw new Error(
      `unsupported Plec server manifest version ${manifest.version} (expected 1)`,
    );
  }
  const publicRoot = await realpath(
    path.resolve(dir, manifest.publicDir ?? 'public'),
  );
  const clientRoot = await realpath(
    path.resolve(dir, manifest.clientDir ?? 'client'),
  );
  const providerManifestPath = path.resolve(
    dir,
    manifest.clientDir ?? 'client',
    'host-providers.json',
  );
  const artifactPath = await realpath(
    path.resolve(
      dir,
      manifest.artifact ?? 'server/route-artifact.json',
    ),
  );
  if (
    typeof manifest.server?.entry !== 'string' ||
    manifest.server.entry.length === 0
  ) {
    throw new Error('Plec server entry is missing');
  }
  const appEntry = await realpath(
    path.resolve(dir, manifest.server.entry),
  );
  if (!inside(dir, publicRoot) || !inside(dir, artifactPath)) {
    throw new Error(
      'Plec manifest path escapes the distribution directory',
    );
  }
  if (!inside(dir, appEntry)) {
    throw new Error(
      'Plec server entry escapes the distribution directory',
    );
  }
  const generated = (await import(
    pathToFileURL(appEntry).href
  )) as GeneratedApplication;
  if (
    typeof generated.handleRequest !== 'function' ||
    typeof generated.hasAction !== 'function' ||
    typeof generated.invokeAction !== 'function'
  ) {
    throw new Error(
      'generated server application has an invalid server interface',
    );
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
    const {
      id,
      arguments: args,
      context,
    } = JSON.parse(payload) as {
      id: string;
      arguments: unknown[];
      context: unknown;
    };
    if (!generated?.hasAction(id))
      return JSON.stringify({ found: false });
    const value = await generated.invokeAction(id, args, context);
    return JSON.stringify({ found: true, value: value ?? null });
  };
  const application: PlecApplication = await loadApplication(
    nativeOptions,
    renderHost,
    invokeAction,
  );
  let closePromise: Promise<void> | undefined;
  let closed = false;
  let activeApiRequests = 0;
  let reservedApiBytes = 0;

  const plecHandler: PlecHandler & {
    /** @internal Raw transport dispatch used by the first-party HTTP host. */
    dispatch(
      request: Request,
      transport: PlecTransportContext,
    ): Promise<Response>;
  } = {
    async fetch(request: Request): Promise<Response> {
      if (closed) throw new Error('PLEC_APPLICATION_CLOSED');
      let target: ReturnType<typeof canonicalRequestTarget>;
      let url: URL;
      try {
        target = canonicalRequestTarget(request.url);
        url = new URL(request.url);
      } catch {
        return new Response('Bad Request', { status: 400 });
      }
      return plecHandler.dispatch(request, {
        pathname: target.path,
        rawQuery: target.query,
        scheme: url.protocol === 'https:' ? 'https' : 'http',
        authority: url.host,
        signal: request.signal,
      });
    },
    async dispatch(
      request: Request,
      transport: PlecTransportContext,
    ): Promise<Response> {
      if (closed) throw new Error('PLEC_APPLICATION_CLOSED');
      const { pathname, rawQuery } = transport;
      const routeClass = classifyPlecPath(pathname);
      if (routeClass === 'api') {
        if (activeApiRequests >= MAX_ACTIVE_REQUESTS)
          return apiError(503, 'service unavailable');
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
          if (request.signal.aborted)
            throw (
              request.signal.reason ??
              new DOMException('The request was aborted', 'AbortError')
            );
          let apiRequest: Request;
          if (request.method === 'GET' || request.method === 'HEAD') {
            apiRequest = new Request(request.url, {
              method: request.method,
              headers: request.headers,
              signal: request.signal,
            });
          } else {
            const declared = request.headers.get('content-length');
            const declaredLength =
              declared !== null && /^\d+$/u.test(declared)
                ? Number(declared)
                : undefined;
            if (
              declaredLength !== undefined &&
              declaredLength > MAX_REQUEST_BODY_BYTES
            ) {
              return apiError(413, 'request body exceeds byte limit');
            }
            if (declaredLength !== undefined) {
              reserved = Math.min(
                declaredLength,
                MAX_REQUEST_BODY_BYTES,
              );
              if (
                reservedApiBytes + reserved >
                MAX_AGGREGATE_API_PREREAD_BYTES
              ) {
                reserved = 0;
                return apiError(503, 'service unavailable');
              }
              reservedApiBytes += reserved;
            }
            const chunks = await readApiBody(
              request,
              reserved,
              (additional) => {
                if (
                  reservedApiBytes + additional >
                  MAX_AGGREGATE_API_PREREAD_BYTES
                )
                  return false;
                reservedApiBytes += additional;
                reserved += additional;
                return true;
              },
            );
            apiRequest = new Request(request.url, {
              method: request.method,
              headers: request.headers,
              body: chunks.length === 0 ? null : chunksAsStream(chunks),
              signal: request.signal,
              ...(chunks.length === 0
                ? {}
                : ({ duplex: 'half' } as RequestInit)),
            });
          }
          if (request.signal.aborted)
            throw (
              request.signal.reason ??
              new DOMException('The request was aborted', 'AbortError')
            );
          const context = buildApiContext(
            apiRequest,
            transport,
            options.trustProxy ?? false,
          );
          const callbackPermit = application.tryAcquireCallback();
          if (!callbackPermit)
            return apiError(503, 'service unavailable');
          let response: Response | null | undefined;
          let handlerPromise: Promise<Response | null | undefined>;
          try {
            handlerPromise = Promise.resolve(
              generated.handleRequest(apiRequest, context),
            );
          } catch (error) {
            callbackPermit.release();
            console.error('[PLEC-API] handler failed', error);
            return apiError(500, 'Internal Server Error');
          }
          void handlerPromise.then(
            () => callbackPermit.release(),
            () => callbackPermit.release(),
          );
          try {
            response = await waitForApiHandler(
              handlerPromise,
              request.signal,
              () => {
                // JS handlers cannot be preempted. Keep capacity until their promise settles.
                releaseOnReturn = false;
                void handlerPromise.then(
                  releaseAdmission,
                  releaseAdmission,
                );
              },
            );
          } catch (error) {
            if (request.signal.aborted) throw error;
            console.error('[PLEC-API] handler failed', error);
            return apiError(500, 'Internal Server Error');
          }
          if (response == null)
            return apiError(404, 'endpoint not found');
          if (!(response instanceof Response)) {
            console.error(
              '[PLEC-API] handler returned an invalid response',
            );
            return apiError(500, 'Internal Server Error');
          }
          if (
            apiRequest.method === 'HEAD' ||
            response.status === 204 ||
            response.status === 304
          ) {
            return new Response(null, {
              status: response.status,
              statusText: response.statusText,
              headers: response.headers,
            });
          }
          if (response.body) {
            releaseOnReturn = false;
            return new Response(
              trackApiResponseBody(response.body, releaseAdmission),
              {
                status: response.status,
                statusText: response.statusText,
                headers: response.headers,
              },
            );
          }
          return response;
        } catch (error) {
          if (error instanceof ApiBodyTooLarge)
            return apiError(413, 'request body exceeds byte limit');
          if (error instanceof ApiBodyOverloaded)
            return apiError(503, 'service unavailable');
          if (error instanceof ApiBodyTimedOut)
            return apiError(408, 'request body timed out');
          if (request.signal.aborted)
            throw request.signal.reason ?? error;
          if (
            error instanceof Error &&
            error.message.includes('PLEC_APPLICATION_CLOSED')
          )
            throw error;
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
          ? new ReadableStream<Uint8Array>(
              {
                start() {
                  reader = request.body!.getReader();
                },
                async pull(controller) {
                  try {
                    const result = await reader!.read();
                    if (result.done) controller.close();
                    else controller.enqueue(result.value);
                  } catch (error) {
                    controller.error(error);
                  }
                },
                async cancel(reason) {
                  await reader?.cancel(reason);
                },
              },
              { highWaterMark: 0 },
            )
          : new ReadableStream<Uint8Array>({
              start(controller) {
                controller.close();
              },
            });
        const headers = nativeHeaders(
          request,
          transport,
          options.trustProxy ?? false,
        );
        const forwarded = options.trustProxy
          ? headerValues(transport, request, 'x-forwarded-proto')[0]
              ?.split(',')[0]
              ?.trim()
              .toLowerCase()
          : undefined;
        const scheme =
          forwarded === 'http' || forwarded === 'https'
            ? forwarded
            : transport.scheme;
        setNativeHeader(headers, 'x-forwarded-proto', scheme);
        const cancellation = application.createCancellation();
        const cancelNative = (): void => cancellation.cancel();
        if (transport.signal.aborted) cancelNative();
        else
          transport.signal.addEventListener('abort', cancelNative, {
            once: true,
          });
        let response;
        try {
          response = await application.handleAction(
            {
              method: request.method,
              url: transportUrl(transport),
              headers,
            },
            body as unknown as ReadableStream<Buffer>,
            async () => {
              await reader?.cancel('native action stream stopped');
            },
            cancellation,
          );
        } finally {
          transport.signal.removeEventListener('abort', cancelNative);
        }
        const responseHeaders = new Headers();
        for (const entry of response.headers)
          responseHeaders.append(entry.name, entry.value);
        return new Response(
          response.body() as unknown as ReadableStream<Uint8Array>,
          {
            status: response.status,
            headers: responseHeaders,
          },
        );
      }
      if (routeClass === 'static')
        return serveStatic(request, pathname, publicRoot, clientRoot);
      if (request.method !== 'GET' && request.method !== 'HEAD') {
        return new Response(null, {
          status: 405,
          headers: { allow: 'GET, HEAD' },
        });
      }
      const headers = nativeHeaders(
        request,
        transport,
        options.trustProxy ?? false,
      );
      const forwarded = options.trustProxy
        ? headerValues(transport, request, 'x-forwarded-proto')[0]
            ?.split(',')[0]
            ?.trim()
            .toLowerCase()
        : undefined;
      const scheme =
        forwarded === 'http' || forwarded === 'https'
          ? forwarded
          : transport.scheme;
      setNativeHeader(headers, 'x-forwarded-proto', scheme);
      const cancellation = application.createCancellation();
      const cancelNative = (): void => cancellation.cancel();
      if (transport.signal.aborted) cancelNative();
      else
        transport.signal.addEventListener('abort', cancelNative, {
          once: true,
        });
      try {
        const nativeResponse = await application.handleDocument(
          {
            method: request.method,
            url: transportUrl(transport),
            headers,
          },
          cancellation,
        );
        const responseHeaders = new Headers();
        for (const entry of nativeResponse.headers)
          responseHeaders.append(entry.name, entry.value);
        const body =
          request.method === 'HEAD' ||
          nativeResponse.status === 204 ||
          nativeResponse.status === 304
            ? null
            : (nativeResponse.body() as unknown as ReadableStream<Uint8Array>);
        return new Response(body, {
          status: nativeResponse.status,
          headers: responseHeaders,
        });
      } catch (error) {
        if (transport.signal.aborted) throw error;
        if (
          error instanceof Error &&
          error.message.includes('PLEC_APPLICATION_CLOSED')
        )
          throw error;
        if (
          error instanceof Error &&
          error.message.includes(
            'application callback capacity is exhausted',
          )
        )
          return apiError(503, 'service unavailable');
        const diagnostic =
          error instanceof Error ? error.message : String(error);
        try {
          const shell = await readFile(
            path.join(publicRoot, 'index.html'),
          );
          const responseHeaders = new Headers({
            'content-type': 'text/html; charset=utf-8',
            'cache-control': 'no-cache',
          });
          if (options.development)
            responseHeaders.set('x-plec-ssr-fallback', diagnostic);
          return new Response(shell, {
            status: 200,
            headers: responseHeaders,
          });
        } catch {
          return new Response(
            JSON.stringify({ error: 'Internal Server Error' }),
            {
              status: 500,
              headers: {
                'content-type': 'application/json; charset=utf-8',
                'cache-control': 'no-store',
              },
            },
          );
        }
      } finally {
        transport.signal.removeEventListener('abort', cancelNative);
      }
    },
    async close(): Promise<void> {
      if (closePromise) return closePromise;
      closed = true;
      closePromise = application.close();
      await closePromise;
    },
  };
  return plecHandler;
}

export interface ServeOptions extends PlecNodeOptions {
  host?: string;
  port?: number;
}

/** Start the first-party Node HTTP host. */
export async function serve(options: ServeOptions): Promise<void> {
  const { servePlecHandler } = await import('./http.js');
  return servePlecHandler(options);
}

function actionError(status: number, message: string): Response {
  return new Response(JSON.stringify({ error: message }), {
    status,
    headers: {
      'content-type': 'application/json; charset=utf-8',
      'cache-control': 'no-store',
    },
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
      reject(
        request.signal.reason ??
          new DOMException('The request was aborted', 'AbortError'),
      );
    };
    if (request.signal.aborted) abortListener();
    else
      request.signal.addEventListener('abort', abortListener, {
        once: true,
      });
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
        const excessReservation =
          Math.max(0, total + size - initialReservation) -
          Math.max(0, total - initialReservation);
        if (
          excessReservation > 0 &&
          !reserveAdditional(excessReservation)
        ) {
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
    if (abortListener)
      request.signal.removeEventListener('abort', abortListener);
    reader.releaseLock();
  }
}

function chunksAsStream(
  chunks: Uint8Array[],
): ReadableStream<Uint8Array> {
  let index = 0;
  return new ReadableStream<Uint8Array>(
    {
      pull(controller) {
        if (index === chunks.length) controller.close();
        else controller.enqueue(chunks[index++]!);
      },
    },
    { highWaterMark: 0 },
  );
}

async function waitForApiHandler<T>(
  handler: Promise<T>,
  signal: AbortSignal,
  detached: () => void,
): Promise<T> {
  if (signal.aborted) {
    detached();
    throw (
      signal.reason ??
      new DOMException('The request was aborted', 'AbortError')
    );
  }
  let onAbort: (() => void) | undefined;
  const aborted = new Promise<never>((_, reject) => {
    onAbort = () => {
      detached();
      reject(
        signal.reason ??
          new DOMException('The request was aborted', 'AbortError'),
      );
    };
    signal.addEventListener('abort', onAbort, { once: true });
  });
  try {
    return await Promise.race([handler, aborted]);
  } finally {
    if (onAbort) signal.removeEventListener('abort', onAbort);
  }
}

function buildApiContext(
  request: Request,
  transport: PlecTransportContext,
  trustProxy: boolean,
): ApiRequestContext {
  const headers: Record<string, string> = {};
  request.headers.forEach((value, name) => {
    headers[name] = value;
  });
  const forwarded = trustProxy
    ? headerValues(transport, request, 'x-forwarded-proto')[0]
        ?.split(',')[0]
        ?.trim()
        .toLowerCase()
    : undefined;
  const scheme =
    forwarded === 'http' || forwarded === 'https'
      ? forwarded
      : transport.scheme;
  const url = `${scheme}://${transport.authority}${transport.pathname}${transport.rawQuery ? `?${transport.rawQuery}` : ''}`;
  const cookies: Record<string, string> = {};
  for (const part of (headers.cookie ?? '').split(';')) {
    const index = part.indexOf('=');
    if (index < 0) continue;
    cookies[part.slice(0, index).trim()] = decodeURIComponent(
      part.slice(index + 1).trim(),
    );
  }
  const query: Record<string, string | string[]> = {};
  for (const pair of transport.rawQuery.split('&')) {
    if (!pair) continue;
    const index = pair.indexOf('=');
    const key = decodeFormComponent(
      index < 0 ? pair : pair.slice(0, index),
    );
    const value = decodeFormComponent(
      index < 0 ? '' : pair.slice(index + 1),
    );
    const existing = query[key];
    if (existing === undefined) query[key] = value;
    else if (Array.isArray(existing)) existing.push(value);
    else query[key] = [existing, value];
  }
  return {
    url,
    pathname: transport.pathname,
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

function headerValues(
  transport: PlecTransportContext,
  request: Request,
  name: string,
): string[] {
  if (transport.rawHeaders) {
    return transport.rawHeaders
      .filter(([header]) => header.toLowerCase() === name)
      .map(([, value]) => value);
  }
  const value = request.headers.get(name);
  return value === null ? [] : [value];
}

function nativeHeaders(
  request: Request,
  transport: PlecTransportContext,
  trustProxy: boolean,
): { name: string; value: string }[] {
  const headers: { name: string; value: string }[] =
    transport.rawHeaders
      ? transport.rawHeaders
          .filter(
            ([name]) =>
              name.toLowerCase() !== 'host' &&
              name.toLowerCase() !== 'x-forwarded-proto' &&
              name.toLowerCase() !== 'cookie',
          )
          .map(([name, value]) => ({ name, value }))
      : (() => {
          const entries: { name: string; value: string }[] = [];
          request.headers.forEach((value, name) => {
            if (name !== 'host' && name !== 'x-forwarded-proto')
              entries.push({ name, value });
          });
          return entries;
        })();
  const cookies = headerValues(transport, request, 'cookie');
  if (cookies.length > 0)
    headers.push({ name: 'cookie', value: cookies.join('; ') });
  headers.push({ name: 'host', value: transport.authority });
  const forwarded = trustProxy
    ? headerValues(transport, request, 'x-forwarded-proto')[0]
        ?.split(',')[0]
        ?.trim()
        .toLowerCase()
    : undefined;
  headers.push({
    name: 'x-forwarded-proto',
    value:
      forwarded === 'http' || forwarded === 'https'
        ? forwarded
        : transport.scheme,
  });
  return headers;
}

function setNativeHeader(
  headers: { name: string; value: string }[],
  name: string,
  value: string,
): void {
  const lowerName = name.toLowerCase();
  for (let index = headers.length - 1; index >= 0; index -= 1) {
    if (headers[index]!.name.toLowerCase() === lowerName)
      headers.splice(index, 1);
  }
  headers.push({ name, value });
}

function transportUrl(transport: PlecTransportContext): string {
  return `${transport.scheme}://${transport.authority}${transport.pathname}${transport.rawQuery ? `?${transport.rawQuery}` : ''}`;
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
      try {
        await reader.cancel(reason);
      } finally {
        release();
      }
    },
  });
}

function apiError(status: number, message: string): Response {
  return new Response(JSON.stringify({ error: message }), {
    status,
    headers: {
      'content-type': 'application/json; charset=utf-8',
      'cache-control': 'no-store',
    },
  });
}

function inside(root: string, candidate: string): boolean {
  const relative = path.relative(root, candidate);
  return (
    relative === '' ||
    (!relative.startsWith(`..${path.sep}`) &&
      relative !== '..' &&
      !path.isAbsolute(relative))
  );
}
