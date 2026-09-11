import { timingSafeEqual } from 'node:crypto';
import { unlinkSync } from 'node:fs';
import {
  createServer,
  type IncomingMessage,
  type Server,
  type ServerResponse,
} from 'node:http';
import { pathToFileURL } from 'node:url';
export type QueryValue = string | string[];

export class BundleError extends Error {}

export interface RequestContext {
  url: string;
  pathname: string;
  method: string;
  headers: Record<string, string>;
  cookies: Record<string, string>;
  params: Record<string, string>;
  query: Record<string, QueryValue>;
}

export type HandleRequest = (
  request: Request,
  context: RequestContext,
) => Response | null | undefined | Promise<Response | null | undefined>;

export interface ApplicationModule {
  handleRequest: HandleRequest;
}

export interface StartOptions {
  /**
   * Absolute Unix socket path, or `tcp:127.0.0.1:<port>`.
   */
  socket: string;

  /**
   * Absolute path to the application's server bundle.
   */
  bundle: string;

  /**
   * Internal authentication token shared with the Rust supervisor.
   */
  token: string;
}

interface RuntimeReady {
  protocol: typeof PROTOCOL_VERSION;
  address: string;
}

const PROTOCOL_VERSION: number = 1;
const INTERNAL_TOKEN_HEADER: string = 'x-plec-internal-token';
const UNHANDLED_HEADER: string = 'x-plec-runtime-result';
const UNHANDLED_VALUE: string = 'unhandled';
const HEALTH_PATH: string = '/_plec-runtime/health';
const LOCALHOST_PRIVATE: string = '127.0.0.1';
const TCP_PREFIX: string = 'tcp:';

enum SIG {
  TERM = 'SIGTERM',
  INT = 'SIGINT',
}

enum ServerState {
  Listening = 'listening',
  Error = 'error',
  Closed = 'closed',
}

enum BufferState {
  Data = 'data',
  End = 'end',
  Error = 'error',
}

enum HttpMethod {
  GET = 'GET',
  POST = 'POST',
  PUT = 'PUT',
  DELETE = 'DELETE',
  PATCH = 'PATCH',
  OPTIONS = 'OPTIONS',
  HEAD = 'HEAD',
}

enum Errors {
  MissingDeps = 'plec-node-runtime requires PLEC_RUNTIME_SOCKET, PLEC_RUNTIME_BUNDLE, and PLEC_RUNTIME_TOKEN',
  ImportFailure = 'server bundle failed to import',
  ExportFailure = 'server entry does not export handleRequest(request, context)',
  CookieEscapePercent = 'malformed percent escape in cookie data',
  RuntimeFailure = 'plec-node-runteime failed',
  InvalidTcpPort = 'invalid TCP port',
  InvalidTcpSocket = 'invalid TCP socket',
  InvalidUnixSocket = 'invalid Unix socket',
  MissingTcpAddress = 'runtime TCP server did not expose a TCP address',
}

/**
 * Starts the sidecar. `socket` is an absolute Unix socket path, or
 * `tcp:127.0.0.1:<port>` on platforms without Unix domain sockets.
 */
export async function start({
  socket,
  bundle,
  token,
}: StartOptions): Promise<Server> {
  const application = await importApplication(bundle);

  const server = createServer(
    (incoming: IncomingMessage, outgoing: ServerResponse) => {
      void serve(incoming, outgoing, application, token);
    },
  );

  const stop = (): void => {
    server.close(() => {
      if (!isTcpSocket(socket)) {
        unlinkSyncIfPresent(socket);
      }

      process.exit(0);
    });

    // Idle keep-alive connections must not hold the supervisor's shutdown
    // open; in-flight requests still finish or die with the process.
    server.closeAllConnections?.();

    setTimeout(() => process.exit(0), 5_000).unref();
  };

  process.once(SIG.TERM, stop);
  process.once(SIG.INT, stop);

  await listen(server, socket);

  const ready: RuntimeReady = {
    protocol: PROTOCOL_VERSION,
    address: runtimeAddress(server, socket),
  };

  process.stdout.write(
    `➠︎          Plec Ready: ${JSON.stringify(ready)}\n`,
  );

  return server;
}

async function serve(
  incoming: IncomingMessage,
  outgoing: ServerResponse,
  application: ApplicationModule,
  token: string,
): Promise<void> {
  try {
    if (!tokenMatches(incoming.headers[INTERNAL_TOKEN_HEADER], token)) {
      outgoing.writeHead(403);
      outgoing.end();
      return;
    }

    if (
      incoming.method === HttpMethod.GET &&
      incoming.url === HEALTH_PATH
    ) {
      outgoing.writeHead(204);
      outgoing.end();
      return;
    }

    const request = await toWebRequest(incoming);
    const context = buildContext(incoming);
    const response = await application.handleRequest(request, context);

    if (response == null) {
      // The only producer of the unhandled sentinel is this runtime.
      outgoing.writeHead(404, {
        [UNHANDLED_HEADER]: UNHANDLED_VALUE,
      });
      outgoing.end();
      return;
    }

    sendWebResponse(outgoing, response);
  } catch (error: unknown) {
    if (!outgoing.headersSent) {
      outgoing.writeHead(500, {
        'content-type': 'application/json; charset=utf-8',
      });
    }

    outgoing.end(
      JSON.stringify({
        error: formatError(error),
      }),
    );
  }
}

async function importApplication(
  bundle: string,
): Promise<ApplicationModule> {
  let imported: unknown;

  try {
    imported = await import(pathToFileURL(bundle).href);
  } catch (error: unknown) {
    throw new BundleError(
      `${Errors.ImportFailure}: ${formatError(error, true)}`,
    );
  }

  if (!isApplicationModule(imported)) {
    throw new BundleError(Errors.ExportFailure);
  }

  return imported;
}

function isApplicationModule(
  value: unknown,
): value is ApplicationModule {
  if (typeof value !== 'object' || value === null) {
    return false;
  }

  if (!('handleRequest' in value)) {
    return false;
  }

  return typeof value.handleRequest === 'function';
}

function tokenMatches(
  received: string | string[] | undefined,
  expected: string,
): boolean {
  if (typeof received !== 'string') {
    return false;
  }

  const left = Buffer.from(received);
  const right = Buffer.from(expected);

  return left.length === right.length && timingSafeEqual(left, right);
}

export async function toWebRequest(
  incoming: IncomingMessage,
): Promise<Request> {
  const headers = new Headers();

  for (let index = 0; index < incoming.rawHeaders.length; index += 2) {
    const name = incoming.rawHeaders[index];
    const value = incoming.rawHeaders[index + 1];

    // rawHeaders is specified by Node as alternating name/value pairs,
    // but keeping the bounds check makes this safe under
    // `noUncheckedIndexedAccess`.
    if (name !== undefined && value !== undefined) {
      headers.append(name, value);
    }
  }

  const url =
    `http://${incoming.headers.host ?? 'plec.internal'}` +
    `${incoming.url ?? '/'}`;

  const method = incoming.method ?? HttpMethod.GET;

  if (method === HttpMethod.GET || method === HttpMethod.HEAD) {
    return new Request(url, {
      method,
      headers,
    });
  }

  const body = Buffer.concat(await readBody(incoming));

  return new Request(url, {
    method,
    headers,

    // Make an ArrayBuffer-backed Uint8Array rather than exposing Buffer's
    // ArrayBufferLike generic to the DOM BodyInit type.
    body: Uint8Array.from(body),
  });
}

function readBody(incoming: IncomingMessage): Promise<Buffer[]> {
  return new Promise<Buffer[]>((resolve, reject) => {
    const chunks: Buffer[] = [];

    incoming.on(BufferState.Data, (chunk: Buffer | string) => {
      chunks.push(
        typeof chunk === 'string' ? Buffer.from(chunk) : chunk,
      );
    });

    incoming.on(BufferState.End, () => {
      resolve(chunks);
    });

    incoming.on(BufferState.Error, reject);
  });
}

/**
 * The same request context the TS host handed to application handlers:
 * request facts only — params stay empty because server-bundle routing is
 * the application's own concern.
 */
export function buildContext(
  incoming: IncomingMessage,
): RequestContext {
  const headers: Record<string, string> = {};

  for (const [name, value] of Object.entries(incoming.headers)) {
    if (value === undefined) {
      continue;
    }

    headers[name] = Array.isArray(value) ? value.join(',') : value;
  }

  const url =
    `http://${incoming.headers.host ?? 'plec.internal'}` +
    `${incoming.url ?? '/'}`;

  return {
    url,
    pathname: new URL(url).pathname,
    method: incoming.method ?? 'GET',
    headers,
    cookies: parseCookies(headers.cookie),
    params: {},
    query: parseQuery(url),
  };
}

function sendWebResponse(
  outgoing: ServerResponse,
  response: Response,
): void {
  outgoing.statusCode = response.status;

  response.headers.forEach((value, name) => {
    if (name === 'set-cookie' || name === UNHANDLED_HEADER) {
      return;
    }

    outgoing.setHeader(name, value);
  });

  const cookies = response.headers.getSetCookie();

  if (cookies.length > 0) {
    outgoing.setHeader('set-cookie', cookies);
  }

  if (response.status === 204 || response.status === 304) {
    outgoing.end();
    return;
  }

  void response.arrayBuffer().then(
    (body) => {
      outgoing.end(Buffer.from(body));
    },
    (error: unknown) => {
      outgoing.destroy(asError(error));
    },
  );
}

export function parseCookies(header = ''): Record<string, string> {
  const cookies: Record<string, string> = {};

  for (const part of header.split(';')) {
    const index = part.indexOf('=');

    if (index < 0) {
      continue;
    }

    const name = part.slice(0, index).trim();

    let value = part.slice(index + 1).trim();

    try {
      value = decodeURIComponent(value);
    } catch {
      throw new Error(Errors.CookieEscapePercent);
    }

    cookies[name] = value;
  }

  return cookies;
}

export function parseQuery(url: string): Record<string, QueryValue> {
  const query: Record<string, QueryValue> = {};
  const { searchParams } = new URL(url);

  for (const key of new Set(searchParams.keys())) {
    const values = searchParams.getAll(key);

    query[key] = values.length === 1 ? values[0]! : values;
  }

  return query;
}

function listen(server: Server, socket: string): Promise<void> {
  return new Promise<void>((resolve, reject) => {
    const onError = (error: Error): void => {
      server.off(ServerState.Listening, onListening);
      reject(error);
    };

    const onListening = (): void => {
      server.off(ServerState.Error, onError);
      resolve();
    };

    server.once(ServerState.Error, onError);
    server.once(ServerState.Listening, onListening);

    if (isTcpSocket(socket)) {
      server.listen(parseTcpPort(socket), LOCALHOST_PRIVATE);
    } else {
      unlinkSyncIfPresent(socket);
      server.listen(socket);
    }
  });
}

function isTcpSocket(socket: string): boolean {
  return socket.startsWith(TCP_PREFIX);
}

function parseTcpPort(socket: string): number {
  const prefix = `${TCP_PREFIX}${LOCALHOST_PRIVATE}:`;

  if (!socket.startsWith(prefix)) {
    throw new Error(`${Errors.InvalidTcpSocket}: ${socket}`);
  }

  const value = socket.slice(prefix.length);
  const port = Number(value);

  if (!Number.isInteger(port) || port < 0 || port > 65_535) {
    throw new Error(`${Errors.InvalidTcpPort}: ${value}`);
  }

  return port;
}

function runtimeAddress(server: Server, socket: string): string {
  if (!isTcpSocket(socket)) {
    return socket;
  }

  const address = server.address();

  if (address === null || typeof address === 'string') {
    throw new Error(Errors.MissingTcpAddress);
  }

  return `tcp:127.0.0.1:${address.port}`;
}

function unlinkSyncIfPresent(path: string): void {
  try {
    unlinkSync(path);
  } catch {
    // The socket may not exist; that is fine.
  }
}

function asError(error: unknown): Error {
  if (error instanceof Error) {
    return error;
  }

  return new Error(String(error));
}

function formatError(error: unknown, includeStack = false): string {
  if (error instanceof Error) {
    if (includeStack && error.stack) {
      return error.stack;
    }

    return error.message;
  }

  return String(error);
}

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(process.argv[1]).href
) {
  const socket = process.env.PLEC_RUNTIME_SOCKET;
  const bundle = process.env.PLEC_RUNTIME_BUNDLE;
  const token = process.env.PLEC_RUNTIME_TOKEN;

  if (!socket || !bundle || !token) {
    console.error(Errors.MissingDeps);

    process.exit(1);
  }

  start({
    socket,
    bundle,
    token,
  }).catch((error: unknown) => {
    if (error instanceof BundleError) {
      process.stdout.write(`PLEC_RUNTIME_ERROR ${error.message}\n`);
    } else {
      console.error(
        `${Errors.RuntimeFailure}: ${formatError(error, true)}`,
      );
    }

    process.exit(1);
  });
}
