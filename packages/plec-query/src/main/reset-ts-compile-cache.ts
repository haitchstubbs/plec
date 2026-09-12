export function __resetTsCompileCache(): void {
  // The current TS facade does not maintain a shared cross-instance compile cache.
  // This hook remains for benchmark harness compatibility.
}
