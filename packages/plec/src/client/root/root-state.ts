import type { PlecNode } from '../jsx';
import type { PlecRouter, RouteMatch } from '../../routes/router';

export interface PlecController {
  dispose(): void;
  diagnostics(): {
    mounts: number;
    domOperations: number;
    stateUpdates: number;
  };
}

export interface RootState {
  root: Element;
  app?: PlecNode;
  state: Map<string, unknown>;
  hookCursor: number;
  component?: string;
  disposed: boolean;
  mounts: number;
  domOperations: number;
  stateUpdates: number;
  cleanup: Array<() => void>;
  renderCleanup: Array<() => void>;
  router?: PlecRouter;
  routerSubscribed?: boolean;
  activeRouteMatch?: RouteMatch;
}
