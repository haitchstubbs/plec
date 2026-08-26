/**
 * Compiler-owned reactive actor. The Rust compiler lowers this declaration
 * into a graph reaction; the lightweight JS renderer deliberately does not
 * emulate React effect timing.
 */
export function useReaction(
  _reaction: () => void | (() => void) | Promise<void | (() => void)>,
  _dependencies: readonly unknown[],
): void {}
