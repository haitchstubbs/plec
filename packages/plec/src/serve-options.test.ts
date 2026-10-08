import { describe, expect, it } from 'vitest';
import { parseServeOptions } from '../bin/serve-options.js';

describe('npm plec serve argument parsing', () => {
  it('preserves directory, host, port, and development options', () => {
    expect(
      parseServeOptions(
        ['dist', '--host=0.0.0.0', '--port', '4312', '--development'],
        { NODE_ENV: 'production' },
      ),
    ).toEqual({
      options: {
        dir: 'dist',
        host: '0.0.0.0',
        port: 4312,
        development: true,
      },
    });
  });

  it('uses the CLI development default and supports help without loading Node', () => {
    expect(parseServeOptions([], {})).toEqual({
      options: { dir: 'dist', development: true },
    });
    expect(parseServeOptions(['--help'])).toEqual({ help: true });
    expect(parseServeOptions([], { NODE_ENV: 'production' })).toEqual({
      options: { dir: 'dist', development: false },
    });
  });

  it('rejects malformed serve options before host startup', () => {
    expect(() => parseServeOptions(['--port', '0'])).toThrow(
      'port must be an integer from 1 to 65535',
    );
    expect(() => parseServeOptions(['one', 'two'])).toThrow(
      'serve accepts one directory',
    );
    expect(() => parseServeOptions(['--unknown'])).toThrow(
      'unknown serve option',
    );
  });
});
