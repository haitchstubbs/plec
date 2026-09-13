/** Request facts passed from the native host's Node sidecar to application code. */
export type RequestContext = {
  url: string;
  pathname: string;
  method: string;
  headers: Record<string, string>;
  cookies: Record<string, string>;
  params: Record<string, string>;
  query: Record<string, string | string[]>;
};

export type ApiRouteHandler = (
  request: Request,
  context: RequestContext,
) => Response | Promise<Response>;

/** Application-owned `/api/*` handler executed by the native host's sidecar. */
export type AppRequestHandler = (
  request: Request,
  context: RequestContext,
) => Response | null | undefined | Promise<Response | null | undefined>;
