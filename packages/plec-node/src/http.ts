import {
  createServer,
  type IncomingMessage,
  type ServerResponse,
} from 'node:http';
import { pipeline } from 'node:stream/promises';
import { Readable } from 'node:stream';
import {
  createPlecHandler,
  type PlecHandler,
  type ServeOptions,
} from './index.js';
import type { PlecTransportContext } from './transport.js';
import {
  canonicalRequestTarget,
  classifyPlecPath,
} from './request-target.js';

const MAX_ACTIVE_REQUESTS = 256;
const HEADER_TIMEOUT = 10_000;
const BODY_TIMEOUT = 30_000;
const SHUTDOWN_GRACE = 5_000;

export interface PlecHttpServer {
  server: ReturnType<typeof createServer>;
  stopAdmission(): void;
  abortActiveRequests(): void;
  waitForActiveRequests(): Promise<void>;
}

type ShutdownHandler = Pick<PlecHandler, 'close'>;

export function createPlecHttpServer(
  handler: PlecHandler,
  maxActiveRequests = MAX_ACTIVE_REQUESTS,
): PlecHttpServer {
  let activeRequests = 0;
  let accepting = true;
  let activeRequestsDrained = Promise.resolve();
  let resolveActiveRequestsDrained: (() => void) | undefined;
  const activeControllers = new Set<AbortController>();
  const server = createServer((request, response) => {
    void dispatch(
      request,
      response,
      handler as PlecHandler & {
        dispatch(
          request: Request,
          transport: PlecTransportContext,
        ): Promise<Response>;
      },
      () => {
        if (!accepting) return false;
        if (activeRequests >= maxActiveRequests) return false;
        if (activeRequests === 0) {
          activeRequestsDrained = new Promise<void>((resolve) => {
            resolveActiveRequestsDrained = resolve;
          });
        }
        activeRequests += 1;
        return true;
      },
      () => {
        activeRequests -= 1;
        if (activeRequests === 0) {
          resolveActiveRequestsDrained?.();
          resolveActiveRequestsDrained = undefined;
        }
      },
      (controller) => {
        activeControllers.add(controller);
        return () => activeControllers.delete(controller);
      },
    ).catch((error: unknown) => {
      console.error('[PLEC] response dispatch failed', error);
      if (response.headersSent) {
        response.destroy(error instanceof Error ? error : undefined);
      } else if (!response.destroyed) {
        writeError(response, 500, 'Internal Server Error');
      }
    });
  });
  server.headersTimeout = HEADER_TIMEOUT;
  server.requestTimeout = BODY_TIMEOUT;
  return {
    server,
    stopAdmission: () => {
      accepting = false;
    },
    abortActiveRequests: () => {
      for (const controller of activeControllers)
        controller.abort(
          new DOMException(
            'Server shutdown grace expired',
            'AbortError',
          ),
        );
    },
    waitForActiveRequests: () => activeRequestsDrained,
  };
}

/** @internal Coordinates HTTP drain and application close for serve() and tests. */
export function createShutdownCoordinator(
  hostServer: PlecHttpServer,
  handler: ShutdownHandler,
  graceMs = SHUTDOWN_GRACE,
): () => Promise<void> {
  let shutdownPromise: Promise<void> | undefined;
  return () => {
    if (shutdownPromise) return shutdownPromise;
    hostServer.stopAdmission();
    shutdownPromise = (async () => {
      let graceTimer: ReturnType<typeof setTimeout> | undefined;
      const listenerClosed = new Promise<void>((resolve, reject) => {
        hostServer.server.close((error) => {
          if (error) reject(error);
          else resolve();
        });
      });
      const drained = Promise.all([
        listenerClosed,
        hostServer.waitForActiveRequests(),
      ]).then(() => undefined);
      const grace = new Promise<boolean>((resolve) => {
        graceTimer = setTimeout(() => resolve(false), graceMs);
      });
      try {
        const drainedWithinGrace = await Promise.race([
          drained.then(() => true),
          grace,
        ]);
        if (!drainedWithinGrace) {
          hostServer.server.closeAllConnections();
          hostServer.abortActiveRequests();
          // Forced socket closure propagates request cancellation, but arbitrary
          // JS streams may not settle their pull/cancel promises. Wait for the
          // transport to close, not detached application work.
          await listenerClosed;
        }
        await handler.close();
      } finally {
        if (graceTimer) clearTimeout(graceTimer);
      }
    })();
    return shutdownPromise;
  };
}

export async function servePlecHandler(
  options: ServeOptions,
): Promise<void> {
  const port = resolvePort(options.port, process.env.PORT);
  const handler = await createPlecHandler(options);
  const hostServer = createPlecHttpServer(handler);
  const { server } = hostServer;
  const host = options.host ?? '127.0.0.1';
  try {
    await new Promise<void>((resolve, reject) => {
      const onError = (error: Error): void => reject(error);
      server.once('error', onError);
      server.listen(port, host, () => {
        server.off('error', onError);
        resolve();
      });
    });
  } catch (error) {
    await handler.close();
    throw error;
  }
  const address = server.address();
  const boundPort =
    typeof address === 'object' && address ? address.port : port;
  console.info(`[PLEC] listening on http://${host}:${boundPort}`);

  const shutdown = createShutdownCoordinator(hostServer, handler);
  await new Promise<void>((resolve, reject) => {
    let settled = false;
    const cleanup = (): void => {
      process.off('SIGINT', onSignal);
      process.off('SIGTERM', onSignal);
    };
    const finish = (error?: unknown): void => {
      if (settled) return;
      settled = true;
      cleanup();
      if (error) reject(error);
      else resolve();
    };
    const onSignal = (): void => {
      void shutdown().then(
        () => finish(),
        (error: unknown) => finish(error),
      );
    };
    process.on('SIGINT', onSignal);
    process.on('SIGTERM', onSignal);
  });
}

async function dispatch(
  incoming: IncomingMessage,
  outgoing: ServerResponse,
  handler: Awaited<ReturnType<typeof createPlecHandler>>,
  acquire: () => boolean,
  release: () => void,
  registerController: (controller: AbortController) => () => void,
): Promise<void> {
  const target = incoming.url;
  let parsed: ReturnType<typeof canonicalRequestTarget>;
  try {
    if (!target) throw new TypeError('missing request target');
    parsed = canonicalRequestTarget(target);
  } catch {
    incoming.pause();
    writeError(outgoing, 400, 'Bad Request', true);
    return;
  }
  const hosts = rawValues(incoming, 'host');
  if (hosts.length !== 1 || !validHost(hosts[0]!)) {
    incoming.pause();
    writeError(outgoing, 400, 'Bad Request', true);
    return;
  }
  const route = classifyPlecPath(parsed.path);
  if (route === 'action' && incoming.method !== 'POST') {
    incoming.pause();
    writeError(outgoing, 405, 'method not allowed', true, {
      allow: 'POST',
    });
    return;
  }
  if (
    (route === 'document' || route === 'static') &&
    !['GET', 'HEAD'].includes(incoming.method ?? '')
  ) {
    incoming.pause();
    writeError(outgoing, 405, 'method not allowed', true, {
      allow: 'GET, HEAD',
    });
    return;
  }
  if (
    route !== 'action' &&
    ['GET', 'HEAD'].includes(incoming.method ?? '') &&
    hasUnreadRequestBody(incoming)
  ) {
    incoming.pause();
    outgoing.shouldKeepAlive = false;
    outgoing.setHeader('connection', 'close');
  }
  if (!acquire()) {
    incoming.pause();
    writeError(outgoing, 503, 'service unavailable', true);
    return;
  }
  let admitted = true;
  const releaseAdmission = (): void => {
    if (!admitted) return;
    admitted = false;
    release();
  };

  const controller = new AbortController();
  const unregisterController = registerController(controller);
  const abort = (): void =>
    controller.abort(
      new DOMException('Client disconnected', 'AbortError'),
    );
  incoming.once('aborted', abort);
  outgoing.once('close', () => {
    if (!outgoing.writableFinished) abort();
  });
  try {
    let body: ReadableStream<Uint8Array> | null = null;
    if (route === 'action') {
      const origins = rawValues(incoming, 'origin');
      if (origins.length !== 1 || !validOrigin(origins[0]!)) {
        incoming.pause();
        writeError(
          outgoing,
          403,
          'same-origin action POST required',
          true,
        );
        return;
      }
      body = Readable.toWeb(incoming) as ReadableStream<Uint8Array>;
    } else if (
      route === 'api' &&
      !['GET', 'HEAD'].includes(incoming.method ?? '')
    ) {
      body = Readable.toWeb(incoming) as ReadableStream<Uint8Array>;
    }
    const headers = new Headers();
    for (
      let index = 0;
      index < incoming.rawHeaders.length;
      index += 2
    ) {
      const name = incoming.rawHeaders[index]!;
      const value = incoming.rawHeaders[index + 1]!;
      if (['set-cookie', 'cookie'].includes(name.toLowerCase()))
        continue;
      headers.append(name, value);
    }
    const cookies = rawValues(incoming, 'cookie');
    if (cookies.length > 0) headers.set('cookie', cookies.join('; '));
    const scheme = (
      incoming.socket as typeof incoming.socket & {
        encrypted?: boolean;
      }
    ).encrypted
      ? 'https'
      : 'http';
    const url = `${scheme}://${hosts[0]}${parsed.path}${parsed.query ? `?${parsed.query}` : ''}`;
    const request = new Request(url, {
      method: incoming.method,
      headers,
      body,
      signal: controller.signal,
      ...(body ? ({ duplex: 'half' } as RequestInit) : {}),
    });
    const rawHeaders: [string, string][] = [];
    for (
      let index = 0;
      index < incoming.rawHeaders.length;
      index += 2
    ) {
      rawHeaders.push([
        incoming.rawHeaders[index]!,
        incoming.rawHeaders[index + 1]!,
      ]);
    }
    const transport: PlecTransportContext = {
      pathname: parsed.path,
      rawQuery: parsed.query,
      rawHeaders,
      scheme,
      authority: hosts[0]!,
      signal: controller.signal,
    };
    const response = await (
      handler as typeof handler & {
        dispatch(
          request: Request,
          transport: PlecTransportContext,
        ): Promise<Response>;
      }
    ).dispatch(request, transport);
    await sendResponse(
      outgoing,
      response,
      incoming.method === 'HEAD',
      controller.signal,
    );
  } catch (error) {
    if (controller.signal.aborted) return;
    throw error;
  } finally {
    incoming.off('aborted', abort);
    unregisterController();
    releaseAdmission();
  }
}

async function sendResponse(
  response: ServerResponse,
  result: Response,
  head: boolean,
  signal: AbortSignal,
): Promise<void> {
  response.statusCode = result.status;
  if ([408, 413, 503].includes(result.status))
    response.setHeader('connection', 'close');
  result.headers.forEach((value, name) => {
    if (
      ![
        'connection',
        'keep-alive',
        'proxy-authenticate',
        'proxy-authorization',
        'te',
        'trailer',
        'transfer-encoding',
        'upgrade',
      ].includes(name.toLowerCase())
    )
      response.setHeader(name, value);
  });
  const cookies = result.headers.getSetCookie();
  if (cookies.length > 0) response.setHeader('set-cookie', cookies);
  if (
    head ||
    (result.status >= 100 && result.status < 200) ||
    [204, 304].includes(result.status)
  ) {
    await result.body?.cancel();
    response.end();
    return;
  }
  if (!result.body) {
    response.end();
    return;
  }
  response.removeHeader('content-length');
  const reader = result.body.getReader();
  let disconnected = false;
  const onAbort = (): void => {
    disconnected = true;
    void reader.cancel(signal.reason).catch(() => undefined);
  };
  const onClose = (): void => {
    if (response.writableFinished) return;
    disconnected = true;
    void reader.cancel('client disconnected').catch(() => undefined);
  };
  const cleanup = (): void => {
    response.off('close', onClose);
    signal.removeEventListener('abort', onAbort);
  };
  response.once('close', onClose);
  if (signal.aborted) onAbort();
  else signal.addEventListener('abort', onAbort, { once: true });
  let first: ReadableStreamReadResult<Uint8Array>;
  try {
    first = await reader.read();
  } catch (error) {
    await reader.cancel(error).catch(() => undefined);
    cleanup();
    if (disconnected) return;
    for (const name of response.getHeaderNames())
      response.removeHeader(name);
    console.error(
      '[PLEC] response stream failed before headers',
      error,
    );
    writeError(response, 500, 'Internal Server Error');
    return;
  }
  if (disconnected) {
    cleanup();
    return;
  }
  if (first.done) {
    cleanup();
    response.end();
    return;
  }
  const source = Readable.from(
    (async function* () {
      try {
        yield Buffer.from(first.value);
        for (;;) {
          const next = await reader.read();
          if (next.done) return;
          yield Buffer.from(next.value);
        }
      } finally {
        await reader
          .cancel('response stream closed')
          .catch(() => undefined);
      }
    })(),
  );
  try {
    await pipeline(source, response);
  } finally {
    cleanup();
  }
}

function writeError(
  response: ServerResponse,
  status: number,
  message: string,
  close = false,
  headers: Record<string, string> = {},
): void {
  const body = JSON.stringify({ error: message });
  response.writeHead(status, {
    'content-type': 'application/json; charset=utf-8',
    'cache-control': 'no-store',
    'content-length': Buffer.byteLength(body),
    ...(close ? { connection: 'close' } : {}),
    ...headers,
  });
  response.end(body);
}

function rawValues(request: IncomingMessage, name: string): string[] {
  const values: string[] = [];
  for (let index = 0; index < request.rawHeaders.length; index += 2)
    if (request.rawHeaders[index]!.toLowerCase() === name)
      values.push(request.rawHeaders[index + 1]!);
  return values;
}

function hasUnreadRequestBody(request: IncomingMessage): boolean {
  if (request.headers['transfer-encoding'] !== undefined) return true;
  const contentLength = request.headers['content-length'];
  return contentLength !== undefined && Number(contentLength) > 0;
}

function validHost(value: string): boolean {
  try {
    return (
      !!new URL(`http://${value}`).hostname && !/[\s/@]/u.test(value)
    );
  } catch {
    return false;
  }
}

function validOrigin(value: string): boolean {
  try {
    const url = new URL(value);
    return (
      ['http:', 'https:'].includes(url.protocol) &&
      url.origin !== 'null' &&
      !url.username &&
      !url.password &&
      url.pathname === '/' &&
      !url.search &&
      !url.hash
    );
  } catch {
    return false;
  }
}

function resolvePort(
  explicit: number | undefined,
  environment: string | undefined,
): number {
  const raw =
    explicit ??
    (environment === undefined ? 3000 : Number(environment));
  if (!Number.isInteger(raw) || raw < 1 || raw > 65535)
    throw new RangeError('port must be an integer from 1 to 65535');
  return raw;
}
