export interface BenchmarkHudState {
  phase: string;
  stage: string;
  jobIndex: number;
  jobTotal: number;
  renderer: string | null;
  size: number | null;
  operation: string | null;
  completed: number;
  sampleTotal: number;
  startedAt: number;
  progressStartedAt?: number;
}

export function estimateRemainingMs(elapsedMs: number, completed: number, total: number): number | null;
export function formatDuration(ms: number | null | undefined): string;
export function formatHud(state: BenchmarkHudState, now?: number): string[];
export function stripAnsi(value: string): string;
export function createBenchmarkHud(options?: {
  tty?: boolean;
  write?: (value: string) => unknown;
  intervalMs?: number;
}): {
  state: BenchmarkHudState;
  set(next: Partial<BenchmarkHudState>, options?: { force?: boolean }): void;
  start(): void;
  stop(): void;
  suspend(): void;
  resume(): void;
  draw(force?: boolean): void;
};
