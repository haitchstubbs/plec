/** Request facts passed from the native host's Node sidecar to application code. */
export { invalidate } from './server-context';

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

export type ApiMiddleware = (
  request: Request,
  context: RequestContext,
  next: () => Promise<Response>,
) => Response | Promise<Response>;

/** Application-owned `/api/*` handler executed by the native host's sidecar. */
export type AppRequestHandler = (
  request: Request,
  context: RequestContext,
) => Response | null | undefined | Promise<Response | null | undefined>;

/** Declares an exported function for invocation from compiled Plec actions.
 * The compiler records only its opaque reference in browser artifacts; the
 * function itself remains ordinary JavaScript in the Node server bundle.
 */
export function action<Args extends unknown[], Result>(
  implementation: (...args: Args) => Promise<Result>,
): (...args: Args) => Promise<Result> {
  return implementation;
}

const actionFailureBrand = Symbol.for('plec.action-failure');

/** Internal signal consumed by the server-action host boundary. */
class ActionFailureSignal extends Error {
  readonly [actionFailureBrand] = true;
  readonly code: string;

  constructor(failure: ActionFailureInput) {
    super(failure.message);
    this.code = failure.code;
    Object.defineProperty(this, 'message', {
      value: failure.message,
      enumerable: true,
      configurable: true,
      writable: true,
    });
  }
}

/** Declare an expected, public failure from a server action. */
export function actionFailure(failure: ActionFailureInput): never {
  throw new ActionFailureSignal(failure);
}

export type ActionFailureInput = {
  code: string;
  message: string;
};

/** @internal Public action failure shape after boundary validation. */
export type ActionFailure = {
  code: string;
  message: string;
};
