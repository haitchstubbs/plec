import type { NodeQueryCompileDiagnostics } from "../types";

export function createNodeQueryCompileDiagnosticsSample(): NodeQueryCompileDiagnostics {
  return {
    pendingOpsMaterializeMs: 0,
    pendingOpsSerializeMs: 0,
    pendingOpsApplyMs: 0,
    compileBundleMs: 0,
    pendingOpCount: 0,
    applyMode: "none",
  };
}
