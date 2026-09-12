import type {
  CompiledQuery,
  DatabaseConnection,
  Dialect,
  Primitive,
  RemoteQueryRequest,
  SqlQuery,
} from "#types";
import { runtimeCompileQuery } from "../runtime/bridge";

type PostgresDriver<TResult> = {
  query(text: string, values: Primitive[]): TResult | Promise<TResult>;
};

type FetchLike = (
  input: string,
  init?: {
    method?: string;
    headers?: HeadersInit;
    body?: string;
  },
) => Promise<Response>;

type RemoteHttpConnectionOptions<TResult> = {
  url: string;
  fetch?: FetchLike;
  headers?: HeadersInit | (() => HeadersInit | Promise<HeadersInit>);
  mapResponse?: (response: Response) => TResult | Promise<TResult>;
};

async function resolveHeaders(
  headers?: HeadersInit | (() => HeadersInit | Promise<HeadersInit>),
): Promise<HeadersInit | undefined> {
  if (!headers) {
    return undefined;
  }

  return typeof headers === "function" ? await headers() : headers;
}

async function defaultMapResponse(response: Response): Promise<unknown> {
  return response.json();
}

function buildRequestHeaders(headers?: HeadersInit): Headers {
  const requestHeaders = new Headers(headers);
  requestHeaders.set("content-type", "application/json");
  return requestHeaders;
}

/**
 * Compiles a query payload for a target SQL dialect.
 *
 * @remarks Runtime rendering is delegated to the Rust-backed query compiler.
 *
 * @param query - The {@link SqlQuery} payload to compile.
 * @param dialect - The {@link Dialect} to render for.
 * @returns The rendered {@link CompiledQuery}.
 * @throws {Error} When the runtime rejects the query payload or cannot render it
 * for the dialect.
 *
 * @example
 * ```ts
 * const compiled = compileQuery(
 *   { text: "select 1", raw: "select 1", values: [] },
 *   "postgres",
 * );
 * ```
 */
export function compileQuery(
  query: SqlQuery,
  dialect: Dialect = "postgres",
): CompiledQuery {
  return runtimeCompileQuery(query, dialect);
}

/**
 * Compiles a query payload for the Postgres dialect.
 *
 * @remarks Use this when the execution target is always Postgres.
 *
 * @param query - The {@link SqlQuery} payload to compile.
 * @returns The rendered {@link CompiledQuery}.
 * @throws {Error} When the runtime rejects the query payload or cannot render it
 * for Postgres.
 *
 * @example
 * ```ts
 * const compiled = compilePostgres({
 *   text: "select 1",
 *   raw: "select 1",
 *   values: [],
 * });
 * ```
 */
export function compilePostgres(query: SqlQuery): CompiledQuery {
  return compileQuery(query, "postgres");
}

/**
 * Creates a connection wrapper around a Postgres-compatible driver.
 *
 * @remarks The wrapper compiles each {@link SqlQuery} before passing it to the
 * driver.
 *
 * @param driver - The driver with a query method that accepts SQL text and
 * parameter values.
 * @returns A {@link DatabaseConnection} that executes compiled Postgres queries.
 * @throws {Error} When query compilation fails before the driver is called.
 *
 * @example
 * ```ts
 * const connection = createPostgresConnection({
 *   query: async (text, values) => ({ text, values }),
 * });
 * ```
 */
export function createPostgresConnection<TResult>(
  driver: PostgresDriver<TResult>,
): DatabaseConnection<TResult> {
  return {
    execute(query) {
      const compiled = compilePostgres(query);
      return driver.query(compiled.text, compiled.values);
    },
  };
}

/**
 * Creates a connection that sends compiled Postgres queries to an HTTP endpoint.
 *
 * @remarks The endpoint receives a {@link RemoteQueryRequest} JSON body with
 * Postgres SQL.
 *
 * @param options - The remote endpoint, fetch implementation, headers, and
 * response mapper.
 * @returns A {@link DatabaseConnection} that resolves with the mapped HTTP response.
 * @throws {Error} When no fetch implementation is available or the remote
 * response is not OK.
 *
 * @example
 * ```ts
 * const connection = createRemoteHttpConnection({
 *   url: "https://example.com/query",
 * });
 * ```
 */
export function createRemoteHttpConnection<TResult = unknown>(
  options: RemoteHttpConnectionOptions<TResult>,
): DatabaseConnection<Promise<TResult>> {
  const fetchImpl = options.fetch ?? globalThis.fetch;

  if (!fetchImpl) {
    throw new Error(
      "createRemoteHttpConnection requires a fetch implementation in this runtime.",
    );
  }

  return {
    async execute(query) {
      const compiled = compilePostgres(query);
      const request: RemoteQueryRequest = {
        dialect: "postgres",
        text: compiled.text,
        values: compiled.values,
        raw: compiled.raw,
      };
      const resolvedHeaders = await resolveHeaders(options.headers);
      const response = await fetchImpl(options.url, {
        method: "POST",
        headers: buildRequestHeaders(resolvedHeaders),
        body: JSON.stringify(request),
      });

      if (!response.ok) {
        const responseText = await response.text().catch(() => "");
        const details = responseText ? ` ${responseText}` : "";
        throw new Error(
          `Remote query request failed with status ${response.status}.${details}`,
        );
      }

      return (options.mapResponse ?? defaultMapResponse)(
        response,
      ) as Promise<TResult>;
    },
  };
}
