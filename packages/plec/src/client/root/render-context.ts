import type { RootState } from './root-state';

let rendering: RootState | undefined;

export function currentRendering(): RootState | undefined {
  return rendering;
}

export function withRendering<T>(
  state: RootState,
  callback: () => T,
): T {
  const previous = rendering;
  rendering = state;
  try {
    return callback();
  } finally {
    rendering = previous;
  }
}
