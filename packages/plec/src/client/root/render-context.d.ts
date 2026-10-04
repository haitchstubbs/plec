import type { RootState } from './root-state';
export declare function currentRendering(): RootState | undefined;
export declare function withRendering<T>(state: RootState, callback: () => T): T;
