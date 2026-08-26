type HostEventMap<T extends Window | Document> = T extends Window
  ? WindowEventMap
  : DocumentEventMap;

/** Compiler-owned, scope-lifetime host subscription declaration. */
export function useListener<
  T extends Window | Document,
  K extends keyof HostEventMap<T>,
>(
  _target: T,
  _event: K,
  _handler: (event: HostEventMap<T>[K]) => void | Promise<void>,
): void {}
