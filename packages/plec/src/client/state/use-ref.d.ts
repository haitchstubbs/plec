export interface PlecRef<T> {
    current: T;
}
export declare function useRef<T>(initial: T): PlecRef<T>;
/** A ref whose value is owned by host-node mount/disposal rather than user code. */
export declare function useHostRef<T extends Element = Element>(): PlecRef<T | null>;
