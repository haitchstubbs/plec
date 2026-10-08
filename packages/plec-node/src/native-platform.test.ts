import { describe, expect, it } from 'vitest';
import {
  resolveNativeTarget,
  SUPPORTED_NATIVE_TARGETS,
} from './native-platform.js';

describe('@plec/node native platform mapping', () => {
  it('lists all six package targets and selects each supported triple', () => {
    const resolved = [
      resolveNativeTarget('linux', 'x64', 'gnu'),
      resolveNativeTarget('linux', 'x64', 'musl'),
      resolveNativeTarget('linux', 'arm64', 'gnu'),
      resolveNativeTarget('darwin', 'arm64'),
      resolveNativeTarget('darwin', 'x64'),
      resolveNativeTarget('win32', 'x64', 'unknown', 'msvc'),
    ];

    expect(resolved).toEqual(SUPPORTED_NATIVE_TARGETS);
  });

  it('keeps unclaimed OS/architecture/ABI combinations unsupported', () => {
    expect(resolveNativeTarget('win32', 'x64', 'unknown', 'gnu')).toBe(
      'win32-x64-gnu',
    );
    expect(resolveNativeTarget('linux', 'arm64', 'musl')).toBe(
      'linux-arm64-musl',
    );
    expect(resolveNativeTarget('freebsd', 'x64')).toBe('freebsd-x64');
  });
});
