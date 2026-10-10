import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';

export const SUPPORTED_NATIVE_TARGETS = [
  'linux-x64-gnu',
  'linux-x64-musl',
  'linux-arm64-gnu',
  'darwin-arm64',
  'darwin-x64',
  'win32-x64-msvc',
] as const;

export function resolveNativeTarget(
  platform: string,
  arch: string,
  libc: 'gnu' | 'musl' | 'unknown' = 'unknown',
  windowsAbi: 'msvc' | 'gnu' = 'msvc',
): string {
  if (platform === 'linux') return `linux-${arch}-${libc}`;
  if (platform === 'darwin' && ['arm64', 'x64'].includes(arch))
    return `darwin-${arch}`;
  if (platform === 'win32' && arch === 'x64')
    return `win32-x64-${windowsAbi}`;
  return `${platform}-${arch}`;
}

export function currentNativeTarget(): string {
  const runtimeReport = process.report?.getReport() as
    | {
        header?: { glibcVersionRuntime?: string };
        sharedObjects?: string[];
      }
    | undefined;
  const sharedObjects = runtimeReport?.sharedObjects ?? [];
  let libc: 'gnu' | 'musl' | 'unknown' = 'unknown';
  if (runtimeReport?.header?.glibcVersionRuntime) libc = 'gnu';
  else if (
    sharedObjects.some(
      (library) =>
        library.includes('libc.musl-') || library.includes('ld-musl-'),
    )
  ) {
    libc = 'musl';
  } else if (process.platform === 'linux') {
    try {
      libc = readFileSync('/usr/bin/ldd', 'utf8').includes('musl')
        ? 'musl'
        : 'gnu';
    } catch {
      try {
        libc = execFileSync('ldd', ['--version'], {
          encoding: 'utf8',
          stdio: ['ignore', 'pipe', 'pipe'],
        }).includes('musl')
          ? 'musl'
          : 'gnu';
      } catch {
        // Keep unknown explicit so the platform diagnostic does not guess.
      }
    }
  }
  const variables = process.config.variables as
    { node_target_type?: string; shlib_suffix?: string } | undefined;
  const windowsAbi: 'msvc' | 'gnu' =
    variables?.shlib_suffix === 'dll.a' ||
    variables?.node_target_type === 'shared_library'
      ? 'gnu'
      : 'msvc';
  return resolveNativeTarget(
    process.platform,
    process.arch,
    libc,
    windowsAbi,
  );
}
