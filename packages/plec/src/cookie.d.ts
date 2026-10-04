export type CookieOptions = {
    path?: string;
    sameSite?: 'lax' | 'strict' | 'none';
    secure?: boolean;
    maxAge?: number;
};
/** Declarative cookie capability. Calls are compiled; direct execution is not supported. */
export declare const cookie: {
    getSync(_name: string): string | null;
    get(_name: string): Promise<string | null>;
    set(_name: string, _value: string, _options?: CookieOptions): Promise<void>;
    delete(_name: string, _options?: CookieOptions): Promise<void>;
};
