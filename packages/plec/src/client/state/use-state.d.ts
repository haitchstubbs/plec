export declare function useState<T>(initial: T): [T, (next: T | ((current: T) => T)) => void];
