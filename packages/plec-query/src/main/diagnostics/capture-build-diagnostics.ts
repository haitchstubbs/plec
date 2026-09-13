import type {
  NodeQueryBuildDiagnostics,
  NodeQueryBuildDiagnosticsCollector,
} from '../types';
import { createNodeQueryBuildDiagnosticsSample } from './build-diagnostics';
import { DiagnosticsCollector } from './collectors';

/** @internal */
export function captureNodeQueryBuildDiagnostics<TResult>(
  action: () => TResult,
  options: {
    now?: () => number;
  } = {},
): { result: TResult; diagnostics: NodeQueryBuildDiagnostics } {
  const sample = createNodeQueryBuildDiagnosticsSample();
  const collector: NodeQueryBuildDiagnosticsCollector = {
    now:
      options.now ??
      (() =>
        typeof performance !== 'undefined' &&
        typeof performance.now === 'function'
          ? performance.now()
          : Date.now()),
    sample,
    recordStep(step, durationMs) {
      sample[step] += durationMs;
    },
    incrementCounter(counter) {
      sample[counter] += 1;
    },
  };

  DiagnosticsCollector.BuildStack.push(collector);
  try {
    return {
      result: action(),
      diagnostics: sample,
    };
  } finally {
    DiagnosticsCollector.BuildStack.pop();
  }
}
