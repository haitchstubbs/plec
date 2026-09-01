export type CookieOptions = {
  path?: string;
  sameSite?: 'lax' | 'strict' | 'none';
  secure?: boolean;
  maxAge?: number;
};

/** Declarative cookie capability. Calls are compiled; direct execution is not supported. */
export const cookie = {
  getSync(_name: string): string | null {
    throw new Error('cookie.getSync must be compiled');
  },
  async get(_name: string): Promise<string | null> {
    throw new Error('cookie.get must be compiled');
  },
  async set(
    _name: string,
    _value: string,
    _options: CookieOptions = {},
  ): Promise<void> {
    throw new Error('cookie.set must be compiled');
  },
  async delete(
    _name: string,
    _options: CookieOptions = {},
  ): Promise<void> {
    throw new Error('cookie.delete must be compiled');
  },
};
