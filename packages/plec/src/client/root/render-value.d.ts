import type { PlecChild } from '../jsx';
import type { RootState } from './root-state';
export declare function flatten(children: PlecChild[]): PlecChild[];
export declare function applyProps(element: Element, props: Record<string, unknown>): void;
export declare function renderValue(value: PlecChild, state: RootState, document: Document): Node[];
