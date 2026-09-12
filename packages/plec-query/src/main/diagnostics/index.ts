import type {
  NodeQueryBuildDiagnosticsCollector,
  NodeQueryBuildDiagnosticsCounter,
} from "../types";
import { captureNodeQueryBuildDiagnostics } from "./capture-build-diagnostics";
import { captureNodeQueryCompileDiagnostics } from "./capture-compile-diagnostics";
import { DiagnosticsCollector } from "./collectors";
import {
  measureBuildDiagnosticsStep,
  measureDiagnosticsStep,
} from "./measure-diagnostics-step";

export const QueryDiagnostics = {
  incrementBuildCounter: (
    collector: NodeQueryBuildDiagnosticsCollector | undefined,
    counter: NodeQueryBuildDiagnosticsCounter,
  ): void => {
    collector?.incrementCounter(counter);
  },
  measureBuildDiagnosticsStep: measureBuildDiagnosticsStep,
  measureDiagnosticsStep: measureDiagnosticsStep,
  captureNodeQueryCompileDiagnostics: captureNodeQueryCompileDiagnostics,
  captureNodeQueryBuildDiagnostics: captureNodeQueryBuildDiagnostics,
};

export { DiagnosticsCollector };
