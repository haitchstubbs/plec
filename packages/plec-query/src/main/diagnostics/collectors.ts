import type {
  NodeQueryBuildDiagnosticsCollector,
  NodeQueryCompileDiagnosticsCollector,
} from "#types";

const nodeQueryCompileDiagnosticsStack: NodeQueryCompileDiagnosticsCollector[] =
  [];
const nodeQueryBuildDiagnosticsStack: NodeQueryBuildDiagnosticsCollector[] = [];

function getActiveNodeQueryCompileDiagnosticsCollector():
  | NodeQueryCompileDiagnosticsCollector
  | undefined {
  return nodeQueryCompileDiagnosticsStack.at(-1);
}

function getActiveNodeQueryBuildDiagnosticsCollector():
  | NodeQueryBuildDiagnosticsCollector
  | undefined {
  return nodeQueryBuildDiagnosticsStack.at(-1);
}

const DiagnosticsCollector = {
  Compiler: getActiveNodeQueryCompileDiagnosticsCollector,
  CompileStack: nodeQueryCompileDiagnosticsStack,
  Builder: getActiveNodeQueryBuildDiagnosticsCollector,
  BuildStack: nodeQueryBuildDiagnosticsStack,
};

export { DiagnosticsCollector };
