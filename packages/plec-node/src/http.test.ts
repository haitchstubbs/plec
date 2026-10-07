import { once } from 'node:events';
import net from 'node:net';
import { afterEach, describe, expect, it } from 'vitest';
import { createPlecHttpServer } from './http.js';
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
});

function fakeHandler(
  dispatch: (
    transport: PlecTransportContext,
    request: Request,
  ) => Response | Promise<Response>,
): PlecHandler {
  return {
    async fetch() {
      return new Response('fetch should not be used by node:http');
    },
    async close() {},
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

function openRequest(port: number): net.Socket {
  const socket = net.connect(port, '127.0.0.1');
  socket.on('data', () => undefined);
  socket.setTimeout(5_000, () =>
    socket.destroy(new Error('socket timed out')),
  );
  return socket;
}
