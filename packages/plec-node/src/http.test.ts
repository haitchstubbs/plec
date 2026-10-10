import { once } from 'node:events';
import net from 'node:net';
import { afterEach, describe, expect, it } from 'vitest';
import {
  createPlecHttpServer,
  createShutdownCoordinator,
} from './http.js';
import type { PlecHandler } from './index.js';
import type { PlecTransportContext } from './transport.js';

const servers: ReturnType<typeof createPlecHttpServer>[] = [];
const activeSockets: net.Socket[] = [];

afterEach(async () => {
  for (const socket of activeSockets.splice(0)) socket.destroy();
  await Promise.all(
    servers.splice(0).map(async ({ server }) => {
      server.closeAllConnections();
      if (server.listening)
        await new Promise<void>((resolve) =>
          server.close(() => resolve()),
        );
    }),
  );
});

describe('Node HTTP transport boundary', () => {
  it('passes the raw canonical path and ordered headers to internal dispatch', async () => {
    const received: PlecTransportContext[] = [];
    const host = createPlecHttpServer(
      fakeHandler((transport) => {
        received.push(transport);
        return new Response('ok');
      }),
    );
    servers.push(host);
    const port = await listen(host.server);
    const response = await rawRequest(
      port,
      'GET /a/../b%2Fz?x=1 HTTP/1.1\r\nHost: localhost\r\nX-Test: one\r\nX-Test: two\r\nConnection: close\r\n\r\n',
    );

    expect(response).toContain('200 OK');
    expect(received).toHaveLength(1);
    expect(received[0]?.pathname).toBe('/a/../b%2Fz');
    expect(received[0]?.rawQuery).toBe('x=1');
    expect(received[0]?.rawHeaders).toContainEqual(['X-Test', 'one']);
    expect(received[0]?.rawHeaders).toContainEqual(['X-Test', 'two']);
  });

  it('rejects repeated action Origin before dispatch', async () => {
    let dispatched = false;
    const host = createPlecHttpServer(
      fakeHandler(() => {
        dispatched = true;
        return new Response('unexpected');
      }),
    );
    servers.push(host);
    const port = await listen(host.server);
    const response = await rawRequest(
      port,
      'POST /_plec/actions/do HTTP/1.1\r\nHost: localhost\r\nOrigin: http://localhost\r\nOrigin: http://localhost\r\nContent-Length: 0\r\nConnection: close\r\n\r\n',
    );

    expect(response).toContain('403');
    expect(dispatched).toBe(false);
  });

  it('rejects duplicate Host values before dispatch', async () => {
    let dispatched = false;
    const host = createPlecHttpServer(
      fakeHandler(() => {
        dispatched = true;
        return new Response('unexpected');
      }),
    );
    servers.push(host);
    const port = await listen(host.server);
    const response = await rawRequest(
      port,
      'GET / HTTP/1.1\r\nHost: localhost\r\nHost: example.com\r\nConnection: close\r\n\r\n',
    );

    expect(response).toContain('400');
    expect(dispatched).toBe(false);
  });

  it('leaves conflicting Content-Length and Transfer-Encoding rejection to Node', async () => {
    let dispatched = false;
    const host = createPlecHttpServer(
      fakeHandler(() => {
        dispatched = true;
        return new Response('unexpected');
      }),
    );
    servers.push(host);
    const response = await malformedRequest(
      await listen(host.server),
      'POST /api/framing HTTP/1.1\r\nHost: localhost\r\nContent-Length: 4\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n0\r\n\r\n',
    );

    expect(dispatched).toBe(false);
    expect(response).not.toContain('200 OK');
  });

  it('ignores API GET bodies and closes the connection when bytes remain unread', async () => {
    let exposedBody: ReadableStream<Uint8Array> | null | undefined;
    const host = createPlecHttpServer(
      fakeHandler((_transport, request) => {
        exposedBody = request.body;
        return new Response('ok');
      }),
    );
    servers.push(host);
    const response = await rawRequest(
      await listen(host.server),
      'GET /api/ignore HTTP/1.1\r\nHost: localhost\r\nContent-Length: 4\r\n\r\ndata',
    );

    expect(response).toContain('200 OK');
    expect(response.toLowerCase()).toContain('connection: close');
    expect(exposedBody).toBeNull();
  });

  it('returns a redacted 500 when a response stream fails before its first chunk', async () => {
    const host = createPlecHttpServer(
      fakeHandler(
        () =>
          new Response(
            new ReadableStream<Uint8Array>({
              start(controller) {
                controller.error(new Error('stream secret'));
              },
            }),
          ),
      ),
    );
    servers.push(host);
    const response = await rawRequest(
      await listen(host.server),
      'GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n',
    );

    expect(response).toContain('500 Internal Server Error');
    expect(response).toContain('{"error":"Internal Server Error"}');
    expect(response).not.toContain('stream secret');
  });

  it('cancels HEAD response bodies without emitting bytes', async () => {
    let cancelled = false;
    const host = createPlecHttpServer(
      fakeHandler(
        () =>
          new Response(
            new ReadableStream<Uint8Array>({
              cancel() {
                cancelled = true;
              },
            }),
          ),
      ),
    );
    servers.push(host);
    const response = await rawRequest(
      await listen(host.server),
      'HEAD / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n',
    );

    expect(response).toContain('200 OK');
    expect(response).not.toContain('body');
    expect(cancelled).toBe(true);
  });

  it('destroys the response after a body stream fails once bytes were sent', async () => {
    const host = createPlecHttpServer(
      fakeHandler(() => {
        let sent = false;
        return new Response(
          new ReadableStream<Uint8Array>({
            pull(controller) {
              if (sent)
                controller.error(new Error('later stream secret'));
              else {
                sent = true;
                controller.enqueue(new TextEncoder().encode('first'));
              }
            },
          }),
        );
      }),
    );
    servers.push(host);
    const response = await rawRequest(
      await listen(host.server),
      'GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n',
    );

    expect(response).toContain('200 OK');
    expect(response).toContain('first');
    expect(response.match(/HTTP\/1\.1/g)).toHaveLength(1);
    expect(response).not.toContain('later stream secret');
  });

  it('cancels a pending response source when the client disconnects before headers', async () => {
    let started!: () => void;
    const responseStarted = new Promise<void>((resolve) => {
      started = resolve;
    });
    let cancelled = false;
    const host = createPlecHttpServer(
      fakeHandler(() => {
        started();
        return new Response(
          new ReadableStream<Uint8Array>({
            pull() {
              return new Promise<void>(() => undefined);
            },
            cancel() {
              cancelled = true;
            },
          }),
        );
      }),
    );
    servers.push(host);
    const socket = openRequest(await listen(host.server));
    activeSockets.push(socket);
    await once(socket, 'connect');
    socket.write(
      'GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n',
    );
    await responseStarted;
    await new Promise<void>((resolve) => setTimeout(resolve, 10));
    socket.destroy();
    for (let attempt = 0; attempt < 100 && !cancelled; attempt++)
      await new Promise<void>((resolve) => setTimeout(resolve, 5));

    expect(cancelled).toBe(true);
  });

  it('rejects requests before dispatch when active-request admission is full', async () => {
    let dispatched = 0;
    let releaseResponses!: () => void;
    const responseGate = new Promise<void>((resolve) => {
      releaseResponses = resolve;
    });
    const host = createPlecHttpServer(
      fakeHandler(() => {
        dispatched += 1;
        return responseGate.then(() => new Response('done'));
      }),
      1,
    );
    servers.push(host);
    const port = await listen(host.server);
    const requests = [openRequest(port)];
    await Promise.all(
      requests.map((socket) => once(socket, 'connect')),
    );
    for (const socket of requests)
      socket.write(
        'GET /api/hold HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n',
      );
    activeSockets.push(...requests);
    for (let attempt = 0; attempt < 100 && dispatched < 1; attempt++)
      await new Promise<void>((resolve) => setTimeout(resolve, 5));
    expect(dispatched).toBe(1);

    const overload = await rawRequest(
      port,
      'GET /api/overload HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n',
    );
    expect(overload).toContain('503');
    expect(dispatched).toBe(1);

    const closed = requests.map((socket) => once(socket, 'close'));
    releaseResponses();
    await Promise.all(closed);
  });

  it('stops Plec admission before closing the listener', async () => {
    let dispatched = 0;
    const host = createPlecHttpServer(
      fakeHandler(() => {
        dispatched += 1;
        return new Response('unexpected');
      }),
    );
    servers.push(host);
    const port = await listen(host.server);

    // Exercise the Plec admission gate while the listener is still open.
    host.stopAdmission();
    const response = await rawRequest(
      port,
      'GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n',
    );

    expect(response).toContain('503');
    expect(dispatched).toBe(0);
  });

  it('drains admitted HTTP work within grace and closes the handler once', async () => {
    let startDispatch!: () => void;
    let releaseResponse!: () => void;
    const dispatchStarted = new Promise<void>((resolve) => {
      startDispatch = resolve;
    });
    const responseGate = new Promise<void>((resolve) => {
      releaseResponse = resolve;
    });
    let dispatchCompleted = false;
    let closeCount = 0;
    const handler = fakeHandler(
      async () => {
        startDispatch();
        await responseGate;
        dispatchCompleted = true;
        return new Response('drained');
      },
      async () => {
        closeCount += 1;
        expect(dispatchCompleted).toBe(true);
      },
    );
    const host = createPlecHttpServer(handler);
    servers.push(host);
    const port = await listen(host.server);
    let stopAdmissionCalled = false;
    const stopAdmission = host.stopAdmission;
    host.stopAdmission = () => {
      stopAdmissionCalled = true;
      stopAdmission();
    };
    const responsePromise = rawRequest(
      port,
      'GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n',
    );
    await dispatchStarted;

    const shutdown = createShutdownCoordinator(host, handler, 1_000);
    const firstShutdown = shutdown();
    const repeatedShutdown = shutdown();
    expect(firstShutdown).toBe(repeatedShutdown);
    expect(stopAdmissionCalled).toBe(true);
    expect(closeCount).toBe(0);

    releaseResponse();
    const response = await responsePromise;
    await Promise.all([firstShutdown, repeatedShutdown]);

    expect(response).toContain('drained');
    expect(closeCount).toBe(1);
  });

  it('force-closes over-grace work, propagates cancellation, and closes once', async () => {
    let cancellationObserved = false;
    let bodyCancelled = false;
    let forceClosed = false;
    let closeCount = 0;
    let dispatchCount = 0;
    const handler = fakeHandler(
      (transport) => {
        dispatchCount += 1;
        let pulls = 0;
        let releasePull!: () => void;
        transport.signal.addEventListener(
          'abort',
          () => {
            cancellationObserved = true;
            releasePull?.();
          },
          { once: true },
        );
        return new Response(
          new ReadableStream<Uint8Array>({
            pull(controller) {
              if (pulls++ === 0) {
                controller.enqueue(new TextEncoder().encode('first'));
                return;
              }
              return new Promise<void>((resolve) => {
                releasePull = resolve;
              });
            },
            cancel() {
              bodyCancelled = true;
            },
          }),
        );
      },
      async () => {
        closeCount += 1;
      },
    );
    const host = createPlecHttpServer(handler);
    servers.push(host);
    const closeAllConnections = host.server.closeAllConnections.bind(
      host.server,
    );
    host.server.closeAllConnections = () => {
      forceClosed = true;
      closeAllConnections();
    };
    const port = await listen(host.server);
    const response = await fetch(`http://127.0.0.1:${port}/`);
    expect(response.status).toBe(200);
    const reader = response.body!.getReader();
    expect(new TextDecoder().decode((await reader.read()).value)).toBe(
      'first',
    );

    const shutdown = createShutdownCoordinator(host, handler, 25);
    const firstShutdown = shutdown();
    const repeatedShutdown = shutdown();
    expect(firstShutdown).toBe(repeatedShutdown);
    await Promise.all([firstShutdown, repeatedShutdown]);

    expect(cancellationObserved).toBe(true);
    expect(forceClosed).toBe(true);
    expect(bodyCancelled).toBe(true);
    expect(dispatchCount).toBe(1);
    expect(closeCount).toBe(1);
    await expect(reader.read()).rejects.toThrow();
  });

  it('does not wait indefinitely for an uncooperative response stream after force close', async () => {
    let cancellationObserved = false;
    let closeCount = 0;
    let forceClosed = false;
    const handler = fakeHandler(
      (transport) => {
        transport.signal.addEventListener(
          'abort',
          () => {
            cancellationObserved = true;
          },
          { once: true },
        );
        let pulls = 0;
        return new Response(
          new ReadableStream<Uint8Array>({
            pull(controller) {
              if (pulls++ === 0)
                controller.enqueue(new TextEncoder().encode('first'));
              else return new Promise<void>(() => undefined);
            },
          }),
        );
      },
      async () => {
        closeCount += 1;
      },
    );
    const host = createPlecHttpServer(handler);
    servers.push(host);
    const closeAllConnections = host.server.closeAllConnections.bind(
      host.server,
    );
    host.server.closeAllConnections = () => {
      forceClosed = true;
      closeAllConnections();
    };
    const port = await listen(host.server);
    const response = await fetch(`http://127.0.0.1:${port}/`);
    const reader = response.body!.getReader();
    expect(new TextDecoder().decode((await reader.read()).value)).toBe(
      'first',
    );

    const shutdown = createShutdownCoordinator(host, handler, 20);
    let timeout: ReturnType<typeof setTimeout> | undefined;
    const completed = await Promise.race([
      shutdown().then(() => true),
      new Promise<boolean>((resolve) => {
        timeout = setTimeout(() => resolve(false), 500);
      }),
    ]);
    if (timeout) clearTimeout(timeout);

    expect(completed).toBe(true);
    expect(forceClosed).toBe(true);
    expect(cancellationObserved).toBe(true);
    expect(closeCount).toBe(1);
  });
});

function fakeHandler(
  dispatch: (
    transport: PlecTransportContext,
    request: Request,
  ) => Response | Promise<Response>,
  close: () => Promise<void> = async () => {},
): PlecHandler {
  return {
    async fetch() {
      return new Response('fetch should not be used by node:http');
    },
    close,
    async dispatch(request: Request, transport: PlecTransportContext) {
      return dispatch(transport, request);
    },
  } as PlecHandler;
}

async function listen(
  server: ReturnType<typeof createPlecHttpServer>['server'],
): Promise<number> {
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  const address = server.address();
  if (!address || typeof address === 'string')
    throw new Error('missing TCP address');
  return address.port;
}

async function rawRequest(
  port: number,
  request: string,
): Promise<string> {
  return new Promise((resolve, reject) => {
    const socket = net.connect(port, '127.0.0.1');
    const chunks: Buffer[] = [];
    socket.setTimeout(5_000, () =>
      socket.destroy(new Error('socket timed out')),
    );
    socket.on('connect', () => socket.write(request));
    socket.on('data', (chunk: Buffer) => chunks.push(chunk));
    socket.on('end', () =>
      resolve(Buffer.concat(chunks).toString('utf8')),
    );
    socket.on('close', () =>
      resolve(Buffer.concat(chunks).toString('utf8')),
    );
    socket.on('error', reject);
  });
}

async function malformedRequest(
  port: number,
  request: string,
): Promise<string> {
  return new Promise((resolve, reject) => {
    const socket = net.connect(port, '127.0.0.1');
    const chunks: Buffer[] = [];
    const finish = (): void =>
      resolve(Buffer.concat(chunks).toString('utf8'));
    socket.setTimeout(5_000, () => socket.destroy());
    socket.on('connect', () => socket.write(request));
    socket.on('data', (chunk: Buffer) => chunks.push(chunk));
    socket.on('close', finish);
    socket.on('error', (error) => {
      if (chunks.length > 0) finish();
      else reject(error);
    });
  });
}

function openRequest(port: number): net.Socket {
  const socket = net.connect(port, '127.0.0.1');
  socket.on('data', () => undefined);
  socket.setTimeout(5_000, () =>
    socket.destroy(new Error('socket timed out')),
  );
  return socket;
}
