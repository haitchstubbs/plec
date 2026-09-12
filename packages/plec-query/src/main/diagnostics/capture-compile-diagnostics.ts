import type {
  NodeQueryCompileDiagnostics,
  NodeQueryCompileDiagnosticsCollector,
} from "../types";
import { DiagnosticsCollector } from "./collectors";
import { createNodeQueryCompileDiagnosticsSample } from "./compile-diagnostics";
/** @internal */
export function captureNodeQueryCompileDiagnostics<TResult>(
  action: () => TResult,
  options: {
    now?: () => number;
  } = {},
): {
  result: TResult;
  diagnostics: NodeQueryCompileDiagnostics;
} {
  const sample = createNodeQueryCompileDiagnosticsSample();
  const collector: NodeQueryCompileDiagnosticsCollector = {
    now:
      options.now ??
      (() =>
        typeof performance !== "undefined" &&
        typeof performance.now === "function"
          ? performance.now()
          : Date.now()),
    sample,
    recordStep(step, durationMs) {
      sample[step] += durationMs;
    },
    recordApplyMode(mode) {
      sample.applyMode =
        sample.applyMode === "none" || sample.applyMode === mode
          ? mode
          : "mixed";
    },
  };

  DiagnosticsCollector.CompileStack.push(collector);
  try {
    return {
      result: action(),
      diagnostics: sample,
    };
  } finally {
    DiagnosticsCollector.CompileStack.pop();
  }
}
