import type { PlecNode } from '../jsx';
import type { PlecController } from './root-state';
export declare function createRoot(root: Element): {
    render(app: PlecNode): Promise<PlecController>;
};
