import type {
  NodeQueryBuildDiagnosticsCollector,
  NodeQueryBuildDiagnosticsStep,
  NodeQueryCompileDiagnosticsCollector,
  NodeQueryCompileDiagnosticsStep,
} from "../types";

export function measureDiagnosticsStep<TResult>(
  collector: NodeQueryCompileDiagnosticsCollector | undefined,
  step: NodeQueryCompileDiagnosticsStep,
  action: () => TResult,
): TResult {
  if (!collector) {
    return action();
  }

  const startedAt = collector.now();
  const result = action();
  collector.recordStep(step, collector.now() - startedAt);
  return result;
}

export function measureBuildDiagnosticsStep<TResult>(
  collector: NodeQueryBuildDiagnosticsCollector | undefined,
  step: NodeQueryBuildDiagnosticsStep,
  action: () => TResult,
): TResult {
  if (!collector) {
    return action();
  }

  const startedAt = collector.now();
  const result = action();
  collector.recordStep(step, collector.now() - startedAt);
  return result;
}
