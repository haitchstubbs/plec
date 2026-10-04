import { type PlecComponent } from '../client/jsx';
import type { RootState } from '../client/root/root-state';
import type { RouteMatch } from './router';
export declare const RouteView: () => null;
export declare function renderRouteView(match: RouteMatch, state: RootState, document: Document): Node[];
export declare function renderOutlet(props: Record<string, unknown>, state: RootState, document: Document): Node[];
export declare function DefaultPending(): import("..").PlecNode;
export declare const DefaultError: PlecComponent;
/** Non-retryable fallback used when no route declares a not-found boundary. */
export declare const DefaultNotFound: PlecComponent;
